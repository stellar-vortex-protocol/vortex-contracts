#![cfg(test)]

//! Test suite for `solver_registry`.
//!
//! Covers: tier-table seeding, registration/stake/unstake/deregister, the
//! settlement write path (`record_fill` / `record_failure` / `slash`), tier
//! boundary transitions (score exactly on a threshold), tier demotion on
//! slash, the zero-fills edge case, and threshold tuning bounds.

use crate::{
    Error, SolverRecord, SolverRegistry, SolverRegistryClient, ADMIN_TIMELOCK_DELAY, USDC,
    WRITER_TIMELOCK_DELAY,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env,
};
use core::sync::atomic::{AtomicU32, Ordering};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, BytesN, Env, Symbol,
};

/// A fresh `intent_id` per call, so tests that don't exercise idempotency
/// (#390) never collide on a `Recorded` key.
fn next_intent(env: &Env) -> BytesN<32> {
    static COUNTER: AtomicU32 = AtomicU32::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&n.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

const FLOOR: i128 = 50 * USDC; // tier-0 (Unranked) bond floor

struct Ctx {
    env: Env,
    admin: Address,
    fee_recipient: Address,
    solver: Address,
    bond_token: Address,
    contract_id: Address,
}

impl Ctx {
    fn client(&self) -> SolverRegistryClient<'_> {
        SolverRegistryClient::new(&self.env, &self.contract_id)
    }
    fn bond(&self) -> token::Client<'_> {
        token::Client::new(&self.env, &self.bond_token)
    }
    fn mint(&self, to: &Address, amount: i128) {
        token::StellarAssetClient::new(&self.env, &self.bond_token).mint(to, &amount);
    }
    /// Mint `bond` to the default solver and register them.
    fn register(&self, bond: i128) {
        self.mint(&self.solver, bond);
        self.client().register_solver(&self.solver, &bond);
    }
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    let solver = Address::generate(&env);
    let bond_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_id = env.register_contract(None, SolverRegistry);

    let ctx = Ctx {
        env,
        admin,
        fee_recipient,
        solver,
        bond_token,
        contract_id,
    };
    ctx.client()
        .initialize(&ctx.admin, &ctx.bond_token, &ctx.fee_recipient);
    ctx
}

// ─── Initialization ─────────────────────────────────────────────────────────

#[test]
fn initialize_seeds_the_design_doc_tier_table() {
    let ctx = setup();
    let table = ctx.client().get_tier_table();
    assert_eq!(table.len(), 5);

    let r0 = table.get(0).unwrap();
    assert_eq!(r0.min_bond, 50 * USDC);
    assert_eq!(r0.min_score_bps, 0);
    assert_eq!(r0.fill_window_bonus_pct, 0);
    assert_eq!(r0.slash_bps, 1_000);

    let r4 = table.get(4).unwrap();
    assert_eq!(r4.min_bond, 50_000 * USDC);
    assert_eq!(r4.min_score_bps, 9_000);
    assert_eq!(r4.fill_window_bonus_pct, 50);
    assert_eq!(r4.slash_bps, 500);
    assert_eq!(r4.fee_rebate_bps, 0); // reserved slot

    assert_eq!(ctx.client().get_admin(), Some(ctx.admin.clone()));
    assert_eq!(ctx.client().get_bond_token(), Some(ctx.bond_token.clone()));
}

#[test]
fn cannot_initialize_twice() {
    let ctx = setup();
    let res = ctx
        .client()
        .try_initialize(&ctx.admin, &ctx.bond_token, &ctx.fee_recipient);
    assert_eq!(res, Err(Ok(Error::AlreadyInitialized.into())));
}

// ─── Registration / staking ────────────────────────────────────────────────

#[test]
fn register_rejects_bond_below_floor() {
    let ctx = setup();
    ctx.mint(&ctx.solver, FLOOR);
    let res = ctx.client().try_register_solver(&ctx.solver, &(FLOOR - 1));
    assert_eq!(res, Err(Ok(Error::BondBelowFloor.into())));
}

#[test]
fn register_rejects_zero() {
    let ctx = setup();
    let res = ctx.client().try_register_solver(&ctx.solver, &0);
    assert_eq!(res, Err(Ok(Error::ZeroAmount.into())));
}

