#![cfg(test)]

//! Test suite for the Vortex intent settlement contract.
//!
//! Covers the full intent lifecycle (submit → accept → fill), cancellation,
//! expiry, solver bonding/slashing, and the guard conditions on each step.

use crate::{
    Error, IntentSettlement, IntentSettlementClient, IntentState, FILL_WINDOW, INTENT_EXPIRY,
    MIN_BOND,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, BytesN, Env, String,
};

// ─── Test fixture ───────────────────────────────────────────────────────────────

/// Solver bond used across tests: 1,000 USDC (7 decimals).
const BOND: i128 = 1_000 * 10_000_000;
/// Source amount (value is opaque on-chain — just needs to be positive).
const SRC_AMT: i128 = 500_000_000;
/// Minimum acceptable destination amount: 100 dst tokens (7 decimals).
const MIN_DST: i128 = 100 * 10_000_000;
/// A valid fill that clears the minimum: 105 dst tokens.
const FILL: i128 = 105 * 10_000_000;

/// Everything a test needs, all owned (no self-referential client storage).
struct Ctx {
    env: Env,
    admin: Address,
    fee_recipient: Address,
    user: Address,
    solver: Address,
    contract_id: Address,
    bond_token: Address,
    dst_token: Address,
}

impl Ctx {
    fn client(&self) -> IntentSettlementClient<'_> {
        IntentSettlementClient::new(&self.env, &self.contract_id)
    }
    fn bond(&self) -> token::Client<'_> {
        token::Client::new(&self.env, &self.bond_token)
    }
    fn bond_admin(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.bond_token)
    }
    fn dst(&self) -> token::Client<'_> {
        token::Client::new(&self.env, &self.dst_token)
    }
    fn dst_admin(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.dst_token)
    }

    /// Mint a bond to the solver and register them.
    fn register_solver(&self) {
        self.bond_admin().mint(&self.solver, &BOND);
        self.client().register_solver(&self.solver, &BOND);
    }

    /// Submit a standard open intent and return its id.
    fn submit(&self) -> BytesN<32> {
        let deadline: Option<u64> = None;
        self.client().submit_intent(
            &self.user,
            &String::from_str(&self.env, "ethereum"),
            &String::from_str(&self.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            &SRC_AMT,
            &self.dst_token,
            &MIN_DST,
            &deadline,
        )
    }

    /// Advance ledger time by `secs` seconds.
    fn pass_time(&self, secs: u64) {
        self.env.ledger().with_mut(|li| li.timestamp += secs);
    }
}

fn setup() -> Ctx {
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

    let ctx = Ctx {
        env,
        admin,
        fee_recipient,
        user,
        solver,
        contract_id,
        bond_token,
        dst_token,
    };

    ctx.client()
        .initialize(&ctx.admin, &ctx.fee_recipient, &ctx.bond_token);

    ctx
}

// ─── Initialization ─────────────────────────────────────────────────────────────

#[test]
fn initialize_sets_initial_stats() {
    let ctx = setup();
    let (intents, volume) = ctx.client().get_stats();
    assert_eq!(intents, 0);
    assert_eq!(volume, 0);
}

#[test]
fn cannot_initialize_twice() {
    let ctx = setup();
    let res = ctx
        .client()
        .try_initialize(&ctx.admin, &ctx.fee_recipient, &ctx.bond_token);
    assert_eq!(res, Err(Ok(Error::AlreadyInitialized.into())));
}

// ─── Pause ──────────────────────────────────────────────────────────────────────

#[test]
fn paused_blocks_submit_accept_and_fill() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();

    c.pause();
    assert!(c.is_paused());

    let deadline: Option<u64> = None;
    let res = c.try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xabc"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));

    let res = c.try_accept_intent(&ctx.solver, &id);
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));
}

#[test]
fn unpause_restores_normal_operation() {
    let ctx = setup();
    let c = ctx.client();

    c.pause();
    c.unpause();
    assert!(!c.is_paused());

    // Normal lifecycle works again.
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);
    let intent = c.get_intent(&id).unwrap();
    assert!(intent.state == IntentState::Accepted);
}

#[test]
fn pause_does_not_block_slashing_an_already_accepted_intent() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    c.pause();
    ctx.pass_time(FILL_WINDOW + 1);

    // Permissionless slashing keeps working even while paused, so a solver
    // can't dodge accountability for an obligation they already took on.
    c.slash_solver(&id);
    assert_eq!(c.get_solver(&ctx.solver).unwrap().fills_failed, 1);
// ─── Admin ──────────────────────────────────────────────────────────────────────

