# Economic Analysis: Caller Incentives for Permissionless `slash_solver` and `expire_intent`

**Issues Addressed:** #268  
**Status:** Audit Completed & Implementation Approved  

---

## 1. Executive Summary

Both `slash_solver` and `expire_intent` are permissionless contract functions designed to enforce protocol state transitions when time windows expire. Prior to this update, neither function compensated transaction callers. 

This analysis evaluates whether the altruistic model is sufficient or if economic incentives are required to ensure timely execution.

---

## 2. Analysis of `expire_intent`

### Mechanism & Trust Model
- `expire_intent` transitions an intent from `Open` or `PartiallyFilled` to `Expired` once its deadline has passed (`now >= deadline`).
- Expiring an intent releases locked escrow (or marks the intent permanently unfillable) and cleans up storage count (`open_intents`).

### Economic Assessment
- **Zero Bond/Capital Pool:** Expired intents hold user escrow, which is returned/unlocked. There is no penalty pool or solver collateral associated with an unaccepted expired intent from which a rebate can be drawn.
- **Protocol/User Incentives:** The intent user themselves has a direct economic incentive to call `expire_intent` (or initiate cancellation) to reclaim liquidity. Furthermore, off-chain indexers and solver bots track intent state off-chain and ignore expired intents regardless of on-chain state materialization.
- **Conclusion:** Adding a contract-level rebate for `expire_intent` is unnecessary and out of scope, as users self-motivate cleanup and no pool exists to fund rebates without protocol inflation/subsidy.

---

## 3. Analysis of `slash_solver`

### Mechanism & Trust Model
- `slash_solver` penalizes a solver who accepted an intent but failed to execute a fill before `accepted_at + FILL_WINDOW`.
- The solver's bond is reduced by 10% (minimum 1 unit).
- The intent is reopened for other solvers.

### Economic Assessment
- **Competitive Dynamics:** Solvers are motivated to slash competitor solvers to free up trapped intents for re-acceptance.
- **Liveness Risk:** In low-volatility or low-competition periods, reliance on altruism or indirect solver competition creates a latency window where a defaulted intent sits in `Accepted` state longer than necessary, delaying recovery.
- **Rebate Mechanism:** A 5% caller rebate (`slash_amount / 20`) carved directly out of the slashed bond amount incentivizes third-party keepers, liquidators, and MEV bots to monitor and execute `slash_solver` immediately upon deadline expiration.
- **Preservation of Total Penalty:** The total bond penalty deducted from the defaulting solver remains 100% of `slash_amount`. The rebate is split from `slash_amount`, transferring `slash_amount - caller_rebate` to the protocol `FeeRecipient` and `caller_rebate` to `env.invoker()`.

---

## 4. Implementation Details

- **Caller Rebate:** `caller_rebate = slash_amount / 20` (5% of slash penalty).
- **Floor Guarantee:** If `slash_amount == 1`, `caller_rebate` is `0` and the full `1` goes to `FeeRecipient` to prevent rounding arithmetic issues.
- **Distribution:**
  - Solver bond deducted: `slash_amount`
  - Fee Recipient received: `slash_amount - caller_rebate`
  - Invoker received: `caller_rebate`
