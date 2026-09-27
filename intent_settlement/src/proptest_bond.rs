//! Property-based bond-conservation invariant tests (issue #43).
//!
//! Invariant: at every point in time,
//!
//!   contract.bond_balance == Σ solver_bond(solver, BOND_TOKEN)  (all registered solvers)
//!
//! Issue #187 made bonds per-token. This sequence only ever touches the default
//! bond token, so the per-token invariant for that token is exactly the global
//! invariant here; `bond_amount` is the mirror of `SolverBond(solver, default)`.
//!
//! This file uses `proptest` to generate random but *valid* call sequences from
//! the state-machine below and asserts the invariant holds after every step.
//!
//! The state machine models only the five functions that move bond tokens:
//!   register_solver / deregister_solver / withdraw_bond / accept_intent+slash_solver
//!
//! It does NOT model fill_intent (fills don't touch bond tokens).
//!
//! Run with:
//!   cargo test --test proptest_bond  --features testutils

#![cfg(test)]

// The crate is `#![no_std]`; the proptest harness pulls in `std`, so bring
// `std::vec::Vec` into scope explicitly for the fixture's owned collections.
extern crate std;
use std::vec::Vec;

use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env, String,
};
use std::vec::Vec;

use crate::{IntentSettlement, IntentSettlementClient, FILL_WINDOW, MIN_BOND, SLASH_COOLDOWN};

// ─── Tunables ────────────────────────────────────────────────────────────────────

/// Number of solvers created at setup (fixed pool keeps things tractable).
const N_SOLVERS: usize = 3;
/// Maximum number of steps per proptest run.
const MAX_STEPS: usize = 20;
/// Bond given to every solver at setup (10× MIN_BOND so slashes don't
/// immediately deactivate them, keeping more paths valid).
const STARTING_BOND: i128 = MIN_BOND * 10;
/// Withdraw amount used in Withdraw steps.
const WITHDRAW_AMT: i128 = MIN_BOND; // always valid: STARTING_BOND - MIN_BOND >= MIN_BOND

// ─── Step enumeration ────────────────────────────────────────────────────────────

/// One step in a randomised call sequence.
#[derive(Debug, Clone)]
enum Step {
    /// register_solver (top-up by MIN_BOND for an already-registered solver,
    /// or full STARTING_BOND registration for a deregistered one).
    Register(usize),
    /// deregister_solver (only valid when active_intents == 0).
    Deregister(usize),
    /// withdraw_bond by WITHDRAW_AMT.
    Withdraw(usize),
    /// accept_intent + advance time past FILL_WINDOW + slash_solver (combined
    /// so the sequence is always self-consistent within one step).
    AcceptAndSlash(usize),
}

fn step_strategy() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0..N_SOLVERS).prop_map(Step::Register),
        (0..N_SOLVERS).prop_map(Step::Deregister),
        (0..N_SOLVERS).prop_map(Step::Withdraw),
        (0..N_SOLVERS).prop_map(Step::AcceptAndSlash),
    ]
}

// ─── Fixture ─────────────────────────────────────────────────────────────────────

struct Fixture {
    env: Env,
    contract_id: Address,
    bond_token: Address,
    dst_token: Address,
    user: Address,
    solvers: Vec<Address>,
    /// Whether each solver is currently registered.
    registered: Vec<bool>,
}

impl Fixture {
    fn client(&self) -> IntentSettlementClient<'_> {
        IntentSettlementClient::new(&self.env, &self.contract_id)
    }

    fn bond(&self) -> token::Client<'_> {
        token::Client::new(&self.env, &self.bond_token)
    }

    fn bond_admin(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.bond_token)
    }

    // Kept for symmetry with the unit-test fixture; this proptest models only
    // the bond-moving calls, so no dst-token minting happens here.
    #[allow(dead_code)]
    fn dst_admin(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.dst_token)
    }

    fn pass_time(&self, secs: u64) {
        self.env.ledger().with_mut(|li| li.timestamp += secs);
    }

    /// Sum of all bond_amount fields across registered solvers.
    fn sum_bond_amounts(&self) -> i128 {
        let c = self.client();
        self.solvers
            .iter()
            .filter_map(|s| c.get_solver(s))
            .map(|r| r.bond_amount)
            .sum()
    }

    /// USDC balance held by the contract.
    fn contract_bond_balance(&self) -> i128 {
        self.bond().balance(&self.contract_id)
    }

    /// Assert the invariant: contract balance == Σ bond_amounts.
    fn assert_invariant(&self) {
        let contract_bal = self.contract_bond_balance();
        let sum = self.sum_bond_amounts();
        assert_eq!(
            contract_bal, sum,
            "Bond conservation violated: contract holds {contract_bal} but Σ bond_amounts = {sum}"
        );
    }
}

