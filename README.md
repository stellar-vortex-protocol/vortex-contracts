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
- `slash_solver()` — permissionless: slashes a solver that failed to fill
- `register_solver()` / `deregister_solver()` — solver bond management

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

- [x] **Contract test suite** — `soroban_sdk` testutils coverage for the full intent lifecycle (23 tests)
- [ ] **Solver registry contract** — tiered staking, reputation NFT, dispute resolution
- [ ] **Cross-chain proof verification** — verify source-chain tx on-chain via Stellar oracle / messaging infra

---

## Contributing

See the org-wide
[CONTRIBUTING.md](https://github.com/vortex-protocol/.github/blob/main/CONTRIBUTING.md).

## License

[MIT](./LICENSE) © 2025–2026 Vortex Protocol Contributors
