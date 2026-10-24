#![no_std]

//! Vortex Protocol — Solver Reputation Tier Badge
//!
//! Minimal, non-transferable on-chain record of a solver's reputation tier,
//! implementing the roadmap item tracked by issue #242. See
//! `docs/242-reputation-tier-badge-design.md` for the design rationale
//! (soulbound vs. transferable, bespoke contract vs. SEP-41 token).
//!
//! `mint_badge` / `burn_badge` are driven by `solver_registry`'s
//! tier-computation logic (issue #1, now closed). Both entry points accept a
//! call from **either** the registered `solver_registry` contract **or** the
//! admin, so the admin retains an emergency override during the rollout period.
//! The `solver_registry` address is stored at `initialize` time and can be
//! updated (with admin auth) via `set_solver_registry`.
//!
//! This crate only records a `Tier` and performs no slash math, so it has no
//! duplication of `vortex_common::TIER_SLASH_BPS` / `compute_slash_bps` to
//! migrate (audited for issue #504). Any TTL constants added here later should
//! be imported from `vortex_common` rather than redefined locally.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, Symbol};

#[cfg(test)]
mod test;

#[cfg(test)]
mod bench;

// ─── TTL Constants ────────────────────────────────────────────────────────────
//
// Follows the convention documented in `docs/ttl-constants-rationale.md` and
// mirrored from `backstop_vault`'s TTL helpers.

/// Bump the instance TTL when it drops below this many ledgers.
const INSTANCE_TTL_THRESHOLD: u32 = 17_280; // ~1 day at 5s/ledger
/// Extend the instance TTL to this many ledgers.
const INSTANCE_TTL_EXTEND_TO: u32 = 518_400; // ~30 days at 5s/ledger

/// Bump a persistent entry's TTL when it drops below this many ledgers.
const PERSISTENT_TTL_THRESHOLD: u32 = 17_280; // ~1 day at 5s/ledger
/// Extend a persistent entry's TTL to this many ledgers.
const PERSISTENT_TTL_EXTEND_TO: u32 = 518_400; // ~30 days at 5s/ledger

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum BadgeKey {
    /// Admin address (set in `initialize`).
    Admin,
    /// Address of the `solver_registry` contract authorised to call
    /// `mint_badge` / `burn_badge`. Set at `initialize` and updatable via
    /// `set_solver_registry` (admin-only). Absent until set.
    SolverRegistry,
    /// A solver's current tier badge, if any. Absence means `Unranked`.
    Badge(Address),
}

/// Reputation tiers, matching `docs/solver-registry-design.md` §3.
/// `Unranked` is intentionally not representable here — it is modeled as
/// the *absence* of a `BadgeKey::Badge` entry, so a badge's mere presence
/// is proof of `Bronze`-or-above standing.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Bronze,
    Silver,
    Gold,
    Platinum,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    /// `initialize` called on an already-initialized contract.
    AlreadyInitialized = 1,
    /// Caller is neither the admin nor the registered `solver_registry`.
    Unauthorized = 2,
    /// Contract not initialized (`Admin` key absent).
    NotInitialized = 3,
    /// `burn_badge` called for a solver with no badge on record.
    BadgeNotFound = 4,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct ReputationBadge;

#[contractimpl]
impl ReputationBadge {
    /// Deploy-time setup. Must be called exactly once.
    ///
    /// `solver_registry` is optional at deploy time (pass `None` to defer) —
    /// the admin can wire it later with [`set_solver_registry`]. This keeps
    /// the upgrade path safe: you can deploy the badge contract before the
    /// registry is live and wire them up atomically afterwards.
    pub fn initialize(env: Env, admin: Address, solver_registry: Option<Address>) {
        if env.storage().instance().has(&BadgeKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&BadgeKey::Admin, &admin);
        if let Some(registry) = solver_registry {
            env.storage()
                .instance()
                .set(&BadgeKey::SolverRegistry, &registry);
        }
        Self::bump_instance_ttl(&env);
    }

    /// Admin-only: update (or set for the first time) the `solver_registry`
    /// address that is permitted to call `mint_badge` / `burn_badge`.
    pub fn set_solver_registry(env: Env, solver_registry: Address) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&BadgeKey::SolverRegistry, &solver_registry);
        Self::bump_instance_ttl(&env);
        env.events().publish(
            (Symbol::new(&env, "solver_registry_set"),),
            solver_registry,
        );
    }

    /// Return the registered `solver_registry` address, if any.
    pub fn get_solver_registry(env: Env) -> Option<Address> {
        env.storage().instance().get(&BadgeKey::SolverRegistry)
    }

    /// Mint (or overwrite, on a tier change) `solver`'s badge to `tier`.
    ///
    /// Caller must be **either** the admin **or** the registered
    /// `solver_registry` contract. `solver_registry` calls this whenever its
    /// tier-computation logic detects that a solver's tier has changed (issue
    /// #512). Overwriting in place means a tier upgrade or downgrade never
    /// leaves a stale old-tier badge for a caller to mistakenly read.
    pub fn mint_badge(env: Env, caller: Address, solver: Address, tier: Tier) {
        Self::require_admin_or_registry(&env, &caller);
        let key = BadgeKey::Badge(solver.clone());
        env.storage().persistent().set(&key, &tier);
        Self::bump_persistent_ttl(&env, &key);
        env.events()
            .publish((Symbol::new(&env, "badge_minted"), solver), tier);
    }

    /// Burn `solver`'s badge (used when a solver drops below the `Bronze`
    /// threshold to `Unranked`). Errors if the solver has no badge.
    ///
    /// Caller must be **either** the admin **or** the registered
    /// `solver_registry` contract (issue #512).
    pub fn burn_badge(env: Env, caller: Address, solver: Address) {
        Self::require_admin_or_registry(&env, &caller);
        if !env
            .storage()
            .persistent()
            .has(&BadgeKey::Badge(solver.clone()))
        {
            panic_with_error!(&env, Error::BadgeNotFound);
        }
        env.storage()
            .persistent()
            .remove(&BadgeKey::Badge(solver.clone()));
        env.events()
            .publish((Symbol::new(&env, "badge_burned"), solver), ());
    }

    /// Read-only: `solver`'s current tier badge, or `None` if `Unranked`.
    pub fn get_badge(env: Env, solver: Address) -> Option<Tier> {
        env.storage().persistent().get(&BadgeKey::Badge(solver))
    }

    // ─── Auth helpers ──────────────────────────────────────────────────────

    /// Require that the caller is the admin.
    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&BadgeKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        admin.require_auth();
    }

    /// Require that `caller` is **either** the admin **or** the registered
    /// `solver_registry` contract, and call `require_auth` on whichever
    /// matches. If neither matches, panics with `Unauthorized`.
    fn require_admin_or_registry(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&BadgeKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));

        let registry_opt: Option<Address> = env
            .storage()
            .instance()
            .get(&BadgeKey::SolverRegistry);

        let is_admin = *caller == admin;
        let is_registry = registry_opt
            .as_ref()
            .map(|r| r == caller)
            .unwrap_or(false);

        if !is_admin && !is_registry {
            panic_with_error!(env, Error::Unauthorized);
        }

        // require_auth on the matched identity (Soroban validates the invoker
        // authorization tree against this address).
        caller.require_auth();
    }

    // ─── TTL helpers ──────────────────────────────────────────────────────

    /// Extend the instance storage TTL if it has fallen below the threshold.
    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

    /// Extend a persistent entry's TTL if it has fallen below the threshold.
    fn bump_persistent_ttl(env: &Env, key: &BadgeKey) {
        env.storage().persistent().extend_ttl(
            key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }
}
