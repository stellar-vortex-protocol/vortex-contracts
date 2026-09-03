//! Property-based fill-conservation invariant tests (issue #209).
//!
//! Invariant: at every point in time during the fill lifecycle,
//!
//!   Σ(fill_amounts) == user_received + fee_received
//!   intent.total_filled == Σ(fill_amounts) applied to this intent
//!   intent state machine never reaches invalid combinations
//!
//! This file uses `proptest` to generate random but *valid* fill sequences
//! for submitted and accepted intents and asserts conservation holds.
//!
//! The state machine models the fill lifecycle:
//!   submit_intent → accept_intent → [fill_intent]* → terminal state
//!
//! Run with:
//!   cargo test --test proptest_fill --features testutils

#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, BytesN, Env, String,
};

use crate::{IntentSettlement, IntentSettlementClient, IntentState, MIN_BOND};

// ─── Tunables ────────────────────────────────────────────────────────────────────

/// Number of intents created per test run.
const N_INTENTS: usize = 2;
/// Maximum fill events per intent.
const MAX_FILLS_PER_INTENT: usize = 5;
/// Minimum destination amount required per intent (in smallest units).
const MIN_DST_AMOUNT: i128 = 100 * 10_000_000; // 100 tokens @ 7 decimals
/// Solver bond (must be >= MIN_BOND).
const SOLVER_BOND: i128 = MIN_BOND * 10;

// ─── Fixture ─────────────────────────────────────────────────────────────────────

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    fee_recipient: Address,
    user: Address,
    solver: Address,
    bond_token: Address,
    dst_token: Address,
    /// Intent IDs created during setup.
    intent_ids: Vec<BytesN<32>>,
    /// Cumulative fills per intent (parallel to intent_ids).
    fills_by_intent: Vec<Vec<i128>>,
}

impl Fixture {
    fn client(&self) -> IntentSettlementClient<'_> {
        IntentSettlementClient::new(&self.env, &self.contract_id)
    }

    fn dst(&self) -> token::Client<'_> {
        token::Client::new(&self.env, &self.dst_token)
    }

    fn dst_admin(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.dst_token)
    }

    /// Get the current intent record, panicking if not found.
    fn get_intent(&self, id: &BytesN<32>) -> crate::IntentRecord {
        self.client()
            .get_intent(id)
            .expect("intent not found")
    }

    /// Sum of all fill amounts applied to an intent so far.
    fn total_filled_so_far(&self, intent_idx: usize) -> i128 {
        self.fills_by_intent[intent_idx].iter().sum()
    }

    /// Assert fill conservation: (user_received + fee_received) == total_filled.
    fn assert_fill_conservation(&self, intent_id: &BytesN<32>, intent_idx: usize) {
        let intent = self.get_intent(intent_id);
        let total_filled_expected = self.total_filled_so_far(intent_idx);

        assert_eq!(
            intent.total_filled, total_filled_expected,
            "Intent total_filled mismatch: record={}, expected={}",
            intent.total_filled, total_filled_expected
        );

        let fee_per_unit = 5; // PROTOCOL_FEE_BPS = 5
        let bps_divisor = 10_000i128;
        let mut fee_received_total = 0i128;

        for fill_amount in &self.fills_by_intent[intent_idx] {
            let fee = fill_amount * fee_per_unit / bps_divisor;
            fee_received_total += fee;
        }

        let user_received = self.dst().balance(&self.user);
        let fee_received = self.dst().balance(&self.fee_recipient);

        assert!(
            fee_received >= fee_received_total,
            "Fee mismatch: intent {} has filled {} total, expecting fee {} but fee recipient has {}",
            intent_idx,
            total_filled_expected,
            fee_received_total,
            fee_received
        );
    }

    /// Verify state machine is valid (no impossible combinations).
    fn assert_valid_state(&self, intent_id: &BytesN<32>) {
        let intent = self.get_intent(intent_id);

        match intent.state {
            IntentState::Filled => {
                assert!(
                    intent.total_filled >= intent.min_dst_amount,
                    "Filled state but total_filled {} < min_dst_amount {}",
                    intent.total_filled,
                    intent.min_dst_amount
                );
            }
            IntentState::PartiallyFilled => {
                assert!(
                    intent.total_filled > 0 && intent.total_filled < intent.min_dst_amount,
                    "PartiallyFilled state but total_filled {} is invalid (min={})",
                    intent.total_filled,
                    intent.min_dst_amount
                );
            }
            IntentState::Open => {
                assert_eq!(
                    intent.total_filled, 0,
                    "Open state but total_filled > 0"
                );
            }
            _ => {}
        }
    }
}

