# TTL Bump Frequency — Unconditional `extend_ttl` Review

**Issue:** [#145](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/145)
**Status:** Reviewed — no change recommended
**Scope:** `bump_instance_ttl` / `bump_intent_ttl` / `bump_solver_ttl`
(`intent_settlement/src/lib.rs`), called on ~25 write paths.

---

## 1. The concern

`bump_instance_ttl`, `bump_intent_ttl`, and `bump_solver_ttl` each call
`extend_ttl` unconditionally on every write, e.g.:

```rust
env.storage().persistent().extend_ttl(
    &DataKey::Intent(intent_id.clone()),
    PERSISTENT_TTL_THRESHOLD,   // DAY_IN_LEDGERS * 14
    PERSISTENT_TTL_EXTEND_TO,   // DAY_IN_LEDGERS * 30
);
```

The worry: paying to bump TTL even when the entry has ~30 days of TTL left
and is nowhere near the 14-day threshold.

## 2. What `extend_ttl(threshold, extend_to)` actually does (soroban-sdk 21)

The two-argument form is already conditional **inside the host**:

- The host reads the entry's current `live_until_ledger`.
- If `live_until_ledger - current_ledger > threshold` (TTL still healthy),
  it **returns without doing anything else** — no ledger write is emitted
  for the entry, and no rent is charged.
- Only when the remaining TTL has decayed **below `threshold`** does it
  extend `live_until_ledger` to `current_ledger + extend_to` and charge
  rent for the added ledger range.

So in the common case (entry written again well before 14 days elapse) the
cost of each `bump_*` call is: **one host-function invocation performing a
subtract-and-compare.** No write, no rent.

## 3. Would a guest-side pre-check help?

A pre-check would be:

```rust
let ttl = env.storage().persistent().get_ttl(&key); // host call
if ttl < PERSISTENT_TTL_THRESHOLD {
    env.storage().persistent().extend_ttl(&key, ..., ...); // host call
}
```

`get_ttl` is itself a host-function call reading the same
`live_until_ledger` field the host already inspects inside `extend_ttl`. In
the common path this **replaces one cheap host call with one cheap host
call** and adds branch logic; in the uncommon path it makes **two** host
calls instead of one. Net negative.

For `bump_instance_ttl` the instance entry is already loaded into the
transaction footprint on every call, so the `extend_ttl` comparison is
running against data the host holds anyway.

## 4. Recommendation

**No change.** The unconditional `extend_ttl(threshold, extend_to)` pattern
is the idiomatic Soroban usage precisely because the host already
short-circuits below-threshold. A guest-side threshold gate would add code
and, in the common case, no saving — in the rare case, an extra host call.
The current constants and their rationale are documented in
[docs/ttl-constants-rationale.md](ttl-constants-rationale.md).
