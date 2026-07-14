# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project has not yet made a versioned release; entries below are grouped
under "Unreleased" and will be cut into a version once `intent_settlement`
first deploys to mainnet.

## [Unreleased]

### Fixed

- `deregister_solver` now refuses to return a solver's bond while they hold
  an `Accepted` intent, closing a path to dodge `slash_solver` by
  withdrawing before the fill window expired.
- `register_solver` checks the *cumulative* bond total against `MIN_BOND`
  instead of each individual deposit, so a solver already above the
  minimum can top up by a smaller amount without being wrongly rejected.
- A solver whose bond falls below `MIN_BOND` after a slash is now
  automatically deactivated, rather than staying eligible to accept
  further intents while under-collateralized.

### Added

- **Storage TTL management**: persistent `Intent`/`Solver` entries and the
  contract instance now have their TTL extended on every write, closing a
  gap where none of Soroban's state-archival requirements were handled.
- **Admin key management**: `set_fee_recipient`, `transfer_admin`
  (requires auth from both the outgoing and incoming admin), and
  `get_admin`/`get_fee_recipient` views -- previously no rotation path
  existed for either role.
- **Emergency pause**: `pause()`/`unpause()`/`is_paused()`, gating
  `submit_intent`/`accept_intent`/`fill_intent` for incident response.
  `slash_solver` and `cancel_intent` stay available throughout.
- **Partial bond withdrawal**: `withdraw_bond(amount)` lets a solver
  reclaim excess collateral above `MIN_BOND` without fully deregistering.
- **Permissionless intent expiry**: `expire_intent()` materializes an
  `Open` intent's `Expired` state once its deadline passes, instead of
  relying on a lazy check inside `accept_intent`.
- **Views**: `get_bond_token`, `get_solver_count` (backed by a new
  `TotalSolvers` stat), `is_solver_eligible`.
- **Destination token allowlist**: `add_allowed_dst_token` /
  `remove_allowed_dst_token` / `is_dst_token_allowed`, enforced in
  `submit_intent` only once an admin opts in via
  `set_dst_allowlist_enabled` (off by default).

### Changed

- CI now also runs a dependency-audit job (`cargo audit` against the
  RustSec advisory database) alongside the existing fmt/clippy/test/build
  checks.

### Documentation

- README: added `stellar contract invoke` usage examples for the core
  intent lifecycle and an up-to-date entrypoint list.
- Filled in missing rustdoc on `unpause`, `is_paused`, and the view
  functions.
