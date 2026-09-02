#![cfg(test)]

//! Test suite for the Vortex intent settlement contract.
//!
//! Covers the full intent lifecycle (submit → accept → fill), cancellation,
//! expiry, solver bonding/slashing, and the guard conditions on each step.

use crate::{
    DataKey, Error, IntentSettlement, IntentSettlementClient, IntentState, SolverRecord,
    FILL_WINDOW, INTENT_EXPIRY, MIN_BOND, ADMIN_TIMELOCK_DELAY,
};
// Issue #190: proof-gated fill tests drive the real ProofRegistry contract and
// inject records through its `mock_set_proof` testutils entry-point.
use vortex_proof_registry::{ProofRecord, ProofRegistry, ProofRegistryClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, BytesN, Env, String, Symbol,
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
            &None,
        )
    }

    /// Advance ledger time by `secs` seconds.
    fn pass_time(&self, secs: u64) {
        self.env.ledger().with_mut(|li| li.timestamp += secs);
    }

    /// Propose + (after the timelock) execute adding `token` to the dst_token
    /// allowlist, in one call. Convenience wrapper for tests that only care
    /// about the end state (#115/#118 replaced the old one-step
    /// `add_allowed_dst_token` with a propose/execute flow).
    fn allow_dst_token(&self, token: &Address) {
        self.client().propose_add_dst_token(token);
        self.pass_time(ADMIN_TIMELOCK_DELAY);
        self.client().execute_add_dst_token(token);
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
    let (intents, volume, open) = ctx.client().get_stats();
    assert_eq!(intents, 0);
    assert_eq!(volume, 0);
    assert_eq!(open, 0);
}

#[test]
fn cannot_initialize_twice() {
    let ctx = setup();
    let res = ctx
        .client()
        .try_initialize(&ctx.admin, &ctx.fee_recipient, &ctx.bond_token);
    assert_eq!(res, Err(Ok(Error::AlreadyInitialized.into())));
}

/// Issue #148: a second `initialize` call must be rejected *and* must not
/// mutate any of the addresses recorded by the first call.
///
/// `cannot_initialize_twice` above only re-passes the original arguments, so it
/// cannot catch an implementation that accepts the second call and silently
/// overwrites `Admin` / `FeeRecipient` / `BondToken`. Here the second call
/// supplies three brand-new, distinct addresses; we assert it fails with
/// `AlreadyInitialized` and that every stored address still equals the value
/// from the first call.
#[test]
fn initialize_rejects_second_call_and_keeps_original_config() {
    let ctx = setup();

    // Snapshot the state established by `setup()`'s first `initialize`.
    let admin_before = ctx.client().get_admin();
    let fee_recipient_before = ctx.client().get_fee_recipient();
    let bond_token_before = ctx.client().get_bond_token();
    assert_eq!(admin_before, Some(ctx.admin.clone()));
    assert_eq!(fee_recipient_before, Some(ctx.fee_recipient.clone()));
    assert_eq!(bond_token_before, Some(ctx.bond_token.clone()));

    // Attempt a second initialization with entirely different parameters.
    let other_admin = Address::generate(&ctx.env);
    let other_fee_recipient = Address::generate(&ctx.env);
    let other_bond_token = ctx
        .env
        .register_stellar_asset_contract_v2(other_admin.clone())
        .address();
    assert_ne!(other_admin, ctx.admin);
    assert_ne!(other_fee_recipient, ctx.fee_recipient);
    assert_ne!(other_bond_token, ctx.bond_token);

    let res = ctx
        .client()
        .try_initialize(&other_admin, &other_fee_recipient, &other_bond_token);
    assert_eq!(res, Err(Ok(Error::AlreadyInitialized.into())));

    // Nothing was reset: the rejected call had no side effects.
    assert_eq!(ctx.client().get_admin(), admin_before);
    assert_eq!(ctx.client().get_fee_recipient(), fee_recipient_before);
    assert_eq!(ctx.client().get_bond_token(), bond_token_before);
}

// ─── Admin ──────────────────────────────────────────────────────────────────────

#[test]
fn admin_can_propose_and_accept_fee_recipient() {
    let ctx = setup();
    let new_recipient = Address::generate(&ctx.env);

    // Step 1: admin proposes
    ctx.client().propose_fee_recipient(&new_recipient);
    let (pending, eta) = ctx.client().get_pending_fee_recipient().unwrap();
    assert_eq!(pending, new_recipient);
    // Active recipient unchanged until accepted
    assert_eq!(
        ctx.client().get_fee_recipient(),
        Some(ctx.fee_recipient.clone())
    );

    // Accepting before the timelock delay elapses is rejected (#115).
    let res = ctx.client().try_accept_fee_recipient(&new_recipient);
    assert_eq!(res, Err(Ok(Error::TimelockNotElapsed.into())));

    // Step 2: new recipient accepts once the timelock has elapsed
    ctx.pass_time(eta - ctx.env.ledger().timestamp());
    ctx.client().accept_fee_recipient(&new_recipient);
    assert_eq!(
        ctx.client().get_fee_recipient(),
        Some(new_recipient.clone())
    );
    assert_eq!(ctx.client().get_pending_fee_recipient(), None);

    // The new recipient actually receives fees going forward.
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);
    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id, &FILL, &false);
    assert_eq!(ctx.dst().balance(&new_recipient), fee);
}

/// #30: A non-pending address cannot hijack the accept step.
#[test]
fn accept_fee_recipient_wrong_address_fails() {
    let ctx = setup();
    let new_recipient = Address::generate(&ctx.env);
    let imposter = Address::generate(&ctx.env);

    ctx.client().propose_fee_recipient(&new_recipient);

    let res = ctx.client().try_accept_fee_recipient(&imposter);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));

    // Original fee recipient unchanged.
    assert_eq!(
        ctx.client().get_fee_recipient(),
        Some(ctx.fee_recipient.clone())
    );
}

/// #30: Calling accept before propose fails cleanly.
#[test]
fn accept_fee_recipient_without_proposal_fails() {
    let ctx = setup();
    let addr = Address::generate(&ctx.env);
    let res = ctx.client().try_accept_fee_recipient(&addr);
    assert_eq!(res, Err(Ok(Error::NoPendingFeeRecipient.into())));
}

#[test]
fn admin_can_transfer_admin() {
    let ctx = setup();
    assert_eq!(ctx.client().get_admin(), Some(ctx.admin.clone()));

    // #115/#116: transfer_admin is now a timelocked propose/accept flow.
    let new_admin = Address::generate(&ctx.env);
    ctx.client().propose_admin_transfer(&new_admin);
    let (pending, eta) = ctx.client().get_pending_admin().unwrap();
    assert_eq!(pending, new_admin);
    assert_eq!(ctx.client().get_admin(), Some(ctx.admin.clone()));

    // Accepting before the timelock delay elapses is rejected.
    let res = ctx.client().try_accept_admin_transfer(&new_admin);
    assert_eq!(res, Err(Ok(Error::TimelockNotElapsed.into())));

    ctx.pass_time(eta - ctx.env.ledger().timestamp());
    ctx.client().accept_admin_transfer(&new_admin);
    assert_eq!(ctx.client().get_admin(), Some(new_admin.clone()));
    assert_eq!(ctx.client().get_pending_admin(), None);

    // The new admin can now exercise admin-only functions — use the two-step
    // propose/accept flow that replaced set_fee_recipient (issue #30).
    let another_recipient = Address::generate(&ctx.env);
    ctx.client().propose_fee_recipient(&another_recipient);
    ctx.pass_time(ADMIN_TIMELOCK_DELAY);
    ctx.client().accept_fee_recipient(&another_recipient);
    assert_eq!(ctx.client().get_fee_recipient(), Some(another_recipient));
}

// ─── Pause ──────────────────────────────────────────────────────────────────────

#[test]
fn paused_blocks_submit_accept_and_fill() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();

    c.pause(&ctx.admin);
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

    c.pause(&ctx.admin);
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

    c.pause(&ctx.admin);
    ctx.pass_time(FILL_WINDOW + 1);

    // Permissionless slashing keeps working even while paused, so a solver
    // can't dodge accountability for an obligation they already took on.
    c.slash_solver(&id);
    assert_eq!(c.get_solver(&ctx.solver).unwrap().fills_failed, 1);
}

// ─── #120 Pauser role ─────────────────────────────────────────────────────────

#[test]
fn admin_can_set_pauser() {
    let ctx = setup();
    let c = ctx.client();
    assert_eq!(c.get_pauser(), None);

    let pauser = Address::generate(&ctx.env);
    c.set_pauser(&pauser);
    assert_eq!(c.get_pauser(), Some(pauser));
}

#[test]
fn set_pauser_only_admin_can_call() {
    let ctx = setup();
    let c = ctx.client();
    let pauser = Address::generate(&ctx.env);

    // With mock_all_auths, verify that the admin auth is recorded by the
    // set_pauser call, the same way rescue_tokens_only_admin_can_call does.
    c.set_pauser(&pauser);

    let auths = ctx.env.auths();
    let admin_authed = auths.iter().any(|(addr, _)| *addr == ctx.admin);
    assert!(
        admin_authed,
        "set_pauser must require admin auth; got: {:?}",
        auths
    );
}

#[test]
fn pauser_can_pause_without_admin_key() {
    let ctx = setup();
    let c = ctx.client();
    let pauser = Address::generate(&ctx.env);
    c.set_pauser(&pauser);

    c.pause(&pauser);
    assert!(c.is_paused());
}

#[test]
fn pause_rejects_caller_who_is_neither_admin_nor_pauser() {
    let ctx = setup();
    let c = ctx.client();
    let pauser = Address::generate(&ctx.env);
    c.set_pauser(&pauser);

    let stranger = Address::generate(&ctx.env);
    let res = c.try_pause(&stranger);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
    assert!(!c.is_paused());
}

#[test]
fn pauser_cannot_unpause() {
    let ctx = setup();
    let c = ctx.client();
    let pauser = Address::generate(&ctx.env);
    c.set_pauser(&pauser);
    c.pause(&pauser);

    // unpause takes no caller argument -- it always requires the stored
    // admin's auth specifically, so under mock_all_auths this call succeeds
    // mechanically, but only the admin address is ever the one authorized.
    c.unpause();
    let auths = ctx.env.auths();
    let admin_authed = auths.iter().any(|(addr, _)| *addr == ctx.admin);
    assert!(
        admin_authed,
        "unpause must require admin auth, not the pauser; got: {:?}",
        auths
    );
#[test]
fn pause_blocks_fill_intent() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    c.pause();

    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    let res = c.try_fill_intent(&ctx.solver, &id, &FILL, &false);
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));
}

