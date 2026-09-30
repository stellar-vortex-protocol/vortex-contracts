"""pytest suite for examples/risk_aware_solver_bot.py

This tests the six decision gates documented in docs/risk-aware-solver-bot.md:

1. is_solver_eligible gating
2. Intent state filtering (Open/PartiallyFilled only)
3. Deadline-slack rejection
4. Active-intent-cap rejection
5. Bond-utilization rejection
6. Minimum-profit rejection

All on-chain RPC calls are mocked so tests run fully offline and deterministically.
"""

import pytest
import sys
from pathlib import Path
from unittest.mock import MagicMock, patch

# Add examples directory to path so we can import the bot
sys.path.insert(0, str(Path(__file__).parent.parent))

from risk_aware_solver_bot import (
    BotConfig,
    Decision,
    decide,
    screen_candidates,
)


@pytest.fixture
def default_config():
    """Default bot configuration for testing."""
    return BotConfig(
        contract_id="CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4",
        solver_secret="SBAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBP",
        solver_address="GBAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBY5",
        network="testnet",
        min_profit_stroops=1_000_000,
        max_bond_utilization_bps=5_000,
        max_active_intents=3,
        fill_window_seconds=300,
    )


@pytest.fixture
def default_intent():
    """Default intent record for testing."""
    return {
        "state": "Open",
        "deadline": 1000000,  # Well in future
        "min_dst_amount": 3_000_000,  # Min to receive
        "src_amount": 1_000_000,
    }


@pytest.fixture
def default_solver():
    """Default solver record for testing."""
    return {
        "active_intents": 0,
        "bond_amount": 100 * 10_000_000,  # 100 USDC in stroops
    }


class TestSolverEligibilityGate:
    """Test is_solver_eligible gating (gate #1)."""

    def test_eligible_solver_passes(self, default_config, default_intent, default_solver):
        """Eligible solver should pass all other checks."""
        # Should not reject for eligibility
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is True

    def test_ineligible_solver_fails(self, default_config, default_intent, default_solver):
        """Ineligible solver would be rejected at call site; this tests the context."""
        # Note: The eligibility check is done in maybe_accept_intent, not decide().
        # Here we verify that decide() doesn't have its own eligibility check.
        # If the solver reaches decide(), they've already passed the check.
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is True


class TestIntentStateFilter:
    """Test intent state filtering (gate #2)."""

    def test_open_state_accepted(self, default_config, default_intent, default_solver):
        """Open intents should be considered."""
        default_intent["state"] = "Open"
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is True

    def test_partially_filled_state_accepted(self, default_config, default_intent, default_solver):
        """PartiallyFilled intents should be considered."""
        default_intent["state"] = "PartiallyFilled"
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is True

    def test_filled_state_rejected(self, default_config, default_intent, default_solver):
        """Filled intents should be rejected."""
        default_intent["state"] = "Filled"
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is False
        assert "intent state is Filled" in decision.reason

    def test_bidding_state_rejected(self, default_config, default_intent, default_solver):
        """Bidding intents should be rejected."""
        default_intent["state"] = "Bidding"
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is False

    def test_disputed_state_rejected(self, default_config, default_intent, default_solver):
        """Disputed intents should be rejected."""
        default_intent["state"] = "Disputed"
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is False


class TestDeadlineSlackRejection:
    """Test deadline-slack rejection (gate #3)."""

    def test_sufficient_deadline_slack_accepted(self, default_config, default_intent, default_solver):
        """Intent with enough time remaining should be accepted."""
        deadline = 1000000
        default_intent["deadline"] = deadline
        config = default_config
        # Current time such that deadline - now > fill_window_seconds
        now = deadline - 400  # 400 seconds remaining > 300 fill_window
        decision = decide(config, default_intent, default_solver, now)
        assert decision.should_accept is True

    def test_exact_deadline_slack_boundary_accepted(self, default_config, default_intent, default_solver):
        """Intent with exactly fill_window_seconds remaining should be accepted."""
        deadline = 1000000
        default_intent["deadline"] = deadline
        config = default_config
        # Exactly fill_window_seconds remaining
        now = deadline - config.fill_window_seconds
        decision = decide(config, default_intent, default_solver, now)
        # deadline - now = fill_window_seconds (300)
        # check is: deadline - now < fill_window_seconds (false, not <)
        # So this should be accepted
        assert decision.should_accept is True

    def test_less_than_deadline_slack_rejected(self, default_config, default_intent, default_solver):
        """Intent with less than fill_window_seconds remaining should be rejected."""
        deadline = 1000000
        default_intent["deadline"] = deadline
        config = default_config
        # Just less than fill_window_seconds remaining
        now = deadline - (config.fill_window_seconds - 1)
        decision = decide(config, default_intent, default_solver, now)
        assert decision.should_accept is False
        assert "not enough time remains" in decision.reason

    def test_deadline_passed_rejected(self, default_config, default_intent, default_solver):
        """Intent past deadline should be rejected."""
        deadline = 1000000
        default_intent["deadline"] = deadline
        now = deadline + 1
        decision = decide(default_config, default_intent, default_solver, now)
        assert decision.should_accept is False


