# vortex-contract

**Soroban smart contracts for [Vortex Protocol](https://github.com/vortex-protocol) — intent-based cross-chain swaps settled on Stellar.**

[![CI](https://github.com/vortex-protocol/vortex-contract/actions/workflows/ci.yml/badge.svg)](https://github.com/vortex-protocol/vortex-contract/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)

This repository holds the on-chain logic that guarantees settlement: intent
lifecycle, solver bonds, and slashing. Part of the multi-repo Vortex stack —
see also [`vortex-backend`](https://github.com/vortex-protocol/vortex-backend)
and [`vortex-frontend`](https://github.com/vortex-protocol/vortex-frontend).

---

## Contracts

### `intent_settlement`

Core protocol logic (`intent_settlement/src/lib.rs`):

- `submit_intent()` — user creates a swap intent
- `accept_intent()` — solver claims exclusive fill rights
- `fill_intent()` — solver delivers output tokens to the user
- `cancel_intent()` — user cancels an open intent
- `expire_intent()` — permissionless: materializes an unfilled intent's expiry
- `slash_solver()` — permissionless: slashes a solver that failed to fill
- `register_solver()` / `deregister_solver()` / `withdraw_bond()` — solver bond management
- `set_fee_recipient()` / `transfer_admin()` — admin key management
- `pause()` / `unpause()` — admin-only incident response
- `add_allowed_dst_token()` / `remove_allowed_dst_token()` / `set_dst_allowlist_enabled()` — optional dst_token allowlist

#### Usage examples

All examples use the [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli)
against a deployed contract. Swap `<CONTRACT_ID>` and `<SECRET_KEY>` for your
deployment; addresses shown are placeholders.

```bash
# User submits a swap intent: 1 ETH on Ethereum for at least 3500 USDC on Stellar
stellar contract invoke --id <CONTRACT_ID> --source <SECRET_KEY> --network testnet -- \
  submit_intent \
  --user <USER_ADDRESS> \
  --src_chain '"ethereum"' \
  --src_token '"0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"' \
  --src_amount 1000000000000000000 \
  --dst_token <USDC_SAC_ADDRESS> \
  --min_dst_amount 35000000000

# Solver registers with a 50 USDC bond (MIN_BOND)
stellar contract invoke --id <CONTRACT_ID> --source <SOLVER_SECRET_KEY> --network testnet -- \
  register_solver --solver <SOLVER_ADDRESS> --bond_amount 500000000

# Solver claims exclusive fill rights on an intent
stellar contract invoke --id <CONTRACT_ID> --source <SOLVER_SECRET_KEY> --network testnet -- \
  accept_intent --solver <SOLVER_ADDRESS> --intent_id <INTENT_ID>

# Solver delivers the output and closes out the intent
stellar contract invoke --id <CONTRACT_ID> --source <SOLVER_SECRET_KEY> --network testnet -- \
  fill_intent --solver <SOLVER_ADDRESS> --intent_id <INTENT_ID> --fill_amount 35000000000

# Anyone can slash a solver that accepted but missed the fill window
stellar contract invoke --id <CONTRACT_ID> --source <ANY_SECRET_KEY> --network testnet -- \
  slash_solver --intent_id <INTENT_ID>

# Read-only: check current protocol stats
stellar contract invoke --id <CONTRACT_ID> --source <ANY_SECRET_KEY> --network testnet -- \
  get_stats
```

### `solver_registry` (planned)

Tiered solver staking with reputation scores. See the roadmap below.

---

## Build & Test

### Prerequisites

- Rust 1.78+ with the `wasm32-unknown-unknown` target
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli)

```bash
cd intent_settlement
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
stellar contract build
```

### Deploy (testnet)

```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm \
  --source <SECRET_KEY> \
  --network testnet
```

---

## Security Model

Settlement relies on two primitives:

1. **Solver bonds** — solvers lock USDC to participate. Failed fills slash 10% of
   their bond, making repeated failures unprofitable.
2. **Fill-window enforcement** — once a solver accepts, the intent is locked for
   5 minutes. If they fail to fill, the intent reverts to `open` and is
   re-auctioned, and the bond is slashed permissionlessly via `slash_solver()`.

To report a vulnerability, see the org
[SECURITY.md](https://github.com/vortex-protocol/.github/blob/main/SECURITY.md).

---

## Roadmap

- [x] **Contract test suite** — `soroban_sdk` testutils coverage for the full intent
      lifecycle, solver bonding/slashing, admin controls, pause, and storage TTL
      management
- [ ] **Solver registry contract** — tiered staking, reputation NFT, dispute resolution
- [ ] **Cross-chain proof verification** — verify source-chain tx on-chain via Stellar oracle / messaging infra

---

## Contributing

See the org-wide
[CONTRIBUTING.md](https://github.com/vortex-protocol/.github/blob/main/CONTRIBUTING.md).

## License

[MIT](./LICENSE) © 2025–2026 Vortex Protocol Contributors