#[test]
fn pause_does_not_block_cancel_intent() {
    let ctx = setup();
    let c = ctx.client();
    let id = ctx.submit();

    c.pause();
    assert!(c.is_paused());

    // cancel_intent should succeed even while paused
    c.cancel_intent(&ctx.user, &id);
    assert!(c.get_intent(&id).unwrap().state == IntentState::Cancelled);
}

#[test]
fn pause_blocks_submit_accept_fill_but_allows_cancel_and_slash() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    // Submit and accept before pausing
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    // Submit another intent to test that it can't be accepted while paused
    let id2 = ctx.submit();

    c.pause();
    assert!(c.is_paused());

    // Test blocked operations
    let deadline: Option<u64> = None;
    let res = c.try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xdef"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));

    let res = c.try_accept_intent(&ctx.solver, &id2);
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));

    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    let res = c.try_fill_intent(&ctx.solver, &id, &FILL, &false);
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));

    // Test allowed operations
    let id3 = ctx.submit();
    c.cancel_intent(&ctx.user, &id3);
    assert!(c.get_intent(&id3).unwrap().state == IntentState::Cancelled);

    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);
    assert_eq!(c.get_solver(&ctx.solver).unwrap().fills_failed, 1);
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
fn is_solver_eligible_reflects_registration_and_bond_state() {
    let ctx = setup();
    let c = ctx.client();

    // Never registered.
    assert!(!c.is_solver_eligible(&ctx.solver));

    ctx.register_solver();
    assert!(c.is_solver_eligible(&ctx.solver));

    // Deactivated by a slash that drops bond below MIN_BOND.
    let thin_bond = MIN_BOND + MIN_BOND / 10;
    let other = Address::generate(&ctx.env);
    ctx.bond_admin().mint(&other, &thin_bond);
    c.register_solver(&other, &thin_bond);
    let id = ctx.submit();
    c.accept_intent(&other, &id);
    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);
    assert!(!c.is_solver_eligible(&other));
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
fn register_solver_new_with_exact_min_bond_succeeds() {
    // New solver registering with exactly MIN_BOND (not above) should succeed.
    let ctx = setup();
    ctx.bond_admin().mint(&ctx.solver, &MIN_BOND);
    let c = ctx.client();
    c.register_solver(&ctx.solver, &MIN_BOND);

    let record = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(record.bond_amount, MIN_BOND);
    assert!(record.is_active);
}

#[test]
fn register_solver_topup_to_exact_min_bond_succeeds() {
    // Existing solver topping up to land exactly at MIN_BOND total should succeed.
    // First: register with half of MIN_BOND
    let ctx = setup();
    let half_min = MIN_BOND / 2;
    ctx.bond_admin().mint(&ctx.solver, &MIN_BOND);
    let c = ctx.client();
    c.register_solver(&ctx.solver, &half_min);

    // Top up by another half to reach exactly MIN_BOND
    c.register_solver(&ctx.solver, &half_min);

    let record = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(record.bond_amount, MIN_BOND);
    assert!(record.is_active);
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
fn deregister_returns_exact_bond_amount_after_topup() {
    // Solver registers, then tops up with additional deposits.
    // Deregistration should return the exact accumulated total.
    let ctx = setup();
    let topup1 = 100 * 10_000_000;
    let topup2 = 200 * 10_000_000;
    let total_expected = BOND + topup1 + topup2;

    ctx.bond_admin().mint(&ctx.solver, &total_expected);
    let c = ctx.client();

    // First deposit
    c.register_solver(&ctx.solver, &BOND);
    // Top up with additional amounts
    c.register_solver(&ctx.solver, &topup1);
    c.register_solver(&ctx.solver, &topup2);

    // Verify accumulated bond
    assert_eq!(
        c.get_solver(&ctx.solver).unwrap().bond_amount,
        total_expected
    );

    // Deregister and verify exact return
    c.deregister_solver(&ctx.solver);
    assert!(c.get_solver(&ctx.solver).is_none());
    assert_eq!(ctx.bond().balance(&ctx.solver), total_expected);
    assert_eq!(ctx.bond().balance(&ctx.contract_id), 0);
}

#[test]
fn withdraw_bond_reduces_balance_without_deregistering() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    let withdraw_amount = 100 * 10_000_000;
    c.withdraw_bond(&ctx.solver, &withdraw_amount);

    let record = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(record.bond_amount, BOND - withdraw_amount);
    assert!(record.is_active);
    assert_eq!(ctx.bond().balance(&ctx.solver), withdraw_amount);
    assert_eq!(ctx.bond().balance(&ctx.contract_id), BOND - withdraw_amount);
}

#[test]
fn withdraw_bond_below_min_bond_fails() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    // BOND is well above MIN_BOND; withdrawing everything but a sliver
    // would leave less than MIN_BOND behind.
    let too_much = BOND - MIN_BOND + 1;
    let res = c.try_withdraw_bond(&ctx.solver, &too_much);
    assert_eq!(res, Err(Ok(Error::SolverBondTooLow.into())));
}

#[test]
fn withdraw_bond_leaving_exactly_min_bond_succeeds() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    // Withdraw exactly the amount that leaves MIN_BOND remaining
    let withdraw_amount = BOND - MIN_BOND;
    c.withdraw_bond(&ctx.solver, &withdraw_amount);

    let record = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(record.bond_amount, MIN_BOND);
    assert!(record.is_active);
    assert_eq!(ctx.bond().balance(&ctx.solver), withdraw_amount);
    assert_eq!(ctx.bond().balance(&ctx.contract_id), MIN_BOND);
}

#[test]
fn withdraw_bond_below_exact_min_bond_fails() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    // Attempt to leave one unit less than MIN_BOND
    let too_much = BOND - MIN_BOND + 1;
    let res = c.try_withdraw_bond(&ctx.solver, &too_much);
    assert_eq!(res, Err(Ok(Error::SolverBondTooLow.into())));
}

#[test]
fn withdraw_bond_more_than_balance_fails() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    let res = c.try_withdraw_bond(&ctx.solver, &(BOND + 1));
    assert_eq!(res, Err(Ok(Error::InsufficientBond.into())));
}

#[test]
fn withdraw_bond_zero_amount_fails() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    let res = c.try_withdraw_bond(&ctx.solver, &0);
    assert_eq!(res, Err(Ok(Error::ZeroAmount.into())));
}

#[test]
fn withdraw_bond_allowed_with_active_intent_if_still_above_minimum() {
    // Partial withdrawal doesn't require active_intents == 0 -- only full
    // deregistration does -- as long as the remaining bond still clears
    // MIN_BOND, the solver stays adequately collateralized.
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    let withdraw_amount = 100 * 10_000_000;
    c.withdraw_bond(&ctx.solver, &withdraw_amount);
    assert_eq!(
        c.get_solver(&ctx.solver).unwrap().bond_amount,
        BOND - withdraw_amount
    );
}

#[test]
fn withdraw_bond_reflects_reduced_balance_after_a_prior_slash() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);
    let bond_after_slash = c.get_solver(&ctx.solver).unwrap().bond_amount;
    assert!(bond_after_slash < BOND);

    // Withdrawing more than the (slash-reduced) balance still fails against
    // the current balance, not the original pre-slash BOND.
    let res = c.try_withdraw_bond(&ctx.solver, &(bond_after_slash + 1));
    assert_eq!(res, Err(Ok(Error::InsufficientBond.into())));

    // A withdrawal that respects the reduced balance and stays above
    // MIN_BOND still succeeds.
    let small_withdrawal = bond_after_slash - MIN_BOND;
    c.withdraw_bond(&ctx.solver, &small_withdrawal);
    assert_eq!(
        c.get_solver(&ctx.solver).unwrap().bond_amount,
        bond_after_slash - small_withdrawal
    );
}

#[test]
fn withdraw_bond_fails_entirely_once_slash_deactivates_solver() {
    // A solver whose bond has already dropped below MIN_BOND (and who was
    // therefore deactivated by PR3's guard) can't withdraw_bond at all --
    // any positive withdrawal would only push them further below MIN_BOND,
    // so the existing SolverBondTooLow check rejects it without needing a
    // separate is_active check in withdraw_bond itself.
    let ctx = setup();
    let c = ctx.client();

    let thin_bond = MIN_BOND + MIN_BOND / 10;
    ctx.bond_admin().mint(&ctx.solver, &thin_bond);
    c.register_solver(&ctx.solver, &thin_bond);

    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);
    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);

    let record = c.get_solver(&ctx.solver).unwrap();
    assert!(record.bond_amount < MIN_BOND);
    assert!(!record.is_active);

    let res = c.try_withdraw_bond(&ctx.solver, &1);
    assert_eq!(res, Err(Ok(Error::SolverBondTooLow.into())));
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
fn active_intents_counts_multiple_concurrent_accepted_intents() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    let id1 = ctx.submit();
    ctx.pass_time(1); // distinct timestamp so compute_intent_id doesn't collide
    let id2 = ctx.submit();

    c.accept_intent(&ctx.solver, &id1);
    c.accept_intent(&ctx.solver, &id2);
    assert_eq!(c.get_solver(&ctx.solver).unwrap().active_intents, 2);

    // Can't deregister while either obligation is outstanding.
    let res = c.try_deregister_solver(&ctx.solver);
    assert_eq!(res, Err(Ok(Error::SolverHasActiveIntents.into())));

    // Clearing one via fill decrements the counter but doesn't zero it.
    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id1, &FILL, &false);
    assert_eq!(c.get_solver(&ctx.solver).unwrap().active_intents, 1);
    let res = c.try_deregister_solver(&ctx.solver);
    assert_eq!(res, Err(Ok(Error::SolverHasActiveIntents.into())));

    // Clearing the second (via slash) zeroes it and unblocks deregistration.
    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id2);
    assert_eq!(c.get_solver(&ctx.solver).unwrap().active_intents, 0);
    c.deregister_solver(&ctx.solver);
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
    c.fill_intent(&ctx.solver, &id, &FILL, &false);

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
fn dst_allowlist_disabled_by_default_allows_any_token() {
    let ctx = setup();
    assert!(!ctx.client().is_dst_allowlist_enabled());
    assert!(!ctx.client().is_dst_token_allowed(&ctx.dst_token));

    // Submission succeeds even though the token was never explicitly allowed.
    ctx.submit();
}

#[test]
fn dst_allowlist_blocks_unlisted_token_once_enabled() {
    let ctx = setup();
    let c = ctx.client();
    c.set_dst_allowlist_enabled(&true);

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
    assert_eq!(res, Err(Ok(Error::DstTokenNotAllowed.into())));
}

