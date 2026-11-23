# Vortex Storage Migration Framework (Issue #353, rewritten for #688)

> **Status.** This document replaces the earlier single-contract design. That
> design (one `migrate()` over one `DataKey` namespace, a shared
> `CURRENT_SCHEMA_VERSION`, a `MigrationError` type and a `schema_version`
> field on every record) was never implemented and does not apply to the
> current layout. Those steps are **obsolete and removed**. This is a written
> procedure only; no migration code is specified here.

## 1. Layout: independent contracts, independent storage

The workspace (`Cargo.toml`) is a set of separate contracts. Each is deployed
as its own contract instance and owns its own **instance** storage (one
ledger entry, see
[144-instance-storage-layout-review.md](144-instance-storage-layout-review.md))
and its own **persistent** entries. There is no storage namespace shared
between contracts: a key such as `Admin` in `intent_settlement` and `Admin`
in `solver_registry` are unrelated entries in unrelated contracts. Contracts
refer to each other only by stored addresses (for example
`intent_settlement` `SolverRegistry`, `ProofRegistry`, `BackstopVault`;
`reputation_badge` `SolverRegistry`; `intent_periphery` `Settlement`).

Consequences for migration:

- A migration is planned, ordered and executed **per contract**. Upgrading
  one contract never rewrites another contract's storage.
- Soroban cannot iterate persistent storage, so persistent records cannot be
  bulk-rewritten. Reshaping a persistent record type needs a read-time
  (lazy) upcast in that contract, or a new key.
- Only contracts that store a key are able to migrate it; cross-contract
  state (an address another contract points at) is migrated by re-pointing
  that address through the owning contract's own admin entrypoint.
- Note: `docs/MODULE_REFACTORING_PLAN.md` records that the split of
  `intent_settlement/src/lib.rs` into modules was **not** pursued; the
  "multiple contracts" in this document means the separate crates, not
  submodules of one crate.

## 2. What exists in code today

| Mechanism | Where | Notes |
|---|---|---|
| `propose_upgrade(new_wasm_hash)` / `execute_upgrade(new_wasm_hash)` / `get_pending_upgrade()` | `intent_settlement/src/lib.rs` ~1917-1957 | Timelocked WASM upgrade. Pending proposal is `(hash, eta)` under instance key `PendingUpgrade`. `execute_upgrade` removes it and calls `update_current_contract_wasm`. |
| `migrate()` | `intent_settlement/src/lib.rs` ~1971 | Admin-only, run-once-per-release hook guarded by instance key `MigrationVersion` against `MIGRATION_VERSION` (`1`). Panics `AlreadyMigrated` if already current. Body is currently a no-op. Emits `("migrated",)` with `(old, new)`. |
| `CURRENT_SCHEMA_VERSION` | `vortex-common/src/lib.rs` ~205 | Constant (`1`) only. **No contract reads it** and no record carries a schema-version field today. |
| `initialize` seeding / `load_config` defaults | `intent_settlement` | `Config` falls back to `DEFAULT_*` constants when absent, which is how config fields added after deployment stay safe. |

No other crate (`solver_registry`, `proof_registry`, `reputation_badge`,
`fee_router`, `governor`, `timelock`, `treasury_streams`, `backstop_vault`,
`arbiter_panel`, `intent_periphery`, `directory`) defines `migrate` or an
upgrade entrypoint at the time of writing. Their upgrade/migration path must
therefore be a redeploy plus re-pointing (section 5) unless an upgrade
entrypoint is added by a separate change.

## 3. Storage keys owned by each contract

Tier is the storage durability the code uses for the key (instance =
single contract-wide entry; persistent = own entry with its own TTL). Source
for every key is the key enum in the named file; line numbers are the enum
declaration and are approximate for the sub-groups.

### intent_settlement (`intent_settlement/src/lib.rs`, `DataKey` at line 464)

> Pre-existing issue, not addressed here: `lib.rs` currently declares several
> `DataKey` variants and constants more than once (for example
> `SolverIntents`, `SolverBond`, `MigrationVersion`, `TotalBonded`,
> `MIGRATION_VERSION`), and the workspace does not build on `main`. The
> tables below list each key once.

