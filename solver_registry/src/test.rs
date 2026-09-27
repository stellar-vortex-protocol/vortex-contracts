#![cfg(test)]

//! Test suite for `solver_registry`.
//!
//! Covers: tier-table seeding, registration/stake/unstake/deregister, the
//! settlement write path (`record_fill` / `record_failure` / `slash`), tier
//! boundary transitions (score exactly on a threshold), tier demotion on
//! slash, the zero-fills edge case, and threshold tuning bounds.

use crate::{Error, SolverRecord, SolverRegistry, SolverRegistryClient, USDC};
use soroban_sdk::{
    testutils::Address as _, token, Address, Env,
};

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
    ctx.client()
        .record_fill(&ctx.admin, &ctx.solver, &(100 * USDC));

    let rec = ctx.client().get_solver(&ctx.solver).unwrap();
    assert_eq!(rec.fills_completed, 1);
    assert_eq!(rec.total_volume, 100 * USDC);
    assert!(ctx.client().get_reputation_score(&ctx.solver).unwrap() > 0);
}

#[test]
fn record_failure_lowers_score() {
    let ctx = setup();
    ctx.register(FLOOR);
    ctx.client().record_fill(&ctx.admin, &ctx.solver, &0);
    let before = ctx.client().get_reputation_score(&ctx.solver).unwrap();
    ctx.client().record_failure(&ctx.admin, &ctx.solver);
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
            .try_record_fill(&stranger, &ctx.solver, &0),
        Err(Ok(Error::WriterNotSet.into()))
    );

    ctx.client().set_writer(&writer);
    assert_eq!(ctx.client().get_writer(), Some(writer.clone()));

    // Writer works…
    ctx.client().record_fill(&writer, &ctx.solver, &(10 * USDC));
    assert_eq!(ctx.client().get_solver(&ctx.solver).unwrap().fills_completed, 1);

    // …a stranger still does not.
    assert_eq!(
        ctx.client()
            .try_record_fill(&stranger, &ctx.solver, &0),
        Err(Ok(Error::Unauthorized.into()))
    );
}

// ─── Tier demotion on slash ────────────────────────────────────────────────

#[test]
fn slash_demotes_tier_and_pays_fee_recipient() {
    let ctx = setup();
    // Bond exactly at the Bronze floor.
    ctx.register(500 * USDC);
    // One clean fill → score ~9_001 → qualifies for Bronze (needs >= 1_000).
    ctx.client().record_fill(&ctx.admin, &ctx.solver, &(100 * USDC));
    assert_eq!(ctx.client().get_tier(&ctx.solver), 1);

    let (slashed, new_tier) = ctx.client().slash(&ctx.admin, &ctx.solver);

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
    ctx.client().record_fill(&ctx.admin, &ctx.solver, &(100 * USDC));
    assert_eq!(ctx.client().get_tier(&ctx.solver), 4);

    // Platinum slash bps = 500 → 5% of 50_000 = 2_500 USDC.
    let (slashed, _new_tier) = ctx.client().slash(&ctx.admin, &ctx.solver);
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