#[test]
fn dst_allowlist_allows_listed_token_once_enabled() {
    let ctx = setup();
    let c = ctx.client();
    ctx.allow_dst_token(&ctx.dst_token);
    c.set_dst_allowlist_enabled(&true);

    assert!(c.is_dst_token_allowed(&ctx.dst_token));
    let listed = c.list_allowed_dst_tokens();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed.get(0).unwrap(), ctx.dst_token);
    ctx.submit();
}

#[test]
fn dst_allowlist_removal_blocks_previously_allowed_token() {
    let ctx = setup();
    let c = ctx.client();
    ctx.allow_dst_token(&ctx.dst_token);
    c.set_dst_allowlist_enabled(&true);

    // #115/#118: removal is now a timelocked propose/execute flow.
    c.propose_remove_dst_token(&ctx.dst_token);
    assert!(c.is_dst_token_allowed(&ctx.dst_token));
    let res = c.try_execute_remove_dst_token(&ctx.dst_token);
    assert_eq!(res, Err(Ok(Error::TimelockNotElapsed.into())));

    ctx.pass_time(ADMIN_TIMELOCK_DELAY);
    c.execute_remove_dst_token(&ctx.dst_token);

    assert!(!c.is_dst_token_allowed(&ctx.dst_token));
    assert_eq!(c.list_allowed_dst_tokens().len(), 0);
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
    assert_eq!(res, Err(Ok(Error::DstTokenNotAllowed.into())));
}

#[test]
fn dst_allowlist_toggled_mid_lifecycle_does_not_retroactively_affect_open_intent() {
    let ctx = setup();
    let c = ctx.client();
    // Allowlist disabled by default, so any token is accepted
    assert!(!c.is_dst_allowlist_enabled());
    let id = ctx.submit();

    // Enable allowlist without adding the dst_token
    c.set_dst_allowlist_enabled(&true);
    assert!(!c.is_dst_token_allowed(&ctx.dst_token));

    // The already-open intent should still be readable/usable
    let intent = c.get_intent(&id).unwrap();
    assert!(intent.state == IntentState::Open);

    // But new submissions with non-allowed tokens fail
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
    assert_eq!(res, Err(Ok(Error::DstTokenNotAllowed.into())));
}

#[test]
fn dst_allowlist_can_be_re_enabled_to_accept_previously_blocked_tokens() {
    let ctx = setup();
    let c = ctx.client();

    // Enable allowlist and block the token
    c.set_dst_allowlist_enabled(&true);
    assert!(!c.is_dst_token_allowed(&ctx.dst_token));

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
    assert_eq!(res, Err(Ok(Error::DstTokenNotAllowed.into())));

    // Disable the allowlist
    c.set_dst_allowlist_enabled(&false);

    // Now submissions with any token succeed again
    ctx.submit();
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

// #25 — same-ledger intents with identical params produce distinct ids (nonce).
//
// Before the fix, compute_intent_id hashed only (user, src_chain, src_amount,
// timestamp). Two submit_intent calls in the same ledger with the same args
// produced the same id and the second silently overwrote the first record.
// After the fix a per-user nonce is included in the preimage, so every call
// yields a unique id regardless of timestamp.
#[test]
fn same_ledger_identical_intents_produce_distinct_ids() {
    let ctx = setup();
    let c = ctx.client();

    // Both submits happen at the same ledger timestamp.
    let id1 = ctx.submit();
    let id2 = ctx.submit();

    // The ids must be different — neither intent overwrote the other.
    assert_ne!(id1, id2);

    // Both records must be independently retrievable.
    assert!(c.get_intent(&id1).is_some());
    assert!(c.get_intent(&id2).is_some());

    // Total intents counter must reflect both submissions.
    assert_eq!(c.get_stats().0, 2);
}

#[test]
fn nonce_increments_per_user_across_submissions() {
    let ctx = setup();
    let c = ctx.client();

    // Submit three intents from the same user in the same ledger close.
    let id1 = ctx.submit();
    let id2 = ctx.submit();
    let id3 = ctx.submit();

    // All three must be distinct.
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);

    // All three records exist.
    assert!(c.get_intent(&id1).is_some());
    assert!(c.get_intent(&id2).is_some());
    assert!(c.get_intent(&id3).is_some());

    assert_eq!(c.get_stats().0, 3);
}

#[test]
fn different_users_same_params_same_ledger_produce_distinct_ids() {
    // Nonces are per-user so two different users both on nonce 0 at the same
    // timestamp must still get different ids (the user address is in the preimage).
    let ctx = setup();
    let c = ctx.client();

    let user2 = Address::generate(&ctx.env);
    let deadline: Option<u64> = None;

    let id1 = ctx.submit(); // ctx.user, nonce=0
    let id2 = c.submit_intent(
        &user2,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    ); // user2, nonce=0

    assert_ne!(id1, id2);
    assert!(c.get_intent(&id1).is_some());
    assert!(c.get_intent(&id2).is_some());
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
    c.fill_intent(&ctx.solver, &id, &FILL, &false);

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

    let (total_intents, total_volume, _open) = c.get_stats();
    assert_eq!(total_intents, 1);
    assert_eq!(total_volume, FILL);
}

#[test]
fn get_stats_reflects_cumulative_totals_across_multiple_fills() {
    let ctx = setup();
    let c = ctx.client();

    ctx.register_solver();

    // First fill cycle
    let id1 = ctx.submit();
    c.accept_intent(&ctx.solver, &id1);
    let fee1 = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee1));
    c.fill_intent(&ctx.solver, &id1, &FILL, &false);

    let (total_intents, total_volume) = c.get_stats();
    assert_eq!(total_intents, 1);
    assert_eq!(total_volume, FILL);

    // Second fill cycle with a different amount
    let id2 = ctx.submit();
    c.accept_intent(&ctx.solver, &id2);
    let fill2 = 200 * 10_000_000;
    let fee2 = fill2 * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(fill2 + fee2));
    c.fill_intent(&ctx.solver, &id2, &fill2, &false);

    let (total_intents, total_volume) = c.get_stats();
    assert_eq!(total_intents, 2);
    assert_eq!(total_volume, FILL + fill2);

    // Submit a cancelled intent (should increment TotalIntents but not TotalVolume)
    let id3 = ctx.submit();
    c.cancel_intent(&ctx.user, &id3);

    let (total_intents, total_volume) = c.get_stats();
    assert_eq!(total_intents, 3);
    assert_eq!(total_volume, FILL + fill2);

    // Submit an expired intent (should increment TotalIntents but not TotalVolume)
    let id4 = ctx.submit();
    ctx.pass_time(INTENT_EXPIRY + 1);
    c.expire_intent(&id4);

    let (total_intents, total_volume) = c.get_stats();
    assert_eq!(total_intents, 4);
    assert_eq!(total_volume, FILL + fill2);
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

#[test]
fn two_solver_race_on_same_intent_id() {
    let ctx = setup();
    let c = ctx.client();

    // Register two solvers
    ctx.register_solver();
    let solver2 = Address::generate(&ctx.env);
    ctx.bond_admin().mint(&solver2, &BOND);
    c.register_solver(&solver2, &BOND);

    // Submit an intent
    let id = ctx.submit();

    // First solver accepts successfully
    c.accept_intent(&ctx.solver, &id);
    let solver1_record = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(solver1_record.active_intents, 1);

    // Second solver tries to accept the same intent — should fail
    let res = c.try_accept_intent(&solver2, &id);
    assert_eq!(res, Err(Ok(Error::IntentNotOpen.into())));

    // Verify second solver's active_intents was never incremented
    let solver2_record = c.get_solver(&solver2).unwrap();
    assert_eq!(solver2_record.active_intents, 0);
}

// ─── Fill guards ────────────────────────────────────────────────────────────────

#[test]
fn fill_zero_amount_fails() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    let res = ctx.client().try_fill_intent(&ctx.solver, &id, &0, &false);
    assert_eq!(res, Err(Ok(Error::ZeroAmount.into())));
}

#[test]
fn fill_after_window_fails() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);

    ctx.pass_time(FILL_WINDOW + 1);
    ctx.dst_admin().mint(&ctx.solver, &FILL);
    let res = ctx.client().try_fill_intent(&ctx.solver, &id, &FILL, &false);
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

    let res = ctx.client().try_fill_intent(&other, &id, &FILL, &false);
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
    // State remains unchanged after rejected cancellation attempt
    assert!(ctx.client().get_intent(&id).unwrap().state == IntentState::Open);
}

#[test]
fn cancel_cooldown_prevents_rapid_cancellations() {
    let ctx = setup();
    let c = ctx.client();

    // Submit two intents at different times to avoid collision
    let id1 = ctx.submit();
    ctx.pass_time(1);
    let id2 = ctx.submit();

    // First cancellation succeeds
    c.cancel_intent(&ctx.user, &id1);
    assert!(c.get_intent(&id1).unwrap().state == IntentState::Cancelled);

    // Second cancellation within cooldown fails
    let res = c.try_cancel_intent(&ctx.user, &id2);
    assert_eq!(res, Err(Ok(Error::CancelCooldownNotExpired.into())));
}

#[test]
fn cancel_cooldown_expires_after_delay() {
    let ctx = setup();
    let c = ctx.client();

    let id1 = ctx.submit();
    ctx.pass_time(1);
    let id2 = ctx.submit();

    // First cancellation
    c.cancel_intent(&ctx.user, &id1);

    // Wait for cooldown to expire
    ctx.pass_time(CANCEL_COOLDOWN);

    // Second cancellation now succeeds
    c.cancel_intent(&ctx.user, &id2);
    assert!(c.get_intent(&id2).unwrap().state == IntentState::Cancelled);
}

#[test]
fn different_users_have_independent_cooldowns() {
    let ctx = setup();
    let c = ctx.client();
    let user2 = Address::generate(&ctx.env);

    let id1 = ctx.submit();
    ctx.pass_time(1);

    let id2_user: BytesN<32> = c.submit_intent(
        &user2,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &None,
    );

    // User 1 cancels
    c.cancel_intent(&ctx.user, &id1);

    // User 2 can immediately cancel despite user 1's recent cancellation
    c.cancel_intent(&user2, &id2_user);
    assert!(c.get_intent(&id2_user).unwrap().state == IntentState::Cancelled);
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
fn slash_above_min_bond_keeps_solver_active() {
    // Solver bonded well above MIN_BOND: a 10% slash still leaves >= MIN_BOND.
    // Verify is_active remains true and solver can still accept intents.
    let ctx = setup();
    let c = ctx.client();

    // BOND is 1000 * 10_000_000; MIN_BOND is 50 * 10_000_000.
    // A 10% slash of BOND is 100 * 10_000_000, leaving 900 * 10_000_000 >> MIN_BOND.
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);
    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);

    let solver = c.get_solver(&ctx.solver).unwrap();
    assert!(solver.bond_amount >= MIN_BOND);
    assert!(solver.is_active);

    // Active solver can accept new intents.
    assert!(c.is_solver_eligible(&ctx.solver));
    let id2 = ctx.submit();
    c.accept_intent(&ctx.solver, &id2);
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