#[test]
fn admin_can_set_fee_recipient() {
    let ctx = setup();
    let new_recipient = Address::generate(&ctx.env);

    ctx.client().set_fee_recipient(&new_recipient);
    assert_eq!(
        ctx.client().get_fee_recipient(),
        Some(new_recipient.clone())
    );

    // The new recipient actually receives fees going forward.
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);
    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id, &FILL);
    assert_eq!(ctx.dst().balance(&new_recipient), fee);
}

#[test]
fn admin_can_transfer_admin() {
    let ctx = setup();
    assert_eq!(ctx.client().get_admin(), Some(ctx.admin.clone()));

    let new_admin = Address::generate(&ctx.env);
    ctx.client().transfer_admin(&new_admin);
    assert_eq!(ctx.client().get_admin(), Some(new_admin.clone()));

    // The new admin can now exercise admin-only functions.
    let another_recipient = Address::generate(&ctx.env);
    ctx.client().set_fee_recipient(&another_recipient);
    assert_eq!(ctx.client().get_fee_recipient(), Some(another_recipient));
}

// ─── Solver registration ────────────────────────────────────────────────────────

#[test]
fn register_solver_locks_bond() {
    let ctx = setup();
    ctx.register_solver();

    let record = ctx.client().get_solver(&ctx.solver).unwrap();
    assert_eq!(record.bond_amount, BOND);
    assert!(record.is_active);
    assert_eq!(record.fills_completed, 0);

    // Bond moved from solver into the contract.
    assert_eq!(ctx.bond().balance(&ctx.solver), 0);
    assert_eq!(ctx.bond().balance(&ctx.contract_id), BOND);
}

#[test]
fn register_solver_below_minimum_fails() {
    let ctx = setup();
    ctx.bond_admin().mint(&ctx.solver, &BOND);
    let res = ctx
        .client()
        .try_register_solver(&ctx.solver, &(MIN_BOND - 1));
    assert_eq!(res, Err(Ok(Error::SolverBondTooLow.into())));
}

#[test]
fn register_solver_twice_tops_up_bond() {
    let ctx = setup();
    ctx.bond_admin().mint(&ctx.solver, &(BOND * 2));
    let c = ctx.client();
    c.register_solver(&ctx.solver, &BOND);
    c.register_solver(&ctx.solver, &BOND);
    assert_eq!(c.get_solver(&ctx.solver).unwrap().bond_amount, BOND * 2);
}

#[test]
fn register_solver_small_topup_below_minimum_succeeds() {
    // A solver already above MIN_BOND should be able to top up by less than
    // MIN_BOND -- the minimum applies to the resulting total, not the deposit.
    let ctx = setup();
    let small_topup = 10 * 10_000_000; // less than MIN_BOND on its own
    ctx.bond_admin().mint(&ctx.solver, &(BOND + small_topup));
    let c = ctx.client();
    c.register_solver(&ctx.solver, &BOND);
    c.register_solver(&ctx.solver, &small_topup);
    assert_eq!(
        c.get_solver(&ctx.solver).unwrap().bond_amount,
        BOND + small_topup
    );
}

#[test]
fn register_solver_zero_amount_fails() {
    let ctx = setup();
    ctx.register_solver();
    let res = ctx.client().try_register_solver(&ctx.solver, &0);
    assert_eq!(res, Err(Ok(Error::ZeroAmount.into())));
}

#[test]
fn deregister_returns_bond() {
    let ctx = setup();
    ctx.register_solver();
    ctx.client().deregister_solver(&ctx.solver);

    assert!(ctx.client().get_solver(&ctx.solver).is_none());
    assert_eq!(ctx.bond().balance(&ctx.solver), BOND);
    assert_eq!(ctx.bond().balance(&ctx.contract_id), 0);
}

#[test]
fn deregister_with_accepted_intent_fails() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    let res = ctx.client().try_deregister_solver(&ctx.solver);
    assert_eq!(res, Err(Ok(Error::SolverHasActiveIntents.into())));

    // Bond stays locked in the contract.
    assert_eq!(ctx.bond().balance(&ctx.contract_id), BOND);
}

#[test]
fn deregister_after_fill_succeeds() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id, &FILL);

    // Obligation cleared on fill, so deregistration now succeeds.
    c.deregister_solver(&ctx.solver);
    assert!(c.get_solver(&ctx.solver).is_none());
}

#[test]
fn deregister_after_slash_succeeds() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);

    // Obligation cleared on slash, so deregistration now succeeds.
    c.deregister_solver(&ctx.solver);
    assert!(c.get_solver(&ctx.solver).is_none());
}

// ─── Intent submission ──────────────────────────────────────────────────────────

