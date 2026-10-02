# Archival-Safety Audit — Persistent Storage Keys

**Status:** Fixed and tested  
**Related issues:** #145, #371, #153, #244, #272, #361  
**Files changed:**
- `intent_settlement/src/lib.rs` — TTL bumps added to 3 write paths
- `intent_settlement/src/test_archival.rs` — new archival-safety test suite

---

## 1. Background

Soroban archives persistent ledger entries whose TTL reaches zero.  Once
archived, an entry is **absent** from the network's active state — any
transaction that attempts to read it without a `RestoreFootprintOp` fails
at the host level before contract code runs.

The dangerous pattern is any place where the contract **substitutes a
default value** for a missing entry, because on-network an archived entry
and an entry that was never written are indistinguishable from naive
`has()` / `get().unwrap_or(default)` calls.

### Test-environment behaviour (Soroban SDK v21)

The SDK test host **auto-restores** any archived persistent entry that
appears in a transaction's footprint — the restore is transparent, only
increasing `write_bytes` / `write_entries` in resource estimates.

This means unit tests cannot directly test "archived entry behaves like
absent entry."  Instead the test suite:
1. Verifies every guard key is TTL-bumped to `PERSISTENT_TTL_EXTEND_TO`
   on write (prevents archival before the parent record it guards).
2. Simulates TTL expiry via `sequence_number` advances and confirms the
   contract never silently substitutes a default after SDK auto-restoration.

---

## 2. Key inventory and risk classification

| Key | Storage tier | Risk without TTL bump | Mitigation |
|---|---|---|---|
| `Intent(id)` | Persistent | `IntentNotFound` panic | ✅ bumped by `save_intent` on every write |
| `IntentTombstone(id)` | Persistent | ID reuse after close_intent | ✅ bumped by `close_intent` on creation |
| `Solver(addr)` | Persistent | `SolverNotRegistered` panic | ✅ bumped by `bump_solver_ttl` on every write |
| `SolverBond(addr,token)` | Persistent | Silently reads as 0 → solver appears unbonded | ✅ bumped alongside `Solver` record on every write |
| `IntentFillHistory(id)` | Persistent | Fill log lost before `close_intent` cleanup | ⚠️ see §3.1 |
| `UserIntents(addr, bucket)` | Persistent | Bucket hidden from pagination | ✅ bumped by `user_intents_append` on every write |
| `UserIntentCount(addr)` | Persistent | Count reset → wrong bucket navigation | ✅ bumped by `user_intents_append` on every write |
| `CancelCooldown(addr)` | Persistent | Cooldown silently cleared → cancel spam | ✅ **fixed** — TTL bump added to `stamp_cancel_cooldown` |
| `AmendmentCooldown(addr)` | Persistent | Cooldown silently cleared → amendment spam | ✅ **fixed** — TTL bump added to `stamp_amendment_cooldown` |
| `ExtensionGranted(id)` | Persistent | One-shot flag archived → double extension | ✅ **fixed** — TTL bump added to `request_extension` write path |
| `MinBondMultiplier(token)` | Persistent | Silently reverts to 1.0× | ✅ pre-existing `bump_min_bond_multiplier_ttl` call confirmed |
| `SolverReputation(addr)` | Persistent | Reputation snapshot lost → anti-Sybil bypass | ⚠️ see §3.2 |
| `SolverIntents(addr)` | Persistent | Duplicate insertion possible | ✅ bumped by `solver_intents_add` on every write |
| `SolverIntentIdx(addr,id)` | Persistent | Idx absent → duplicate detection fails | ✅ bumped by `solver_intents_add` on every write |
| `BestBid(id)` | Persistent | Bid lost on archival | ℹ️ `extend_ttl` already called at creation; cleaned by `close_intent` |
| `ConsumedNonce(pubkey,nonce)` | Persistent | Replay of gasless intent | ⚠️ see §3.3 |

---

## 3. Open gaps

### 3.1 `IntentFillHistory` not explicitly TTL-bumped on append

The `IntentFillHistory(id)` entry is written by `fill_intent` (partially
implemented — the key exists in `DataKey` and is deleted by `close_intent`,
but the append write path is not yet in the codebase).

**Risk:** If the history entry is archived before `close_intent` deletes it,
the `remove()` call is a no-op on a non-existent entry (safe, no panic), but
any on-chain fill-history data would be silently lost.  This is a
**data-availability** risk, not a security bypass.

**Fix required when fill-history writes are added:** call `extend_ttl` after
every `push_back` to the history entry, using the same
`PERSISTENT_TTL_THRESHOLD` / `PERSISTENT_TTL_EXTEND_TO` schedule as the
parent `Intent` record.

### 3.2 `SolverReputation` snapshot not yet implemented