// ─── Expiry ─────────────────────────────────────────────────────────────────────

#[test]
fn expire_intent_marks_open_intent_expired_after_deadline() {
    let ctx = setup();
    let c = ctx.client();
    let id = ctx.submit();

    ctx.pass_time(INTENT_EXPIRY + 1);
    c.expire_intent(&id);

    assert!(c.get_intent(&id).unwrap().state == IntentState::Expired);
}

#[test]
fn expire_intent_before_deadline_fails() {
    let ctx = setup();
    let c = ctx.client();
    let id = ctx.submit();

    let res = c.try_expire_intent(&id);
    assert_eq!(res, Err(Ok(Error::DeadlineNotReached.into())));
}

#[test]
fn expire_intent_on_accepted_intent_fails() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    ctx.pass_time(FILL_WINDOW + 1);
    let res = c.try_expire_intent(&id);
    assert_eq!(res, Err(Ok(Error::IntentNotOpen.into())));
}

#[test]
fn expire_intent_unknown_id_fails() {
    let ctx = setup();
    let unknown = BytesN::from_array(&ctx.env, &[0u8; 32]);
    let res = ctx.client().try_expire_intent(&unknown);
    assert_eq!(res, Err(Ok(Error::IntentNotFound.into())));
}

#[test]
fn expire_intent_before_deadline_state_unchanged() {
    let ctx = setup();
    let c = ctx.client();
    let id = ctx.submit();

    let initial_state = c.get_intent(&id).unwrap().state;
    assert!(initial_state == IntentState::Open);

    let res = c.try_expire_intent(&id);
    assert_eq!(res, Err(Ok(Error::DeadlineNotReached.into())));

    let final_state = c.get_intent(&id).unwrap().state;
    assert!(final_state == IntentState::Open);
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

#[test]
fn state_changing_calls_extend_instance_ttl() {
    use soroban_sdk::testutils::storage::Instance as _;

    let ctx = setup();
    ctx.register_solver();

    let instance_ttl = ctx
        .env
        .as_contract(&ctx.contract_id, || ctx.env.storage().instance().get_ttl());

    assert!(instance_ttl >= crate::INSTANCE_TTL_EXTEND_TO - 1);
}

// ─── CEI regression tests ────────────────────────────────────────────────────────

// #28 — double-deregister is rejected cleanly after the storage-first reorder.
//
// Before the fix, the record was still present when the bond transfer ran, so
// a second deregister_solver call before the remove could (in a future async or
// re-entrant context) see the record and transfer the bond a second time.
// After the fix the record is removed first, so the second call panics with
// SolverNotRegistered before it reaches the transfer.
#[test]
fn double_deregister_rejected_cleanly() {
    let ctx = setup();
    ctx.register_solver();

    // First deregister succeeds and removes the record.
    ctx.client().deregister_solver(&ctx.solver);
    assert!(ctx.client().get_solver(&ctx.solver).is_none());
    // Bond is back with the solver.
    assert_eq!(ctx.bond().balance(&ctx.solver), BOND);

    // Second call on the same address must be rejected — no record to refund.
    let res = ctx.client().try_deregister_solver(&ctx.solver);
    assert_eq!(res, Err(Ok(Error::SolverNotRegistered.into())));

    // Crucially the contract's bond balance must not have changed further —
    // it was zero after the first deregister and should still be zero.
    assert_eq!(ctx.bond().balance(&ctx.contract_id), 0);
    // Solver's balance should still equal exactly one bond refund, not two.
    assert_eq!(ctx.bond().balance(&ctx.solver), BOND);
}

// #27 — SolverRecord state is consistent with actual token balances after
// register_solver.  After the storage-first reorder the record is written
// before the transfer, so if the transfer were ever to fail the record simply
// wouldn't reflect a deposit that never happened.  Here we verify the happy
// path: record.bond_amount == tokens held by the contract.
#[test]
fn solver_record_consistent_with_token_balances_after_register() {
    let ctx = setup();

    // Mint exactly BOND to the solver, then register.
    ctx.bond_admin().mint(&ctx.solver, &BOND);
    ctx.client().register_solver(&ctx.solver, &BOND);

    let record = ctx.client().get_solver(&ctx.solver).unwrap();

    // Storage says the bond is BOND.
    assert_eq!(record.bond_amount, BOND);
    // The contract actually holds BOND tokens — no discrepancy.
    assert_eq!(ctx.bond().balance(&ctx.contract_id), BOND);
    // Solver's wallet is empty — tokens moved.
    assert_eq!(ctx.bond().balance(&ctx.solver), 0);

    // Top-up path: record.bond_amount must keep tracking reality after a second deposit.
    let topup = 200 * 10_000_000;
    ctx.bond_admin().mint(&ctx.solver, &topup);
    ctx.client().register_solver(&ctx.solver, &topup);

    let record2 = ctx.client().get_solver(&ctx.solver).unwrap();
    assert_eq!(record2.bond_amount, BOND + topup);
    assert_eq!(ctx.bond().balance(&ctx.contract_id), BOND + topup);
    assert_eq!(ctx.bond().balance(&ctx.solver), 0);
}

// #26 — CEI ordering in fill_intent: state is committed before transfers.
//
// We verify two complementary properties:
//
// 1. After a successful fill the intent is Filled in storage *and* tokens
//    have moved — state and funds are always in sync (the core CEI invariant).
//
// 2. A second fill_intent call on the same (already-Filled) intent is
//    rejected with IntentAlreadyFilled before any transfer attempt.  This is
//    the on-chain guard that would stop a re-entrant token from triggering a
//    double-fill: whatever point during the transfers the re-entrant call is
//    made, storage already shows Filled.
#[test]
fn fill_intent_state_committed_before_transfer_and_double_fill_rejected() {
    let ctx = setup();
    let c = ctx.client();

    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    // Fund the solver with enough for the fill + fee.
    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));

    // Happy-path fill.
    c.fill_intent(&ctx.solver, &id, &FILL, &false);

    // 1. Storage reflects Filled and fill_amount is set.
    let intent = c.get_intent(&id).unwrap();
    assert_eq!(intent.state, IntentState::Filled);
    assert_eq!(intent.fill_amount, Some(FILL));

    // 2. Tokens have moved: user got FILL, fee_recipient got fee, solver has 0.
    assert_eq!(ctx.dst().balance(&ctx.user), FILL);
    assert_eq!(ctx.dst().balance(&ctx.fee_recipient), fee);
    assert_eq!(ctx.dst().balance(&ctx.solver), 0);

    // 3. A second fill attempt is rejected before any transfer — this is exactly
    //    what a re-entrant token would hit mid-transfer after the CEI reorder.
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee)); // give solver funds again
    let res = c.try_fill_intent(&ctx.solver, &id, &FILL, &false);
    assert_eq!(res, Err(Ok(Error::IntentAlreadyFilled.into())));

    // User's balance must not have increased — no double-payment.
    assert_eq!(ctx.dst().balance(&ctx.user), FILL);
}

// ─── Views ──────────────────────────────────────────────────────────────────────

#[test]
fn get_intent_returns_none_for_unknown_id() {
    let ctx = setup();
    let unknown = BytesN::from_array(&ctx.env, &[0u8; 32]);
    assert!(ctx.client().get_intent(&unknown).is_none());
}

#[test]
fn get_bond_token_returns_configured_token() {
    let ctx = setup();
    assert_eq!(ctx.client().get_bond_token(), Some(ctx.bond_token.clone()));
}

#[test]
fn get_min_bond_returns_enforced_minimum() {
    let ctx = setup();
    assert_eq!(ctx.client().get_min_bond(), MIN_BOND);
}

#[test]
fn get_min_bond_multiplier_defaults_to_one() {
    let ctx = setup();
    assert_eq!(ctx.client().get_min_bond_multiplier(&ctx.dst_token), 10);
}

#[test]
fn set_min_bond_multiplier_updates_requirement() {
    let ctx = setup();
    ctx.register_solver();
    let id = ctx.submit();

    // Set multiplier to 1.5x (15 in fixed-point)
    ctx.client().set_min_bond_multiplier(&ctx.dst_token, &15);
    assert_eq!(ctx.client().get_min_bond_multiplier(&ctx.dst_token), 15);

    // Submit another intent targeting same token
    let id2 = ctx.submit();

    // Solver with 1000 USDC bond (10x MIN_BOND) can still accept
    ctx.client().accept_intent(&ctx.solver, &id2);
    assert_eq!(ctx.client().get_intent(&id2).unwrap().solver, Some(ctx.solver.clone()));
}

#[test]
fn accept_intent_checks_token_specific_bond_requirement() {
    let ctx = setup();
    // Register solver with exactly MIN_BOND
    let min_bond = MIN_BOND;
    ctx.bond_admin().mint(&ctx.solver, &min_bond);
    ctx.client().register_solver(&ctx.solver, &min_bond);

    let id = ctx.submit();

    // Set multiplier to 2.0x for this token
    ctx.client().set_min_bond_multiplier(&ctx.dst_token, &20);

    // Solver's bond is now insufficient (50 USDC < 100 USDC required)
    let res = ctx.client().try_accept_intent(&ctx.solver, &id);
    assert_eq!(res, Err(Ok(Error::SolverBondTooLow.into())));
}

#[test]
fn list_intents_by_user_returns_empty_for_new_user() {
    let ctx = setup();
    let other_user = Address::generate(&ctx.env);
    let intents = ctx.client().list_intents_by_user(&other_user);
    assert_eq!(intents.len(), 0);
}

#[test]
fn list_intents_by_user_returns_submitted_intents() {
    let ctx = setup();
    let id1 = ctx.submit();
    let id2 = ctx.submit();

    let intents = ctx.client().list_intents_by_user(&ctx.user);
    assert_eq!(intents.len(), 2);
    assert_eq!(intents.get(0), id1);
    assert_eq!(intents.get(1), id2);
}

