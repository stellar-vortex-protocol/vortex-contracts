# Resource Cost per Entrypoint (Solver Gas Estimation)

**Issue:** [#149](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/149)
**Status:** Populated — first snapshot from the `bench` harness (issue #195)
**Harness:** `intent_settlement/src/bench.rs`

---

## 1. Purpose

Solver bots decide whether filling an intent is profitable before submitting
a transaction. That decision needs an estimate of the transaction's resource
cost so the bot can convert resource usage into an expected fee without
running the Soroban resource simulator for every candidate intent.

This document publishes the first real measurements and the methodology used
to produce them, so the numbers can be regenerated after future contract
changes (resource costs drift as the code does).

## 2. Methodology

The numbers come from `intent_settlement/src/bench.rs`, a `#[cfg(test)]`
harness that:

1. builds an isolated fixture per entrypoint (`Env::default()` +
   `mock_all_auths()`, a fresh contract instance, freshly registered
   solver/intent as needed),
2. calls `env.budget().reset_default()`,
3. invokes the single entrypoint under measurement,
4. reads `Budget::cpu_instruction_cost()` and
   `Budget::memory_bytes_cost()`.

Regenerate with:

```text
cd intent_settlement
cargo test --features testutils bench::resource_cost_report -- --nocapture
```

`bench::resource_cost_is_reproducible` is a smoke test that runs the same
measurement twice and asserts the two results are byte-for-byte identical, so
the published figures are stable run to run for a given toolchain + SDK
version.

### Caveats (read before using these for fee bids)

- **Native, not Wasm.** The SDK executes the contract as native Rust in
  tests. Per the SDK's own documentation, CPU-instruction and memory figures
  are **approximate and generally an underestimate** of on-chain cost. Use
  them as a consistent *relative ranking* between entrypoints and a lower
  bound, not as a fee quote. For an authoritative per-transaction cost, run
  `stellar contract invoke --cost …` against the built Wasm on a network.
- **Ledger entry read/write counts** are not exposed by the `soroban-sdk`
  21 testutils `Budget`. Getting them requires the on-chain simulator or
  `soroban-sdk >= 22`'s `Env::cost_estimate`. The record-size table in
  section 4 covers the write-bytes dimension that matters most for cost
  (and for issue #196).
- Token transfers inside `fill_intent`, `register_solver`, `withdraw_bond`,
  `deregister_solver`, and `slash_solver` invoke the Stellar Asset Contract;
  that cost is included in the row.
- **Toolchain / SDK pinning.** Numbers below were taken with
  `soroban-sdk 21.7.7` on stable Rust. A different SDK patch or `rustc`
  version will shift them; rerun the harness after bumping either.

## 3. Per-entrypoint cost

| Entrypoint | CPU instructions | Memory bytes |
|---|--:|--:|
| `submit_intent` | 281,113 | 39,630 |
| `accept_intent` | 297,608 | 47,422 |
| `fill_intent` (full fill — closes the intent) | 622,328 | 96,723 |
| `fill_intent` (partial fill — re-opens the intent) | 642,020 | 97,463 |
| `cancel_intent` | 239,820 | 39,480 |
| `expire_intent` | 204,451 | 32,082 |
| `slash_solver` | 443,049 | 65,189 |
| `request_extension` | 176,128 | 32,790 |
| `register_solver` (first registration) | 342,082 | 51,837 |
| `register_solver` (top-up of an existing bond) | 311,498 | 44,278 |
| `withdraw_bond` | 313,992 | 44,895 |
| `deregister_solver` | 332,088 | 48,280 |

`fill_intent` also resolves the caller's volume-tier fee discount (#192): with
no schedule set this is one extra instance read; with a schedule it also loads
the `SolverRecord` to read `total_volume`.

Notes:

- **`fill_intent` is the most expensive solver call by ~2x** — it does two
  token transfers (output to user, fee to recipient), rewrites the full
  `IntentRecord` and `SolverRecord`, and updates instance stats.
- The **partial-fill path costs slightly more than the full-fill path**: it
  re-opens the intent (resets solver/deadline, bumps `OpenIntents` back up)
  instead of closing it out.
- `slash_solver` is the next most expensive — one token transfer plus a full
  rewrite of both records.

### Multi-item throughput (per-item)

Cost of N sequential `submit_intent` / `accept_intent` calls:

| Sequence | CPU total | CPU / item | Mem total | Mem / item |
|---|--:|--:|--:|--:|
| `submit_intent` ×1 | 281,113 | 281,113 | 39,630 | 39,630 |
| `submit_intent` ×5 | 1,529,308 | 305,861 | 217,634 | 43,526 |
| `submit_intent` ×10 | 3,226,891 | 322,689 | 477,039 | 47,703 |
| `accept_intent` ×1 | 297,608 | 297,608 | 47,422 | 47,422 |
| `accept_intent` ×5 | 1,528,447 | 305,689 | 254,574 | 50,914 |
| `accept_intent` ×10 | 3,235,890 | 323,589 | 565,039 | 56,503 |

Per-item cost is roughly flat (a mild upward drift from the growing
`UserIntents` vector on `submit_intent`).

## 4. Persistent record sizes

Serialised XDR size of the two records rewritten on the hot paths, read back
from storage after `accept_intent`:

| Record | Serialised size |
|---|--:|
| `IntentRecord` | 624 bytes |
| `SolverRecord` | 340 bytes |

`accept_intent` and both `fill_intent` paths rewrite the **entire**
`IntentRecord` (624 bytes) even though only a few fields change, plus the
full `SolverRecord` (340 bytes). Trimming this is issue #196 — splitting the
write-once `src_chain` / `src_token` (~110 bytes) into a separate entry that
state transitions never touch cuts the per-transition write by ~8%; a deeper
split cuts more. It is deferred until the wasm-size budget has room for the
extra `#[contracttype]` codegen (the contract currently sits ~120 bytes under
the 65,536-byte limit).

## 6. Follow-up

- A CI job that regenerates this table on every change is intentionally out
  of scope here (separate DevOps issue); for now, rerun the harness manually
  after any change to `lib.rs` storage shape or the SDK version and update
  sections 3–4.
- Issue #196 (the `IntentRecord` split), once the wasm-size budget allows it.

---

*Snapshot generated from `src/bench.rs` at `soroban-sdk 21.7.7`.*
