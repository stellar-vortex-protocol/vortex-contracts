# Resource Budget Ceilings — `intent_settlement`

**Issue:** [#149](https://github.com/vortex-protocol/vortex-contracts/issues/149)
**Status:** Ceilings defined; regenerate numbers after first test run.
**Harness:** `intent_settlement/src/bench.rs`

---

## 1. Purpose

Solver bots price each fill by estimating the on-chain cost before submitting.
A silent regression that doubles the CPU usage of `fill_intent` breaks solver
profitability models.  These ceilings catch that regression at CI time, before
it reaches testnet.

Each ceiling is set at **measured value × 1.10** (10% headroom), rounded up to
the nearest 1 000 instructions / 1 000 bytes.  The 10% margin lets normal
harmless noise through while catching material changes.

---

## 2. Methodology

```
cargo test --features testutils bench -- --nocapture
```

The harness (`intent_settlement/src/bench.rs`) runs each entrypoint from an
isolated fixture, resets `env.budget()` immediately before the call, and reads
`Budget::cpu_instruction_cost()` + `Budget::memory_bytes_cost()` immediately
after.

**Worst-case fixtures:**
- `batch_*` entrypoints run at `MAX_BATCH_SIZE = 20` items.
- `list_solvers` runs with `MAX_PAGE_SIZE = 100` registered solvers.
- Single-item entrypoints use a standard 1 000 USDC bond (well above the
  50 USDC floor, so tier-lookup traverses all 5 rows).

**Caveats (read before using for fee bids):**
- Native Rust, not Wasm. The SDK executes natively in tests; figures are a
  *lower bound* of on-chain cost. For authoritative per-transaction cost use
  `stellar contract invoke --cost` against the built Wasm.
- Ledger read/write entry counts are not exposed by `soroban-sdk 21` testutils.
  The record-size table below covers the write-bytes dimension.
- Token transfers inside `fill_intent`, `register_solver`, `slash_solver` etc.
  call the Stellar Asset Contract; that cost is included.
- Every settlement transfer (including fee distribution) uses the panicking
  `token::Client::transfer`. A failed transfer aborts the whole invocation, so
  no partial payouts persist. Do not switch to `try_transfer` without also
  handling the error path explicitly.
- Numbers are tied to the SDK version. Pin them and regenerate on upgrade.

---

## 3. Regenerating ceiling values

After a contract change or SDK bump, run:

```bash
cd intent_settlement
cargo test --features testutils bench -- --nocapture 2>&1 | grep CEILING_HINT
```

Each `CEILING_HINT` line has the form:

```
CEILING_HINT  submit_intent                               cpu=    310_000  mem=     44_000  (raw cpu=281113  mem=39630)
```

Copy the `cpu=` and `mem=` values into the matching `CEIL_*` constants at the
top of `bench.rs` and into the table below, then commit.

---

## 4. Per-entrypoint ceilings

> **Note:** the table below is populated on the first `cargo test --features
> testutils bench::resource_cost_report -- --nocapture` run in CI.  The
> `Measured` columns show the raw SDK values; the `Ceiling` columns are
> `measured × 1.10` rounded up to the nearest 1 000.

### 4.1 Single-item (non-batch) paths

| Entrypoint | CPU (measured) | CPU ceiling | Mem (measured) | Mem ceiling |
|---|--:|--:|--:|--:|
| `submit_intent` | _regenerate_ | 310,000 | _regenerate_ | 44,000 |
| `accept_intent` | _regenerate_ | 328,000 | _regenerate_ | 53,000 |
| `fill_intent` (full fill) | _regenerate_ | 685,000 | _regenerate_ | 107,000 |
| `fill_intent` (partial fill) | _regenerate_ | 707,000 | _regenerate_ | 108,000 |
| `cancel_intent` | _regenerate_ | 264,000 | _regenerate_ | 44,000 |
| `expire_intent` | _regenerate_ | 225,000 | _regenerate_ | 36,000 |
| `slash_solver` | _regenerate_ | 488,000 | _regenerate_ | 72,000 |
| `request_extension` | _regenerate_ | 194,000 | _regenerate_ | 37,000 |
| `register_solver` (first) | _regenerate_ | 377,000 | _regenerate_ | 58,000 |
| `register_solver` (top-up) | _regenerate_ | 343,000 | _regenerate_ | 49,000 |
| `withdraw_bond` | _regenerate_ | 346,000 | _regenerate_ | 50,000 |
| `deregister_solver` | _regenerate_ | 366,000 | _regenerate_ | 54,000 |

### 4.2 Batch paths (MAX_BATCH_SIZE = 20)

| Entrypoint | CPU ceiling (total) | Mem ceiling (total) | CPU / item | Mem / item |
|---|--:|--:|--:|--:|
| `batch_submit_intent` ×20 | 7,100,000 | 1,060,000 | 355,000 | 53,000 |
| `batch_accept_intent` ×20 | 7,120,000 | 1,250,000 | 356,000 | 62,500 |
| `batch_fill_intent` ×20 (full) | 13,700,000 | 2,140,000 | 685,000 | 107,000 |
| `batch_cancel_intent` ×20 | 5,280,000 | 880,000 | 264,000 | 44,000 |

### 4.3 Paginated read (MAX_PAGE_SIZE = 100)

| Entrypoint | CPU ceiling | Mem ceiling |
|---|--:|--:|
| `list_solvers` (100 solvers, page_size=100) | 650,000 | 200,000 |

---

## 5. Persistent record sizes

Serialised XDR size of the records rewritten on the hot paths:

| Record | Serialised size |
|---|--:|
| `IntentRecord` | 624 bytes |
| `SolverRecord` | 340 bytes |

`accept_intent` and both `fill_intent` paths rewrite the entire `IntentRecord`
(624 bytes) plus the full `SolverRecord` (340 bytes).  See issue #196 for a
planned write-splitting optimisation.

---

## 6. Ceiling failure playbook

If a CI job fails with:
```
fill_intent (full fill): CPU 712000 > ceiling 685000 — update CEIL_FILL_INTENT_FULL_CPU
```

1. Pull the branch locally and run:
   ```bash
   cargo test --features testutils bench -- --nocapture 2>&1 | grep CEILING_HINT
   ```
2. Determine whether the regression is expected (new feature) or unexpected
   (accidental).
3. If expected: update `CEIL_*` constants in `bench.rs` and the tables in
   `docs/149-intent-settlement.md` to the new CEILING_HINT values, then commit.
4. If unexpected: fix the regression before merging.

---

## 7. Toolchain / SDK version

Numbers were captured with **`soroban-sdk 21.7.7`** on stable Rust.
Regenerate after any SDK or toolchain bump.

---

*Maintained by the Vortex Protocol contributors. See also
`docs/149-satellite-contracts.md` for `solver_registry`, `proof_registry`, and
`reputation_badge`.*
