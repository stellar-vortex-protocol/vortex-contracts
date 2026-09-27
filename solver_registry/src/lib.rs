#![no_std]

//! Vortex Protocol — Solver Registry (`solver_registry`)
//!
//! Standalone Soroban contract implementing the tiered-staking model from
//! [`docs/solver-registry-design.md`] and the public ABI in
//! [`docs/solver-registry-interface.md`].
//!
//! It follows **Option A** of the design doc (§4): `solver_registry` is the
//! canonical store for a solver's bond and fill history, and derives a
//! reputation score and a bond/score-gated **tier** (0 = Unranked … 4 =
//! Platinum) from that state. `intent_settlement` is expected to *read* the
//! tier and perk views from here (and, once wired up, to feed fill outcomes in
//! via [`SolverRegistry::record_fill`] / [`record_failure`] / [`slash`]).
//! Actually consuming the tier perks inside `intent_settlement`
//! (`accept_intent` / `slash_solver`) is a deliberately separate follow-up —
//! only the read interface is defined here, and it is meant to stay stable.
//!
//! The reputation formula is ported **byte-for-byte** from
//! `intent_settlement::compute_reputation_score`; `score_test_vector` in the
//! test module is the shared cross-check.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, Address, Env,
    Symbol, Vec,
};

#[cfg(test)]
mod test;

// ─── Units & tier defaults ───────────────────────────────────────────────────

/// Smallest unit of the 7-decimal USDC bond token (1 USDC = 10_000_000).
pub const USDC: i128 = 10_000_000;

/// Number of tiers (0..=4).
pub const TIER_COUNT: u32 = 5;

/// Human-readable tier names, indexed by tier level.
pub const TIER_NAMES: [&str; 5] = ["Unranked", "Bronze", "Silver", "Gold", "Platinum"];

/// Default `(min_bond_usdc, min_score_bps)` per tier — the 5-row table from
/// design doc §3. Stored in instance storage at `initialize` and tunable
/// (within bounds) via [`SolverRegistry::set_tier_threshold`].
const DEFAULT_THRESHOLDS: [(i128, u32); 5] = [
    (50, 0),        // 0 Unranked
    (500, 1_000),   // 1 Bronze
    (2_000, 3_500), // 2 Silver
    (10_000, 7_000),// 3 Gold
    (50_000, 9_000),// 4 Platinum
];

/// Fixed (non-tunable) perk: fill-window extension in percent, per tier
/// (design §3 / §6).
const FILL_WINDOW_BONUS_PCT: [u32; 5] = [0, 10, 20, 30, 50];

/// Fixed (non-tunable) perk: slash size in basis points of bond, per tier
/// (design §3 / §7). Tier 0 and 1 pay the full 10%.
const SLASH_BPS: [u32; 5] = [1_000, 1_000, 800, 600, 500];

/// Fixed perk slot: fee rebate in basis points, per tier (design §3 / §8).
/// Left at 0 pending the tokenomics review — the slot is reserved so the ABI
/// does not change when the numbers land.
const FEE_REBATE_BPS: [u32; 5] = [0, 0, 0, 0, 0];

// ─── Tuning bounds for set_tier_threshold ────────────────────────────────────

/// A tunable `min_bond` may not exceed 1,000,000 USDC.
const MAX_TUNABLE_MIN_BOND: i128 = 1_000_000 * USDC;
/// The maximum achievable score is 9,999 bps by construction, so a threshold
/// above that would make a tier unreachable.
const MAX_TUNABLE_MIN_SCORE_BPS: u32 = 9_999;

// ─── TTL bumping (see docs/ttl-constants-rationale.md) ───────────────────────

const DAY_IN_LEDGERS: u32 = 17_280; // ~5s per ledger
const PERSISTENT_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 14;
const PERSISTENT_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 30;
const INSTANCE_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 30;
const INSTANCE_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 60;

// ─── Storage ─────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Instance: admin `Address` (set in `initialize`).
    Admin,
    /// Instance: bond token (`Address`) solvers stake.
    BondToken,
    /// Instance: `Address` that receives slashed bond.
    FeeRecipient,
    /// Instance: the settlement contract authorized to call `record_fill` /
    /// `record_failure` / `slash`. Absent until `set_writer`.
    Writer,
    /// Instance: `Vec<TierThreshold>` of length 5 — the effective tier table.
    Thresholds,
    /// Instance: registered-solver count (`u32`).
    TotalSolvers,
    /// Persistent: per-solver record.
    Solver(Address),
}

