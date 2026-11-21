# Backstop Compensation — Collusion Analysis (#369)

## Attack: Self-Slash Collusion

A solver and user collude:
1. Solver accepts an intent for the colluding user.
2. Solver intentionally misses the fill window.
3. `slash_solver` is called — slash proceeds go to the fee recipient or
   backstop pool depending on `backstop_vault_share_bps`.
4. User calls `claim_backstop_compensation` to extract funds from the pool.

## Why This Is Unprofitable

### Solver's loss (slash cost)

```
slash_amount = min(intent.min_dst_amount − total_filled, bond) / 10
             ≥ 1 stroop
             ≤ bond × SLASH_BPS / 10_000  (10% cap)
```

For a typical intent where `min_dst_amount ≈ bond`:

```
slash_amount ≈ bond × 10%
```

### User's gain (claim payout)

```
intent_cap   = min_dst_amount × MAX_BACKSTOP_INTENT_BPS / 10_000
             = min_dst_amount × 10%

epoch_cap    = MAX_BACKSTOP_USER_EPOCH_CLAIM  (fixed ceiling per 24 h)

payout       = min(intent_cap, epoch_remaining, pool)
```

The backstop pool is **shared** across all users. The colluding user
competes with every other legitimate claimant; their slash proceeds enter
the pool that all claimants draw from, not a private reserve.

### Net value: negative for the colluding pair

| Item                        | Value                                |
|-----------------------------|--------------------------------------|
| Solver loses                | `slash_amount` (10% of bond)         |
| Pool receives               | `vault_share × slash_amount`         |
| User claims (best case)     | `min(intent_cap, epoch_cap, pool)`   |
| Net (pair)                  | < 0 in all realistic scenarios       |

For the claim to offset the slash loss at 1:1:

```
min_dst_amount × 10%  ≥  bond × 10%
⟹  min_dst_amount    ≥  bond
```

But the **exposure check (#367)** enforces:

```
accepted_notional  ≤  bond × coverage_multiplier
```

So `min_dst_amount ≤ bond × coverage_multiplier`, meaning the maximum
possible claim payout is bounded by `bond × coverage_multiplier × 10%`.
For the default `coverage_multiplier = 10`, this is `bond × 100%` — but
the pool only holds what all prior slashes contributed, divided among all
claimants.

### Additional deterrents

1. **Slash cooldown**: After a slash, `SLASH_COOLDOWN` (1 hour) blocks the
   solver from accepting new intents, limiting attack throughput to one
   cycle per hour.

2. **Per-user epoch cap**: `MAX_BACKSTOP_USER_EPOCH_CLAIM` bounds how much
   any single user can extract per 24-hour epoch, regardless of how many
   colluding intents are set up.

3. **Exposure check (#367)**: `ExposureExceeded` prevents a solver from
   accepting intents with notional far exceeding their bond. This caps the
   maximum slash impact and therefore the maximum claim the colluding user
   could trigger.

4. **Active-intent deactivation**: A slash that drops the bond below
   `min_bond` for the backing token sets `is_active = false`, blocking
   further accepts until the solver tops back up.

5. **Double-claim guard**: `BackstopClaimed(intent_id)` ensures each
   intent can only be claimed once, regardless of how many slash cycles it
   accumulates.

6. **Eligible-state gate (#369)**: Only intents with `slash_cycles > 0` (or
   in `Slashed` state) are eligible, preventing fresh intents from claiming.

## Conclusion

Self-slash collusion has **negative expected value** for the colluding pair
because:

- The solver loses the full slash amount from their bond (real economic loss).
- The user can claim at most `MAX_BACKSTOP_INTENT_BPS` of `min_dst_amount`
  from a pool that is shared with all other claimants and may be near-empty.
- Epoch caps prevent repeated extraction over time.
- Slash cooldowns limit throughput to one attack per hour per solver.
- The exposure check limits how large any single intent can be relative to
  the solver's bond, capping the maximum yield from any single attack.

The only scenario where the attack could be net-positive is if the backstop
pool happens to be very large (from many legitimate slashes) while no other
claimants are present — a condition that is both self-defeating (requires
many prior slashes, each costly to the attacker) and transient.

## Appendix: Vault accounting reads

`deposit` and `withdraw` load `TotalShares` and `TotalAssets` from instance
storage exactly once per call and reuse the locals for the share/asset
conversion, the cooldown-scoping check, and the state write-back. The totals
cannot change mid-call, so no step re-reads them.