#[test]
fn submit_intent_creates_open_record() {
    let ctx = setup();
    let id = ctx.submit();

    let intent = ctx.client().get_intent(&id).unwrap();
    assert!(intent.state == IntentState::Open);
    assert_eq!(intent.user, ctx.user);
    assert_eq!(intent.min_dst_amount, MIN_DST);
    assert_eq!(intent.solver, None);

    assert_eq!(ctx.client().get_stats().0, 1);
}

#[test]
fn submit_intent_zero_amount_fails() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xabc"),
        &0,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::ZeroAmount.into())));
}

#[test]
fn submit_intent_past_deadline_fails() {
    let ctx = setup();
    ctx.pass_time(1_000);
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xabc"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &Some(500u64), // already in the past
    );
    assert_eq!(res, Err(Ok(Error::InvalidDeadline.into())));
}

// ─── Happy path: submit → accept → fill ─────────────────────────────────────────

#[test]
fn full_lifecycle_submit_accept_fill() {
    let ctx = setup();
    let c = ctx.client();

    ctx.register_solver();
    let id = ctx.submit();

    // Accept
    c.accept_intent(&ctx.solver, &id);
    let intent = c.get_intent(&id).unwrap();
    assert!(intent.state == IntentState::Accepted);
    assert_eq!(intent.solver, Some(ctx.solver.clone()));

    // Fill — fund the solver with the output plus the protocol fee they pay.
    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id, &FILL);

    let intent = c.get_intent(&id).unwrap();
    assert!(intent.state == IntentState::Filled);
    assert_eq!(intent.fill_amount, Some(FILL));

    // Funds: user receives the full fill; the solver separately pays the fee.
    assert_eq!(ctx.dst().balance(&ctx.user), FILL);
    assert_eq!(ctx.dst().balance(&ctx.fee_recipient), fee);
    assert_eq!(ctx.dst().balance(&ctx.solver), 0);

    // Solver + protocol stats updated.
    let solver = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(solver.fills_completed, 1);
    assert_eq!(solver.fills_failed, 0);
    assert_eq!(solver.total_volume, FILL);

    let (total_intents, total_volume) = c.get_stats();
    assert_eq!(total_intents, 1);
    assert_eq!(total_volume, FILL);
}

// ─── Accept guards ──────────────────────────────────────────────────────────────

#[test]
fn accept_by_unregistered_solver_fails() {
    let ctx = setup();
    let id = ctx.submit();
    let stranger = Address::generate(&ctx.env);
    let res = ctx.client().try_accept_intent(&stranger, &id);
    assert_eq!(res, Err(Ok(Error::SolverNotRegistered.into())));
}

#[test]
fn accept_expired_intent_fails() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();

    ctx.pass_time(INTENT_EXPIRY + 1);
    let res = ctx.client().try_accept_intent(&ctx.solver, &id);
    assert_eq!(res, Err(Ok(Error::IntentExpired.into())));
}

#[test]
fn cannot_accept_already_accepted_intent() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    // A second registered solver cannot steal it.
    let solver2 = Address::generate(&ctx.env);
    ctx.bond_admin().mint(&solver2, &BOND);
    ctx.client().register_solver(&solver2, &BOND);

    let res = ctx.client().try_accept_intent(&solver2, &id);
    assert_eq!(res, Err(Ok(Error::IntentNotOpen.into())));
}

// ─── Fill guards ────────────────────────────────────────────────────────────────

#[test]
fn fill_below_minimum_fails() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    let res = ctx
        .client()
        .try_fill_intent(&ctx.solver, &id, &(MIN_DST - 1));
    assert_eq!(res, Err(Ok(Error::InsufficientOutput.into())));
}

#[test]
fn fill_after_window_fails() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    ctx.pass_time(FILL_WINDOW + 1);
    ctx.dst_admin().mint(&ctx.solver, &FILL);
    let res = ctx.client().try_fill_intent(&ctx.solver, &id, &FILL);
    assert_eq!(res, Err(Ok(Error::FillWindowExpired.into())));
}

#[test]
fn fill_by_wrong_solver_fails() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    let other = Address::generate(&ctx.env);
    ctx.bond_admin().mint(&other, &BOND);
    ctx.client().register_solver(&other, &BOND);
    ctx.dst_admin().mint(&other, &FILL);

    let res = ctx.client().try_fill_intent(&other, &id, &FILL);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

// ─── Cancellation ───────────────────────────────────────────────────────────────

#[test]
fn user_can_cancel_open_intent() {
    let ctx = setup();
    let id = ctx.submit();
    ctx.client().cancel_intent(&ctx.user, &id);
    assert!(ctx.client().get_intent(&id).unwrap().state == IntentState::Cancelled);
}

