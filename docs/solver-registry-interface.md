# Solver Registry — Public ABI

**Issue:** [#186](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/186)
**Contract:** `solver_registry` (`solver_registry/src/lib.rs`)
**Design:** [`solver-registry-design.md`](./solver-registry-design.md)
**Status:** Implemented — read interface frozen for downstream callers.

This document is the stable contract surface `intent_settlement` (and off-chain
solver bots) may depend on. The **read** side — `get_tier`, `tier_for`,
`get_tier_table`, the perk getters, `get_reputation_score` — will not change
shape without a new major version. The **write** side (`record_fill` /
`record_failure` / `slash`) is stable but not yet consumed: wiring
`accept_intent` / `slash_solver` to call it is a separate follow-up (design §4,
Option A).

---

## 1. Tier model

Tier is `min(bond gate, score gate)` — a solver holds tier `t` only if **both**
`bond_amount ≥ threshold[t].min_bond` **and** `score ≥ threshold[t].min_score_bps`.
Score is the 0–10 000 bps reputation value (§3).

Default table (design §3; `min_bond` shown in USDC, stored in 7-decimal
smallest units):

| Tier | Name     | `min_bond` | `min_score_bps` | `fill_window_bonus_pct` | `slash_bps` | `fee_rebate_bps` |
|-----:|----------|-----------:|----------------:|------------------------:|------------:|-----------------:|
| 0    | Unranked |         50 |               0 |                      0 |       1 000 |                0 |
| 1    | Bronze   |        500 |           1 000 |                     10 |       1 000 |                0 |
| 2    | Silver   |      2 000 |           3 500 |                     20 |         800 |                0 |
| 3    | Gold     |     10 000 |           7 000 |                     30 |         600 |                0 |
| 4    | Platinum |     50 000 |           9 000 |                     50 |         500 |                0 |

`min_bond` / `min_score_bps` are **tunable** by the admin within bounds (§2.4).
`fill_window_bonus_pct` / `slash_bps` / `fee_rebate_bps` are **fixed** in the
contract. `fee_rebate_bps` is a reserved slot (design §8, pending tokenomics)
and currently returns 0 for every tier.

---

## 2. Entry points

### 2.1 Lifecycle / admin

| Function | Auth | Notes |
|---|---|---|
| `initialize(admin, bond_token, fee_recipient)` | `admin` | Once. Seeds the default tier table. |
| `set_writer(writer)` | admin | Address allowed to drive the write path (§2.3). |
| `set_tier_threshold(tier, min_bond, min_score_bps)` | admin | `tier ∈ 1..=4`; see 2.4. |

### 2.2 Solver self-service

| Function | Auth | Notes |
|---|---|---|
| `register_solver(solver, bond_amount)` | `solver` | `bond_amount ≥ tier-0 floor`; pulls bond token. |
| `stake(solver, amount)` | `solver` | Top up bond. |
| `unstake(solver, amount)` | `solver` | Remaining bond must stay ≥ tier-0 floor. |
| `deregister_solver(solver)` | `solver` | Returns full bond, deletes the record. |

### 2.3 Settlement write path — `caller` must be the configured writer or the admin

| Function | Returns | Effect |
|---|---|---|
| `record_fill(caller, solver, amount)` | — | `fills_completed += 1`, `total_volume += amount`. |
| `record_failure(caller, solver)` | — | `fills_failed += 1` (no bond movement). |
| `slash(caller, solver)` | `(slash_amount: i128, new_tier: u32)` | Takes `bond * slash_bps(tier) / 10_000` (min 1), transfers it to the fee recipient, `fills_failed += 1`. |

`caller` is explicit (mirrors `intent_settlement::pause`) so the registry can
accept calls from either the admin or the settlement contract without an
implicit-signer ambiguity.

### 2.4 `set_tier_threshold` bounds

- `tier ∈ 1..=4` — tier 0 is the fixed entry floor (`InvalidTier` otherwise).
- `1 ≤ min_bond ≤ 1_000_000 USDC` (`ThresholdOutOfBounds`).
- `min_score_bps ≤ 9_999` (`ThresholdOutOfBounds`) — 10 000 is unreachable by construction.
- The row must stay **strictly** greater than tier `tier-1` and strictly less
  than tier `tier+1` on **both** axes (`ThresholdsNotMonotonic`).

---

## 3. Views (stable)

| Function | Returns |
|---|---|
| `get_tier(solver)` | `u32` 0–4 (unknown solver → 0). |
| `tier_for(score_bps, bond_amount)` | `u32` — pure lookup against the current thresholds; for off-chain pricing. |
| `get_tier_table()` | `Vec<TierInfo>` — 5 rows, effective thresholds + fixed perks. |
| `get_fill_window_bonus_pct(tier)` / `get_slash_bps(tier)` / `get_fee_rebate_bps(tier)` | `u32` (`InvalidTier` for `tier ≥ 5`). |
| `get_reputation_score(solver)` | `Option<u32>` bps. |
| `compute_reputation_score(record)` | `u32` bps — pure. |
| `get_solver(solver)` | `Option<SolverRecord>`. |
| `get_solver_count()` | `u32`. |
| `get_writer()` / `get_admin()` / `get_bond_token()` | `Option<Address>`. |

### `SolverRecord`

```rust
struct SolverRecord {
    address: Address,
    bond_amount: i128,      // staked bond, smallest unit
    fills_completed: u32,
    fills_failed: u32,
    total_volume: i128,     // dst-token units, cumulative
    registered_at: u64,
    last_slash_time: u64,   // 0 = never
    slashed_total: i128,    // lifetime bond taken by slashing
}
```

---

## 4. Reputation formula (shared with `intent_settlement`)

Ported **byte-for-byte** from `intent_settlement::compute_reputation_score`.
Integer-only, cannot panic.

```text
total  = fills_completed + fills_failed            (total == 0  → score 0)
base   = fills_completed * 10_000 / total          success rate, 0..10_000 bps
VOLUME_SCALE = 1_000 * 100 * 10_000_000            (= 1e12)
vol    = max(total_volume, 0)
decay  = VOLUME_SCALE * 10_000 / (VOLUME_SCALE + vol + 1)
mult   = 10_000 - decay / 10                       9_000..10_000 bps
score  = base * mult / 10_000
```

### Shared test vector

`solver_registry`'s `score_test_vector` test pins these; the same inputs must
yield the same outputs in `intent_settlement`:

| `fills_completed` | `fills_failed` | `total_volume` | `score` |
|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 |
| 0 | 5 | 0 | 0 |
| 1 | 0 | 0 | 9 001 |
| 8 | 2 | 0 | 7 200 |
| 8 | 2 | 100 000 000 USDC | 8 000 |

---

## 5. Errors

| Code | Variant | Trigger |
|---:|---|---|
| 1 | `AlreadyInitialized` | second `initialize` |
| 2 | `NotInitialized` | call before `initialize` |
| 3 | `Unauthorized` | write-path `caller` is neither writer nor admin |
| 4 | `SolverNotRegistered` | unknown solver on a mutating call |
| 5 | `SolverAlreadyRegistered` | `register_solver` for an existing solver |
| 6 | `BondBelowFloor` | register/unstake would leave bond < tier-0 floor |
| 7 | `ZeroAmount` | non-positive amount where positive required |
| 8 | `InsufficientBond` | `unstake` amount > staked bond |
| 9 | `InvalidTier` | tier index ∉ 0..=4, or tier 0 to `set_tier_threshold` |
| 10 | `ThresholdOutOfBounds` | threshold value outside its bound |
| 11 | `ThresholdsNotMonotonic` | thresholds not strictly increasing |
| 12 | `WriterNotSet` | write path used before `set_writer` by a non-admin caller |

---

*Closes #186 (interface documentation).*
