# Solver Registry — Design Document

> **Status:** Partially implemented.  
> **Closes:** #46  
> **Last updated:** 2026-08-28

---

## 0. Implementation status (#197)

The **tier-perk enforcement** described in §3, §6 and §7 is now live:

- A minimal `solver_registry` crate exists (`solver_registry/`). It stores an
  admin-managed tier per solver and exposes `get_tier(solver) -> u32` plus the
  `get_fill_window_bonus_bps` / `get_slash_bps` schedule views. Score-gated
  automatic promotion (porting `compute_reputation_score`, `record_fill` /
  `record_failure`, staking, `migrate_solver`) is still §2/§5/§9 future work
  under #186.
- `intent_settlement` calls **only** `get_tier` on the hot path and maps the
  tier to perk values from local tables (`TIER_FILL_WINDOW_BONUS_BPS`,
  `TIER_SLASH_BPS`). This deviates from §6's "keep `intent_settlement` free of
  tier constants" for two reasons: one cross-contract call per
  `accept_intent` / `slash_solver` instead of three, and the settlement
  contract still enforces the agreed schedule even if the registry returns a
  bad value. The two copies of the table **must be kept in sync** — a comment
  in each says so.
- The integration is **optional**: `set_solver_registry(None)` (the default)
  makes every solver Unranked, and any failure of the cross-contract call
  falls back to Unranked. `accept_intent` / `slash_solver` never hard-fail on
  the registry.
- **Tier snapshot timing:** the tier is read at **accept-time** and stored on
  the `IntentRecord` (`solver_tier`). `slash_solver` uses that snapshot, not
  the solver's live tier. Rationale: the fill window and the slash rate are
  both part of the deal struck at accept-time, so a mid-flight promotion can't
  soften an abandonment and a mid-flight demotion can't harden it.
- **Fee rebate (§8) is not implemented** and is out of scope for #197. It
  overlaps with the volume-based fee discount in #7; the two should be unified
  in one design rather than built twice.

---

## 1. Motivation

`intent_settlement` already tracks per-solver fill history (`fills_completed`,
`fills_failed`, `total_volume`) and enforces a flat `MIN_BOND` floor.  The
roadmap item calls for a tiered staking model where solver reputation determines
capital efficiency and, eventually, priority access to intents.

The goals of this design are:

1. Define bond tiers and their staking requirements.
2. Define how the on-chain reputation score is computed and stored.
3. Choose a clean contract boundary between `intent_settlement` (existing) and
   the new `solver_registry` contract (to be built).
4. Keep the design auditable: all scoring maths are integer-only and
   deterministic.

---

## 2. Reputation Score (carried forward from #47)

The score computed in `intent_settlement::compute_reputation_score` is the
authoritative definition.  The `solver_registry` contract will *read* it, not
re-implement it.

```
total  = fills_completed + fills_failed
base   = fills_completed * 10_000 / total          -- success rate, 0..10_000 bps
decay  = VOLUME_SCALE * 10_000 / (VOLUME_SCALE + vol + 1)  -- 0..10_000 bps
mult   = 10_000 - decay / 10                       -- 9_000..10_000 bps
score  = base * mult / 10_000                      -- 0..10_000 bps
```

Where `VOLUME_SCALE = 1_000 × 100 × 10_000_000` (1 000 fills of 100 tokens).

Edge cases: `total == 0 → score = 0`.

The maximum achievable score is **9 999 bps** (< 10 000 by construction).
Reaching 10 000 would require infinite fill volume, which is unreachable.

---

## 3. Tiers

| Tier | Name      | Min bond (USDC) | Min score (bps) | Perks |
|------|-----------|----------------|-----------------|-------|
| 0    | Unranked  | 50             | 0               | Can submit fills; no priority |
| 1    | Bronze    | 500            | 1 000           | +10% fill-window extension |
| 2    | Silver    | 2 000          | 3 500           | +20% fill-window; reduced slash (8%) |
| 3    | Gold      | 10 000         | 7 000           | +30% fill-window; reduced slash (6%); fee rebate |
| 4    | Platinum  | 50 000         | 9 000           | +50% fill-window; slash (5%); max fee rebate |

### Rationale

- Minimum bonds scale roughly 10× between tiers so each tier represents a
  genuine capital commitment.
- Score thresholds are generous at the bottom (Bronze is only 10%) to give new
  solvers a path in; they steepen near the top where the volume bonus is
  needed.
- Slash percentage reductions are small (no more than 50% of the base 10%)
  so that slashing still hurts even for Platinum solvers.
- Fee rebate percentages are left as TBD pending tokenomics discussion; the
  slot is reserved in the contract interface.

---

## 4. Contract Boundary

### Option A — `solver_registry` owns all solver state (recommended)

`solver_registry` becomes the canonical store for `SolverRecord`.  
`intent_settlement` calls into `solver_registry` via cross-contract calls to:

- Check eligibility (`is_eligible(solver) → bool`).
- Record fill outcomes (`record_fill(solver, amount)` / `record_failure(solver)`).
- Execute bond slashing (`slash(solver, amount) → (slashed, new_tier)`).

**Pros:**  
- Clean separation of concerns.  
- Bond accounting lives in one place.  
- Tier logic never leaks into the settlement contract.

**Cons:**  
- Cross-contract calls increase per-intent gas cost.  
- Migration required to move existing solver records from `intent_settlement`.

### Option B — `intent_settlement` writes, `solver_registry` is read-only

`intent_settlement` keeps `SolverRecord` as-is.  
`solver_registry` reads `intent_settlement` storage via cross-contract reads
and exposes the tier view externally.

**Pros:**  
- No migration; existing data stays in place.  
- No extra call overhead on the hot fill/slash path.

**Cons:**  
- Tier-related logic (fill-window extension, reduced slash %) has to live in
  `intent_settlement`, coupling two concerns in one contract.  
- Makes it hard to upgrade tier rules without redeploying `intent_settlement`.

### Decision

**Option A is recommended.**  The migration cost is a one-time operation
(admin calls `migrate_solver(addr)` on `solver_registry` which reads from
the old contract and writes the record locally).  Clean separation will pay
off as tier rules evolve.

---

## 5. Cross-Contract Interface (Option A)

```rust
// solver_registry public API (abridged)

/// Called by intent_settlement before accept_intent.
fn is_eligible(solver: Address) -> bool;

/// Called by intent_settlement after a successful fill.
fn record_fill(solver: Address, amount: i128);

/// Called by intent_settlement inside slash_solver.
/// Returns (slash_amount_taken, new_tier_level).
fn slash(solver: Address) -> (i128, u32);

/// Public view: current tier (0–4) for a solver.
fn get_tier(solver: Address) -> u32;

/// Public view: current reputation score (0–10_000 bps).
fn get_reputation_score(solver: Address) -> Option<u32>;

/// Solver self-service: stake more to reach a higher tier.
fn stake(solver: Address, amount: i128);

/// Solver self-service: unstake (subject to MIN_BOND and active_intents == 0).
fn unstake(solver: Address, amount: i128);

/// Admin: migrate an existing SolverRecord from intent_settlement.
fn migrate_solver(solver: Address, old_contract: Address);
```

`intent_settlement` will be updated to call `solver_registry` instead of
reading its own `DataKey::Solver` storage directly, behind a feature flag so
existing tests continue to work until the registry is deployed.

---

## 6. Fill-Window Extension (tier perk)

`accept_intent` in `intent_settlement` currently hard-codes `FILL_WINDOW = 300s`.

After the registry integration, the actual window will be:

```
effective_window = FILL_WINDOW * (100 + tier_bonus_pct) / 100
```

Where `tier_bonus_pct` comes from the registry's `get_fill_window_bonus(tier)` view.

This keeps `intent_settlement` free of tier constants — all tuning happens in
`solver_registry`.

---

## 7. Reduced Slash Percentage (tier perk)

`slash_solver` currently takes `bond / 10` (10%).  After integration:

```
slash_bps     = solver_registry.get_slash_bps(tier)   // 1000 / 800 / 600 / 500
slash_amount  = bond_amount * slash_bps / 10_000
```

The `get_slash_bps` view is intentionally public so off-chain solvers can
price this into their quotes.

---

## 8. Fee Rebate (tier perk)

Currently `intent_settlement` sends the full protocol fee (`fill * 5 / 10_000`)
to `fee_recipient`.  For Gold and Platinum tiers, a portion (TBD) is returned
to the solver.  Implementation details deferred to the tokenomics review.

---

## 9. Migration Plan

1. Deploy `solver_registry` pointing at `intent_settlement`'s bond token.
2. Admin calls `migrate_solver(addr, old_contract)` for each existing solver.
   The migration function reads the old `SolverRecord`, inserts it into the
   registry, and emits a `solver_migrated` event.
3. Admin updates `intent_settlement`'s `registry_contract` instance key to
   the new registry address.
4. At a coordinated cut-over block, `intent_settlement` begins calling the
   registry instead of its own storage.
5. Old `DataKey::Solver` entries in `intent_settlement` are left to expire
   via normal Soroban TTL; no explicit cleanup needed.

---

## 10. Open Questions

| # | Question | Owner |
|---|----------|-------|
| 1 | Exact fee-rebate percentages per tier | Tokenomics |
| 2 | Should tier downgrades be immediate or epoch-lagged? | Protocol |
| 3 | NFT-style on-chain tier badge (mentioned in roadmap) — defer to v2? | Protocol |
| 4 | Oracle for USDC price if bond denominator changes | Engineering |
| 5 | Multi-sig or DAO governance for tier parameter updates | Governance |

---

## 11. Out of Scope (this design phase)

- Dispute resolution mechanism (roadmap item, separate issue).
- Off-chain reputation indexer / API.
- Frontend tier badge display.
- Cross-chain solver identity linking.