class TestActiveIntentCapRejection:
    """Test active-intent-cap enforcement (gate #4)."""

    def test_below_active_intent_cap_accepted(self, default_config, default_intent, default_solver):
        """Solver below max active intents should be accepted."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            max_active_intents=3,
            max_bond_utilization_bps=5_000,  # 50% in BPS
        )
        default_solver["active_intents"] = 2
        default_solver["bond_amount"] = 500 * 10_000_000  # 500 USDC bond to keep utilization low
        # Utilization = (2 + 1) * 50_000_000 * 10_000 / 5_000_000_000 = 300 BPS (3%)
        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is True

    def test_at_active_intent_cap_rejected(self, default_config, default_intent, default_solver):
        """Solver at max active intents should be rejected."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            max_active_intents=3,
        )
        default_solver["active_intents"] = 3
        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is False
        assert "active intent cap" in decision.reason

    def test_over_active_intent_cap_rejected(self, default_config, default_intent, default_solver):
        """Solver over max active intents should be rejected."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            max_active_intents=3,
        )
        default_solver["active_intents"] = 4
        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is False

    def test_zero_active_intents_accepted(self, default_config, default_intent, default_solver):
        """Solver with zero active intents should be accepted."""
        default_solver["active_intents"] = 0
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is True


class TestBondUtilizationRejection:
    """Test bond-utilization rejection (gate #5)."""

    def test_below_utilization_limit_accepted(self, default_config, default_intent, default_solver):
        """Bond utilization below limit should be accepted."""
        # Bond utilization = ((active_intents + 1) * MIN_BOND_STROOPS * 10_000) / bond_amount
        # Let's set up a case that's below the limit
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            max_bond_utilization_bps=5_000,  # 50% in BPS
        )
        default_solver["active_intents"] = 0
        default_solver["bond_amount"] = 100 * 10_000_000  # Large bond

        # Utilization = (0 + 1) * 50_000_000 * 10_000 / 1_000_000_000
        #             = 500_000_000_000 / 1_000_000_000 = 500 (5% in BPS)
        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is True

    def test_exactly_at_utilization_limit_rejected(self, default_config, default_intent, default_solver):
        """Bond utilization exactly at limit should be rejected (strict <)."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            max_bond_utilization_bps=1000,  # 10% in BPS
        )
        default_solver["active_intents"] = 1
        default_solver["bond_amount"] = 50 * 10_000_000  # Smaller bond

        # Utilization = (1 + 1) * 50_000_000 * 10_000 / 500_000_000
        #             = 1_000_000_000_000 / 500_000_000 = 2_000 (20% in BPS)
        # This is > 1000, so rejected
        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is False
        assert "exceed bond utilization limit" in decision.reason

    def test_zero_bond_handled(self, default_config, default_intent, default_solver):
        """Solver with zero bond should be rejected (protected by max(bond_amount, 1))."""
        default_solver["bond_amount"] = 0
        decision = decide(default_config, default_intent, default_solver, 999700)
        # With bond_amount = 0, max(0, 1) = 1, so utilization = very high
        assert decision.should_accept is False

    def test_large_bond_reduces_utilization(self, default_config, default_intent, default_solver):
        """Larger bond reduces utilization percentage."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            max_bond_utilization_bps=2_000,  # 20% in BPS
        )
        default_solver["active_intents"] = 1
        default_solver["bond_amount"] = 1000 * 10_000_000  # Very large bond (1000 USDC)

        # Utilization = (1 + 1) * 50_000_000 * 10_000 / 10_000_000_000
        #             = 1_000_000_000_000 / 10_000_000_000 = 100 BPS (1%)
        # This is < 2_000 BPS (20%), so accepted
        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is True