/// One row of the tunable part of the tier table.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TierThreshold {
    /// Minimum bond, in the bond token's smallest unit.
    pub min_bond: i128,
    /// Minimum reputation score, in basis points (0..=10_000).
    pub min_score_bps: u32,
}

/// Full tier-table row returned by [`SolverRegistry::get_tier_table`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TierInfo {
    pub tier: u32,
    pub name: Symbol,
    pub min_bond: i128,
    pub min_score_bps: u32,
    pub fill_window_bonus_pct: u32,
    pub slash_bps: u32,
    pub fee_rebate_bps: u32,
}

/// Canonical per-solver state (design §4, Option A).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SolverRecord {
    pub address: Address,
    /// Bond currently staked, in the bond token's smallest unit.
    pub bond_amount: i128,
    pub fills_completed: u32,
    pub fills_failed: u32,
    pub total_volume: i128,
    pub registered_at: u64,
    /// Ledger timestamp of the most recent slash (0 = never).
    pub last_slash_time: u64,
    /// Cumulative bond taken by slashing over this solver's lifetime.
    pub slashed_total: i128,
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    SolverNotRegistered = 4,
    SolverAlreadyRegistered = 5,
    /// Bond would fall below the tier-0 (`Unranked`) floor.
    BondBelowFloor = 6,
    /// A positive amount was required but zero/negative was supplied.
    ZeroAmount = 7,
    /// Unstake amount exceeds the staked bond.
    InsufficientBond = 8,
    /// Tier index outside 0..=4, or tier 0 passed to `set_tier_threshold`.
    InvalidTier = 9,
    /// Threshold value outside its documented bound.
    ThresholdOutOfBounds = 10,
    /// Thresholds must strictly increase from one tier to the next.
    ThresholdsNotMonotonic = 11,
    /// `record_fill` / `record_failure` / `slash` called before `set_writer`
    /// with a caller that is not the admin.
    WriterNotSet = 12,
}

// ─── Reputation formula ──────────────────────────────────────────────────────

/// Reputation score in basis points (0..=10_000).
///
/// **Ported byte-for-byte from `intent_settlement::compute_reputation_score`**
/// — the two implementations must not drift. `score_test_vector` in the test
/// module pins the shared input→output vector referenced by
/// `docs/solver-registry-design.md` §2.
///
/// ```text
/// total  = fills_completed + fills_failed          (0 → score 0)
/// base   = fills_completed * 10_000 / total        success rate, 0..10_000 bps
/// decay  = VOLUME_SCALE * 10_000 / (VOLUME_SCALE + vol + 1)
/// mult   = 10_000 - decay / 10                     9_000..10_000 bps
/// score  = base * mult / 10_000
/// ```
fn score_of(record: &SolverRecord) -> u32 {
    let total_fills = record.fills_completed as u64 + record.fills_failed as u64;
    if total_fills == 0 {
        return 0;
    }

    let base_bps = (record.fills_completed as u64 * 10_000) / total_fills;

    const VOLUME_SCALE: i128 = 1_000 * 100 * 10_000_000;

    let vol = record.total_volume.max(0);
    let decay_bps = ((VOLUME_SCALE as u64) * 10_000) / ((VOLUME_SCALE + vol + 1) as u64);

    let multiplier_bps = 10_000u64 - decay_bps / 10;

    let score = base_bps * multiplier_bps / 10_000;
    score as u32
}

// ─── Contract ────────────────────────────────────────────────────────────────

#[contract]
pub struct SolverRegistry;

#[contractimpl]
impl SolverRegistry {
    // ── Initialization ───────────────────────────────────────────────────────

