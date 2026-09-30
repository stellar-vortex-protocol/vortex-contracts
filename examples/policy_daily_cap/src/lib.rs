//! Vortex Protocol — Reference Policy Contract: Per-User Daily Cap (issue #378)
//!
//! A reference implementation of the `PolicyContract` interface that
//! `intent_settlement` calls when a policy is configured. Demonstrates the
//! minimal ABI required:
//!
//!   `check_intent(user: Address, dst_token: Address, amount: i128) -> bool`
//!
//! ## What it does
//!
//! Tracks each user's cumulative `amount` over a rolling 24-hour window
//! (measured in ledger timestamps). Returns `false` — causing the settlement
//! contract to reject the submission with `PolicyRejected` — when the user's
//! daily total would exceed the configured cap.
//!
//! ## Fail-closed contract
//!
//! The settlement contract calls this via `invoke_contract`. If this contract
//! panics for any reason (storage error, arithmetic overflow, etc.), the
//! `invoke_contract` call propagates the trap and the settlement call reverts.
//! This is intentional: a broken policy is safer than a bypassed one.
//!
//! ## Upgrade path
//!
//! Because the settlement contract calls this via a raw symbol invocation, the
//! policy can be upgraded or replaced (via `propose_set_policy` /
//! `execute_set_policy`) without changing the settlement contract. The only
//! required invariant is that the policy exports `check_intent` with the exact
//! signature above.
//!
//! ## Use cases
//!
//! * Regulated on-ramps: per-user KYC volume caps
//! * Risk management: circuit-breaker limits per user or global
//! * Testing: easy to swap in a permissive or strict policy for tests

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, Symbol,
};

// ─── Constants ─────────────────────────────────────────────────────────────────

/// 24-hour window in seconds for the rolling cap.
const DAY_SECS: u64 = 86_400;

// TTL: user volume records live for 2 days so the rolling window doesn't
// falsely carry forward stale data after a long gap.
const DAY_IN_LEDGERS: u32 = 17_280; // ~5s per ledger
const RECORD_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 2;
const RECORD_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 3;

// ─── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    /// Per-user daily cap in `dst_token` stroop-equivalent units.
    DailyCap,
    /// Per-user rolling volume: `(window_start: u64, cumulative: i128)`.
    UserVolume(Address),
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ZeroAmount = 4,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct PolicyDailyCap;

#[contractimpl]
impl PolicyDailyCap {
    // ── Initialization ────────────────────────────────────────────────────────

    /// One-time setup: record `admin` and the per-user `daily_cap`.
    ///
    /// `daily_cap` is expressed in the same unit as `amount` passed to
    /// `check_intent` — typically dst_token stroops (7-decimal USDC units).
    pub fn initialize(env: Env, admin: Address, daily_cap: i128) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        if daily_cap <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::DailyCap, &daily_cap);
        env.events().publish(
            (Symbol::new(&env, "policy_initialized"),),
            (admin, daily_cap),
        );
    }

    /// Admin-only: update the per-user daily cap.
    pub fn set_daily_cap(env: Env, daily_cap: i128) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        admin.require_auth();
        if daily_cap <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        env.storage().instance().set(&DataKey::DailyCap, &daily_cap);
        env.events()
            .publish((Symbol::new(&env, "cap_updated"),), daily_cap);
    }

    /// Return the configured daily cap.
    pub fn get_daily_cap(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::DailyCap)
            .unwrap_or(0)
    }

    // ── Policy interface (called by intent_settlement) ────────────────────────

    /// Check whether `user` may submit an intent for `amount` units of
    /// `dst_token`. Returns `true` (allow) if the user's rolling 24-hour
    /// cumulative volume plus `amount` does not exceed `daily_cap`.
    ///
    /// Note: `dst_token` is available for policies that cap per-token rather
    /// than in aggregate; this implementation ignores it and applies the cap
    /// across all dst tokens.
    ///
    /// This function is **intentionally infallible** (no `panic_with_error!`
    /// on the happy path) — only hard configuration errors (uninitialized
    /// contract) can cause a trap, which is the desired fail-closed behaviour.
    pub fn check_intent(env: Env, user: Address, _dst_token: Address, amount: i128) -> bool {
        let daily_cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::DailyCap)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));

        if amount <= 0 {
            // Negative or zero amounts are rejected by settlement before
            // reaching here, but be defensive.
            return false;
        }

        let now = env.ledger().timestamp();
        let key = DataKey::UserVolume(user.clone());

        // Load the user's current rolling window record.
        let (window_start, cumulative): (u64, i128) = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or((now, 0i128));

        // If the window has expired, start a fresh one.
        let (effective_start, effective_cumulative) = if now >= window_start + DAY_SECS {
            (now, 0i128)
        } else {
            (window_start, cumulative)
        };

        let new_total = effective_cumulative.saturating_add(amount);
        if new_total > daily_cap {
            return false;
        }

        // Update the rolling window record.
        env.storage()
            .persistent()
            .set(&key, &(effective_start, new_total));
        env.storage().persistent().extend_ttl(
            &key,
            RECORD_TTL_THRESHOLD,
            RECORD_TTL_EXTEND_TO,
        );

        true
    }

    /// Read a user's current rolling-window volume and window start.
    /// Returns `(window_start, cumulative)`.
    pub fn get_user_volume(env: Env, user: Address) -> (u64, i128) {
        let now = env.ledger().timestamp();
        let key = DataKey::UserVolume(user);
        let (window_start, cumulative): (u64, i128) = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or((now, 0i128));

        // If the window has expired return a fresh (zeroed) record.
        if now >= window_start + DAY_SECS {
            (now, 0)
        } else {
            (window_start, cumulative)
        }
    }
}
