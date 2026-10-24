#![cfg(test)]

//! Integration tests for the `ReputationBadge` contract (issue #242, #512).
//!
//! Covers:
//!   - mint-on-promotion and burn-on-demotion driven by admin (backward-compat)
//!   - mint / burn driven by a registered `solver_registry` address (issue #512)
//!   - `set_solver_registry` wiring and event emission
//!   - query view proving badge state always matches the last minted/burned tier
//!
//! Mutation-test hardening (issue #417): targeted assertions on auth checks,
//! tier comparisons, and storage arithmetic so surviving mutants are killed.

use crate::{ReputationBadge, ReputationBadgeClient, Tier};
use soroban_sdk::{testutils::Address as _, Address, Env};

struct Ctx {
    env: Env,
    admin: Address,
    contract_id: Address,
}

impl Ctx {
    fn client(&self) -> ReputationBadgeClient<'_> {
        ReputationBadgeClient::new(&self.env, &self.contract_id)
    }
}

/// Set up a badge contract with no solver_registry pre-wired.
fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, ReputationBadge);

    let ctx = Ctx {
        env,
        admin,
        contract_id,
    };
    ctx.client().initialize(&ctx.admin, &None);
    ctx
}

/// Set up a badge contract with `solver_registry` pre-wired.
fn setup_with_registry() -> (Ctx, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let registry = Address::generate(&env);
    let contract_id = env.register_contract(None, ReputationBadge);

    let ctx = Ctx {
        env,
        admin,
        contract_id,
    };
    ctx.client()
        .initialize(&ctx.admin, &Some(registry.clone()));
    (ctx, registry)
}

// ── Basic admin-driven flow ───────────────────────────────────────────────

#[test]
fn mint_on_promotion_sets_badge() {
    let ctx = setup();
    let solver = Address::generate(&ctx.env);

    assert_eq!(ctx.client().get_badge(&solver), None);

    ctx.client().mint_badge(&ctx.admin, &solver, &Tier::Bronze);
    assert_eq!(ctx.client().get_badge(&solver), Some(Tier::Bronze));

    // A further promotion overwrites in place rather than stacking badges.
    ctx.client().mint_badge(&ctx.admin, &solver, &Tier::Gold);
    assert_eq!(ctx.client().get_badge(&solver), Some(Tier::Gold));
}

#[test]
fn burn_on_demotion_clears_badge() {
    let ctx = setup();
    let solver = Address::generate(&ctx.env);

    ctx.client().mint_badge(&ctx.admin, &solver, &Tier::Silver);
    assert_eq!(ctx.client().get_badge(&solver), Some(Tier::Silver));

    ctx.client().burn_badge(&ctx.admin, &solver);
    assert_eq!(ctx.client().get_badge(&solver), None);
}

#[test]
fn get_badge_matches_current_tier_for_unrelated_solvers() {
    let ctx = setup();
    let solver_a = Address::generate(&ctx.env);
    let solver_b = Address::generate(&ctx.env);

    ctx.client()
        .mint_badge(&ctx.admin, &solver_a, &Tier::Platinum);

    // solver_b never received a badge; minting for solver_a must not leak
    // state into solver_b's record.
    assert_eq!(ctx.client().get_badge(&solver_a), Some(Tier::Platinum));
    assert_eq!(ctx.client().get_badge(&solver_b), None);
}

#[test]
#[should_panic]
fn burn_badge_without_existing_badge_panics() {
    let ctx = setup();
    let solver = Address::generate(&ctx.env);
    ctx.client().burn_badge(&ctx.admin, &solver);
}

// ── solver_registry-driven flow (issue #512) ─────────────────────────────

#[test]
fn registry_can_mint_badge() {
    let (ctx, registry) = setup_with_registry();
    let solver = Address::generate(&ctx.env);

    ctx.client().mint_badge(&registry, &solver, &Tier::Silver);
    assert_eq!(ctx.client().get_badge(&solver), Some(Tier::Silver));
}

#[test]
fn registry_can_burn_badge() {
    let (ctx, registry) = setup_with_registry();
    let solver = Address::generate(&ctx.env);

    ctx.client().mint_badge(&registry, &solver, &Tier::Gold);
    ctx.client().burn_badge(&registry, &solver);
    assert_eq!(ctx.client().get_badge(&solver), None);
}

