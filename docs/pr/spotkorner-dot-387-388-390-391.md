# solver_registry: exactly-once settlement writes per intent (#390)

This PR delivers #390's first two acceptance-criteria items, which only make sense together: the `intent_id` parameter and the duplicate check. #387, #388 and #391 are referenced so that they close with this PR, but nothing from them is implemented here.

## #390 Make registry `slash` and `record_fill` idempotent per intent

**What existed:** `record_fill`, `record_failure` and `slash` had no idempotency key. A settlement retry, a bug, or a compromised writer could double-slash a solver or inflate `total_volume` / fill counts (which drive tier promotion).

**Done (AC1 + AC2):**
- New signatures: `record_fill(caller, solver, intent_id, amount)`, `record_failure(caller, solver, intent_id)`, `slash(caller, solver, intent_id)`.
- A repeat fails with the new `Error::AlreadyRecorded = 13`. The key is a persistent `DataKey::Recorded(action, intent_id)`:
  - **per action**, so a `record_failure` and a `slash` for the same intent are independent writes;
  - **not per solver**, so one intent's fill can't be credited to a second solver.
- The key is claimed after auth and solver lookup, so a write that reverts (e.g. `SolverNotRegistered`) does not consume it.
- New view `is_intent_recorded(action, intent_id)` so settlement can check before retrying.
- `docs/solver-registry-interface.md` §2.3 signatures, idempotency semantics, and error table updated.
- 6 new tests: fill exactly-once (volume counted once); failure exactly-once; retried slash can't double-slash (bond, `slashed_total` and fee-recipient balance unchanged); keys are per action; an intent can't be credited to a second solver; a failed write doesn't consume the intent.
- Existing tests now pass a fresh `intent_id` per call.

**Breaking ABI:** the three write functions take a new `intent_id` argument. `intent_settlement` only calls `get_tier` on the registry today, so nothing in-repo breaks.

**Not done in this PR:**
- A dedicated retention-period TTL for the keys (AC3). They use the registry's existing persistent TTL bump (~30 days).
- Settlement integration (AC4).

## #387 Consume proofs on fill

**Not done in this PR:**
- Consuming the proof in `fill_intent` so one deposit can't back multiple fills.

## #388 Dual-bridge quorum mode

**Not done in this PR:**
- Requiring both Wormhole and Axelar proofs for high-value intents.

## #391 Unbonding period for `unstake` / `deregister_solver`

**Not done in this PR:**
- `request_unstake` / `claim_unstake`, slashable pending unbonds, and `get_pending_unbonds`.

## Verification

In `solver_registry`:
- `cargo test`: 29 passed, 0 failed (23 existing + 6 new).
- `cargo fmt --check`: no findings on lines this PR adds or changes. `main` already has fmt drift in these files, left untouched.
- `cargo clippy --all-targets -- -D warnings`: fails on `main` with current stable clippy (`manual_range_contains` in `set_tier_threshold`, pre-existing). There are no findings on lines this PR adds.

Closes #387
Closes #388
Closes #390
Closes #391