fn setup_fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    let user = Address::generate(&env);

    let bond_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let dst_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_id = env.register_contract(None, IntentSettlement);

    let ctx = Fixture {
        env,
        contract_id,
        bond_token,
        dst_token,
        user,
        solvers: Vec::new(),
        registered: Vec::new(),
    };

    ctx.client()
        .initialize(&admin, &fee_recipient, &ctx.bond_token);

    // Create solver pool and register everyone.
    let mut solvers = Vec::with_capacity(N_SOLVERS);
    let mut registered = Vec::with_capacity(N_SOLVERS);
    for _ in 0..N_SOLVERS {
        let s = Address::generate(&ctx.env);
        ctx.bond_admin().mint(&s, &STARTING_BOND);
        ctx.client().register_solver(&s, &STARTING_BOND);
        solvers.push(s);
        registered.push(true);
    }

    Fixture {
        solvers,
        registered,
        ..ctx
    }
}

// ─── Step executor ───────────────────────────────────────────────────────────────

fn execute_step(f: &mut Fixture, step: &Step) {
    let c = f.client();

    match step {
        Step::Register(idx) => {
            let solver = &f.solvers[*idx];
            let amount = if f.registered[*idx] {
                // top-up: deposit one more MIN_BOND
                MIN_BOND
            } else {
                // re-register: need at least MIN_BOND
                STARTING_BOND
            };
            f.bond_admin().mint(solver, &amount);
            c.register_solver(solver, &amount);
            f.registered[*idx] = true;
        }

        Step::Deregister(idx) => {
            let solver = &f.solvers[*idx];
            if !f.registered[*idx] {
                return; // already gone, skip
            }
            let record = match c.get_solver(solver) {
                Some(r) => r,
                None => return,
            };
            if record.active_intents > 0 {
                return; // can't deregister with outstanding obligations
            }
            c.deregister_solver(solver);
            f.registered[*idx] = false;
        }

        Step::Withdraw(idx) => {
            let solver = &f.solvers[*idx];
            if !f.registered[*idx] {
                return;
            }
            let record = match c.get_solver(solver) {
                Some(r) => r,
                None => return,
            };
            // Only withdraw if the remainder stays >= MIN_BOND.
            if record.bond_amount - WITHDRAW_AMT < MIN_BOND {
                return;
            }
            c.withdraw_bond(solver, &WITHDRAW_AMT);
        }

        Step::AcceptAndSlash(idx) => {
            let solver = &f.solvers[*idx];
            if !f.registered[*idx] {
                return;
            }
            let record = match c.get_solver(solver) {
                Some(r) => r,
                None => return,
            };
            if !record.is_active {
                return; // deactivated; accept_intent would fail
            }

            // Submit a fresh intent and immediately accept + slash it.
            // Advance past SLASH_COOLDOWN first so a solver slashed in an
            // earlier step is eligible to accept again (accept_intent enforces
            // the post-slash cooldown).
            f.pass_time(SLASH_COOLDOWN + 1);
            let intent_id = c.submit_intent(
                &f.user,
                &String::from_str(&f.env, "ethereum"),
                &String::from_str(&f.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
                &(MIN_BOND / 10), // src_amount (opaque on-chain)
                &f.dst_token,
                &(MIN_BOND / 100), // min_dst_amount
                &(None as Option<u64>),
            );
            let bond_before = c.get_solver(solver).unwrap().bond_amount;
            c.accept_intent(solver, &intent_id);
            f.pass_time(FILL_WINDOW + 1);
            c.slash_solver(&intent_id);
            let bond_after = c.get_solver(solver).unwrap().bond_amount;

            // Issue #193: the slash is now proportional to the *intent* the
            // solver failed to fill, not a flat 10% of the bond. Here the
            // outstanding output is `min_dst_amount = MIN_BOND / 100` and the
            // bond always exceeds it, so the expected slash is
            // `min(min_dst_amount, bond) / 10 = MIN_BOND / 1000`, floored at 1
            // and capped at 10% of the bond (issue #32 / the flat-rate cap).
            // Exact mirror of `IntentSettlement::compute_slash_amount`.
            let unfilled = MIN_BOND / 100;
            let exposure = unfilled.min(bond_before).max(0);
            let cap = ((bond_before / 10_000) * 1_000).min(bond_before).max(1);
            let expected = (exposure / 10).max(1).min(cap);
            assert_eq!(
                bond_before - bond_after,
                expected,
                "issue #193 proportional slash formula"
            );
        }
    }
}

// ─── Proptest entry point ─────────────────────────────────────────────────────────

proptest! {
    /// Generates up to MAX_STEPS random valid operations and asserts the bond
    /// conservation invariant holds after every single step.
    #[test]
    fn bond_conservation_invariant(
        steps in proptest::collection::vec(step_strategy(), 1..=MAX_STEPS)
    ) {
        let mut f = setup_fixture();
        f.assert_invariant(); // invariant must hold at baseline

        for step in &steps {
            execute_step(&mut f, step);
            f.assert_invariant();
        }
    }
}