Instance:
`Admin`(470), `FeeRecipient`(476), `PendingFeeRecipient`(480),
`PendingAdmin`(485), `Config`(491), `BondToken`(493), `Pauser`(635),
`ProofRegistry`(643), `SolverRegistry`(672), `BackstopPool`(693),
`BackstopVault`(713), `BackstopVaultShareBps`(715), `Arbiter`(689),
`OracleAddr`(703), `PendingOracle`(705), `PolicyContract`(741),
`PendingPolicy`(745), `PendingUpgrade`(681), `MigrationVersion`(682),
counters `TotalIntents`(527), `OpenIntents`(544), `TotalVolume`(548),
`TotalFeesCollected`(555), `TotalSolvers`(560), `TotalBonded`(658),
`Paused`(566), `BidWindowEnabled`(685), `UserNonce(Address)`(613),
`IntentRetention`(802), allowlists and their pending proposals
(`AllowedDstToken`, `DstAllowlistEnabled`, `PendingDstTokenAdd`,
`PendingDstTokenRemove`, `AllowedSrcChain`, `SrcChainAllowlistEnabled`,
`AllowedBondToken`, `MinBond`, `TokenVolume`, `TokenFees`,
`PendingChainOnboarding`, `ChainConfig`).

Persistent:
`Intent(id)`(494), `IntentFillHistory(id)`(500), `IntentTombstone(id)`(797),
`Solver(Address)`(501), `SolverBond(solver, token)`(647),
`SolverRoutes(Address)`(675), `SolverReputation(Address)`(678),
`SolverExposure(Address)`(699), `SolverIntents(Address)`(655),
`SolverIntentIdx(solver, id)`(790), `BestBid(id)`(686), `BackstopClaimed(id)`(695),
`UserIntents(Address, u32)`(597), `UserIntentCount(Address)`(603),
`CancelCooldown(Address)`(607), `AmendmentCooldown(Address)`(851),
`ExtensionGranted(id)`(611), `MinBondMultiplier(Address)`(590),
`OutboundIntent(id)`(855), `UserOutboundIntents(Address)`(859), paged
solver/dst-token lists (`SolverListPage`, `SolverListCount`, `SolverListIdx`,
`DstTokenListPage`, `DstTokenListCount`, `DstTokenListIdx`; 763-784).

This is the only contract with a `migrate()` hook and a timelocked upgrade.
`Intent` and `Solver` records are the shapes most likely to be reshaped;
add new fields as new keys or `Option` fields with read-time defaults, since
persistent records cannot be iterated.

### solver_registry (`solver_registry/src/lib.rs`, `DataKey` at line 105)
Instance: `Admin`, `PendingAdmin`, `BondToken`, `FeeRecipient`, `Writer`,
`PendingWriter`, `Thresholds`, `TotalSolvers`.
Persistent: `Solver(Address)`, `Recorded(Symbol, BytesN<32>)`,
`Obligation(Address, BytesN<32>)`, `OpenObligations(Address)`.

### proof_registry (`proof_registry/src/lib.rs`, `ProofKey` at line 121)
Instance: `Admin`, `WormholeCore`, `AxelarGateway`, `Configurator`,
`AuthorizedEmitter(u32)`, `AuthorizedAxelarSource(Symbol)`.
Persistent: `Proof(BytesN<32>)`, `SeenVaa(u32, u64)` (replay protection;
must be preserved across any change so already-consumed VAAs stay consumed).

### reputation_badge (`reputation_badge/src/lib.rs`, `BadgeKey` at line 49)
Instance: `Admin`, `SolverRegistry`. Persistent: `Badge(Address)`.

### fee_router (`fee_router/src/lib.rs`, `DataKey` at line 114)
Instance: `Governance`, `WeightDelay`, `Sinks`, `Pending`.
Persistent: `Balance(Address)`, `Claimable(Address, Address)`.

### treasury_streams (`treasury_streams/src/lib.rs`, `DataKey` at line 10)
Instance: `Governor`, `PendingGovernor`, `NextId`, `StreamCount`, `Paused`.
Persistent: `Stream(u64)`, `SenderStreams(Address)`, `RecipientStreams(Address)`.

### backstop_vault (`backstop_vault/src/lib.rs`, `DataKey` at line 79)
Instance: `Admin`, `BondToken`, `Settlement`, `TotalShares`, `TotalAssets`,
`TotalQueued`, `TotalCoverPaid`, `TotalIncome`, `EmergencyMode`, `Config`.
Persistent: `Shares(Address)`, `WithdrawCooldown(Address)`,
`WithdrawQueue(Address)`.