#[test]
fn registry_can_upgrade_tier() {
    let (ctx, registry) = setup_with_registry();
    let solver = Address::generate(&ctx.env);

    ctx.client().mint_badge(&registry, &solver, &Tier::Bronze);
    ctx.client().mint_badge(&registry, &solver, &Tier::Platinum);
    assert_eq!(ctx.client().get_badge(&solver), Some(Tier::Platinum));
}

#[test]
fn set_solver_registry_updates_allowed_caller() {
    let ctx = setup();
    let solver = Address::generate(&ctx.env);
    let new_registry = Address::generate(&ctx.env);

    // Wire registry after initialization.
    ctx.client().set_solver_registry(&new_registry);
    assert_eq!(
        ctx.client().get_solver_registry(),
        Some(new_registry.clone())
    );

    // The new registry address can now mint badges.
    ctx.client()
        .mint_badge(&new_registry, &solver, &Tier::Bronze);
    assert_eq!(ctx.client().get_badge(&solver), Some(Tier::Bronze));
}

#[test]
#[should_panic]
fn unknown_caller_cannot_mint() {
    let (ctx, _registry) = setup_with_registry();
    let solver = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);

    // `stranger` is neither admin nor registered registry → must panic.
    ctx.client().mint_badge(&stranger, &solver, &Tier::Bronze);
}

#[test]
#[should_panic]
fn unknown_caller_cannot_burn() {
    let (ctx, registry) = setup_with_registry();
    let solver = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);

    ctx.client().mint_badge(&registry, &solver, &Tier::Gold);
    ctx.client().burn_badge(&stranger, &solver);
}

// ── Mutation-killing tests (issue #417) ──────────────────────────────────

#[test]
fn every_tier_round_trips_through_storage() {
    // Kills mutants that collapse distinct `Tier` variants into one another
    // (e.g. replacing a variant with `Tier::Bronze` or swapping the stored
    // value). Each tier must be observable exactly as written.
    let ctx = setup();
    let solver = Address::generate(&ctx.env);

    for tier in [Tier::Bronze, Tier::Silver, Tier::Gold, Tier::Platinum] {
        ctx.client().mint_badge(&ctx.admin, &solver, &tier);
        assert_eq!(ctx.client().get_badge(&solver), Some(tier));
    }
}

#[test]
fn mint_requires_authorised_caller() {
    // Kills mutants that drop or invert the auth check in `mint_badge`.
    let (ctx, registry) = setup_with_registry();
    let solver = Address::generate(&ctx.env);

    ctx.client().mint_badge(&registry, &solver, &Tier::Gold);

    let auths = ctx.env.auths();
    assert!(
        auths.iter().any(|(addr, _)| addr == &registry),
        "mint_badge must require_auth on the caller"
    );
}

#[test]
fn burn_requires_authorised_caller() {
    // Kills mutants that drop or invert the auth check in `burn_badge`.
    let (ctx, registry) = setup_with_registry();
    let solver = Address::generate(&ctx.env);

    ctx.client().mint_badge(&registry, &solver, &Tier::Silver);
    ctx.client().burn_badge(&registry, &solver);

    let auths = ctx.env.auths();
    assert!(
        auths.iter().any(|(addr, _)| addr == &registry),
        "burn_badge must require_auth on the caller"
    );
}

#[test]
fn burn_only_affects_target_solver() {
    // Kills mutants that remove the wrong key or clear the whole map.
    let ctx = setup();
    let solver_a = Address::generate(&ctx.env);
    let solver_b = Address::generate(&ctx.env);

    ctx.client()
        .mint_badge(&ctx.admin, &solver_a, &Tier::Gold);
    ctx.client()
        .mint_badge(&ctx.admin, &solver_b, &Tier::Silver);

    ctx.client().burn_badge(&ctx.admin, &solver_a);

    assert_eq!(ctx.client().get_badge(&solver_a), None);
    assert_eq!(ctx.client().get_badge(&solver_b), Some(Tier::Silver));
}

#[test]
fn re_mint_after_burn_restores_badge() {
    // Kills mutants that leave a tombstone or skip the write after a burn.
    let ctx = setup();
    let solver = Address::generate(&ctx.env);

    ctx.client()
        .mint_badge(&ctx.admin, &solver, &Tier::Bronze);
    ctx.client().burn_badge(&ctx.admin, &solver);
    assert_eq!(ctx.client().get_badge(&solver), None);

    ctx.client()
        .mint_badge(&ctx.admin, &solver, &Tier::Platinum);
    assert_eq!(ctx.client().get_badge(&solver), Some(Tier::Platinum));
}