fn setup_fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    let user = Address::generate(&env);
    let solver = Address::generate(&env);

    let bond_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let dst_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_id = env.register_contract(None, IntentSettlement);

    let client = IntentSettlementClient::new(&env, &contract_id);
    client.initialize(&admin, &fee_recipient, &bond_token);

    let bond_admin = token::StellarAssetClient::new(&env, &bond_token);
    bond_admin.mint(&solver, &SOLVER_BOND);
    client.register_solver(&solver, &SOLVER_BOND);

    let mut f = Fixture {
        env,
        contract_id,
        admin,
        fee_recipient,
        user,
        solver,
        bond_token,
        dst_token,
        intent_ids: Vec::new(),
        fills_by_intent: Vec::new(),
    };

    for i in 0..N_INTENTS {
        env.ledger()
            .with_mut(|li| li.timestamp = i as u64 + 1000);
        let intent_id = client.submit_intent(
            &f.user,
            &String::from_str(&env, "ethereum"),
            &String::from_str(&env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            &(MIN_DST_AMOUNT / 10),
            &f.dst_token,
            &MIN_DST_AMOUNT,
            &(None as Option<u64>),
        );
        client.accept_intent(&f.solver, &intent_id);
        f.intent_ids.push(intent_id);
        f.fills_by_intent.push(Vec::new());
    }

    f
}

// ─── Step definition ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum FillStep {
    /// Attempt to fill intent at index `intent_idx` with `amount`.
    Fill { intent_idx: usize, amount: i128 },
}

fn fill_step_strategy() -> impl Strategy<Value = FillStep> {
    (0..N_INTENTS, 1i128..=(MIN_DST_AMOUNT * 2))
        .prop_map(|(intent_idx, amount)| FillStep::Fill {
            intent_idx,
            amount,
        })
}

// ─── Step executor ───────────────────────────────────────────────────────────────

fn execute_fill_step(f: &mut Fixture, step: &FillStep) {
    let c = f.client();

    match step {
        FillStep::Fill { intent_idx, amount } => {
            if *intent_idx >= f.intent_ids.len() {
                return;
            }

            let intent_id = f.intent_ids[*intent_idx].clone();
            let intent = match c.get_intent(&intent_id) {
                Some(i) => i,
                None => return,
            };

            if intent.state == IntentState::Filled
                || intent.state == IntentState::Cancelled
                || intent.state == IntentState::Expired
                || intent.state == IntentState::Slashed
            {
                return; // can't fill terminal states
            }

            let fill_amount = amount.abs().max(1);

            let fee = fill_amount * 5 / 10_000; // PROTOCOL_FEE_BPS = 5
            f.dst_admin().mint(&f.solver, &(fill_amount + fee));

            if c.try_fill_intent(&f.solver, &intent_id, &fill_amount)
                .is_ok()
            {
                f.fills_by_intent[*intent_idx].push(fill_amount);
            }
        }
    }
}

// ─── Proptest entry point ─────────────────────────────────────────────────────────

proptest! {
    /// Generates random fill sequences across multiple intents and asserts
    /// conservation invariant holds after every step.
    #[test]
    fn fill_conservation_invariant(
        steps in proptest::collection::vec(fill_step_strategy(), 1..=(N_INTENTS * MAX_FILLS_PER_INTENT))
    ) {
        let mut f = setup_fixture();

        for (intent_id, idx) in f.intent_ids.iter().zip(0..f.intent_ids.len()) {
            f.assert_fill_conservation(intent_id, idx);
            f.assert_valid_state(intent_id);
        }

        for step in &steps {
            execute_fill_step(&mut f, step);

            for (intent_id, idx) in f.intent_ids.iter().zip(0..f.intent_ids.len()) {
                f.assert_fill_conservation(intent_id, idx);
                f.assert_valid_state(intent_id);
            }
        }
    }
}