#[test]
fn register_locks_bond_and_counts_solver() {
    let ctx = setup();
    ctx.register(FLOOR);
    assert_eq!(ctx.client().get_solver_count(), 1);
    assert_eq!(ctx.bond().balance(&ctx.contract_id), FLOOR);
    assert_eq!(ctx.bond().balance(&ctx.solver), 0);
    // No fills yet → score 0 → Unranked.
    assert_eq!(ctx.client().get_tier(&ctx.solver), 0);
}

#[test]
fn register_twice_rejected() {
    let ctx = setup();
    ctx.register(FLOOR);
    ctx.mint(&ctx.solver, FLOOR);
    let res = ctx.client().try_register_solver(&ctx.solver, &FLOOR);
    assert_eq!(res, Err(Ok(Error::SolverAlreadyRegistered.into())));
}

#[test]
fn stake_then_unstake_back_to_floor() {
    let ctx = setup();
    ctx.register(FLOOR);
    ctx.mint(&ctx.solver, 1_000 * USDC);
    ctx.client().stake(&ctx.solver, &(1_000 * USDC));
    assert_eq!(ctx.client().get_solver(&ctx.solver).unwrap().bond_amount, FLOOR + 1_000 * USDC);

    ctx.client().unstake(&ctx.solver, &(1_000 * USDC));
    assert_eq!(ctx.client().get_solver(&ctx.solver).unwrap().bond_amount, FLOOR);

    // One more unit would drop below the floor.
    let res = ctx.client().try_unstake(&ctx.solver, &1);
    assert_eq!(res, Err(Ok(Error::BondBelowFloor.into())));
}

#[test]
fn unstake_more_than_bond_rejected() {
    let ctx = setup();
    ctx.register(FLOOR);
    let res = ctx.client().try_unstake(&ctx.solver, &(FLOOR + 1));
    assert_eq!(res, Err(Ok(Error::InsufficientBond.into())));
}

#[test]
fn deregister_returns_full_bond() {
    let ctx = setup();
    ctx.register(100 * USDC);
    ctx.client().deregister_solver(&ctx.solver);
    assert_eq!(ctx.bond().balance(&ctx.solver), 100 * USDC);
    assert!(ctx.client().get_solver(&ctx.solver).is_none());
    assert_eq!(ctx.client().get_solver_count(), 0);
}

// ─── Reputation formula ────────────────────────────────────────────────────

/// Shared input→output vector. `intent_settlement::compute_reputation_score`
/// MUST produce the identical values for the same inputs — this is the
/// cross-check referenced by the design doc.
#[test]
fn score_test_vector() {
    let env = Env::default();
    let who = Address::generate(&env);
    let rec = |completed: u32, failed: u32, vol: i128| SolverRecord {
        address: who.clone(),
        bond_amount: 0,
        fills_completed: completed,
        fills_failed: failed,
        total_volume: vol,
        registered_at: 0,
        last_slash_time: 0,
        slashed_total: 0,
    };

    // no activity → 0
    assert_eq!(SolverRegistry::compute_reputation_score(rec(0, 0, 0)), 0);
    // all failures → 0
    assert_eq!(SolverRegistry::compute_reputation_score(rec(0, 5, 0)), 0);
    // perfect record, no volume → 9_001 (90% floor + integer rounding)
    assert_eq!(SolverRegistry::compute_reputation_score(rec(1, 0, 0)), 9_001);
    // 8/10 success, no volume → 7_200 (0.8 * 9_001, truncated)
    assert_eq!(SolverRegistry::compute_reputation_score(rec(8, 2, 0)), 7_200);
    // 8/10 success, very high volume → decay ≈ 0, multiplier ≈ 10_000 → 8_000
    assert_eq!(
        SolverRegistry::compute_reputation_score(rec(8, 2, 100_000_000 * USDC)),
        8_000
    );
}

#[test]
fn get_reputation_score_none_for_unknown() {
    let ctx = setup();
    assert_eq!(ctx.client().get_reputation_score(&ctx.solver), None);
}

// ─── Tier boundaries (pure lookup) ─────────────────────────────────────────

