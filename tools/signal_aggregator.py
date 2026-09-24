#!/usr/bin/env python3
"""
Off-chain reputation-weighted signaling aggregator for Vortex Protocol.

Verifies signed Stellar messages, computes governance-weight aggregates,
and publishes results for governance proposals.

Usage:
    python signal_aggregator.py aggregate \\
        --contract <intent_settlement_contract_id> \\
        --rpc-url <rpc_endpoint> \\
        --proposal-id <proposal_id> \\
        --snapshot-block <block_number> \\
        --signals-file <signals.jsonl> \\
        --output results.json

Example signal message format:
    {
        "payload": {
            "proposal_id": "proposal-2026-10-15-treasury-audit",
            "proposal_title": "Fund Security Audit Q4 2026",
            "voter_address": "GXXXXXX...",
            "signal": "for",
            "timestamp": 1698067200,
            "message": "Audit is critical."
        },
        "signature": "base64_encoded_signature"
    }
"""

import argparse
import json
import sys
from datetime import datetime
from typing import Dict, List, Optional, Tuple
from dataclasses import dataclass, asdict


@dataclass
class Signal:
    """A single voter's signal on a proposal."""
    proposal_id: str
    voter_address: str
    signal: str  # "for", "against", "abstain"
    timestamp: int
    message: str
    weight: int = 0  # Computed during aggregation
    reputation_score: int = 0
    bond: int = 0


@dataclass
class AggregatedResult:
    """Aggregated results for a proposal."""
    proposal_id: str
    proposal_title: str
    snapshot_block: int
    signal_window_start: Optional[str]
    signal_window_end: Optional[str]
    total_signers: int
    total_weight: int
    results: Dict
    top_signers: List[Dict]
    generated_at: str