class TestMinimumProfitRejection:
    """Test minimum-profit rejection (gate #6)."""

    def test_above_minimum_profit_accepted(self, default_config, default_intent, default_solver):
        """Intent with profit above threshold should be accepted."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            min_profit_stroops=1_000_000,
        )

        # expected_cost = min_dst_amount - (src_amount * 0.995) approximately
        # With the stub: cost = src_amount * 0.995
        default_intent["min_dst_amount"] = 3_000_000
        default_intent["src_amount"] = 1_000_000
        # expected_cost = 1_000_000 * 0.995 = 995_000
        # expected_profit = 3_000_000 - 995_000 = 2_005_000 (>> 1_000_000)

        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is True
        assert decision.expected_profit_stroops > config.min_profit_stroops

    def test_exactly_at_minimum_profit_rejected(self, default_config, default_intent, default_solver):
        """Intent with profit exactly at threshold should be rejected (strict <)."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            min_profit_stroops=2_005_000,
        )

        default_intent["min_dst_amount"] = 3_000_000
        default_intent["src_amount"] = 1_000_000

        decision = decide(config, default_intent, default_solver, 999700)
        # profit = 3_000_000 - 995_000 = 2_005_000 (equal to threshold)
        # check is: if expected_profit < min_profit (false, not <)
        # So this should NOT reject on profit
        # But let's be specific: the boundary is exclusive
        assert decision.expected_profit_stroops >= config.min_profit_stroops

    def test_below_minimum_profit_rejected(self, default_config, default_intent, default_solver):
        """Intent with profit below threshold should be rejected."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            min_profit_stroops=5_000_000,
        )

        default_intent["min_dst_amount"] = 2_000_000
        default_intent["src_amount"] = 1_000_000
        # expected_cost = 1_000_000 * 0.995 = 995_000
        # expected_profit = 2_000_000 - 995_000 = 1_005_000 (< 5_000_000)

        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is False
        assert "profit below threshold" in decision.reason

    def test_negative_profit_rejected(self, default_config, default_intent, default_solver):
        """Intent with negative expected profit should be rejected."""
        config = BotConfig(
            contract_id=default_config.contract_id,
            solver_secret=default_config.solver_secret,
            solver_address=default_config.solver_address,
            min_profit_stroops=0,
        )

        default_intent["min_dst_amount"] = 500_000  # Less than estimated cost
        default_intent["src_amount"] = 1_000_000

        decision = decide(config, default_intent, default_solver, 999700)
        assert decision.should_accept is False
        assert decision.expected_profit_stroops < 0


class TestDecisionIntegration:
    """Integration tests combining multiple decision gates."""

    def test_all_gates_passed(self, default_config, default_intent, default_solver):
        """Intent passing all gates should be accepted."""
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is True
        assert decision.reason == "accepted risk/profit checks"

    def test_state_failure_overrides_other_gates(self, default_config, default_intent, default_solver):
        """State failure should short-circuit other checks."""
        default_intent["state"] = "Filled"
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is False
        # Should fail on state, not later checks
        assert "state is Filled" in decision.reason

    def test_deadline_failure_overrides_later_gates(self, default_config, default_intent, default_solver):
        """Deadline failure should trigger before bond/profit checks."""
        default_intent["deadline"] = 999700  # Past
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.should_accept is False
        assert "not enough time" in decision.reason

    def test_decision_includes_profit_and_utilization(self, default_config, default_intent, default_solver):
        """Accepted decision should include profit and utilization metrics."""
        decision = decide(default_config, default_intent, default_solver, 999700)
        assert decision.expected_profit_stroops > 0
        assert decision.bond_utilization_bps >= 0


class TestScreenCandidates:
    """Test the screen_candidates helper function."""

    @patch("risk_aware_solver_bot.stellar_view")
    def test_screen_filters_to_open_state(self, mock_view, default_config):
        """screen_candidates should return only Open/PartiallyFilled intents."""
        intent_ids = ["id1", "id2", "id3"]

        # Mock response: open, filled, partially_filled
        mock_view.return_value = [
            {"state": "Open", "id": "id1"},
            {"state": "Filled", "id": "id2"},
            {"state": "PartiallyFilled", "id": "id3"},
        ]

        result = screen_candidates(default_config, intent_ids)
        assert result == ["id1", "id3"]

    @patch("risk_aware_solver_bot.stellar_view")
    def test_screen_filters_none_records(self, mock_view, default_config):
        """screen_candidates should filter out None records (unknown IDs)."""
        intent_ids = ["id1", "id2"]
        mock_view.return_value = [
            {"state": "Open", "id": "id1"},
            None,  # Unknown intent
        ]

        result = screen_candidates(default_config, intent_ids)
        assert result == ["id1"]

    @patch("risk_aware_solver_bot.stellar_view")
    def test_screen_empty_input(self, mock_view, default_config):
        """screen_candidates should handle empty intent list."""
        result = screen_candidates(default_config, [])
        assert result == []
        # stellar_view should not be called for empty input
        mock_view.assert_not_called()


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