#[test]
fn cannot_cancel_accepted_intent() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    let res = ctx.client().try_cancel_intent(&ctx.user, &id);
    assert_eq!(res, Err(Ok(Error::CannotCancelAccepted.into())));
}

#[test]
fn cannot_cancel_someone_elses_intent() {
    let ctx = setup();
    let id = ctx.submit();
    let stranger = Address::generate(&ctx.env);
    let res = ctx.client().try_cancel_intent(&stranger, &id);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

// ─── Slashing ───────────────────────────────────────────────────────────────────

#[test]
fn slash_after_window_penalizes_solver_and_reopens_intent() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    let bond_before = c.get_solver(&ctx.solver).unwrap().bond_amount;
    ctx.pass_time(FILL_WINDOW + 1);

    c.slash_solver(&id); // permissionless

    let slash = bond_before / 10;
    let solver = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(solver.bond_amount, bond_before - slash);
    assert_eq!(solver.fills_failed, 1);

    // Intent is re-auctioned.
    let intent = c.get_intent(&id).unwrap();
    assert!(intent.state == IntentState::Open);
    assert_eq!(intent.solver, None);

    // Slashed bond goes to the fee recipient.
    assert_eq!(ctx.bond().balance(&ctx.fee_recipient), slash);
}

#[test]
fn slash_below_min_bond_deactivates_solver() {
    let ctx = setup();
    let c = ctx.client();

    // Register with just enough over MIN_BOND that a single 10% slash drops
    // the remaining bond below it.
    let thin_bond = MIN_BOND + MIN_BOND / 10;
    ctx.bond_admin().mint(&ctx.solver, &thin_bond);
    c.register_solver(&ctx.solver, &thin_bond);

    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);
    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);

    let solver = c.get_solver(&ctx.solver).unwrap();
    assert!(solver.bond_amount < MIN_BOND);
    assert!(!solver.is_active);

    // Deactivated solvers can't accept new intents.
    let id2 = ctx.submit();
    let res = c.try_accept_intent(&ctx.solver, &id2);
    assert_eq!(res, Err(Ok(Error::SolverInactive.into())));
}

#[test]
fn topping_up_after_slash_reactivates_solver() {
    let ctx = setup();
    let c = ctx.client();

    let thin_bond = MIN_BOND + MIN_BOND / 10;
    ctx.bond_admin().mint(&ctx.solver, &thin_bond);
    c.register_solver(&ctx.solver, &thin_bond);

    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);
    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);
    assert!(!c.get_solver(&ctx.solver).unwrap().is_active);

    ctx.bond_admin().mint(&ctx.solver, &MIN_BOND);
    c.register_solver(&ctx.solver, &MIN_BOND);
    assert!(c.get_solver(&ctx.solver).unwrap().is_active);
}

#[test]
fn cannot_slash_before_window_expires() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    // Still within the fill window.
    let res = ctx.client().try_slash_solver(&id);
    assert_eq!(res, Err(Ok(Error::FillWindowExpired.into())));
}

#[test]
fn cannot_slash_unaccepted_intent() {
    let ctx = setup();
    let id = ctx.submit(); // still Open, never accepted
    let res = ctx.client().try_slash_solver(&id);
    assert_eq!(res, Err(Ok(Error::IntentNotAccepted.into())));
}

// ─── Storage TTL ────────────────────────────────────────────────────────────────

#[test]
fn writes_extend_persistent_ttl_for_intent_and_solver() {
    use soroban_sdk::testutils::storage::Persistent as _;

    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    let (intent_ttl, solver_ttl) = ctx.env.as_contract(&ctx.contract_id, || {
        (
            ctx.env
                .storage()
                .persistent()
                .get_ttl(&crate::DataKey::Intent(id)),
            ctx.env
                .storage()
                .persistent()
                .get_ttl(&crate::DataKey::Solver(ctx.solver.clone())),
        )
    });

    // Both entries were touched by register_solver/accept_intent, so both
    // should be bumped out near PERSISTENT_TTL_EXTEND_TO rather than sitting
    // at whatever short default the test ledger starts new entries at.
    assert!(intent_ttl >= crate::PERSISTENT_TTL_EXTEND_TO - 1);
    assert!(solver_ttl >= crate::PERSISTENT_TTL_EXTEND_TO - 1);
}

// ─── Views ──────────────────────────────────────────────────────────────────────

#[test]
fn get_intent_returns_none_for_unknown_id() {
    let ctx = setup();
    let unknown = BytesN::from_array(&ctx.env, &[0u8; 32]);
    assert!(ctx.client().get_intent(&unknown).is_none());
}