`DataKey::SolverReputation(addr)` is declared and documented as a snapshot
written by `deregister_solver` and read on re-registration to preserve slash
history and fill ratios across deregister/re-register cycles (#272).

The write path does not yet exist in `deregister_solver`; the read path does
not yet exist in `register_solver_inner`.

**Risk once implemented:** if the snapshot is written without a TTL bump and
the solver waits months before re-registering, the snapshot may be archived —
the fill ratio and slash history resets to zero on re-registration, defeating
anti-Sybil protection.

**Fix required before implementing #272:** call `extend_ttl` after writing
`SolverReputation` in `deregister_solver`, and ensure `register_solver_inner`
bumps the key's TTL again before reading it (or just reads + deletes on the
same call, which triggers auto-restoration).

### 3.3 `ConsumedNonce` anti-replay keys

`DataKey::ConsumedNonce(pubkey, nonce)` marks gasless intents already
processed (#361).  If archived, the `has()` check returns false and the
same signed intent could be replayed.

**Risk:** Double-spend of gasless intent via nonce archival.  This is
**critical** for the replay-prevention guarantee.

**Fix required when gasless intent feature is completed:** bump
`ConsumedNonce` to `PERSISTENT_TTL_EXTEND_TO` on write.  Because replay
replay keys must live at least as long as the signature they guard is
still meaningful, consider bumping to `INSTANCE_TTL_EXTEND_TO` (60 days)
or tying the TTL to the intent's `user_deadline`.

---

## 4. Fixes applied in this PR

### 4.1 `stamp_cancel_cooldown` — TTL bump added

```rust
// Before (unsafe):
fn stamp_cancel_cooldown(env: &Env, user: &Address, now: u64) {
    env.storage().persistent().set(&DataKey::CancelCooldown(user.clone()), &now);
}

// After (archival-safe):
fn stamp_cancel_cooldown(env: &Env, user: &Address, now: u64) {
    let key = DataKey::CancelCooldown(user.clone());
    env.storage().persistent().set(&key, &now);
    env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO);
}
```

Without the bump, a cooldown entry written at ledger L and never touched
again would be archived after `min_persistent_entry_ttl` ledgers
(~500 ledgers on mainnet at the time of writing), which is around 42 minutes
at 5 s/ledger.  `CANCEL_COOLDOWN` is 60 seconds — well within the archival
window — but the *next* cooldown stamp would only last until the entry
archived again.  With the bump the entry lives for 30 days, matching the
intent record's lifetime.

### 4.2 `stamp_amendment_cooldown` — TTL bump added

Same pattern as cancel cooldown.  Amendment cooldown is also 60 seconds,
same archival risk.

### 4.3 `ExtensionGranted` write in `request_extension` — TTL bump added

```rust
// Before (unsafe):
env.storage().persistent().set(&DataKey::ExtensionGranted(intent_id.clone()), &new_used);

// After (archival-safe):
let ext_key = DataKey::ExtensionGranted(intent_id.clone());
env.storage().persistent().set(&ext_key, &new_used);
env.storage().persistent().extend_ttl(&ext_key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO);
```

The `ExtensionGranted` flag is a one-shot guard: once set, `request_extension`
must reject further calls on the same intent.  If archived, the `has()` check
returns false and the solver gets an extra extension, effectively getting
double the fill window at no cost.  The bump ensures the flag outlives the
intent it guards.

---

## 5. Why the test environment auto-restores

From the Soroban SDK v21 documentation:

> Persistent entries are more subtle: when a transaction executed on-chain
> contains a persistent entry that has been archived in the footprint, the
> entry will be automatically restored.  Automatic restoration is mostly
> transparent; the main side effect is increased fees.

This means `test_archival.rs` cannot simulate the on-network failure mode
(where the call is rejected at the host level before contract code runs).
Instead, the tests:

1. Prove TTL bounds hold via `get_ttl` assertions immediately after writes.
2. Confirm the contract never *silently* substitutes a default after SDK
   auto-restoration (loud failure = `IntentNotFound` panic, which is safe).
3. Confirm cooldown / one-shot flag enforcement survives modest ledger
   sequence advancement within the bumped TTL window.

The full on-network failure is tested by integration tests (off-chain
restore automation — out of scope per this task).

---

## 6. Relationship to docs/145-ttl-bump-frequency-review.md

The existing `docs/145-ttl-bump-frequency-review.md` reviewed the
**frequency** of TTL bumps on hot paths (Intent, Solver) and concluded
the unconditional `extend_ttl(threshold, extend_to)` pattern is correct
and cheap.

This document covers a different gap: **coverage** — which persistent keys
were written but *not* bumped at all.  The three fixes in §4 address
`CancelCooldown`, `AmendmentCooldown`, and `ExtensionGranted`.  The open
gaps in §3 cover entries whose write paths are not yet implemented.