    /// One-time setup. Seeds the tier table from [`DEFAULT_THRESHOLDS`]
    /// (converting the USDC figures to the token's smallest unit).
    pub fn initialize(env: Env, admin: Address, bond_token: Address, fee_recipient: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::BondToken, &bond_token);
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &fee_recipient);
        env.storage().instance().set(&DataKey::TotalSolvers, &0u32);

        let mut rows: Vec<TierThreshold> = Vec::new(&env);
        for (min_bond_usdc, min_score_bps) in DEFAULT_THRESHOLDS.iter() {
            rows.push_back(TierThreshold {
                min_bond: *min_bond_usdc * USDC,
                min_score_bps: *min_score_bps,
            });
        }
        env.storage().instance().set(&DataKey::Thresholds, &rows);
        Self::bump_instance_ttl(&env);
    }

    // ── Admin ────────────────────────────────────────────────────────────────

    /// Admin-only: set (or rotate) the settlement contract permitted to call
    /// `record_fill` / `record_failure` / `slash`.
    pub fn set_writer(env: Env, writer: Address) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Writer, &writer);
        Self::bump_instance_ttl(&env);
        env.events()
            .publish((Symbol::new(&env, "writer_set"),), writer);
    }

    /// The configured settlement writer, if any.
    pub fn get_writer(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Writer)
    }

    /// Admin-only: tune one tier's `min_bond` / `min_score_bps`.
    ///
    /// Bounds (any violation → `ThresholdOutOfBounds` / `ThresholdsNotMonotonic`
    /// / `InvalidTier`):
    /// * `tier` ∈ 1..=4 (tier 0 is the fixed entry floor).
    /// * `min_bond` ∈ [1, [`MAX_TUNABLE_MIN_BOND`]].
    /// * `min_score_bps` ≤ [`MAX_TUNABLE_MIN_SCORE_BPS`].
    /// * Row must stay strictly greater than tier `tier-1` and strictly less
    ///   than tier `tier+1` on **both** axes.
    pub fn set_tier_threshold(env: Env, tier: u32, min_bond: i128, min_score_bps: u32) {
        Self::require_admin(&env);

        if tier == 0 || tier >= TIER_COUNT {
            panic_with_error!(&env, Error::InvalidTier);
        }
        if min_bond < 1 || min_bond > MAX_TUNABLE_MIN_BOND {
            panic_with_error!(&env, Error::ThresholdOutOfBounds);
        }
        if min_score_bps > MAX_TUNABLE_MIN_SCORE_BPS {
            panic_with_error!(&env, Error::ThresholdOutOfBounds);
        }

        let mut rows: Vec<TierThreshold> = env
            .storage()
            .instance()
            .get(&DataKey::Thresholds)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));

        let below = rows.get(tier - 1).unwrap();
        if min_bond <= below.min_bond || min_score_bps <= below.min_score_bps {
            panic_with_error!(&env, Error::ThresholdsNotMonotonic);
        }
        if tier + 1 < TIER_COUNT {
            let above = rows.get(tier + 1).unwrap();
            if min_bond >= above.min_bond || min_score_bps >= above.min_score_bps {
                panic_with_error!(&env, Error::ThresholdsNotMonotonic);
            }
        }

        rows.set(
            tier,
            TierThreshold {
                min_bond,
                min_score_bps,
            },
        );
        env.storage().instance().set(&DataKey::Thresholds, &rows);
        Self::bump_instance_ttl(&env);

        env.events().publish(
            (Symbol::new(&env, "tier_threshold_set"),),
            (tier, min_bond, min_score_bps),
        );
    }

    // ── Solver self-service ──────────────────────────────────────────────────

    /// Register as a solver by staking `bond_amount` (must clear the tier-0
    /// floor). The bond token is pulled from `solver`.
    pub fn register_solver(env: Env, solver: Address, bond_amount: i128) {
        solver.require_auth();

        if bond_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Solver(solver.clone()))
        {
            panic_with_error!(&env, Error::SolverAlreadyRegistered);
        }
        if bond_amount < Self::tier0_floor(&env) {
            panic_with_error!(&env, Error::BondBelowFloor);
        }

        let record = SolverRecord {
            address: solver.clone(),
            bond_amount,
            fills_completed: 0,
            fills_failed: 0,
            total_volume: 0,
            registered_at: env.ledger().timestamp(),
            last_slash_time: 0,
            slashed_total: 0,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        let total: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSolvers)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSolvers, &(total + 1));

        Self::bond_client(&env).transfer(
            &solver,
            &env.current_contract_address(),
            &bond_amount,
        );

        env.events()
            .publish((Symbol::new(&env, "solver_registered"), solver), bond_amount);
    }

    /// Add `amount` to an existing solver's bond.
    pub fn stake(env: Env, solver: Address, amount: i128) {
        solver.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        let mut record = Self::load_solver(&env, &solver);
        record.bond_amount += amount;
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        Self::bond_client(&env).transfer(
            &solver,
            &env.current_contract_address(),
            &amount,
        );
        env.events().publish(
            (Symbol::new(&env, "staked"), solver),
            (amount, record.bond_amount),
        );
    }

    /// Withdraw `amount` of bond. The remaining balance must still clear the
    /// tier-0 floor — use [`SolverRegistry::deregister_solver`] to exit fully.
    pub fn unstake(env: Env, solver: Address, amount: i128) {
        solver.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        let mut record = Self::load_solver(&env, &solver);
        if amount > record.bond_amount {
            panic_with_error!(&env, Error::InsufficientBond);
        }
        let remaining = record.bond_amount - amount;
        if remaining < Self::tier0_floor(&env) {
            panic_with_error!(&env, Error::BondBelowFloor);
        }
        record.bond_amount = remaining;
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        Self::bond_client(&env).transfer(
            &env.current_contract_address(),
            &solver,
            &amount,
        );
        env.events()
            .publish((Symbol::new(&env, "unstaked"), solver), (amount, remaining));
    }

    /// Exit the registry: returns the full remaining bond and deletes the
    /// record.
    pub fn deregister_solver(env: Env, solver: Address) {
        solver.require_auth();
        let record = Self::load_solver(&env, &solver);

        env.storage()
            .persistent()
            .remove(&DataKey::Solver(solver.clone()));
        let total: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSolvers)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSolvers, &total.saturating_sub(1));

        if record.bond_amount > 0 {
            Self::bond_client(&env).transfer(
                &env.current_contract_address(),
                &solver,
                &record.bond_amount,
            );
        }
        env.events().publish(
            (Symbol::new(&env, "solver_deregistered"), solver),
            record.bond_amount,
        );
    }

    // ── Settlement write path (writer or admin) ─────────────────────────────

    /// Record a successful fill of `amount` (dst-token units) by `solver`.
    /// `caller` must be the configured writer or the admin.
    pub fn record_fill(env: Env, caller: Address, solver: Address, amount: i128) {
        Self::require_writer_or_admin(&env, &caller);
        if amount < 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        let mut record = Self::load_solver(&env, &solver);
        record.fills_completed += 1;
        record.total_volume += amount;
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);
        env.events().publish(
            (Symbol::new(&env, "fill_recorded"), solver),
            (amount, score_of(&record)),
        );
    }

    /// Record a failed fill by `solver` (no slash — that is `slash`).
    /// `caller` must be the configured writer or the admin.
    pub fn record_failure(env: Env, caller: Address, solver: Address) {
        Self::require_writer_or_admin(&env, &caller);
        let mut record = Self::load_solver(&env, &solver);
        record.fills_failed += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);
        env.events().publish(
            (Symbol::new(&env, "failure_recorded"), solver),
            score_of(&record),
        );
    }

    /// Slash `solver`'s bond by the tier-dependent [`SLASH_BPS`] (minimum 1
    /// unit), transfer it to the fee recipient, and record a failed fill.
    ///
    /// Returns `(slash_amount, new_tier)`. `caller` must be the configured
    /// writer or the admin.
    pub fn slash(env: Env, caller: Address, solver: Address) -> (i128, u32) {
        Self::require_writer_or_admin(&env, &caller);
        let mut record = Self::load_solver(&env, &solver);

        let tier_before = Self::tier_of(&env, &record);
        let bps = SLASH_BPS[tier_before as usize] as i128;
        let slash_amount = if record.bond_amount > 0 {
            (record.bond_amount * bps / 10_000).max(1)
        } else {
            0
        };

        record.bond_amount -= slash_amount;
        record.fills_failed += 1;
        record.slashed_total += slash_amount;
        record.last_slash_time = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        if slash_amount > 0 {
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
            Self::bond_client(&env).transfer(
                &env.current_contract_address(),
                &fee_recipient,
                &slash_amount,
            );
        }

        let new_tier = Self::tier_of(&env, &record);
        env.events().publish(
            (Symbol::new(&env, "solver_slashed"), solver),
            (slash_amount, new_tier),
        );
        (slash_amount, new_tier)
    }

    // ── Views ───────────────────────────────────────────────────────────────

    /// Current tier (0..=4) for `solver`. Unknown solver → 0.
    pub fn get_tier(env: Env, solver: Address) -> u32 {
        match env
            .storage()
            .persistent()
            .get::<_, SolverRecord>(&DataKey::Solver(solver))
        {
            Some(record) => Self::tier_of(&env, &record),
            None => 0,
        }
    }

    /// Pure tier lookup for an arbitrary `(score_bps, bond_amount)` pair — lets
    /// off-chain solver bots price perks without a stored record.
    pub fn tier_for(env: Env, score_bps: u32, bond_amount: i128) -> u32 {
        let rows = Self::thresholds(&env);
        let mut tier = 0u32;
        let mut i = 0u32;
        while i < TIER_COUNT {
            let row = rows.get(i).unwrap();
            if bond_amount >= row.min_bond && score_bps >= row.min_score_bps {
                tier = i;
            }
            i += 1;
        }
        tier
    }

    /// Reputation score (0..=10_000 bps) for `solver`, or `None` if unknown.
    pub fn get_reputation_score(env: Env, solver: Address) -> Option<u32> {
        let record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver))?;
        Some(score_of(&record))
    }

    /// Full per-solver record, or `None`.
    pub fn get_solver(env: Env, solver: Address) -> Option<SolverRecord> {
        env.storage().persistent().get(&DataKey::Solver(solver))
    }

    /// Count of currently registered solvers.
    pub fn get_solver_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSolvers)
            .unwrap_or(0)
    }

    /// The effective 5-row tier table (tunable thresholds + fixed perks).
    pub fn get_tier_table(env: Env) -> Vec<TierInfo> {
        let rows = Self::thresholds(&env);
        let mut out: Vec<TierInfo> = Vec::new(&env);
        let mut i = 0u32;
        while i < TIER_COUNT {
            let row = rows.get(i).unwrap();
            out.push_back(TierInfo {
                tier: i,
                name: Symbol::new(&env, TIER_NAMES[i as usize]),
                min_bond: row.min_bond,
                min_score_bps: row.min_score_bps,
                fill_window_bonus_pct: FILL_WINDOW_BONUS_PCT[i as usize],
                slash_bps: SLASH_BPS[i as usize],
                fee_rebate_bps: FEE_REBATE_BPS[i as usize],
            });
            i += 1;
        }
        out
    }

    /// Fixed perk: fill-window extension percent for `tier`.
    pub fn get_fill_window_bonus_pct(env: Env, tier: u32) -> u32 {
        Self::perk(&env, tier, &FILL_WINDOW_BONUS_PCT)
    }

    /// Fixed perk: slash size in bps of bond for `tier`.
    pub fn get_slash_bps(env: Env, tier: u32) -> u32 {
        Self::perk(&env, tier, &SLASH_BPS)
    }

    /// Reserved perk slot: fee rebate in bps for `tier` (currently 0).
    pub fn get_fee_rebate_bps(env: Env, tier: u32) -> u32 {
        Self::perk(&env, tier, &FEE_REBATE_BPS)
    }

    /// Admin address, or `None` before initialization.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Admin)
    }

    /// Bond token address, or `None` before initialization.
    pub fn get_bond_token(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::BondToken)
    }

    // ── Reputation formula ─────────────────────────────────────────────────

    /// Reputation score (0..=10_000 bps) for a solver record. Callable view;
    /// see [`score_of`] for the implementation the rest of the contract uses.
    pub fn compute_reputation_score(record: SolverRecord) -> u32 {
        score_of(&record)
    }

    // ── Internal ──────────────────────────────────────────────────────────

    fn tier_of(env: &Env, record: &SolverRecord) -> u32 {
        let score = score_of(record);
        Self::tier_for(env.clone(), score, record.bond_amount)
    }

    fn thresholds(env: &Env) -> Vec<TierThreshold> {
        env.storage()
            .instance()
            .get(&DataKey::Thresholds)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized))
    }

    fn tier0_floor(env: &Env) -> i128 {
        Self::thresholds(env).get(0).unwrap().min_bond
    }

    fn perk(env: &Env, tier: u32, table: &[u32; 5]) -> u32 {
        if tier >= TIER_COUNT {
            panic_with_error!(env, Error::InvalidTier);
        }
        table[tier as usize]
    }

    fn load_solver(env: &Env, solver: &Address) -> SolverRecord {
        env.storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap_or_else(|| panic_with_error!(env, Error::SolverNotRegistered))
    }

    fn bond_client<'a>(env: &Env) -> token::Client<'a> {
        let bond_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::BondToken)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        token::Client::new(env, &bond_token)
    }

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        admin.require_auth();
    }

    fn require_writer_or_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        let writer: Option<Address> = env.storage().instance().get(&DataKey::Writer);
        match writer {
            Some(w) => {
                if *caller != w && *caller != admin {
                    panic_with_error!(env, Error::Unauthorized);
                }
            }
            None => {
                if *caller != admin {
                    // No writer configured yet: only the admin may drive the
                    // write path.
                    panic_with_error!(env, Error::WriterNotSet);
                }
            }
        }
        caller.require_auth();
    }

    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

    fn bump_solver_ttl(env: &Env, solver: &Address) {
        env.storage().persistent().extend_ttl(
            &DataKey::Solver(solver.clone()),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }
}