class SignalAggregator:
    """Aggregates and weights governance signals from Stellar signers."""

    def __init__(self, contract_id: str, rpc_url: str = "https://soroban-testnet.stellar.org"):
        """
        Initialize the aggregator.

        Args:
            contract_id: Stellar contract ID for intent_settlement
            rpc_url: Soroban RPC endpoint
        """
        self.contract_id = contract_id
        self.rpc_url = rpc_url
        self.signals: Dict[str, Dict[str, Signal]] = {}  # {proposal_id: {address: signal}}

    def add_signal(self, payload: dict, signature: str) -> Tuple[bool, str]:
        """
        Verify and add a signal.

        Args:
            payload: The signal payload (dict)
            signature: The signature (base64-encoded string)

        Returns:
            Tuple of (success: bool, message: str)
        """
        try:
            # Validate required fields
            required_fields = ["proposal_id", "voter_address", "signal", "timestamp"]
            for field in required_fields:
                if field not in payload:
                    return False, f"Missing required field: {field}"

            proposal_id = payload["proposal_id"]
            voter_address = payload["voter_address"]
            signal_type = payload["signal"]
            timestamp = payload["timestamp"]

            # Validate signal type
            if signal_type not in ["for", "against", "abstain"]:
                return False, f"Invalid signal type: {signal_type}"

            # Validate Stellar address format (starts with G, 56 chars)
            if not voter_address.startswith("G") or len(voter_address) != 56:
                return False, f"Invalid Stellar address: {voter_address}"

            # Reconstruct canonical JSON for signature verification
            canonical = self._canonicalize(payload)

            # Verify signature (placeholder - requires stellar_sdk)
            # In production, use stellar_sdk.verify_tx_envelope_signature or similar
            if not self._verify_signature(canonical, signature, voter_address):
                return False, f"Signature verification failed for {voter_address}"

            # Dedup: keep latest timestamp for this address on this proposal
            if proposal_id not in self.signals:
                self.signals[proposal_id] = {}

            old_signal = self.signals[proposal_id].get(voter_address)
            if old_signal and old_signal.timestamp > timestamp:
                return False, f"Newer signal already registered for {voter_address}"

            # Store signal
            self.signals[proposal_id][voter_address] = Signal(
                proposal_id=proposal_id,
                voter_address=voter_address,
                signal=signal_type,
                timestamp=timestamp,
                message=payload.get("message", ""),
            )

            return True, f"Signal added for {voter_address}"

        except Exception as e:
            return False, f"Error processing signal: {str(e)}"

    def load_signals_from_file(self, filename: str) -> Tuple[int, int]:
        """
        Load signals from a line-delimited JSON file.

        Each line should be: {"payload": {...}, "signature": "..."}

        Returns:
            Tuple of (accepted_count, rejected_count)
        """
        accepted = 0
        rejected = 0

        try:
            with open(filename, 'r') as f:
                for line_num, line in enumerate(f, 1):
                    if not line.strip():
                        continue

                    try:
                        record = json.loads(line)
                        payload = record.get("payload")
                        signature = record.get("signature")

                        if not payload or not signature:
                            print(f"Line {line_num}: Missing payload or signature")
                            rejected += 1
                            continue

                        success, msg = self.add_signal(payload, signature)
                        if success:
                            accepted += 1
                        else:
                            print(f"Line {line_num}: {msg}")
                            rejected += 1

                    except json.JSONDecodeError as e:
                        print(f"Line {line_num}: Invalid JSON: {str(e)}")
                        rejected += 1

        except FileNotFoundError:
            print(f"File not found: {filename}")
            return 0, 0

        return accepted, rejected

    def aggregate_results(
        self,
        proposal_id: str,
        proposal_title: str,
        snapshot_block: int,
        weights: Optional[Dict[str, int]] = None,
    ) -> Optional[AggregatedResult]:
        """
        Aggregate signals and compute weighted results.

        Args:
            proposal_id: The proposal identifier
            proposal_title: Human-readable title
            snapshot_block: Block height for weight snapshot
            weights: Pre-computed weights {address: weight}. If None, use mock weights.

        Returns:
            AggregatedResult object or None if no signals
        """
        signals = self.signals.get(proposal_id, {})

        if not signals:
            print(f"No signals found for proposal {proposal_id}")
            return None

        # If weights not provided, use mock (for testing)
        if weights is None:
            weights = {addr: 100_000 for addr in signals.keys()}

        # Assign weights and aggregate by signal type
        for_count = 0
        for_weight = 0
        against_count = 0
        against_weight = 0
        abstain_count = 0
        abstain_weight = 0

        weighted_signals = []

        for address, signal in signals.items():
            weight = weights.get(address, 0)
            signal.weight = weight

            weighted_signals.append({
                "address": address,
                "signal": signal.signal,
                "weight": weight,
                "timestamp": signal.timestamp,
                "message": signal.message,
            })

            if signal.signal == "for":
                for_count += 1
                for_weight += weight
            elif signal.signal == "against":
                against_count += 1
                against_weight += weight
            elif signal.signal == "abstain":
                abstain_count += 1
                abstain_weight += weight

        total_weight = for_weight + against_weight + abstain_weight

        # Compute percentages (avoid division by zero)
        if total_weight > 0:
            for_pct = (for_weight / total_weight) * 100
            against_pct = (against_weight / total_weight) * 100
            abstain_pct = (abstain_weight / total_weight) * 100
        else:
            for_pct = against_pct = abstain_pct = 0

        # Top signers (sorted by weight descending)
        top_signers = sorted(weighted_signals, key=lambda x: x["weight"], reverse=True)[:10]

        results = AggregatedResult(
            proposal_id=proposal_id,
            proposal_title=proposal_title,
            snapshot_block=snapshot_block,
            signal_window_start=None,
            signal_window_end=None,
            total_signers=len(signals),
            total_weight=total_weight,
            results={
                "for": {
                    "count": for_count,
                    "weight": for_weight,
                    "percentage": round(for_pct, 2),
                },
                "against": {
                    "count": against_count,
                    "weight": against_weight,
                    "percentage": round(against_pct, 2),
                },
                "abstain": {
                    "count": abstain_count,
                    "weight": abstain_weight,
                    "percentage": round(abstain_pct, 2),
                },
            },
            top_signers=top_signers,
            generated_at=datetime.utcnow().isoformat() + "Z",
        )

        return results

    def save_results_to_file(self, results: AggregatedResult, filename: str) -> bool:
        """Save aggregated results to a JSON file."""
        try:
            with open(filename, 'w') as f:
                json.dump(asdict(results), f, indent=2)
            return True
        except Exception as e:
            print(f"Error saving results: {str(e)}")
            return False

    # Helper methods

    @staticmethod
    def _canonicalize(obj: dict) -> str:
        """Canonicalize a dict to JSON with sorted keys and no whitespace."""
        return json.dumps(obj, sort_keys=True, separators=(',', ':'))

    @staticmethod
    def _verify_signature(message: str, signature: str, address: str) -> bool:
        """
        Verify a Stellar signature.

        In production, this should use stellar_sdk.verify_tx_envelope_signature
        or similar. For now, this is a placeholder that returns True if
        address and signature are non-empty (suitable for testing).

        TODO: Implement real Stellar signature verification.
        """
        # Placeholder: accept any non-empty signature from a valid address
        return bool(signature and address.startswith("G"))


