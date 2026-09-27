# Design Doc: Multi-Bond Token Support with Per-Token Accounting

**Issue:** [#60](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/60)
(implementation tracked in #187)  
**Branch:** `docs/task-spike`  
**Status:** Implemented in `intent_settlement` (issue #187).

> **Implementation note.** The shipped version is *additive* rather than the
> `bond_amount`-removing schema below: `SolverRecord.bond_amount` is kept as the
> mirror of the default token's `DataKey::SolverBond(solver, default)` entry so
> pre-#187 readers and the bond-conservation proptest keep working unmodified,
> and `SolverRecord` gains `bond_tokens: Vec<Address>` for enumeration.
> `register_solver` / `withdraw_bond` / `accept_intent` keep their original
> signatures (pinned to the default token) and gain `*_with_token` /
> `*_token` siblings for the multi-token path, matching "Option A" in §7.2.
> Per-token minimums are `DataKey::MinBond(token)` (admin-set via
> `set_bond_token_min`), falling back to `ProtocolConfig.min_bond` for the
> default token and `MIN_BOND` otherwise. Error discriminants differ from §6
> to avoid colliding with existing variants: `BondTokenNotAllowed = 40`,
> `TooManyBondTokens = 41`.

---

## 1. Problem Statement

`BondToken` is a single global `Address` set once in `initialize` (stored at
`DataKey::BondToken`). Every solver bonds in the same token (USDC per the
README). `SolverRecord.bond_amount` is a single `i128` field with no token
label.

Supporting additional bond tokens (e.g., USDC, XLM, EURC, a governance
token) would allow solver onboarding with assets other than USDC and reduce
capital concentration risk. This is a significant data-model change that must
be designed before any code is touched.

---

## 2. Goals

1. Allow solvers to post bonds in any admin-approved token.
2. Maintain per-token accounting so slash and withdrawal amounts are computed
   correctly in each token's units.
3. Do not regress existing functionality — current USDC-only solvers must
   continue to work without re-registering.
4. Keep slash semantics (10% of posted bond per token) consistent across tokens.
5. Define a safe migration path for already-registered solvers.

---

## 3. Non-Goals

- Cross-token aggregation for a single "bond value" in USD (requires price
  oracles; deferred).
- Fractional slashing proportional to token price (same reason).
- Letting solvers choose which token gets slashed first (too complex for v1).

---

## 4. Data-Model Changes

### 4.1 New `DataKey` entries

```rust
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    // ... existing keys unchanged ...

    /// Set of approved bond tokens. Value: true (present = approved).
    AllowedBondToken(Address),

    /// Per-solver, per-token bond balance.
    /// Key: (solver_address, token_address) → i128
    SolverBond(Address, Address),
}
```

`DataKey::BondToken` is **retained** as the legacy/default bond token for
backwards compatibility during migration (see §7).

### 4.2 `SolverRecord` changes

Remove `bond_amount: i128`; it is replaced by per-token entries in storage.

```rust
#[contracttype]
#[derive(Clone)]
pub struct SolverRecord {
    pub address: Address,
    // bond_amount: i128  ← REMOVED
    pub fills_completed: u32,
    pub fills_failed: u32,
    pub total_volume: i128,
    pub is_active: bool,
    pub registered_at: u64,
    pub active_intents: u32,
    /// Tokens this solver has ever posted a bond in (for enumeration / UI).
    /// Stored as a Vec<Address>; max 8 entries enforced in register_solver.
    pub bond_tokens: Vec<Address>,
}
```

`bond_amount` is removed from `SolverRecord` because keeping both a
per-record aggregate and the per-token entries would create a consistency
hazard. All reads and writes go through `DataKey::SolverBond`.

### 4.3 `MIN_BOND` per token

`MIN_BOND` is currently a single constant (`50 * 10_000_000` = 50 USDC in
7-decimal units). Different tokens have different decimals and values.

```rust
// Replace the single constant with per-token minimums stored in contract
// instance storage.
DataKey::MinBond(Address),  // Address → i128
```

Admin sets minimum bond amounts via `set_min_bond(token, amount)`. The legacy
`MIN_BOND` constant is used as the default minimum for the original bond token
if no explicit minimum has been set, preserving backwards compatibility.

---

## 5. API Changes

### 5.1 New admin functions

```rust
/// Admin-only: approve a token for use as a solver bond.
pub fn add_allowed_bond_token(env: Env, token: Address)

/// Admin-only: remove a token from the allowed bond set.
/// Existing solvers with bonds in this token keep their funds; they simply
/// cannot add more bonds in this token after removal.
pub fn remove_allowed_bond_token(env: Env, token: Address)

/// Admin-only: set the minimum bond amount for a given token.
pub fn set_min_bond(env: Env, token: Address, amount: i128)

/// Read-only: minimum bond amount for a token (0 if not set).
pub fn get_min_bond(env: Env, token: Address) -> i128
```

### 5.2 Updated `register_solver`

```rust
pub fn register_solver(
    env: Env,
    solver: Address,
    bond_token: Address,   // ← new parameter (was implicit BondToken)
    bond_amount: i128,
)
```

Internal logic:
1. Check `AllowedBondToken(bond_token)` is set; panic `BondTokenNotAllowed` if not.
2. Read `DataKey::SolverBond(solver, bond_token)` (default `0`).
3. Check `existing_bond + bond_amount >= min_bond(bond_token)`.
4. Transfer `bond_amount` from solver to contract.
5. Write `DataKey::SolverBond(solver, bond_token) = existing + bond_amount`.
6. If `bond_token` not already in `solver_record.bond_tokens`, append it (cap at 8).
7. Create or update `SolverRecord` (no `bond_amount` field).

### 5.3 Updated `withdraw_bond`

```rust
pub fn withdraw_bond(
    env: Env,
    solver: Address,
    bond_token: Address,   // ← new
    amount: i128,
)
```

Checks: `remaining >= min_bond(bond_token)` after withdrawal. Remaining can
be zero only when deregistering (see §5.4).

### 5.4 Updated `deregister_solver`

```rust
pub fn deregister_solver(env: Env, solver: Address)
```

Iterates `solver_record.bond_tokens` and returns the full balance of each
token to the solver in a single call. Fails if `active_intents > 0` (unchanged).

### 5.5 Updated `slash_solver`

Slashing is per the token used for the **intent being slashed**. Each
`IntentRecord` must record which bond token backs it.

```rust
// IntentRecord gains:
pub bond_token: Address,  // token that backs this intent's fill guarantee
```

Slash logic:
```rust
let bond = env.storage().persistent()
    .get::<_, i128>(&DataKey::SolverBond(solver_addr.clone(), intent.bond_token.clone()))
    .unwrap_or(0);

let slash_amount = bond / 10;
let new_bond = bond - slash_amount;

env.storage().persistent()
    .set(&DataKey::SolverBond(solver_addr.clone(), intent.bond_token.clone()), &new_bond);

if new_bond < min_bond(intent.bond_token.clone()) {
    solver_record.is_active = false;
}
```

The slash amount is transferred to `FeeRecipient` in `intent.bond_token`.

### 5.6 Updated `accept_intent`

`accept_intent` must record which bond token the solver is using for this
intent. Solver passes a `bond_token: Address` parameter; the contract verifies
`SolverBond(solver, bond_token) >= min_bond(bond_token)`.

```rust
pub fn accept_intent(
    env: Env,
    solver: Address,
    intent_id: BytesN<32>,
    bond_token: Address,   // ← new
)
```

`intent.bond_token` is set to this value.

### 5.7 `is_solver_eligible` update

```rust
pub fn is_solver_eligible(env: Env, solver: Address, bond_token: Address) -> bool
```

Returns `true` iff solver is active AND has at least `min_bond(bond_token)` in
that token.

### 5.8 `get_solver` and views

```rust
/// Return total bond for a specific token.
pub fn get_solver_bond(env: Env, solver: Address, token: Address) -> i128

/// Return all bond tokens and amounts for a solver.
pub fn get_solver_bonds(env: Env, solver: Address) -> Vec<(Address, i128)>
```

---

## 6. New Error Codes

```rust
BondTokenNotAllowed = 22,
TooManyBondTokens   = 23,
```

---

## 7. Migration Strategy

### 7.1 Existing `SolverRecord` structs in storage

Soroban persistent storage is schema-less XDR. An existing `SolverRecord`
written by the old contract code includes a `bond_amount: i128` field. After a
contract upgrade that removes this field, reading the stored XDR will fail
deserialization.

**Chosen approach: lazy migration in `register_solver` / `deregister_solver`.**

A helper `migrate_solver_if_needed` attempts to read the record using the
**old schema** (a separate `LegacySolverRecord` type). If successful, it
writes the converted record under the new schema and creates the corresponding
`SolverBond` storage entry.

```rust
#[contracttype]
#[derive(Clone)]
struct LegacySolverRecord {
    pub address: Address,
    pub bond_amount: i128,   // the field being removed
    pub fills_completed: u32,
    pub fills_failed: u32,
    pub total_volume: i128,
    pub is_active: bool,
    pub registered_at: u64,
    pub active_intents: u32,
}
```

Migration steps (executed once per solver on next interaction):
1. Try to read `DataKey::Solver(solver)` as `LegacySolverRecord`.
2. If successful: write `DataKey::SolverBond(solver, legacy_bond_token)
   = legacy.bond_amount`.
3. Write the new `SolverRecord` (with `bond_tokens = [legacy_bond_token]`,
   no `bond_amount`).
4. Future reads use the new schema.

`legacy_bond_token` is read from `DataKey::BondToken` (the old global config
key, which is preserved during migration).

### 7.2 Backwards compatibility of `register_solver` call signature

Old callers pass `(solver, bond_amount)`. The new signature adds `bond_token`.
Because Soroban contract upgrades are in-place wasm replacements, the ABI
changes immediately. Options:

- **Option A (recommended):** Keep the old `register_solver(solver, amount)`
  as a deprecated alias that infers `bond_token = DataKey::BondToken`. Mark it
  `#[deprecated]` in a doc comment. Remove in a later upgrade.
- **Option B:** Require all callers to update — acceptable for a testnet-only
  protocol at this stage.

The recommendation is Option A for mainnet readiness.

### 7.3 `DataKey::BondToken` after migration

Retained as a read-only legacy reference pointing to the original USDC address.
A new `DataKey::AllowedBondToken(usdc_address)` entry is written during the
upgrade's `migrate` function so USDC is automatically in the allowed set.

---

## 8. Storage Cost Analysis

Each `DataKey::SolverBond(solver, token)` entry is a persistent storage entry.
At 8 tokens per solver and 1,000 registered solvers:

- 8,000 additional persistent entries.
- Each entry: ~100 bytes (key) + 16 bytes (i128 value) ≈ 120 bytes.
- Total: ~960 KB of additional storage state.

Soroban charges per-entry for persistent storage TTL extension. At 8 tokens per
solver, `register_solver` and `slash_solver` each pay for 1 additional
`extend_ttl` call. This is negligible compared to the existing per-intent cost.

---

## 9. Test Cases Required

1. **Single-token baseline** — register/fill/slash with USDC, identical to
   existing tests.
2. **Multi-token registration** — register with USDC + XLM bonds; verify both
   `SolverBond` entries.
3. **Per-token slash** — slash on an intent backed by XLM; verify USDC bond
   unchanged.
4. **Withdrawal below minimum** — should panic `SolverBondTooLow`.
5. **Deregister with multiple tokens** — all token balances returned.
6. **Disallowed bond token** — `register_solver` with an unapproved token panics
   `BondTokenNotAllowed`.
7. **Too many bond tokens** — 9th token panics `TooManyBondTokens`.
8. **Legacy migration** — write an old-schema record, trigger migration via
   `register_solver`, verify new schema in storage.
9. **is_solver_eligible per token** — eligible for USDC, not eligible for XLM
   if XLM bond is zero.

---

## 10. Summary of All Changed Symbols

| Symbol | Change |
|--------|--------|
| `DataKey::BondToken` | Kept (legacy) |
| `DataKey::AllowedBondToken(Address)` | New |
| `DataKey::SolverBond(Address, Address)` | New |
| `DataKey::MinBond(Address)` | New |
| `SolverRecord.bond_amount` | Removed; replaced by `SolverBond` storage |
| `SolverRecord.bond_tokens` | New field |
| `IntentRecord.bond_token` | New field |
| `register_solver` | Add `bond_token` param |
| `withdraw_bond` | Add `bond_token` param |
| `accept_intent` | Add `bond_token` param |
| `slash_solver` | Slash in `intent.bond_token` |
| `is_solver_eligible` | Add `bond_token` param |
| `add_allowed_bond_token` | New |
| `remove_allowed_bond_token` | New |
| `set_min_bond` | New |
| `get_min_bond` | New |
| `get_solver_bond` | New |
| `get_solver_bonds` | New |
| `Error::BondTokenNotAllowed` | New (22) |
| `Error::TooManyBondTokens` | New (23) |

---

*Closes #60*