### arbiter_panel (`arbiter_panel/src/lib.rs`, `DataKey` at line 56)
Instance: `Admin`, `Settlement`, `BondToken`, `BondAmount`, `PanelSize`,
`Threshold`, `Arbiters`. Persistent: `Arbiter(Address)`, `Round(BytesN<32>)`.

### intent_periphery (`intent_periphery/src/lib.rs`, `DataKey` at line 74)
Instance: `Admin`, `Settlement`, `PendingSettlement`.

### directory (`directory/src/lib.rs`, `DataKey` at line 22)
Instance: `Admin`, `PendingAdmin`, `Names`. Persistent-style per-name
entry: `Entry(Symbol)`.

### governor and timelock (Symbol-keyed, no key enum)
`governor/src/lib.rs`: instance `"config"`, `"admin"`, `"count"`; persistent
per-proposal, per-vote and per-checkpoint tuple keys built from
`("proposal", id)`, `("vote", ...)`, `("checkpoint", ...)` (see the key
helpers near lines 682-697).
`timelock/src/lib.rs`: instance `MinDelay`, `Grace` and the admin/role
symbols (`Admin`, `Proposer`, `Executor`, `Canceller`, lines 85-94);
persistent operations keyed `(Op, id)` and roles keyed `(Role, ...)`.

## 4. Per-contract migration procedure

Repeat for each contract being upgraded; do not batch across contracts.

1. **Scope.** List the keys from section 3 whose stored type changes. Keys
   whose type is unchanged need no action.
2. **Classify each change.**
   - *Instance key, new or changed value:* write it in an admin-gated step
     (for `intent_settlement`, inside `migrate()`; the instance map is one
     entry, so this is a single bounded write). Reads must keep a default
     for the not-yet-migrated case (`unwrap_or`), as `load_config` does.
   - *Persistent record, reshaped:* cannot be bulk-migrated. Either add the
     new data under a **new key**, or make the reader tolerate both shapes
     and rewrite on next touch. Keep the old key readable until every
     record has been touched or expired.
   - *Key removed:* leave it in place; unreferenced entries expire with
     their TTL.
3. **Version guard.** `intent_settlement` guards a one-time backfill with
   `MigrationVersion` / `MIGRATION_VERSION` and `migrate()`: bump the
   constant, put the backfill in `migrate()`, and the hook refuses to run
   twice (`AlreadyMigrated`). Other contracts have no such guard today; add
   one in the same change as the first reshaping, using that contract's own
   key.
4. **Upgrade sequence (`intent_settlement`).** `propose_upgrade(hash)` ->
   wait out the timelock -> `execute_upgrade(hash)` -> `migrate()` ->
   verify via views and the `upgraded` / `migrated` events.
5. **Upgrade sequence (other contracts).** There is no in-place upgrade
   entrypoint; deploy the new WASM as a new instance, initialize it, and
   re-point dependents (section 5).
6. **Verify.** Read back the migrated keys, confirm counters and totals,
   and re-run the relevant crate tests before announcing.

## 5. Cross-contract dependencies when re-pointing

Update dependents through their own admin entrypoints, in this order, so no
contract points at an uninitialized one:

- `solver_registry` is referenced by `intent_settlement` (`SolverRegistry`)
  and `reputation_badge` (`SolverRegistry`).
- `proof_registry` is referenced by `intent_settlement` (`ProofRegistry`).
- `backstop_vault` and `arbiter_panel` hold the `intent_settlement` address
  as `Settlement`; `intent_periphery` holds it as `Settlement` (with a
  two-step `PendingSettlement`).
- `intent_settlement` holds `BackstopVault`, `Arbiter`, `PolicyContract`.

## 6. Safety properties

- **Idempotent where guarded:** `intent_settlement::migrate()` cannot run
  twice for one `MIGRATION_VERSION`.
- **Isolated:** one contract's migration cannot corrupt another's storage.
- **TTL:** after a migration, bump instance TTL (as `migrate()` does) and
  persistent TTLs on touch; see
  [ttl-constants-rationale.md](ttl-constants-rationale.md).

## 7. Out of scope

Migration code, generated schemas, and adding upgrade entrypoints to the
contracts that lack one.