#[test]
fn tier_for_hits_each_threshold_exactly() {
    let ctx = setup();
    let c = ctx.client();

    // Unranked
    assert_eq!(c.tier_for(&0, &FLOOR), 0);
    assert_eq!(c.tier_for(&0, &(FLOOR - 1)), 0);

    // Bronze: score 1_000 bps AND bond 500 USDC
    assert_eq!(c.tier_for(&1_000, &(500 * USDC)), 1);
    assert_eq!(c.tier_for(&999, &(500 * USDC)), 0); // score one bp short
    assert_eq!(c.tier_for(&1_000, &(500 * USDC - 1)), 0); // bond one unit short

    // Silver / Gold
    assert_eq!(c.tier_for(&3_500, &(2_000 * USDC)), 2);
    assert_eq!(c.tier_for(&3_499, &(2_000 * USDC)), 1);
    assert_eq!(c.tier_for(&7_000, &(10_000 * USDC)), 3);

    // Platinum
    assert_eq!(c.tier_for(&9_000, &(50_000 * USDC)), 4);
    assert_eq!(c.tier_for(&8_999, &(50_000 * USDC)), 3);
    // Score above the (unreachable) 10_000 ceiling still resolves fine.
    assert_eq!(c.tier_for(&10_000, &(60_000 * USDC)), 4);
}

// ─── Zero-fills edge case ──────────────────────────────────────────────────

#[test]
fn zero_fills_pins_tier_to_unranked_regardless_of_bond() {
    let ctx = setup();
    // Bond large enough for Platinum, but no fills → score 0 → tier 0.
    ctx.register(60_000 * USDC);
    assert_eq!(ctx.client().get_reputation_score(&ctx.solver), Some(0));
    assert_eq!(ctx.client().get_tier(&ctx.solver), 0);
}

// ─── Settlement write path ─────────────────────────────────────────────────

#[test]
fn record_fill_updates_volume_and_score() {
    let ctx = setup();
    ctx.register(FLOOR);
    // No writer configured yet → admin drives the write path.
    ctx.client().record_fill(
        &ctx.admin,
        &ctx.solver,
        &next_intent(&ctx.env),
        &(100 * USDC),
    );

    let rec = ctx.client().get_solver(&ctx.solver).unwrap();
    assert_eq!(rec.fills_completed, 1);
    assert_eq!(rec.total_volume, 100 * USDC);
    assert!(ctx.client().get_reputation_score(&ctx.solver).unwrap() > 0);
}

#[test]
fn record_failure_lowers_score() {
    let ctx = setup();
    ctx.register(FLOOR);
    ctx.client()
        .record_fill(&ctx.admin, &ctx.solver, &next_intent(&ctx.env), &0);
    let before = ctx.client().get_reputation_score(&ctx.solver).unwrap();
    ctx.client()
        .record_failure(&ctx.admin, &ctx.solver, &next_intent(&ctx.env));
    let after = ctx.client().get_reputation_score(&ctx.solver).unwrap();
    assert!(after < before, "{after} !< {before}");
}

#[test]
fn writer_can_drive_write_path_and_strangers_cannot() {
    let ctx = setup();
    ctx.register(FLOOR);
    let writer = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);

    // Before a writer is set, a stranger is rejected with WriterNotSet.
    assert_eq!(
        ctx.client()
            .try_record_fill(&stranger, &ctx.solver, &next_intent(&ctx.env), &0),
        Err(Ok(Error::WriterNotSet.into()))
    );

    ctx.client().set_writer(&writer);
    assert_eq!(ctx.client().get_writer(), Some(writer.clone()));

    // Writer works…
    ctx.client()
        .record_fill(&writer, &ctx.solver, &next_intent(&ctx.env), &(10 * USDC));
    assert_eq!(ctx.client().get_solver(&ctx.solver).unwrap().fills_completed, 1);

    // …a stranger still does not.
    assert_eq!(
        ctx.client()
            .try_record_fill(&stranger, &ctx.solver, &next_intent(&ctx.env), &0),
        Err(Ok(Error::Unauthorized.into()))
    );
}

// ─── Timelocked two-step admin transfer (#393) ─────────────────────────────

fn advance(ctx: &Ctx, secs: u64) {
    let now = ctx.env.ledger().timestamp();
    ctx.env.ledger().set_timestamp(now + secs);
}

