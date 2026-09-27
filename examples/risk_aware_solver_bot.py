#!/usr/bin/env python3
"""Reference risk-aware Vortex solver bot.

This example shows the decision layer a production solver should place in
front of the mechanical accept/fill loop:

1. Confirm the solver is eligible.
2. Fetch the intent and current solver record.
3. Reject intents that miss basic profitability or bond-risk thresholds.
4. Accept only candidates that pass those checks.

The implementation uses only Python's standard library and the Stellar CLI so
it can be copied into an operator environment without adding repository
dependencies. Chain-specific quoting and fill execution are intentionally
stubbed; real solvers should replace `estimate_fill_cost` with their own route,
bridge, inventory, and fee model.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from dataclasses import dataclass
from decimal import Decimal
from typing import Any


MIN_BOND_STROOPS = 50 * 10_000_000


@dataclass(frozen=True)
class BotConfig:
    contract_id: str
    solver_secret: str
    solver_address: str
    network: str = "testnet"
    min_profit_stroops: int = 1_000_000
    max_bond_utilization_bps: int = 5_000
    max_active_intents: int = 3
    fill_window_seconds: int = 300

    @classmethod
    def from_env(cls) -> "BotConfig":
        return cls(
            contract_id=required_env("VORTEX_CONTRACT_ID"),
            solver_secret=required_env("SOLVER_SECRET_KEY"),
            solver_address=required_env("SOLVER_ADDRESS"),
            network=os.getenv("STELLAR_NETWORK", "testnet"),
            min_profit_stroops=int(os.getenv("MIN_PROFIT_STROOPS", "1000000")),
            max_bond_utilization_bps=int(os.getenv("MAX_BOND_UTILIZATION_BPS", "5000")),
            max_active_intents=int(os.getenv("MAX_ACTIVE_INTENTS", "3")),
            fill_window_seconds=int(os.getenv("FILL_WINDOW_SECONDS", "300")),
        )


@dataclass(frozen=True)
class Decision:
    should_accept: bool
    reason: str
    expected_profit_stroops: int = 0
    bond_utilization_bps: int = 0


def required_env(name: str) -> str:
    value = os.getenv(name)
    if not value:
        raise SystemExit(f"missing required environment variable: {name}")
    return value


def stellar_view(config: BotConfig, function: str, *args: str) -> Any:
    cmd = [
        "stellar",
        "contract",
        "invoke",
        "--id",
        config.contract_id,
        "--source",
        config.solver_address,
        "--network",
        config.network,
        "--",
        function,
        *args,
    ]
    completed = subprocess.run(cmd, check=True, capture_output=True, text=True)
    return parse_cli_output(completed.stdout)


def stellar_tx(config: BotConfig, function: str, *args: str) -> str:
    cmd = [
        "stellar",
        "contract",
        "invoke",
        "--id",
        config.contract_id,
        "--source",
        config.solver_secret,
        "--network",
        config.network,
        "--",
        function,
        *args,
    ]
    completed = subprocess.run(cmd, check=True, capture_output=True, text=True)
    return completed.stdout.strip()


def parse_cli_output(output: str) -> Any:
    text = output.strip()
    if text in {"true", "false"}:
        return text == "true"
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


def estimate_fill_cost(intent: dict[str, Any]) -> int:
    """Return expected total cost to deliver the fill, in dst-token stroops.

    Replace this placeholder with a real model that includes:
    - source-chain execution and bridge cost,
    - destination token inventory cost,
    - Stellar transaction fees,
    - slippage and failed-fill risk.
    """

    src_amount = int(intent.get("src_amount", 0))
    return int(Decimal(src_amount) * Decimal("0.995"))


def decide(config: BotConfig, intent: dict[str, Any], solver: dict[str, Any], now: int) -> Decision:
    state = intent.get("state")
    if state not in {"Open", "PartiallyFilled"}:
        return Decision(False, f"intent state is {state}")

    deadline = int(intent["deadline"])
    if deadline - now < config.fill_window_seconds:
        return Decision(False, "not enough time remains to accept and fill")

    active_intents = int(solver.get("active_intents", 0))
    if active_intents >= config.max_active_intents:
        return Decision(False, "solver active intent cap reached")

    bond_amount = int(solver.get("bond_amount", 0))
    bond_utilization_bps = ((active_intents + 1) * MIN_BOND_STROOPS * 10_000) // max(
        bond_amount, 1
    )
    if bond_utilization_bps > config.max_bond_utilization_bps:
        return Decision(False, "accepting would exceed bond utilization limit", 0, bond_utilization_bps)

    min_dst_amount = int(intent["min_dst_amount"])
    expected_cost = estimate_fill_cost(intent)
    expected_profit = min_dst_amount - expected_cost
    if expected_profit < config.min_profit_stroops:
        return Decision(False, "expected profit below threshold", expected_profit, bond_utilization_bps)

    return Decision(True, "accepted risk/profit checks", expected_profit, bond_utilization_bps)


def get_intents_batch(config: BotConfig, intent_ids: list[str]) -> list[Any]:
    """Fetch many candidate intents in a single RPC round-trip via
    get_intents_batch, instead of one stellar_view call per id. Each
    position mirrors get_intent's semantics: an unknown id comes back None.
    """
    if not intent_ids:
        return []
    return stellar_view(config, "get_intents_batch", "--intent_ids", json.dumps(intent_ids))


def screen_candidates(config: BotConfig, intent_ids: list[str]) -> list[str]:
    """Given a list of candidate intent ids (e.g. from list_open_intents,
    issue #64), return only the ones still Open/PartiallyFilled -- cheaply,
    via one batched view call rather than one per candidate.
    """
    records = get_intents_batch(config, intent_ids)
    return [
        intent_id
        for intent_id, record in zip(intent_ids, records)
        if record is not None and record.get("state") in {"Open", "PartiallyFilled"}
    ]


def maybe_accept_intent(config: BotConfig, intent_id: str, now: int) -> Decision:
    eligible = stellar_view(
        config,
        "is_solver_eligible",
        "--solver",
        config.solver_address,
    )
    if eligible is not True:
        return Decision(False, "solver is not eligible")

    intent = stellar_view(config, "get_intent", "--intent_id", intent_id)
    solver = stellar_view(config, "get_solver", "--solver", config.solver_address)
    decision = decide(config, intent, solver, now)
    if not decision.should_accept:
        return decision

    stellar_tx(config, "accept_intent", "--intent_id", intent_id, "--solver", config.solver_address)
    return decision


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: risk_aware_solver_bot.py <intent_id> <current_unix_time>", file=sys.stderr)
        return 2

    config = BotConfig.from_env()
    decision = maybe_accept_intent(config, sys.argv[1], int(sys.argv[2]))
    print(json.dumps(decision.__dict__, sort_keys=True))
    return 0 if decision.should_accept else 1


if __name__ == "__main__":
    raise SystemExit(main())
