# Risk-aware solver bot example

`examples/risk_aware_solver_bot.py` extends the basic accept/fill loop with a
small decision gate before it accepts an intent.

## Configuration

Set these environment variables before running the example:

```bash
export VORTEX_CONTRACT_ID="<deployed intent_settlement contract>"
export SOLVER_ADDRESS="<solver public address>"
export SOLVER_SECRET_KEY="<solver signing key or Stellar CLI identity>"
export STELLAR_NETWORK="testnet"

# Optional thresholds
export MIN_PROFIT_STROOPS="1000000"
export MAX_BOND_UTILIZATION_BPS="5000"
export MAX_ACTIVE_INTENTS="3"
export FILL_WINDOW_SECONDS="300"
```

Run it against a candidate intent:

```bash
python3 examples/risk_aware_solver_bot.py "$INTENT_ID" "$(date +%s)"
```

## Checks and rationale

- `is_solver_eligible`: avoids submitting an `accept_intent` transaction when
  the contract would reject the solver for inactive or under-bonded status.
- Intent state: only `Open` or `PartiallyFilled` intents are candidates.
- Deadline slack: rejects intents without at least one fill window remaining,
  reducing the chance of accepting an obligation that cannot be completed in
  time.
- Active intent cap: limits concurrent obligations so one solver does not take
  more fills than its operations can service.
- Bond utilization: estimates how much of the posted bond would be tied to
  outstanding obligations after accepting the intent. The example rejects
  candidates above `MAX_BOND_UTILIZATION_BPS`.
- Minimum expected profit: compares the user's minimum destination amount with
  a placeholder fill-cost estimate and rejects candidates below
  `MIN_PROFIT_STROOPS`.

The `estimate_fill_cost` function is intentionally simple. Production solvers
should replace it with a route-specific model that includes source-chain
execution, bridge fees, destination inventory, Stellar transaction fees,
slippage, and failed-fill risk.