#[test]
fn admin_transfer_waits_for_the_timelock_and_the_new_admin() {
    let ctx = setup();
    let c = ctx.client();
    let new_admin = Address::generate(&ctx.env);

    let proposed_at = ctx.env.ledger().timestamp();
    c.propose_admin(&new_admin);
    let eta = proposed_at + ADMIN_TIMELOCK_DELAY;
    assert_eq!(c.get_pending_admin(), Some((new_admin.clone(), eta)));

    // One second early: rejected, and the old admin is still in charge.
    advance(&ctx, ADMIN_TIMELOCK_DELAY - 1);
    assert_eq!(
        c.try_accept_admin(&new_admin),
        Err(Ok(Error::AdminTimelockNotElapsed.into()))
    );
    assert_eq!(c.get_admin(), Some(ctx.admin.clone()));

    // Exactly at the eta: the new admin signs and takes over.
    advance(&ctx, 1);
    c.accept_admin(&new_admin);
    let auths = ctx.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, new_admin);
    assert_eq!(c.get_admin(), Some(new_admin.clone()));
    assert_eq!(c.get_pending_admin(), None);

    // Admin-only calls now authenticate against the new admin.
    c.set_writer(&Address::generate(&ctx.env));
    assert_eq!(ctx.env.auths()[0].0, new_admin);
}

#[test]
fn accept_admin_must_come_from_the_proposed_address() {
    let ctx = setup();
    let c = ctx.client();
    c.propose_admin(&Address::generate(&ctx.env));
    advance(&ctx, ADMIN_TIMELOCK_DELAY);
    assert_eq!(
        c.try_accept_admin(&Address::generate(&ctx.env)),
        Err(Ok(Error::Unauthorized.into()))
    );
    assert_eq!(c.get_admin(), Some(ctx.admin.clone()));
}

#[test]
fn accept_admin_requires_the_new_admins_signature() {
    let ctx = setup();
    let c = ctx.client();
    let new_admin = Address::generate(&ctx.env);
    c.propose_admin(&new_admin);
    advance(&ctx, ADMIN_TIMELOCK_DELAY);

    // Drop the blanket auth mock: nobody has signed.
    ctx.env.set_auths(&[]);
    assert!(c.try_accept_admin(&new_admin).is_err());
    assert_eq!(c.get_admin(), Some(ctx.admin.clone()));
}

#[test]
fn a_new_admin_proposal_replaces_the_old_one_and_resets_the_timelock() {
    let ctx = setup();
    let c = ctx.client();
    let first = Address::generate(&ctx.env);
    let second = Address::generate(&ctx.env);

    c.propose_admin(&first);
    advance(&ctx, ADMIN_TIMELOCK_DELAY - 10);
    c.propose_admin(&second);
    advance(&ctx, 10);

    assert_eq!(
        c.try_accept_admin(&first),
        Err(Ok(Error::Unauthorized.into()))
    );
    assert_eq!(
        c.try_accept_admin(&second),
        Err(Ok(Error::AdminTimelockNotElapsed.into()))
    );
    advance(&ctx, ADMIN_TIMELOCK_DELAY);
    c.accept_admin(&second);
    assert_eq!(c.get_admin(), Some(second));
}

#[test]
fn cancel_admin_transfer_discards_the_pending_handover() {
    let ctx = setup();
    let c = ctx.client();
    assert_eq!(
        c.try_cancel_admin_transfer(),
        Err(Ok(Error::NoPendingAdminTransfer.into()))
    );

    let proposed = Address::generate(&ctx.env);
    c.propose_admin(&proposed);
    c.cancel_admin_transfer();
    assert_eq!(c.get_pending_admin(), None);

    advance(&ctx, ADMIN_TIMELOCK_DELAY);
    assert_eq!(
        c.try_accept_admin(&proposed),
        Err(Ok(Error::NoPendingAdminTransfer.into()))
    );
    assert_eq!(c.get_admin(), Some(ctx.admin.clone()));
}

#[test]
fn propose_and_cancel_require_the_current_admin() {
    let ctx = setup();
    let c = ctx.client();
    let proposed = Address::generate(&ctx.env);
    c.propose_admin(&proposed);
    assert_eq!(ctx.env.auths()[0].0, ctx.admin);

    ctx.env.set_auths(&[]);
    assert!(c.try_propose_admin(&Address::generate(&ctx.env)).is_err());
    assert!(c.try_cancel_admin_transfer().is_err());
    assert_eq!(c.get_pending_admin().map(|(a, _)| a), Some(proposed));
}