#[test]
fn slash_cooldown_prevents_accept_after_slash() {
    let ctx = setup();
    ctx.register_solver();

    let id1 = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id1);

    // Slash the solver
    ctx.pass_time(FILL_WINDOW + 1);
    ctx.client().slash_solver(&id1);

    // Try to accept another intent immediately
    let id2 = ctx.submit();
    let res = ctx.client().try_accept_intent(&ctx.solver, &id2);
    assert_eq!(res, Err(Ok(Error::SolverInactive.into())));
}

#[test]
fn slash_cooldown_expires_after_time_window() {
    let ctx = setup();
    ctx.register_solver();

    let id1 = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id1);

    // Slash the solver
    ctx.pass_time(FILL_WINDOW + 1);
    ctx.client().slash_solver(&id1);

    // Wait for cooldown to expire (1 hour)
    ctx.pass_time(3600);

    // Should be able to accept now
    let id2 = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id2);
    assert_eq!(ctx.client().get_intent(&id2).unwrap().solver, Some(ctx.solver.clone()));
// ─── get_protocol_params view ────────────────────────────────────────────────────

#[test]
fn get_protocol_params_returns_current_constants() {
    use crate::{FILL_WINDOW, INTENT_EXPIRY, MIN_BOND};
    const PROTOCOL_FEE_BPS: i128 = 5;

    let ctx = setup();
    let params = ctx.client().get_protocol_params();

    assert_eq!(params.min_bond, MIN_BOND);
    assert_eq!(params.fill_window, FILL_WINDOW);
    assert_eq!(params.intent_expiry, INTENT_EXPIRY);
    assert_eq!(params.protocol_fee_bps, PROTOCOL_FEE_BPS);
    assert_eq!(params.referral_share_bps, 0);
}

// ─── Partial fills ───────────────────────────────────────────────────────────────

#[test]
fn two_partial_fills_complete_intent() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    // Submit with MIN_DST as the full target.
    let id = ctx.submit();

    // First partial fill: half of MIN_DST.
    let half = MIN_DST / 2;
    let fee1 = half * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(half + fee1));
    c.accept_intent(&ctx.solver, &id);
    c.fill_intent(&ctx.solver, &id, &half, &false);

    // Intent should now be PartiallyFilled and re-opened (solver reset).
    let intent = c.get_intent(&id).unwrap();
    assert_eq!(intent.state, IntentState::PartiallyFilled);
    assert_eq!(intent.total_filled, half);
    assert!(intent.solver.is_none());

    // User already received the first half.
    assert_eq!(ctx.dst().balance(&ctx.user), half);

    // Second fill: the remainder — brings total to MIN_DST.
    let remainder = MIN_DST - half;
    let fee2 = remainder * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(remainder + fee2));
    c.accept_intent(&ctx.solver, &id);
    c.fill_intent(&ctx.solver, &id, &remainder, &false);

    let intent = c.get_intent(&id).unwrap();
    assert_eq!(intent.state, IntentState::Filled);
    assert_eq!(intent.total_filled, MIN_DST);
    assert_eq!(intent.fill_amount, Some(MIN_DST));

    // User has received the full MIN_DST across both fills.
    assert_eq!(ctx.dst().balance(&ctx.user), MIN_DST);

    // Solver credited fills_completed once (on the completing fill).
    let solver = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(solver.fills_completed, 1);
    assert_eq!(solver.total_volume, MIN_DST);
}

#[test]
fn partial_fill_left_incomplete_past_deadline_can_be_expired() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    let id = ctx.submit();

    // Deliver a partial fill (less than MIN_DST).
    let partial = MIN_DST / 3;
    let fee = partial * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(partial + fee));
    c.accept_intent(&ctx.solver, &id);
    c.fill_intent(&ctx.solver, &id, &partial, &false);

    // Intent is PartiallyFilled and re-opened with a fresh INTENT_EXPIRY deadline.
    assert_eq!(
        c.get_intent(&id).unwrap().state,
        IntentState::PartiallyFilled
    );

    // Let the new deadline expire without anyone picking up the remainder.
    ctx.pass_time(INTENT_EXPIRY + 1);

    // expire_intent works on PartiallyFilled intents past their deadline.
    c.expire_intent(&id);
    assert_eq!(c.get_intent(&id).unwrap().state, IntentState::Expired);
}

#[test]
fn single_fill_at_or_above_minimum_completes_immediately() {
// ─── #29: slash_solver ordering ─────────────────────────────────────────────────

/// Calling slash_solver twice on the same intent_id must fail on the second
/// call with IntentNotAccepted — the first call flips the state to Open before
/// the token transfer, so the second call hits the guard immediately.
#[test]
fn double_slash_second_call_rejected() {
// ─── Issue #31: fee overflow boundary ────────────────────────────────────────────

/// #31: fill_amount just above i128::MAX / PROTOCOL_FEE_BPS (5) overflows the
/// checked_mul and returns FeeOverflow rather than silently wrapping.
///
/// Boundary: i128::MAX / 5 = 34_028_236_692_093_846_346_337_460_743_176_821_145.
/// Any value above that will cause `fill_amount * 5` to overflow i128.
#[test]
fn fill_intent_fee_overflow_returns_error() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    ctx.pass_time(FILL_WINDOW + 1);

    // First slash succeeds.
    c.slash_solver(&id);
    let intent = c.get_intent(&id).unwrap();
    assert!(intent.state == IntentState::Open);

    // Second slash on the same id must be rejected: the intent is now Open,
    // not Accepted.
    let res = c.try_slash_solver(&id);
    assert_eq!(res, Err(Ok(crate::Error::IntentNotAccepted.into())));
}

// ─── #47: reputation score ───────────────────────────────────────────────────────

use crate::{IntentSettlement, SolverRecord};

/// Helper: build a SolverRecord with just the fields that affect scoring.
fn make_record(
    env: &soroban_sdk::Env,
    solver: &soroban_sdk::Address,
    fills_completed: u32,
    fills_failed: u32,
    total_volume: i128,
) -> SolverRecord {
    SolverRecord {
        address: solver.clone(),
        bond_amount: MIN_BOND,
        fills_completed,
        fills_failed,
        total_volume,
        is_active: true,
        registered_at: env.ledger().timestamp(),
        active_intents: 0,
    }
}

/// A solver that has never attempted any fill has score 0.
#[test]
fn reputation_score_zero_fills_returns_zero() {
    let ctx = setup();
    let r = make_record(&ctx.env, &ctx.solver, 0, 0, 0);
    assert_eq!(IntentSettlement::compute_reputation_score(&r), 0);
}

/// A solver that has only failures has score 0 regardless of volume.
#[test]
fn reputation_score_all_failures_returns_zero() {
    let ctx = setup();
    let r = make_record(&ctx.env, &ctx.solver, 0, 50, 0);
    assert_eq!(IntentSettlement::compute_reputation_score(&r), 0);
}

/// A perfect solver with no volume scores 9_000 (= 90% × 10_000 bps).
#[test]
fn reputation_score_perfect_rate_no_volume_is_nine_thousand() {
    let ctx = setup();
    let r = make_record(&ctx.env, &ctx.solver, 100, 0, 0);
    // At zero volume, decay_bps ≈ 10_000, multiplier = 9_000.
    let score = IntentSettlement::compute_reputation_score(&r);
    assert_eq!(score, 9_000);
}

/// A perfect solver with very high volume scores close to (but below) 10_000.
#[test]
fn reputation_score_perfect_rate_high_volume_approaches_ten_thousand() {
    let ctx = setup();
    // volume = 100 × VOLUME_SCALE makes decay negligible.
    let high_vol: i128 = 100 * 1_000 * 100 * 10_000_000;
    let r = make_record(&ctx.env, &ctx.solver, 1_000, 0, high_vol);
    let score = IntentSettlement::compute_reputation_score(&r);
    // Must be strictly greater than 9_000 and less than 10_000.
    assert!(score > 9_000, "score {score} should be > 9_000");
    assert!(score < 10_000, "score {score} should be < 10_000");
}

/// A mixed solver (some failures) scores strictly less than a perfect solver
/// with the same volume.
#[test]
fn reputation_score_partial_failures_lower_than_perfect() {
    let ctx = setup();
    let vol = 100 * 10_000_000i128;
    let perfect = make_record(&ctx.env, &ctx.solver, 90, 0, vol);
    let mixed = make_record(&ctx.env, &ctx.solver, 90, 10, vol);
    assert!(
        IntentSettlement::compute_reputation_score(&perfect)
            > IntentSettlement::compute_reputation_score(&mixed)
    );
}

/// get_reputation_score returns None for an unregistered solver.
#[test]
fn get_reputation_score_unregistered_returns_none() {
    let ctx = setup();
    let stranger = soroban_sdk::Address::generate(&ctx.env);
    assert!(ctx.client().get_reputation_score(&stranger).is_none());
}

/// get_reputation_score returns Some(0) for a registered but never-filled solver.
#[test]
fn get_reputation_score_registered_no_fills_returns_zero() {
    let ctx = setup();
    ctx.register_solver();
    assert_eq!(ctx.client().get_reputation_score(&ctx.solver), Some(0));
}

/// After a successful fill the score should be non-zero.
#[test]
fn get_reputation_score_after_fill_is_nonzero() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);
    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id, &FILL, &false);

    let score = c.get_reputation_score(&ctx.solver).unwrap();
    assert!(score > 0, "score after fill should be > 0");
    // Smallest fill_amount that overflows: (i128::MAX / 5) + 1.
    // We satisfy min_dst_amount by keeping fill_amount >> MIN_DST.
    let overflow_fill: i128 = i128::MAX / 5 + 1;

    // Fund the solver so the dst transfer can proceed; the overflow is caught
    // in the fee calculation that follows the transfer (the full transaction
    // rolls back on panic_with_error, so the user's balance stays zero).
    ctx.dst_admin().mint(&ctx.solver, &overflow_fill);

    let res = c.try_fill_intent(&ctx.solver, &id, &overflow_fill, &false);
    assert_eq!(res, Err(Ok(Error::FeeOverflow.into())));
}

/// Sanity: a fill_amount just *at* the boundary (i128::MAX / 5) does not overflow.
#[test]
fn fill_intent_fee_at_boundary_does_not_overflow() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    // i128::MAX / 5 — fee = (i128::MAX / 5) * 5 / 10_000, which fits in i128.
    let boundary_fill: i128 = i128::MAX / 5;
    let fee = boundary_fill * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(boundary_fill + fee));

    // Should succeed (no overflow).
    c.fill_intent(&ctx.solver, &id, &boundary_fill, &false);
    assert!(c.get_intent(&id).unwrap().state == IntentState::Filled);
}

// ─── Issue #32: tiny bond slash floor ────────────────────────────────────────────