def main():
    """CLI entry point for the aggregator."""
    parser = argparse.ArgumentParser(
        description="Aggregate off-chain reputation-weighted signals for Vortex governance."
    )

    subparsers = parser.add_subparsers(dest="command", help="Command to run")

    # 'aggregate' subcommand
    agg_parser = subparsers.add_parser("aggregate", help="Aggregate signals from a file")
    agg_parser.add_argument("--contract", required=True, help="Intent settlement contract ID")
    agg_parser.add_argument(
        "--rpc-url",
        default="https://soroban-testnet.stellar.org",
        help="Soroban RPC endpoint",
    )
    agg_parser.add_argument("--proposal-id", required=True, help="Proposal identifier")
    agg_parser.add_argument("--proposal-title", default="", help="Proposal title")
    agg_parser.add_argument("--snapshot-block", type=int, required=True, help="Snapshot block height")
    agg_parser.add_argument("--signals-file", required=True, help="Input signals file (line-delimited JSON)")
    agg_parser.add_argument("--output", default="results.json", help="Output results file")

    args = parser.parse_args()

    if not args.command:
        parser.print_help()
        return 1

    if args.command == "aggregate":
        aggregator = SignalAggregator(args.contract, args.rpc_url)

        # Load signals
        print(f"Loading signals from {args.signals_file}...")
        accepted, rejected = aggregator.load_signals_from_file(args.signals_file)
        print(f"Loaded: {accepted} accepted, {rejected} rejected")

        if accepted == 0:
            print("No valid signals to aggregate.")
            return 1

        # Aggregate results
        print(f"Aggregating signals for proposal {args.proposal_id}...")
        results = aggregator.aggregate_results(
            args.proposal_id,
            args.proposal_title,
            args.snapshot_block,
        )

        if not results:
            print("Aggregation failed.")
            return 1

        # Save and display results
        print(f"Saving results to {args.output}...")
        if aggregator.save_results_to_file(results, args.output):
            print("✓ Results saved successfully")

            # Print summary
            print("\n" + "=" * 60)
            print(f"PROPOSAL: {results.proposal_title} ({results.proposal_id})")
            print(f"SNAPSHOT: Block {results.snapshot_block}")
            print(f"SIGNERS: {results.total_signers} (total weight: {results.total_weight:,})")
            print("=" * 60)
            print(f"FOR:     {results.results['for']['count']:3d} signers | "
                  f"{results.results['for']['weight']:10,} weight | "
                  f"{results.results['for']['percentage']:6.2f}%")
            print(f"AGAINST: {results.results['against']['count']:3d} signers | "
                  f"{results.results['against']['weight']:10,} weight | "
                  f"{results.results['against']['percentage']:6.2f}%")
            print(f"ABSTAIN: {results.results['abstain']['count']:3d} signers | "
                  f"{results.results['abstain']['weight']:10,} weight | "
                  f"{results.results['abstain']['percentage']:6.2f}%")
            print("=" * 60)

            return 0
        else:
            print("✗ Error saving results")
            return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