#[test]
fn set_writer_only_bootstraps_the_first_writer() {
    let ctx = setup();
    let c = ctx.client();
    let first = Address::generate(&ctx.env);
    c.set_writer(&first);
    // Rotation can no longer bypass the timelock through set_writer.
    assert_eq!(
        c.try_set_writer(&Address::generate(&ctx.env)),
        Err(Ok(Error::WriterAlreadySet.into()))
    );
    assert_eq!(c.get_writer(), Some(first));
}

#[test]
fn writer_rotation_waits_for_the_timelock() {
    let ctx = setup();
    ctx.register(FLOOR);
    let c = ctx.client();
    let old = Address::generate(&ctx.env);
    let new = Address::generate(&ctx.env);
    c.set_writer(&old);

    let proposed_at = ctx.env.ledger().timestamp();
    c.propose_writer(&new);
    let eta = proposed_at + WRITER_TIMELOCK_DELAY;
    assert_eq!(c.get_pending_writer(), Some((new.clone(), eta)));

    // One second early: rejected, and the old writer is still in charge.
    advance(&ctx, WRITER_TIMELOCK_DELAY - 1);
    assert_eq!(
        c.try_execute_writer(&new),
        Err(Ok(Error::TimelockNotElapsed.into()))
    );
    assert_eq!(c.get_writer(), Some(old.clone()));
    c.record_fill(&old, &ctx.solver, &0);

    // Exactly at the eta: applied.
    advance(&ctx, 1);
    c.execute_writer(&new);
    assert_eq!(c.get_writer(), Some(new.clone()));
    assert_eq!(c.get_pending_writer(), None);

    // The old writer lost the write path; the new one has it.
    assert_eq!(
        c.try_record_fill(&old, &ctx.solver, &0),
        Err(Ok(Error::Unauthorized.into()))
    );
    c.record_fill(&new, &ctx.solver, &0);
}

#[test]
fn execute_writer_must_match_the_proposal() {
    let ctx = setup();
    let c = ctx.client();
    c.set_writer(&Address::generate(&ctx.env));
    let proposed = Address::generate(&ctx.env);
    c.propose_writer(&proposed);
    advance(&ctx, WRITER_TIMELOCK_DELAY);
    assert_eq!(
        c.try_execute_writer(&Address::generate(&ctx.env)),
        Err(Ok(Error::Unauthorized.into()))
    );
}

#[test]
fn a_new_proposal_replaces_the_old_one_and_resets_the_timelock() {
    let ctx = setup();
    let c = ctx.client();
    c.set_writer(&Address::generate(&ctx.env));
    let first = Address::generate(&ctx.env);
    let second = Address::generate(&ctx.env);

    c.propose_writer(&first);
    advance(&ctx, WRITER_TIMELOCK_DELAY - 10);
    c.propose_writer(&second);
    advance(&ctx, 10);

    // The first proposal is gone, and the second hasn't aged enough.
    assert_eq!(
        c.try_execute_writer(&first),
        Err(Ok(Error::Unauthorized.into()))
    );
    assert_eq!(
        c.try_execute_writer(&second),
        Err(Ok(Error::TimelockNotElapsed.into()))
    );
    advance(&ctx, WRITER_TIMELOCK_DELAY);
    c.execute_writer(&second);
    assert_eq!(c.get_writer(), Some(second));
}

#[test]
fn cancel_writer_discards_the_pending_rotation() {
    let ctx = setup();
    let c = ctx.client();
    let current = Address::generate(&ctx.env);
    c.set_writer(&current);
    assert_eq!(
        c.try_cancel_writer(),
        Err(Ok(Error::NoPendingWriter.into()))
    );

    let proposed = Address::generate(&ctx.env);
    c.propose_writer(&proposed);
    c.cancel_writer();
    assert_eq!(c.get_pending_writer(), None);

    advance(&ctx, WRITER_TIMELOCK_DELAY);
    assert_eq!(
        c.try_execute_writer(&proposed),
        Err(Ok(Error::NoPendingWriter.into()))
    );
    assert_eq!(c.get_writer(), Some(current));
}

