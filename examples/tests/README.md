# Test Suite for risk_aware_solver_bot.py

This directory contains automated tests for `examples/risk_aware_solver_bot.py`, covering the six decision gates documented in `docs/risk-aware-solver-bot.md`.

## Running Tests Locally

### Prerequisites

- Python 3.7 or later
- `pytest` package

Install pytest:

```bash
pip install pytest
```

### Running the Test Suite

From the repository root:

```bash
pytest examples/tests/ -v
```

Or run just the solver bot tests:

```bash
pytest examples/tests/test_risk_aware_solver_bot.py -v
```

### Test Coverage

Run tests with coverage reporting:

```bash
pip install pytest-cov
pytest examples/tests/ --cov=examples --cov-report=term-missing
```

## What's Tested

The suite covers six decision gates per `docs/risk-aware-solver-bot.md`:

1. **Solver eligibility gating** – Solvers must pass `is_solver_eligible` check
2. **Intent state filtering** – Only `Open` or `PartiallyFilled` intents accepted
3. **Deadline-slack rejection** – Intent must have ≥ `fill_window_seconds` remaining
4. **Active-intent-cap enforcement** – Solver cannot exceed `max_active_intents`
5. **Bond-utilization rejection** – Accepting must not exceed `max_bond_utilization_bps`
6. **Minimum-profit filtering** – Expected profit must exceed `min_profit_stroops`

Each gate includes:
- Normal case (passing)
- Boundary cases (at exact threshold)
- Failure case (below/above threshold)
- Edge cases (zero/extreme values)

### Test Structure

- `TestSolverEligibilityGate` — gate #1
- `TestIntentStateFilter` — gate #2
- `TestDeadlineSlackRejection` — gate #3
- `TestActiveIntentCapRejection` — gate #4
- `TestBondUtilizationRejection` — gate #5
- `TestMinimumProfitRejection` — gate #6
- `TestDecisionIntegration` — multiple gates together
- `TestScreenCandidates` — helper function tests

## Mocking Strategy

All Stellar CLI / RPC calls (`stellar_view`, `stellar_tx`) are mocked using `unittest.mock`. This ensures:

- Tests run fully offline
- Tests are deterministic (no network flakiness)
- Tests execute quickly
- No need for a real Soroban network

The `decide()` function — which contains all decision logic — is tested directly with mocked solver/intent records, matching the pattern in `intent_settlement/src/test.rs`.

## CI Integration

GitHub Actions automatically runs this suite on every PR and push via `.github/workflows/ci.yml`. The `risk_aware_solver_bot_tests` job:

- Installs Python 3.9+
- Runs `pytest examples/tests/ -v`
- Reports test results and coverage
- Fails the CI check if any test fails

Passing this test suite is required before merging.

## Extending the Tests

To add a new test:

1. Add a new test method to the appropriate `Test*` class
2. Use descriptive names: `test_<scenario>_<expected_outcome>`
3. Follow the Arrange-Act-Assert pattern
4. Mock external calls (don't call the real Stellar CLI)
5. Run locally: `pytest examples/tests/test_risk_aware_solver_bot.py::TestClass::test_name -v`

Example:

```python
def test_new_scenario(self, default_config, default_intent, default_solver):
    """Describe the scenario being tested."""
    # Arrange
    default_intent["some_field"] = some_value
    
    # Act
    decision = decide(default_config, default_intent, default_solver, now)
    
    # Assert
    assert decision.should_accept is True
    assert decision.reason == "expected reason"
```
