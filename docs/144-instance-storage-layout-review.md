# Instance-Storage Layout — Single Entry vs. Split Entries

**Issue:** [#144](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/144)
**Status:** Reviewed — no split recommended; before/after benchmark blocked
on the resource-benchmarking harness (see
[docs/149](149-resource-cost-per-entrypoint.md))

---

## 1. Soroban's instance-storage model (confirmed)

All of a contract's **instance** storage is a single `ScMap` held in **one
ledger entry** — the `ContractData` entry with `durability = instance`,
which also carries the reference to the contract's executable. This is
already stated in
[docs/ttl-constants-rationale.md](ttl-constants-rationale.md): *"a single
ledger entry that holds the contract's global state (`Admin`,
`FeeRecipient`, `BondToken`, protocol stats) and the contract's own
executable code."*

Therefore the issue's premise is **correct**: `Admin`, `FeeRecipient`,
`BondToken`, `TotalIntents`, `OpenIntents`, `TotalVolume`, `TotalSolvers`,
`Paused`, `DstAllowlistEnabled`, `ProtocolConfig`, … all live in that one
map, and any `env.storage().instance().set(...)` re-serializes and
re-writes the **entire** map. `submit_intent` bumping `TotalIntents`
rewrites every other instance key alongside it.

## 2. Why splitting the hot counters is still not worth it

Splitting `TotalIntents` / `TotalVolume` into their own entries means
**persistent** storage (instance storage cannot have per-key entries). For
each such counter, per call that touches it:

| | Current (in instance map) | Split into a persistent entry |
|---|---|---|
| Read | free — instance entry already in footprint | **extra** persistent read (own footprint slot) |
| Write | re-serialize the instance map (~a few hundred bytes of small scalars) | write a tiny dedicated entry … |
| TTL | covered by the existing `bump_instance_ttl` | … **plus** its own `extend_ttl` + rent |

The instance map here is ~a dozen small scalars and addresses. Re-writing
it is a small "write bytes" cost. Moving a counter out trades that for a
whole extra ledger entry with its own read, write, and TTL lifecycle —
**strictly more ledger I/O per call**, not less.

Splitting a combined entry only pays off when the entry is large enough
that re-serializing the *unrelated* fields dominates. That is not the case
here.

## 3. Context: the counter update is already in the noise

Every `submit_intent` also performs a **persistent `IntentRecord` write**
plus a `UserNonce` read/write. Those persistent operations dwarf the cost
of re-writing the small instance map. Optimising the instance-entry write
would not move the needle on the entrypoint's total cost.

## 4. Benchmark

A before/after measurement (the issue asks for one via the
resource-benchmarking harness) cannot be run: the harness does not exist in
this repo — same blocker as [docs/149](149-resource-cost-per-entrypoint.md).

## 5. Recommendation

**Do not split.** Keep the single instance entry. This doc records the
storage model (one entry, full rewrite on every instance write) so the
trade-off is on file. Revisit only if either becomes true:

- the instance map grows large (many big values), so unrelated-field
  re-serialization becomes the dominant write cost; or
- a counter becomes extremely hot on a path that does **not** already do a
  persistent write, so a dedicated entry would not be adding an otherwise
  absent persistent operation.