#[test]
fn writer_rotation_requires_admin_auth() {
    let ctx = setup();
    let c = ctx.client();
    c.set_writer(&Address::generate(&ctx.env));
    let proposed = Address::generate(&ctx.env);
    c.propose_writer(&proposed);
    advance(&ctx, WRITER_TIMELOCK_DELAY);

    // Drop the blanket auth mock: the admin has not signed.
    ctx.env.set_auths(&[]);
    assert!(c.try_propose_writer(&Address::generate(&ctx.env)).is_err());
    assert!(c.try_execute_writer(&proposed).is_err());
    assert!(c.try_cancel_writer().is_err());
    assert_eq!(c.get_pending_writer().map(|(w, _)| w), Some(proposed));
}

// ─── Per-intent idempotency (#390) ─────────────────────────────────────────

#[test]
fn record_fill_is_exactly_once_per_intent() {
    let ctx = setup();
    ctx.register(FLOOR);
    let c = ctx.client();
    let intent = next_intent(&ctx.env);

    c.record_fill(&ctx.admin, &ctx.solver, &intent, &(10 * USDC));
    assert_eq!(
        c.try_record_fill(&ctx.admin, &ctx.solver, &intent, &(10 * USDC)),
        Err(Ok(Error::AlreadyRecorded.into()))
    );
    let record = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(record.fills_completed, 1);
    assert_eq!(record.total_volume, 10 * USDC);
}

#[test]
fn record_failure_is_exactly_once_per_intent() {
    let ctx = setup();
    ctx.register(FLOOR);
    let c = ctx.client();
    let intent = next_intent(&ctx.env);

    c.record_failure(&ctx.admin, &ctx.solver, &intent);
    assert_eq!(
        c.try_record_failure(&ctx.admin, &ctx.solver, &intent),
        Err(Ok(Error::AlreadyRecorded.into()))
    );
    assert_eq!(c.get_solver(&ctx.solver).unwrap().fills_failed, 1);
}

#[test]
fn a_retried_slash_cannot_double_slash() {
    let ctx = setup();
    ctx.register(10 * FLOOR);
    let c = ctx.client();
    let intent = next_intent(&ctx.env);

    let (slashed, _) = c.slash(&ctx.admin, &ctx.solver, &intent);
    let bond_after_first = c.get_solver(&ctx.solver).unwrap().bond_amount;
    assert_eq!(
        c.try_slash(&ctx.admin, &ctx.solver, &intent),
        Err(Ok(Error::AlreadyRecorded.into()))
    );
    let record = c.get_solver(&ctx.solver).unwrap();
    assert_eq!(record.bond_amount, bond_after_first);
    assert_eq!(record.slashed_total, slashed);
    assert_eq!(ctx.bond().balance(&ctx.fee_recipient), slashed);
}

#[test]
fn idempotency_keys_are_per_action() {
    let ctx = setup();
    ctx.register(10 * FLOOR);
    let c = ctx.client();
    let intent = next_intent(&ctx.env);

    // A failure and a slash for the same intent are distinct writes.
    c.record_failure(&ctx.admin, &ctx.solver, &intent);
    c.slash(&ctx.admin, &ctx.solver, &intent);
    let fill = Symbol::new(&ctx.env, "fill");
    let failure = Symbol::new(&ctx.env, "failure");
    let slash = Symbol::new(&ctx.env, "slash");
    assert!(c.is_intent_recorded(&failure, &intent));
    assert!(c.is_intent_recorded(&slash, &intent));
    assert!(!c.is_intent_recorded(&fill, &intent));
}

#[test]
fn an_intent_fill_cannot_be_credited_to_a_second_solver() {
    let ctx = setup();
    ctx.register(FLOOR);
    let c = ctx.client();
    let other = Address::generate(&ctx.env);
    ctx.mint(&other, FLOOR);
    c.register_solver(&other, &FLOOR);
    let intent = next_intent(&ctx.env);

    c.record_fill(&ctx.admin, &ctx.solver, &intent, &(10 * USDC));
    assert_eq!(
        c.try_record_fill(&ctx.admin, &other, &intent, &(10 * USDC)),
        Err(Ok(Error::AlreadyRecorded.into()))
    );
    assert_eq!(c.get_solver(&other).unwrap().fills_completed, 0);
}