/// #32: When a solver's bond has been whittled to a very small value (< 10 in
/// the token's smallest unit), integer division `bond / 10` rounds to 0.  The
/// `.max(1)` floor ensures the slash is never economically free — a non-zero
/// bond always produces a non-zero slash.
///
/// We plant a SolverRecord with bond_amount = 5 directly into storage (bypassing
/// the MIN_BOND registration guard) to test the math boundary in isolation.
#[test]
fn slash_tiny_bond_always_yields_nonzero_slash() {
    let ctx = setup();
    let c = ctx.client();

    // Register normally first so the contract recognises ctx.solver.
    ctx.register_solver();

    // Plant a SolverRecord with an artificially tiny bond directly into
    // contract storage, simulating a bond that has been slashed many times.
    let tiny_bond: i128 = 5; // 5 / 10 = 0 without the .max(1) floor
    ctx.env.as_contract(&ctx.contract_id, || {
        let mut record: SolverRecord = ctx
            .env
            .storage()
            .persistent()
            .get(&DataKey::Solver(ctx.solver.clone()))
            .unwrap();
        record.bond_amount = tiny_bond;
        record.active_intents = 0;
        ctx.env
            .storage()
            .persistent()
            .set(&DataKey::Solver(ctx.solver.clone()), &record);
    });

    // Submit and accept an intent so slash_solver has something to slash.
    let id = ctx.submit();
    c.accept_intent(&ctx.solver, &id);

    ctx.pass_time(FILL_WINDOW + 1);
    c.slash_solver(&id);

    // The slash must be >= 1 even though 5 / 10 == 0.
    let solver = c.get_solver(&ctx.solver).unwrap();
    assert!(
        solver.bond_amount < tiny_bond,
        "bond should have decreased after slash"
    );
    let slashed = tiny_bond - solver.bond_amount;
    assert!(slashed >= 1, "slash_amount must be at least 1, got {slashed}");
}

// ─── Issue #33: add_allowed_dst_token validates SEP-41 interface ─────────────────

/// #33: Passing the settlement contract's own address (which is not a token)
/// to propose_add_dst_token must fail.  The `decimals()` probe inside
/// propose_add_dst_token will trap on a contract that doesn't implement SEP-41,
/// reverting the transaction before any storage entry is written.
#[test]
fn propose_add_dst_token_rejects_non_token_contract() {
    let ctx = setup();

    // ctx.contract_id is a real deployed contract (IntentSettlement) but it
    // does not implement the SEP-41 token interface, so decimals() will trap.
    let res = ctx
        .client()
        .try_propose_add_dst_token(&ctx.contract_id);

    // The call must fail — either with InvalidTokenInterface or a generic
    // contract-trap error (the host converts a trapped cross-contract call
    // into an Err result in the test environment).
    assert!(
        res.is_err(),
        "proposing a non-token address should fail"
    );

    // No storage entry must have been written for the bogus address.
    assert!(
        !ctx.client().is_dst_token_allowed(&ctx.contract_id),
        "non-token address must not be stored in the allowlist"
    );
}

/// #33 (positive case): a real SEP-41 token passes the probe and is stored
/// once the timelocked propose/execute flow (#115/#118) completes.
#[test]
fn add_allowed_dst_token_accepts_real_token() {
    let ctx = setup();

    // dst_token was registered as a StellarAssetContract — it implements SEP-41.
    ctx.allow_dst_token(&ctx.dst_token);
    assert!(ctx.client().is_dst_token_allowed(&ctx.dst_token));
}
// ─── #34 Source chain allowlist ──────────────────────────────────────────────────

#[test]
fn src_chain_allowlist_disabled_by_default() {
    // The SrcChainAllowlistEnabled flag must default to false so any
    // existing deployment keeps working until an admin explicitly opts in.
    let ctx = setup();
    assert!(!ctx.client().is_src_chain_allowlist_enabled());
}

#[test]
fn src_chain_allowlist_disabled_allows_any_chain() {
    // With enforcement off, free-text src_chain values still go through --
    // matches the pre-#34 behaviour so no migration is required.
    let ctx = setup();
    assert!(!ctx.client().is_src_chain_allowlist_enabled());
    ctx.submit(); // "ethereum" -- would be rejected if enforcement were on and list were empty
}

#[test]
fn src_chain_allowlist_blocks_unlisted_chain_when_enabled() {
    let ctx = setup();
    let c = ctx.client();
    c.set_src_chain_allowlist_enabled(&true);

    let deadline: Option<u64> = None;
    let res = c.try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "etherium"), // typo -- not on list
        &String::from_str(&ctx.env, "0xabc"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::SrcChainNotAllowed.into())));
}

#[test]
fn src_chain_allowlist_allows_listed_chain_when_enabled() {
    let ctx = setup();
    let c = ctx.client();
    c.add_allowed_src_chain(&String::from_str(&ctx.env, "ethereum"));
    c.set_src_chain_allowlist_enabled(&true);

    assert!(c.is_src_chain_allowed(&String::from_str(&ctx.env, "ethereum")));
    // ctx.submit() uses "ethereum" -- should now succeed.
    ctx.submit();
}

#[test]
fn src_chain_allowlist_removal_blocks_previously_allowed_chain() {
    let ctx = setup();
    let c = ctx.client();
    let chain = String::from_str(&ctx.env, "ethereum");
    c.add_allowed_src_chain(&chain);
    c.set_src_chain_allowlist_enabled(&true);
    c.remove_allowed_src_chain(&chain);

    assert!(!c.is_src_chain_allowed(&String::from_str(&ctx.env, "ethereum")));

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
    assert_eq!(res, Err(Ok(Error::SrcChainNotAllowed.into())));
}

#[test]
fn src_chain_unlisted_accepted_after_disabling_enforcement() {
    // Disabling the flag after enabling it should restore open submission.
    let ctx = setup();
    let c = ctx.client();
    c.set_src_chain_allowlist_enabled(&true);
    c.set_src_chain_allowlist_enabled(&false);

    // "base" was never added to the list, but enforcement is off.
    let deadline: Option<u64> = None;
    c.submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "base"),
        &String::from_str(&ctx.env, "0xabc"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
}

// ─── #35 rescue_tokens ──────────────────────────────────────────────────────────

#[test]
fn rescue_tokens_moves_non_protocol_token_to_recipient() {
    let ctx = setup();
    let c = ctx.client();

    // Mint a random "lost" token directly to the contract.
    let rescue_token = ctx
        .env
        .register_stellar_asset_contract_v2(ctx.admin.clone())
        .address();
    let rescue_admin = token::StellarAssetClient::new(&ctx.env, &rescue_token);
    let rescue_client = token::Client::new(&ctx.env, &rescue_token);
    let rescue_amount: i128 = 1_000_000;
    rescue_admin.mint(&ctx.contract_id, &rescue_amount);

    assert_eq!(rescue_client.balance(&ctx.contract_id), rescue_amount);

    let recipient = Address::generate(&ctx.env);
    c.rescue_tokens(&rescue_token, &recipient, &rescue_amount);

    assert_eq!(rescue_client.balance(&ctx.contract_id), 0);
    assert_eq!(rescue_client.balance(&recipient), rescue_amount);
}

#[test]
fn rescue_tokens_blocked_for_bond_token() {
    // The bond_token is protected: rescuing it could drain solver collateral.
    let ctx = setup();
    let recipient = Address::generate(&ctx.env);
    let res = ctx
        .client()
        .try_rescue_tokens(&ctx.bond_token, &recipient, &1);
    assert_eq!(res, Err(Ok(Error::RescueProtectedToken.into())));
}

#[test]
fn rescue_tokens_zero_amount_fails() {
    let ctx = setup();
    // Register a different token so the zero-amount check fires, not the
    // protected-token check.
    let other_token = ctx
        .env
        .register_stellar_asset_contract_v2(ctx.admin.clone())
        .address();
    let recipient = Address::generate(&ctx.env);
    let res = ctx.client().try_rescue_tokens(&other_token, &recipient, &0);
    assert_eq!(res, Err(Ok(Error::ZeroAmount.into())));
}

#[test]
fn rescue_tokens_only_admin_can_call() {
    let ctx = setup();
    let other_token = ctx
        .env
        .register_stellar_asset_contract_v2(ctx.admin.clone())
        .address();
    let recipient = Address::generate(&ctx.env);

    // With mock_all_auths, verify that the admin auth is recorded by the
    // rescue_tokens call. If require_admin weren't present, the call would
    // succeed but would NOT record an auth for the admin address.
    let c = ctx.client();
    let token_admin = token::StellarAssetClient::new(&ctx.env, &other_token);
    token_admin.mint(&ctx.contract_id, &1_000);
    c.rescue_tokens(&other_token, &recipient, &1_000);

    let auths = ctx.env.auths();
    let admin_authed = auths.iter().any(|(addr, _)| *addr == ctx.admin);
    assert!(
        admin_authed,
        "rescue_tokens must require admin auth; got: {:?}",
        auths
    );
}

// ─── #36 Pause gates solver bond management ──────────────────────────────────────

#[test]
fn pause_blocks_register_solver() {
    let ctx = setup();
    let c = ctx.client();
    c.pause(&ctx.admin);

    ctx.bond_admin().mint(&ctx.solver, &BOND);
    let res = c.try_register_solver(&ctx.solver, &BOND);
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));
}

#[test]
fn pause_blocks_deregister_solver() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    c.pause(&ctx.admin);
    let res = c.try_deregister_solver(&ctx.solver);
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));
}

#[test]
fn pause_blocks_withdraw_bond() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    c.pause(&ctx.admin);
    let res = c.try_withdraw_bond(&ctx.solver, &(100 * 10_000_000));
    assert_eq!(res, Err(Ok(Error::ContractPaused.into())));
}

#[test]
fn unpause_restores_solver_bond_management() {
    let ctx = setup();
    let c = ctx.client();
    ctx.register_solver();

    let id = ctx.submit();

    // A single fill that exactly meets min_dst_amount.
    let fee = MIN_DST * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(MIN_DST + fee));
    c.accept_intent(&ctx.solver, &id);
    c.fill_intent(&ctx.solver, &id, &MIN_DST, &false);

    let intent = c.get_intent(&id).unwrap();
    assert_eq!(intent.state, IntentState::Filled);
    assert_eq!(intent.total_filled, MIN_DST);
    c.pause(&ctx.admin);
    c.unpause();

    // All three operations should succeed after unpause.
    let withdraw_amount = 100 * 10_000_000;
    c.withdraw_bond(&ctx.solver, &withdraw_amount);
    assert_eq!(
        c.get_solver(&ctx.solver).unwrap().bond_amount,
        BOND - withdraw_amount
    );

    c.deregister_solver(&ctx.solver);
    assert!(c.get_solver(&ctx.solver).is_none());
}

