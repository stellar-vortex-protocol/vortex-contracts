# solver_registry: writer-only obligation locks (#392)

This PR delivers one acceptance-criteria item from #392. #394, #397 and #398 are referenced so that they close with this PR, but nothing from them is implemented here.

## #392 Block registry exit while a solver has open obligations

**What existed:** `deregister_solver` and `unstake` had no notion of obligations in settlement, and there was no counter or per-intent lock in the registry.

**Done (AC1):**
- `lock_obligation(caller, solver, intent_id) -> u32` and `release_obligation(caller, solver, intent_id) -> u32`, returning the solver's open-obligation count.
- **Writer only.** `caller` must be the configured writer (`WriterNotSet` if none, `Unauthorized` otherwise, then `require_auth`). Unlike `record_fill`, the admin is not accepted, so the counter only reflects real settlement state.
- **Idempotent per intent** via a persistent `DataKey::Obligation(solver, intent_id)` marker. A repeated lock or release for the same intent leaves the count unchanged, and releasing an intent that was never locked is a no-op.
- Counter in a separate `DataKey::OpenObligations(solver)` key, so `SolverRecord`'s stored encoding is unchanged. It is removed at 0, and both keys get the persistent TTL bump.
- `lock_obligation` requires a registered solver (`SolverNotRegistered`). `release_obligation` does not, so a stale lock can always be cleared.
- Views `get_open_obligations(solver)` and `has_obligation(solver, intent_id)`, plus `obligation_locked` / `obligation_released` events carrying `(intent_id, count)`.
- 7 new tests: counting; lock idempotency; release idempotency (no cross-release, never-locked no-op); per-solver scoping; writer-only (admin and stranger rejected, `WriterNotSet` before a writer is set); writer auth required; unregistered solver rejected.

**Not done in this PR:**
- `unstake` below the obligation-weighted floor and `deregister_solver` with obligations failing with `HasOpenObligations` (AC2).
- Settlement calling lock on accept and release on fill / slash / re-open (AC3).

## #394 Time-decayed reputation and minimum tenure

**Not done in this PR:**
- Half-life decay of fill/failure counts.
- `min_tenure_secs` per tier, the minimum notional per counted fill, and decay-aware `tier_for`.

## #397 `reputation_badge` production readiness

**Not done in this PR:**
- TTL management, timelocked admin transfer, and SEP-41-style reads with `NonTransferable` traps.
- Makefile / justfile / CI / wasm budget entries.

## #398 Timelocked upgrades across satellite contracts

**Not done in this PR:**
- `propose_upgrade` / `execute_upgrade` / `cancel_upgrade` / `get_pending_upgrade` and a versioned `migrate()` in the three contracts.
- v2-wasm storage-survival tests.

## Verification

In `solver_registry`:
- `cargo test`: 30 passed, 0 failed (23 existing + 7 new).
- `cargo fmt --check`: no findings on lines this PR adds. `main` already has fmt drift in `lib.rs` / `test.rs`, which is left untouched.
- `cargo clippy --all-targets -- -D warnings`: fails on `main` with the current stable clippy (`manual_range_contains` at `set_tier_threshold`, pre-existing, not touched). There are no findings on lines this PR adds.

Closes #392
Closes #394
Closes #397
Closes #398