#[test]
fn a_failed_write_does_not_consume_the_intent() {
    let ctx = setup();
    let c = ctx.client();
    let intent = next_intent(&ctx.env);

    // Solver not registered yet: the write reverts, key included.
    assert_eq!(
        c.try_record_fill(&ctx.admin, &ctx.solver, &intent, &0),
        Err(Ok(Error::SolverNotRegistered.into()))
    );
    assert!(!c.is_intent_recorded(&Symbol::new(&ctx.env, "fill"), &intent));

    ctx.register(FLOOR);
    c.record_fill(&ctx.admin, &ctx.solver, &intent, &0);
    assert_eq!(c.get_solver(&ctx.solver).unwrap().fills_completed, 1);
}

#[test]
fn obligation_path_accepts_only_the_writer() {
    let ctx = setup();
    ctx.register(FLOOR);
    let c = ctx.client();
    let a = intent(&ctx.env, 1);
    let writer = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);

    // No writer configured: even the admin is rejected.
    assert_eq!(
        c.try_lock_obligation(&ctx.admin, &ctx.solver, &a),
        Err(Ok(Error::WriterNotSet.into()))
    );
    assert_eq!(
        c.try_release_obligation(&ctx.admin, &ctx.solver, &a),
        Err(Ok(Error::WriterNotSet.into()))
    );

    c.set_writer(&writer);
    for caller in [&ctx.admin, &stranger] {
        assert_eq!(
            c.try_lock_obligation(caller, &ctx.solver, &a),
            Err(Ok(Error::Unauthorized.into()))
        );
        assert_eq!(
            c.try_release_obligation(caller, &ctx.solver, &a),
            Err(Ok(Error::Unauthorized.into()))
        );
    }
    assert_eq!(c.get_open_obligations(&ctx.solver), 0);
}

#[test]
fn obligation_path_requires_writer_auth() {
    let ctx = setup();
    let writer = with_writer(&ctx);
    let c = ctx.client();
    // Drop the blanket auth mock: the writer has not signed.
    ctx.env.set_auths(&[]);
    assert!(c
        .try_lock_obligation(&writer, &ctx.solver, &intent(&ctx.env, 1))
        .is_err());
    assert_eq!(c.get_open_obligations(&ctx.solver), 0);
}

#[test]
fn lock_obligation_rejects_unregistered_solver() {
    let ctx = setup();
    let writer = with_writer(&ctx);
    let unknown = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client()
            .try_lock_obligation(&writer, &unknown, &intent(&ctx.env, 1)),
        Err(Ok(Error::SolverNotRegistered.into()))
    );
}

#[test]
fn obligations_are_scoped_per_solver() {
    let ctx = setup();
    let writer = with_writer(&ctx);
    let c = ctx.client();
    let other = Address::generate(&ctx.env);
    ctx.mint(&other, FLOOR);
    c.register_solver(&other, &FLOOR);
}

// ─── Tier demotion on slash ────────────────────────────────────────────────

#[test]
fn slash_demotes_tier_and_pays_fee_recipient() {
    let ctx = setup();
    // Bond exactly at the Bronze floor.
    ctx.register(500 * USDC);
    // One clean fill → score ~9_001 → qualifies for Bronze (needs >= 1_000).
    ctx.client().record_fill(
        &ctx.admin,
        &ctx.solver,
        &next_intent(&ctx.env),
        &(100 * USDC),
    );
    assert_eq!(ctx.client().get_tier(&ctx.solver), 1);
}

// ─── Tier demotion on slash ────────────────────────────────────────────────

#[test]
fn slash_demotes_tier_and_pays_fee_recipient() {
    let ctx = setup();
    // Bond exactly at the Bronze floor.
    ctx.register(500 * USDC);
    // One clean fill → score ~9_001 → qualifies for Bronze (needs >= 1_000).
    ctx.client().record_fill(
        &ctx.admin,
        &ctx.solver,
        &next_intent(&ctx.env),
        &(100 * USDC),
    );
    assert_eq!(ctx.client().get_tier(&ctx.solver), 1);

    let (slashed, new_tier) = ctx
        .client()
        .slash(&ctx.admin, &ctx.solver, &next_intent(&ctx.env));

    // Bronze slash is the full 10% → 50 USDC, dropping bond to 450 USDC,
    // below the 500 USDC Bronze floor → demoted to Unranked.
    assert_eq!(slashed, 50 * USDC);
    assert_eq!(new_tier, 0);
    assert_eq!(ctx.client().get_tier(&ctx.solver), 0);
    assert_eq!(ctx.bond().balance(&ctx.fee_recipient), 50 * USDC);

    let rec = ctx.client().get_solver(&ctx.solver).unwrap();
    assert_eq!(rec.bond_amount, 450 * USDC);
    assert_eq!(rec.fills_failed, 1);
    assert_eq!(rec.slashed_total, 50 * USDC);
    assert!(rec.last_slash_time >= rec.registered_at);
}