#[test]
fn pause_does_not_block_cancel_intent() {
    // cancel_intent stays open during a pause so users can always reclaim
    // their Open intents -- they shouldn't be locked in by an admin pause.
    let ctx = setup();
    let c = ctx.client();
    let id = ctx.submit();

    c.pause(&ctx.admin);
    c.cancel_intent(&ctx.user, &id);
    assert!(c.get_intent(&id).unwrap().state == IntentState::Cancelled);
}

// ─── #37 DstAllowlistEnabled default is false ────────────────────────────────────

#[test]
fn dst_allowlist_enabled_defaults_to_false() {
    // This test acts as a CI sentinel: if the default is ever changed from
    // false, this test will catch it before it reaches mainnet.
    //
    // Pre-launch action: once the allowed dst_token list is populated,
    // call set_dst_allowlist_enabled(true) before the contract goes live so
    // submit_intent validates every destination token.
    let ctx = setup();
    assert!(
        !ctx.client().is_dst_allowlist_enabled(),
        "DstAllowlistEnabled must default to false; \
         enable it explicitly via set_dst_allowlist_enabled before mainnet launch"
    );
}

// ─── #126 src_chain enum / allowlist coverage ────────────────────────────────────
// These tests document the full set of supported chain names and confirm that:
//   (a) each supported chain is accepted when the allowlist contains it, and
//   (b) an unsupported / typo'd chain is rejected when enforcement is on.

/// All five EVM chains in the supported set are accepted when individually
/// added to the allowlist and enforcement is enabled.
#[test]
fn src_chain_allowlist_accepts_all_supported_evm_chains() {
    let ctx = setup();
    let c = ctx.client();

    let chains = [
        "ethereum", "base", "polygon", "arbitrum", "optimism",
    ];

    for chain_str in &chains {
        let chain = String::from_str(&ctx.env, chain_str);
        c.add_allowed_src_chain(&chain);
    }
    c.set_src_chain_allowlist_enabled(&true);

    for chain_str in &chains {
        let chain = String::from_str(&ctx.env, chain_str);
        assert!(
            c.is_src_chain_allowed(&chain),
            "chain '{}' should be in allowlist",
            chain_str
        );
    }

    // submit_intent accepts each chain (uses a valid EVM token address).
    for chain_str in &chains {
        let deadline: Option<u64> = None;
        c.submit_intent(
            &ctx.user,
            &String::from_str(&ctx.env, chain_str),
            &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            &SRC_AMT,
            &ctx.dst_token,
            &MIN_DST,
            &deadline,
        );
    }
}

/// "solana" is accepted as a supported chain when on the allowlist.
#[test]
fn src_chain_allowlist_accepts_solana() {
    let ctx = setup();
    let c = ctx.client();
    let chain = String::from_str(&ctx.env, "solana");
    c.add_allowed_src_chain(&chain);
    c.set_src_chain_allowlist_enabled(&true);

    assert!(c.is_src_chain_allowed(&String::from_str(&ctx.env, "solana")));

    let deadline: Option<u64> = None;
    // Valid Solana SPL mint address (base58, 44 chars).
    c.submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "solana"),
        &String::from_str(&ctx.env, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
}

/// A completely unknown chain (not in the supported set) is rejected when
/// the allowlist is enabled, even if the name "looks" plausible.
#[test]
fn src_chain_allowlist_rejects_unknown_chain_when_enabled() {
    let ctx = setup();
    let c = ctx.client();
    // Enable enforcement without adding anything — all chains are blocked.
    c.set_src_chain_allowlist_enabled(&true);

    let unknown_chains = ["avalanche", "bnb", "etherium", "ETHEREUM", "eth"];
    for chain_str in &unknown_chains {
        let deadline: Option<u64> = None;
        let res = c.try_submit_intent(
            &ctx.user,
            &String::from_str(&ctx.env, chain_str),
            &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            &SRC_AMT,
            &ctx.dst_token,
            &MIN_DST,
            &deadline,
        );
        assert_eq!(
            res,
            Err(Ok(Error::SrcChainNotAllowed.into())),
            "chain '{}' should be rejected when not on allowlist",
            chain_str
        );
    }
}

/// Removing a chain from the allowlist immediately blocks it.
#[test]
fn src_chain_allowlist_removal_is_immediate() {
    let ctx = setup();
    let c = ctx.client();
    let chain = String::from_str(&ctx.env, "polygon");
    c.add_allowed_src_chain(&chain);
    c.set_src_chain_allowlist_enabled(&true);

    // Confirm it was there.
    assert!(c.is_src_chain_allowed(&String::from_str(&ctx.env, "polygon")));

    c.remove_allowed_src_chain(&chain);
    assert!(!c.is_src_chain_allowed(&String::from_str(&ctx.env, "polygon")));

    let deadline: Option<u64> = None;
    let res = c.try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "polygon"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::SrcChainNotAllowed.into())));
}

// ─── #127 src_token address format validation ────────────────────────────────────

// ── EVM chains ───────────────────────────────────────────────────────────────────

/// A well-formed EVM address (0x + 40 hex chars) is accepted on all EVM chains.
#[test]
fn valid_evm_token_accepted_on_evm_chains() {
    let ctx = setup();
    let c = ctx.client();

    let evm_chains = ["ethereum", "base", "polygon", "arbitrum", "optimism"];
    for chain_str in &evm_chains {
        let deadline: Option<u64> = None;
        c.submit_intent(
            &ctx.user,
            &String::from_str(&ctx.env, chain_str),
            // Canonical mixed-case checksum address.
            &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            &SRC_AMT,
            &ctx.dst_token,
            &MIN_DST,
            &deadline,
        );
    }
}

/// All-lowercase hex is also a valid EVM address format.
#[test]
fn valid_evm_token_lowercase_accepted() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    ctx.client().submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
}

/// Missing "0x" prefix on an EVM chain is rejected with InvalidSrcToken.
#[test]
fn evm_token_without_0x_prefix_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        // No "0x" prefix — 40 hex chars only.
        &String::from_str(&ctx.env, "A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

/// An EVM address that is too short (< 42 chars) is rejected.
#[test]
fn evm_token_too_short_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "base"),
        &String::from_str(&ctx.env, "0xabc"),   // only 5 chars
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

/// An EVM address that is too long (> 42 chars) is rejected.
#[test]
fn evm_token_too_long_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    // 43 chars total (0x + 41 hex).
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "polygon"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB4800"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

/// Non-hex characters after "0x" are rejected on an EVM chain.
#[test]
fn evm_token_non_hex_chars_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    // 42 chars but contains 'g' which is not a hex digit.
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "arbitrum"),
        &String::from_str(&ctx.env, "0xG0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

/// An empty token string on an EVM chain is rejected.
#[test]
fn evm_token_empty_string_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "optimism"),
        &String::from_str(&ctx.env, ""),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

// ── Solana chain ──────────────────────────────────────────────────────────────────

/// A valid Solana SPL mint address (44 base58 chars) is accepted.
#[test]
fn valid_solana_token_44_chars_accepted() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    ctx.client().submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "solana"),
        // USDC on Solana mainnet — 44 base58 chars.
        &String::from_str(&ctx.env, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
}

/// A 32-character base58 Solana address (minimum valid length) is accepted.
#[test]
fn valid_solana_token_32_chars_accepted() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    ctx.client().submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "solana"),
        // 32 valid base58 chars.
        &String::from_str(&ctx.env, "So11111111111111111111111111111z"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
}

/// A Solana token address shorter than 32 chars is rejected.
#[test]
fn solana_token_too_short_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "solana"),
        &String::from_str(&ctx.env, "EPjFWdd5AufqSSqeM2qN1xzybapC8"),  // 29 chars
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

/// A Solana token address longer than 44 chars is rejected.
#[test]
fn solana_token_too_long_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    // 45 chars — one too many.
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "solana"),
        &String::from_str(&ctx.env, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1vX"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

/// A Solana token with a "0x" prefix (EVM-style) is rejected.
#[test]
fn solana_token_with_0x_prefix_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "solana"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

/// A Solana token containing a character excluded from base58 ('0', 'I', 'O', 'l')
/// is rejected.
#[test]
fn solana_token_invalid_base58_char_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    // Contains '0' which is not in the base58 alphabet.
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "solana"),
        &String::from_str(&ctx.env, "0PjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    assert_eq!(res, Err(Ok(Error::InvalidSrcToken.into())));
}

// ── Unknown chain — validation bypass ────────────────────────────────────────────

/// An unknown (future) chain bypasses src_token format validation entirely.
/// This ensures forward-compatibility: a chain added later won't silently
/// reject all its tokens while the allowlist is off.
#[test]
fn unknown_chain_bypasses_token_format_validation() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    // "cosmos" is not a known chain — any token string should pass format validation.
    // (The allowlist is off by default so this reaches the format check.)
    ctx.client().submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "cosmos"),
        &String::from_str(&ctx.env, "cosmos1qyqa2zn5c925lyz4gq5qxsrx5gq5qxsr"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
}

// ─── Proof-gated fills (issue #190, docs/129-proof-mismatch-fallback.md) ─────

/// Deploy a `ProofRegistry`, initialise it, and point the settlement contract
/// at it via `set_proof_registry`. Returns the registry address.
fn deploy_proof_registry(ctx: &Ctx) -> Address {
    // The Wormhole Core address is irrelevant here — these tests inject proofs
    // through `mock_set_proof`, which never touches the verification path.
    let wormhole_core = Address::generate(&ctx.env);
    let reg_id = ctx.env.register_contract(None, ProofRegistry);
    ProofRegistryClient::new(&ctx.env, &reg_id).initialize(&ctx.admin, &wormhole_core);
    ctx.client().set_proof_registry(&reg_id);
    reg_id
}

/// Inject a proof record for `intent_id` into the registry at `reg_id`.
fn set_proof(ctx: &Ctx, reg_id: &Address, intent_id: &BytesN<32>, src_chain_id: u32, src_amount: i128) {
    ProofRegistryClient::new(&ctx.env, reg_id).mock_set_proof(&ProofRecord {
        intent_id: intent_id.clone(),
        src_user: String::from_str(&ctx.env, "0x0000000000000000000000000000000000000000"),
        src_chain_id,
        src_token: String::from_str(&ctx.env, "0x0000000000000000000000000000000000000000"),
        src_amount,
        vaa_sequence: 1,
        received_at: 0,
    });
}

