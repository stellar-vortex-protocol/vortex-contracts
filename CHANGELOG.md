# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project has not yet made a versioned release; entries below are grouped
under "Unreleased" and will be cut into a version once `intent_settlement`
first deploys to mainnet.

## [Unreleased]

### Changed

- **Storage layout — `SolverRecord` / `IntentRecord` (issue #187, #188).**
  `SolverRecord` gains a `bond_tokens: Vec<Address>` field enumerating every
  approved token a solver holds a bond in; `bond_amount` is retained as the
  mirror of the *default* token's balance for backward compatibility, with
  non-default balances living under the new `DataKey::SolverBond(solver, token)`
  persistent key. `IntentRecord` gains `bond_token`, `dispute_deadline`,
  `dispute_raised_at`, and `resolution`. New `DataKey` variants:
  `BidWindowEnabled`, `BestBid`, `Arbiter`, `AllowedBondToken`, `SolverBond`,
  `MinBond`. Contracts upgrading from a pre-#187 build read old `SolverRecord`
  values with `bond_tokens` defaulting to empty and the legacy `bond_amount`
  intact; the first `register_solver*` call re-materialises `bond_tokens`.
- **`slash_solver` is now proportional (issue #193).** The flat 10 %-of-bond
  slash is replaced by `min(unfilled_output, bond) / 10`, capped at 10 % of the
  bond and floored at 1 stroop (issue #32). The `solver_slashed` event payload
  is unchanged.
- **`is_bid_window_enabled` no longer reads `DataKey::DstAllowlistEnabled`
  (issue #191).** It now reads a dedicated `DataKey::BidWindowEnabled`, closing
  a storage-key collision where `set_dst_allowlist_enabled` also toggled
  bid-window mode. `set_dst_allowlist_enabled` and `set_bid_window_enabled` are
  fully independent.
- **`fill_intent` de-duplicated.** A bad merge had left the output/fee transfer
  block written three times; it now transfers once, after all state is
  committed (checks-effects-interactions).

### Added

- **Competitive bid window (issue #191).** `bid_intent(solver, intent_id,
  quoted_dst_amount)` records the strictly-highest quote for an intent in
  `Bidding` state (ties keep the incumbent); `settle_bids(intent_id)` is
  permissionless and either promotes the winner to `Accepted` with a fresh fill
  window or re-opens the intent as `Open` when no usable bid exists. Toggle via
  `set_bid_window_enabled`; view via `is_bid_window_enabled` and `get_best_bid`.
- **Dispute-resolution flow (issue #188, docs/dispute-resolution-design.md).**
  New states `Filling`, `Disputed`, `Resolved` and enum `DisputeResolution`.
  `begin_fill` escrows a completing fill in the contract and opens a
  `DISPUTE_WINDOW`; `dispute_fill` lets the user contest it; `resolve_dispute`
  (arbiter-only — `set_arbiter` / `get_arbiter`, defaults to admin) rules
  `Upheld` (proportional slash) or `Dismissed` (fee taken, no slash);
  `release_fill` is the permissionless clean-release / arbiter-timeout path. The
  user receives the escrowed tokens in every outcome.
- **Multi-bond-token support (issue #187, docs/60-multi-bond-token-design.md).**
  `add_allowed_bond_token` / `remove_allowed_bond_token` /
  `set_bond_token_min` / `get_bond_token_min`, plus token-aware
  `register_solver_with_token`, `withdraw_bond_token`, `accept_intent_with_bond`
  and the `get_solver_bond` / `get_solver_bonds` views. Bonds, minimums, and
  slashes are all accounted per token; the slash for an intent is taken from —
  and paid out in — the token the solver bonded when accepting it. Solvers may
  hold bonds in up to `MAX_BOND_TOKENS` (8) distinct tokens;
  `deregister_solver` refunds every one.

### Fixed

- **Compiling, green baseline (#202, #203, #204)**: `intent_settlement` did
  not build — `lib.rs` referenced ~12 undeclared constants, 9 undeclared
  `DataKey` variants and 6 undeclared `Error` variants, the `Error` enum had
  duplicate discriminants, `validate_src_token` called a non-existent
  `String::get`, `compute_reputation_score` was a `pub` contract fn taking a
  non-ABI `&SolverRecord`, and `fill_intent` transferred the fill amount and
  fee three times each. Constants are now declared with rationale comments,
  the enums are reconciled (discriminants renumbered sequentially), the
  string validation reads bytes via `copy_into_slice`, and `fill_intent`
  makes exactly one user transfer and one fee transfer. The test suite and
  the bond-conservation proptest, both damaged by earlier bad merges, are
  repaired.
- **Batch operations were unusable for more than one item**:
  `batch_submit_intent` / `batch_accept_intent` called `require_auth()` once
  per loop iteration, which Soroban rejects with `Auth, ExistingValue` on the
  second item. The batch entrypoints now authorise the actor once and invoke
  un-gated `*_inner` bodies.
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

- **#240** (`proof_registry`): Contract upgrade mechanism (`upgrade` entrypoint
  and `migrate()` guard) matching the pattern in `intent_settlement`. Allows 
  `proof_registry` to evolve without data loss or re-initialization. Migration 
  guard prevents double-execution on the same version.
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
- **Aggregate health view**: `get_protocol_health` bundles `is_paused`,
  `get_stats`, and `get_solver_count` into a single `ProtocolHealth`
  struct so dashboard/monitoring integrations need one call instead of
  three (#112).
- **Destination token allowlist**: `add_allowed_dst_token` /
  `remove_allowed_dst_token` / `is_dst_token_allowed`, enforced in
  `submit_intent` only once an admin opts in via
  `set_dst_allowlist_enabled` (off by default).
- **Timelocked admin actions** (#115, #116): sensitive admin changes now go
  through a propose-then-execute flow with a 48-hour delay, so users and
  solvers have a window to notice and react before a change takes effect.
  A distinct `*_proposed` event fires immediately at proposal time, ahead of
  the delay, giving off-chain monitors advance notice either way.
  - `set_fee_recipient` is superseded by `propose_fee_recipient` /
    `accept_fee_recipient`, now timelocked (`get_pending_fee_recipient`
    returns `(Address, u64 eta)`).
  - `transfer_admin` is superseded by `propose_admin_transfer` /
    `accept_admin_transfer`, now timelocked (`get_pending_admin`).
  - `add_allowed_dst_token` / `remove_allowed_dst_token` are superseded by
    `propose_add_dst_token` / `execute_add_dst_token` and
    `propose_remove_dst_token` / `execute_remove_dst_token` (#118).
    `execute_*` is permissionless once the delay has elapsed, since the
    change was already authorized by the admin at proposal time.
- **Enumerable dst_token allowlist** (#117): `list_allowed_dst_tokens()`
  returns every token currently on the allowlist, so integrators and
  auditors no longer have to replay `dst_token_allowed` /
  `dst_token_disallowed` events to reconstruct the full list.
- **Batch fill / cancel** (#199): `batch_fill_intent(solver, fills)` and
  `batch_cancel_intent(user, intent_ids)` complete the batch API alongside
  the existing `batch_submit_intent` / `batch_accept_intent`. All four are
  capped at `MAX_BATCH_SIZE` and revert the whole batch on any failure.
  `batch_cancel_intent` checks and stamps the per-user `CANCEL_COOLDOWN`
  once for the call, so a user can clear all of their open intents in one
  transaction.
- **Paginated solver enumeration** (#198): `list_solvers(start, limit)`
  returns registered solver addresses a bounded page at a time (limit
  clamped to `MAX_BATCH_SIZE`), kept in sync by `register_solver` /
  `deregister_solver`. Integrators can enumerate solvers without replaying
  `solver_registered` / `solver_deregistered` events.
- **Solana as a fully-supported source chain** (#201): `src_chain =
  "solana"` is validated end-to-end (base58 SPL mint, 32–44 chars, no `0x`
  prefix) and documented alongside the EVM chains; the README's "planned"
  marker is removed.
- **`solver_registry` contract + tier perks** (#197, partial #186): new
  `solver_registry/` crate storing an admin-managed tier per solver
  (Unranked → Platinum) with `get_tier` and the perk-schedule views.
  `intent_settlement` gains `set_solver_registry(Option<Address>)`: when a
  registry is linked, `accept_intent` extends the fill window by the tier's
  bonus (+0 / +10 / +20 / +30 / +50 %) and `slash_solver` slashes at the
  tier's reduced rate (10 / 10 / 8 / 6 / 5 %, 5% floor). The tier is
  snapshotted on the `IntentRecord` at accept-time, so a mid-flight
  promotion/demotion doesn't change the slash. The integration is optional
  and degrades to Unranked when unset or unreachable — behaviour with no
  registry is byte-for-byte the pre-#197 flat 10% slash / fixed window.
  Score-gated promotion, staking and migration remain #186.

### Changed

- CI now also runs a dependency-audit job (`cargo audit` against the
  RustSec advisory database) alongside the existing fmt/clippy/test/build
  checks.
- CI `wasm-size` job now measures the `wasm-opt -Oz` artifact (the size
  that actually deploys) against a pinned binaryen, and `[profile.release]`
  enables `lto`. After #197 the optimized `intent_settlement` wasm is
  ~63.7 KB — within ~1.8 KB of Soroban's 64 KB hard limit — so the budget
  is 64 500 bytes and a dedicated size-reduction pass is now **blocking**
  further feature work.
- Removed the never-functional bid-window scaffolding from
  `intent_settlement` (`BID_WINDOW`, `BestBidRecord`, `is_bid_window_enabled`
  and the dead `Bidding` branch in `submit_intent`); `submit_intent` always
  opened intents as `Open` already. The `IntentState::Bidding` variant is
  retained as reserved.
- CI: new `solver-registry` job (fmt / clippy / test / wasm build) for the
  new crate.

### Documentation

- README: added `stellar contract invoke` usage examples for the core
  intent lifecycle and an up-to-date entrypoint list.
- Filled in missing rustdoc on `unpause`, `is_paused`, and the view
  functions.
- Added `docs/110-monitoring-alerting-spec.md`: signals and thresholds an
  ops team should watch, including slash rate, bond utilization, and
  pause/unpause activity (#110).
- Added `docs/111-expire-intent-event-coverage.md`: confirms and documents
  the gap between the `intent_expired` event and an intent that is merely
  past its deadline but not yet materialized as `Expired` (#111).
- Added `docs/113-event-topic-naming-conventions.md`: documents the
  current event topic conventions in `intent_settlement` and sets the
  naming convention future contracts (e.g. `solver_registry`) should
  follow (#113).
- `docs/132-supported-chains.md`: promoted Solana from "Planned" to
  "Supported" with full base58/decimals rigor (§3.2) and a real SPL-mint
  address table (§4.8); noted that `avalanche`/`bsc` `src_token`s are not
  yet format-checked on-chain (#201).
- `docs/solver-integration-guide.md`: added per-source-chain guidance for
  interpreting `src_token` / `src_amount`, including how to resolve a
  Solana SPL mint and read its (non-uniform) decimals (#201).
- README: Solana row in the decimal-normalization table; batch and
  `list_solvers` entrypoints added to the function list (#198, #199, #201).
- `indexer/reference-indexer.js`: noted `list_solvers` as the on-chain
  alternative to full `solver_registered` / `solver_deregistered` replay
  (#198).
- `docs/solver-registry-design.md`: added an "Implementation status" section
  recording what #197 shipped, the local-tier-table deviation from §6, the
  accept-time snapshot decision, and that the §8 fee rebate is deferred and
  should be unified with #7 (#197).