#[test]
fn slash_uses_the_tier_specific_bps() {
    let ctx = setup();
    // Platinum: bond 50_000 USDC + a clean fill → score ~9_001 ≥ 9_000.
    ctx.register(50_000 * USDC);
    ctx.client().record_fill(
        &ctx.admin,
        &ctx.solver,
        &next_intent(&ctx.env),
        &(100 * USDC),
    );
    assert_eq!(ctx.client().get_tier(&ctx.solver), 4);

    // Platinum slash bps = 500 → 5% of 50_000 = 2_500 USDC.
    let (slashed, _new_tier) = ctx
        .client()
        .slash(&ctx.admin, &ctx.solver, &next_intent(&ctx.env));
    assert_eq!(slashed, 2_500 * USDC);
}

// ─── Threshold tuning ──────────────────────────────────────────────────────

#[test]
fn set_tier_threshold_changes_gating() {
    let ctx = setup();
    let c = ctx.client();

    // Raise Bronze to (600 USDC, 1_200 bps).
    c.set_tier_threshold(&1, &(600 * USDC), &1_200);
    let row = c.get_tier_table().get(1).unwrap();
    assert_eq!(row.min_bond, 600 * USDC);
    assert_eq!(row.min_score_bps, 1_200);

    assert_eq!(c.tier_for(&1_200, &(600 * USDC)), 1);
    assert_eq!(c.tier_for(&1_100, &(600 * USDC)), 0); // below the raised score bar
    assert_eq!(c.tier_for(&1_200, &(599 * USDC)), 0); // below the raised bond bar
}

#[test]
fn set_tier_threshold_rejects_tier_zero() {
    let ctx = setup();
    let res = ctx.client().try_set_tier_threshold(&0, &(10 * USDC), &0);
    assert_eq!(res, Err(Ok(Error::InvalidTier.into())));
}

#[test]
fn set_tier_threshold_rejects_out_of_bounds() {
    let ctx = setup();
    let c = ctx.client();
    assert_eq!(
        c.try_set_tier_threshold(&1, &(600 * USDC), &10_000),
        Err(Ok(Error::ThresholdOutOfBounds.into()))
    );
    assert_eq!(
        c.try_set_tier_threshold(&1, &0, &1_200),
        Err(Ok(Error::ThresholdOutOfBounds.into()))
    );
    assert_eq!(
        c.try_set_tier_threshold(&1, &(2_000_000 * USDC), &1_200),
        Err(Ok(Error::ThresholdOutOfBounds.into()))
    );
}

#[test]
fn set_tier_threshold_rejects_non_monotonic() {
    let ctx = setup();
    let c = ctx.client();
    // Tier 2 dropping below tier 1's bond.
    assert_eq!(
        c.try_set_tier_threshold(&2, &(100 * USDC), &3_500),
        Err(Ok(Error::ThresholdsNotMonotonic.into()))
    );
    // Tier 1 rising to/above tier 2's score.
    assert_eq!(
        c.try_set_tier_threshold(&1, &(600 * USDC), &3_500),
        Err(Ok(Error::ThresholdsNotMonotonic.into()))
    );
}

// ─── Perk getters ─────────────────────────────────────────────────────────

#[test]
fn perk_getters_match_table() {
    let ctx = setup();
    let c = ctx.client();
    assert_eq!(c.get_slash_bps(&0), 1_000);
    assert_eq!(c.get_slash_bps(&2), 800);
    assert_eq!(c.get_slash_bps(&4), 500);
    assert_eq!(c.get_fill_window_bonus_pct(&3), 30);
    assert_eq!(c.get_fee_rebate_bps(&4), 0);
    assert_eq!(
        c.try_get_slash_bps(&5),
        Err(Ok(Error::InvalidTier.into()))
    );
}