/// Register the solver, submit a standard `"ethereum"` intent (Wormhole chain
/// id 2), accept it, and mint the solver enough dst token to fill + fee.
fn accepted_intent(ctx: &Ctx) -> BytesN<32> {
    ctx.register_solver();
    let id = ctx.submit();
    ctx.client().accept_intent(&ctx.solver, &id);
    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    id
}

/// `require_proof = false` behaves exactly as before — no registry needed.
#[test]
fn proof_gate_off_leaves_behaviour_unchanged() {
    let ctx = setup();
    let id = accepted_intent(&ctx);
    ctx.client().fill_intent(&ctx.solver, &id, &FILL, &false);
    assert_eq!(ctx.client().get_intent(&id).unwrap().state, IntentState::Filled);
}

/// docs/129 §2.4 — `require_proof = true` with no registry configured.
#[test]
fn proof_required_without_registry_is_config_error() {
    let ctx = setup();
    let id = accepted_intent(&ctx);
    let res = ctx.client().try_fill_intent(&ctx.solver, &id, &FILL, &true);
    assert_eq!(res, Err(Ok(Error::ProofRegistryNotSet.into())));
    // No slash path triggered — intent is still Accepted.
    assert_eq!(ctx.client().get_intent(&id).unwrap().state, IntentState::Accepted);
}

/// docs/129 §2.3 — registry configured but no proof for this intent.
#[test]
fn proof_required_but_absent_rejects_fill() {
    let ctx = setup();
    let _reg = deploy_proof_registry(&ctx);
    let id = accepted_intent(&ctx);
    let res = ctx.client().try_fill_intent(&ctx.solver, &id, &FILL, &true);
    assert_eq!(res, Err(Ok(Error::ProofNotFound.into())));
    assert_eq!(ctx.client().get_intent(&id).unwrap().state, IntentState::Accepted);
}

/// docs/129 §2.2 — proof exists but for the wrong source chain.
#[test]
fn proof_chain_mismatch_rejects_fill() {
    let ctx = setup();
    let reg = deploy_proof_registry(&ctx);
    let id = accepted_intent(&ctx);
    // Intent is "ethereum" (chain id 2); proof claims Polygon (5).
    set_proof(&ctx, &reg, &id, 5, SRC_AMT);
    let res = ctx.client().try_fill_intent(&ctx.solver, &id, &FILL, &true);
    assert_eq!(res, Err(Ok(Error::ProofChainMismatch.into())));
    let intent = ctx.client().get_intent(&id).unwrap();
    assert_eq!(intent.state, IntentState::Accepted); // still slashable

    // slash_solver stays reachable after the mismatch rejection (docs/129 §3).
    ctx.pass_time(FILL_WINDOW + 1);
    ctx.client().slash_solver(&id);
    assert_eq!(ctx.client().get_intent(&id).unwrap().state, IntentState::Open);
}

/// docs/129 §2.1 — proof's source deposit is smaller than the intent requires.
#[test]
fn proof_amount_insufficient_rejects_fill() {
    let ctx = setup();
    let reg = deploy_proof_registry(&ctx);
    let id = accepted_intent(&ctx);
    set_proof(&ctx, &reg, &id, 2, SRC_AMT - 1);
    let res = ctx.client().try_fill_intent(&ctx.solver, &id, &FILL, &true);
    assert_eq!(res, Err(Ok(Error::ProofAmountInsufficient.into())));
    assert_eq!(ctx.client().get_intent(&id).unwrap().state, IntentState::Accepted);
}

/// Happy path — matching chain and sufficient amount → the fill goes through.
#[test]
fn matching_proof_allows_fill() {
    let ctx = setup();
    let reg = deploy_proof_registry(&ctx);
    let id = accepted_intent(&ctx);
    set_proof(&ctx, &reg, &id, 2, SRC_AMT);
    ctx.client().fill_intent(&ctx.solver, &id, &FILL, &true);
    assert_eq!(ctx.client().get_intent(&id).unwrap().state, IntentState::Filled);
}

/// An `intent.src_chain` outside the docs/129 §4 mapping table cannot be
/// proof-validated.
#[test]
fn unsupported_src_chain_rejects_gated_fill() {
    let ctx = setup();
    let reg = deploy_proof_registry(&ctx);
    ctx.register_solver();
    // Submit a "cosmos" intent (not in the Wormhole chain-id table).
    let deadline: Option<u64> = None;
    let id = ctx.client().submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "cosmos"),
        &String::from_str(&ctx.env, "cosmos1qyqa2zn5c925lyz4gq5qxsrx5gq5qxsr"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
    );
    ctx.client().accept_intent(&ctx.solver, &id);
    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    set_proof(&ctx, &reg, &id, 2, SRC_AMT);
    let res = ctx.client().try_fill_intent(&ctx.solver, &id, &FILL, &true);
    assert_eq!(res, Err(Ok(Error::SrcChainNotSupported.into())));
}

// ─── #281 On-chain referral fee-share ────────────────────────────────────────────

/// #281: `submit_intent` rejects a referrer that equals the submitting user.
#[test]
fn submit_intent_self_referral_rejected() {
    let ctx = setup();
    let deadline: Option<u64> = None;
    let res = ctx.client().try_submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &deadline,
        &Some(ctx.user.clone()),
    );
    assert_eq!(res, Err(Ok(Error::SelfReferral.into())));
}

/// #281: with `referral_share_bps` > 0 the referrer receives its configured
/// slice and the FeeRecipient receives the remainder.  The user still gets the
/// full fill_amount; the fee is paid entirely by the solver.
#[test]
fn fill_intent_referrer_receives_configured_split() {
    let ctx = setup();
    let c = ctx.client();
    let referrer = Address::generate(&ctx.env);

    // Configure a 20% referral share (2000 bps of the protocol fee).
    c.set_config(&MIN_BOND, &FILL_WINDOW, &INTENT_EXPIRY, &5_i128, &2000_i128);

    ctx.register_solver();

    let id = c.submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &None,
        &Some(referrer.clone()),
    );

    c.accept_intent(&ctx.solver, &id);

    let fee = FILL * 5 / 10_000;
    let referral = fee * 2000 / 10_000;
    let recipient = fee - referral; // dust absorbed by FeeRecipient
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id, &FILL, &false);

    assert_eq!(ctx.dst().balance(&ctx.user), FILL);
    assert_eq!(ctx.dst().balance(&referrer), referral);
    assert_eq!(ctx.dst().balance(&ctx.fee_recipient), recipient);
    assert_eq!(ctx.dst().balance(&ctx.solver), 0);
}

/// #281: referral fee-share accrues on every partial fill, not just once at
/// final settlement.  With a 100% share the entire fee lands on the referrer
/// across both fills.
#[test]
fn partial_fills_accrue_referral_share_across_fills() {
    let ctx = setup();
    let c = ctx.client();
    let referrer = Address::generate(&ctx.env);

    // Configure 100% referral share for easy arithmetic.
    c.set_config(&MIN_BOND, &FILL_WINDOW, &INTENT_EXPIRY, &5_i128, &10_000_i128);

    ctx.register_solver();

    let id = c.submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &None,
        &Some(referrer.clone()),
    );

    // First partial fill: half of MIN_DST.
    let half = MIN_DST / 2;
    let fee1 = half * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(half + fee1));
    c.accept_intent(&ctx.solver, &id);
    c.fill_intent(&ctx.solver, &id, &half, &false);

    // 100% share: entire fee1 goes to the referrer.
    assert_eq!(ctx.dst().balance(&referrer), fee1);
    assert_eq!(ctx.dst().balance(&ctx.fee_recipient), 0);

    // Second partial fill: the remainder brings the intent to Filled.
    let remainder = MIN_DST - half;
    let fee2 = remainder * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(remainder + fee2));
    c.accept_intent(&ctx.solver, &id);
    c.fill_intent(&ctx.solver, &id, &remainder, &false);

    // Total referral: fee1 + fee2.
    assert_eq!(ctx.dst().balance(&referrer), fee1 + fee2);
    assert_eq!(ctx.dst().balance(&ctx.fee_recipient), 0);
}

/// #281: a 0 referral_share_bps (the default) routes 100% of the fee to
/// FeeRecipient even when a referrer is named — the share must be explicitly
/// configured by an admin to take effect.
#[test]
fn zero_referral_share_sends_all_fee_to_fee_recipient() {
    let ctx = setup();
    let c = ctx.client();
    let referrer = Address::generate(&ctx.env);

    // Explicitly set referral_share_bps to 0 for determinism (avoids relying
    // on the initialized default, which references DEFAULT_* constants).
    c.set_config(&MIN_BOND, &FILL_WINDOW, &INTENT_EXPIRY, &5_i128, &0_i128);

    ctx.register_solver();

    let id = c.submit_intent(
        &ctx.user,
        &String::from_str(&ctx.env, "ethereum"),
        &String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        &SRC_AMT,
        &ctx.dst_token,
        &MIN_DST,
        &None,
        &Some(referrer.clone()),
    );

    c.accept_intent(&ctx.solver, &id);

    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id, &FILL, &false);

    // With 0 share, the entire fee goes to FeeRecipient even though a
    // referrer was named.
    assert_eq!(ctx.dst().balance(&ctx.user), FILL);
    assert_eq!(ctx.dst().balance(&referrer), 0);
    assert_eq!(ctx.dst().balance(&ctx.fee_recipient), fee);
    assert_eq!(ctx.dst().balance(&ctx.solver), 0);
}

/// #281 regression: no referrer set behaves identically to before #281 — the
/// full fee goes to FeeRecipient, even when a non-zero referral share is
/// configured.
#[test]
fn fill_intent_no_referrer_unchanged_behavior() {
    let ctx = setup();
    let c = ctx.client();

    // Configure a non-zero referral share to prove it has no effect when
    // no referrer is set on the intent.
    c.set_config(&MIN_BOND, &FILL_WINDOW, &INTENT_EXPIRY, &5_i128, &2000_i128);

    ctx.register_solver();
    let id = ctx.submit(); // referrer = None
    c.accept_intent(&ctx.solver, &id);

    let fee = FILL * 5 / 10_000;
    ctx.dst_admin().mint(&ctx.solver, &(FILL + fee));
    c.fill_intent(&ctx.solver, &id, &FILL, &false);

    // No referrer → full fee to FeeRecipient, even though the share is 20%.
    assert_eq!(ctx.dst().balance(&ctx.user), FILL);
    assert_eq!(ctx.dst().balance(&ctx.fee_recipient), fee);
    assert_eq!(ctx.dst().balance(&ctx.solver), 0);
}
