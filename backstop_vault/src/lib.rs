#![no_std]

//! Vortex Protocol — Backstop LP Vault (`backstop_vault`) — Issue #370
//!
//! A standalone Soroban contract where liquidity providers deposit the bond
//! token (USDC) in exchange for vault shares. The vault:
//!
//! - Absorbs user-compensation claims via `cover()` (callable only by the
//!   settlement contract address set at initialization).
//! - Earns a configurable share of protocol fees and slash proceeds routed
//!   by `intent_settlement` via `receive_income()`.
//! - Issues pro-rata shares that appreciate as the vault earns income and
//!   depreciate as it pays out claims (loss socialization across all LPs).
//! - Enforces a deposit-triggered cooldown to prevent withdrawal front-running
//!   ahead of a known fee inflow.
//! - Supports an emergency withdrawal-queue mode when the vault cannot service
//!   all withdrawals immediately (bank-run protection).
//!
//! ## Share accounting (inflation-attack protection)
//!
//! The vault is seeded with `VIRTUAL_SHARES` / `VIRTUAL_ASSETS` at
//! initialization so the initial exchange rate starts at 1:1 and a dust
//! first-deposit cannot manipulate the rate to an extreme.
//!
//! ```text
//! shares_minted  = deposit_amount  * total_shares / total_assets
//! assets_out     = shares_redeemed * total_assets / total_shares
//! ```

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token,
    Address, BytesN, Env, Symbol,
};
use vortex_common::{
    require_admin, CommonError, INSTANCE_TTL_EXTEND_TO, INSTANCE_TTL_THRESHOLD,
    PERSISTENT_TTL_EXTEND_TO, PERSISTENT_TTL_THRESHOLD,
};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Virtual shares/assets seeded at initialization to prevent inflation attacks.
const VIRTUAL_SHARES: i128 = 1_000_000_000; // 100 USDC-equivalent at 1:1
const VIRTUAL_ASSETS: i128 = 1_000_000_000;

/// Default minimum deposit (1 USDC in 7-decimal units). Used to seed the
/// admin-configurable config in `initialize` and as a fallback if unset.
const MIN_DEPOSIT: i128 = 10_000_000;

/// Default withdrawal cooldown in seconds. An LP who just deposited must wait
/// this long before they can withdraw, preventing deposit front-running ahead
/// of a known incoming fee or slash payment. Used to seed the config and as a
/// fallback if unset.
const WITHDRAWAL_COOLDOWN_SECS: u64 = 172_800; // 48 hours

/// Lower bound for the configurable minimum deposit (1 USDC in 7-decimal units).
const MIN_DEPOSIT_FLOOR: i128 = 10_000_000;
/// Upper bound for the configurable minimum deposit (1,000,000 USDC).
const MIN_DEPOSIT_CEILING: i128 = 10_000_000_000_000;

/// Lower bound for the configurable withdrawal cooldown (1 hour).
const WITHDRAWAL_COOLDOWN_SECS_MIN: u64 = 3_600;
/// Upper bound for the configurable withdrawal cooldown (30 days).
const WITHDRAWAL_COOLDOWN_SECS_MAX: u64 = 2_592_000;

/// A top-up only (re)starts the withdrawal cooldown when it is at least this
/// fraction (in basis points) of the LP's existing position value. Smaller
/// top-ups leave an already-eligible balance's cooldown untouched. See the
/// comment in `deposit` for the rationale and tradeoff.
const COOLDOWN_RESET_FRACTION_BPS: i128 = 1_000; // 10%

// TTL constants (`PERSISTENT_TTL_THRESHOLD`, `PERSISTENT_TTL_EXTEND_TO`,
// `INSTANCE_TTL_THRESHOLD`, `INSTANCE_TTL_EXTEND_TO`) are imported from
// `vortex_common` so every contract shares one definition.

// ─── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address (set in `initialize`).
    Admin,
    /// Bond token address (USDC or equivalent).
    BondToken,
    /// Settlement contract authorized to call `cover` and `receive_income`.
    Settlement,
    /// Total shares issued (i128).
    TotalShares,
    /// Total assets held by the vault (i128).
    TotalAssets,
    /// Per-LP share balance (i128).
    Shares(Address),
    /// Per-LP withdrawal cooldown: earliest allowed withdrawal timestamp (u64).
    WithdrawCooldown(Address),
    /// Emergency mode flag (bool). When `true`, withdrawals queue instead of
    /// executing immediately.
    EmergencyMode,
    /// Per-LP queued withdrawal amount (i128).
    WithdrawQueue(Address),
    /// Total queued withdrawal amount (i128).
    TotalQueued,
    /// Cumulative cover paid out (i128) — informational.
    TotalCoverPaid,
    /// Cumulative income received (fees + slashes) (i128).
    TotalIncome,
    /// Admin-configurable vault configuration (`BackstopConfig`).
    Config,
}

// ─── Config ───────────────────────────────────────────────────────────────────

/// Admin-configurable vault parameters (issue #543, following the
/// `ProtocolConfig`/`set_config` precedent from `intent_settlement` #52).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackstopConfig {
    /// Minimum deposit amount (in bond-token units).
    pub min_deposit: i128,
    /// Withdrawal cooldown in seconds applied after a deposit.
    pub withdrawal_cooldown_secs: u64,
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
    BelowMinDeposit = 5,
    CooldownNotExpired = 6,
    InsufficientShares = 7,
    VaultInsolvent = 8,
    EmergencyModeActive = 9,
    EmergencyModeNotActive = 10,
    NothingQueued = 11,
    InvalidConfig = 12,
    /// Checked arithmetic overflowed in a share/asset conversion. We fail
    /// closed rather than silently substituting a fallback value.
    ArithmeticOverflow = 13,
    /// `rescue_tokens` was asked to move the vault's own bond token.
    ProtectedToken = 14,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct BackstopVault;

#[contractimpl]
impl BackstopVault {
    // ── Initialization ────────────────────────────────────────────────────────

    /// One-time setup. Records the admin, bond_token, and authorized settlement
    /// contract. Seeds virtual shares/assets for inflation-attack resistance.
    pub fn initialize(
        env: Env,
        admin: Address,
        bond_token: Address,
        settlement: Address,
    ) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::BondToken, &bond_token);
        env.storage().instance().set(&DataKey::Settlement, &settlement);
        // Seed virtual entries.
        env.storage().instance().set(&DataKey::TotalShares, &VIRTUAL_SHARES);
        env.storage().instance().set(&DataKey::TotalAssets, &VIRTUAL_ASSETS);
        env.storage().instance().set(&DataKey::TotalCoverPaid, &0i128);
        env.storage().instance().set(&DataKey::TotalIncome, &0i128);
        env.storage().instance().set(&DataKey::TotalQueued, &0i128);
        env.storage().instance().set(&DataKey::EmergencyMode, &false);
        // Seed the admin-configurable config with the default constants so
        // behavior is unchanged until an admin updates it.
        env.storage().instance().set(
            &DataKey::Config,
            &BackstopConfig {
                min_deposit: MIN_DEPOSIT,
                withdrawal_cooldown_secs: WITHDRAWAL_COOLDOWN_SECS,
            },
        );
        Self::bump_instance_ttl(&env);
    }

    // ── LP operations ─────────────────────────────────────────────────────────

    /// Deposit `amount` of bond_token and receive vault shares.
    ///
    /// Shares are minted proportional to the current assets:shares ratio.
    /// Starts a new withdrawal cooldown on the depositing LP's account to
    /// prevent deposit front-running ahead of incoming income.
    ///
    /// Returns the number of shares minted.
    pub fn deposit(env: Env, lp: Address, amount: i128) -> i128 {
        lp.require_auth();
        Self::bump_instance_ttl(&env);

        let config = Self::load_config(&env);
        if amount < config.min_deposit {
            panic_with_error!(&env, Error::BelowMinDeposit);
        }

        let total_assets = Self::load_total_assets(&env);
        let total_shares = Self::load_total_shares(&env);

        // shares_minted = amount * total_shares / total_assets
        // Virtual entries guarantee total_assets > 0 always. Any overflow in
        // the intermediate product or a zero divisor fails closed with a
        // dedicated error instead of silently substituting a wrong value.
        let shares_minted = amount
            .checked_mul(total_shares)
            .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow))
            .checked_div(total_assets)
            .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow))
            .max(1);

        // Transfer bond_token from LP to vault.
        let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
        token::Client::new(&env, &bond_token)
            .transfer(&lp, &env.current_contract_address(), &amount);

        // Update state.
        let lp_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(lp.clone()))
            .unwrap_or(0);
        let new_lp_shares = lp_shares
            .checked_add(shares_minted)
            .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow));
        let new_total_shares = total_shares
            .checked_add(shares_minted)
            .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow));
        let new_total_assets = total_assets
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow));
        env.storage()
            .persistent()
            .set(&DataKey::Shares(lp.clone()), &new_lp_shares);
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &new_total_shares);
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &new_total_assets);
        Self::bump_persistent_ttl(&env, &DataKey::Shares(lp.clone()));

        // Cooldown scoping (Issue #485).
        //
        // The cooldown exists to stop deposit front-running of a known
        // incoming fee/slash payment. Applying a full 48h reset to an LP's
        // *entire* pre-existing balance on any top-up is disproportionate and
        // discourages topping up. We therefore only (re)start the cooldown
        // when the deposit is "significant" relative to the LP's existing
        // position:
        //
        //   - an LP with no existing position always starts a cooldown;
        //   - otherwise the cooldown is reset only when the new deposit is at
        //     least `COOLDOWN_RESET_FRACTION_BPS` (10%) of the LP's existing
        //     position value (measured in assets, using the pre-deposit
        //     exchange rate). Smaller top-ups leave the existing cooldown
        //     untouched, so an already-eligible balance stays eligible.
        //
        // Tradeoff: we deliberately avoid a full per-tranche ledger (which
        // would be disproportionate complexity for this goal). The cost is
        // that a dust top-up inherits the existing position's cooldown state
        // rather than getting its own fresh window; the front-running window
        // for the new increment is bounded by the existing position's
        // cooldown, which is acceptable given the small size of such top-ups.
        let existing_position_assets = if total_shares > 0 {
            lp_shares
                .checked_mul(total_assets)
                .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow))
                .checked_div(total_shares)
                .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow))
        } else {
            0
        };
        let significant_deposit = existing_position_assets <= 0
            || amount
                .checked_mul(10_000)
                .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow))
                >= existing_position_assets
                    .checked_mul(COOLDOWN_RESET_FRACTION_BPS)
                    .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow));

        if significant_deposit {
            let cooldown_until = env.ledger().timestamp() + config.withdrawal_cooldown_secs;
            env.storage()
                .persistent()
                .set(&DataKey::WithdrawCooldown(lp.clone()), &cooldown_until);
            Self::bump_persistent_ttl(&env, &DataKey::WithdrawCooldown(lp.clone()));
        }

        env.events().publish(
            (Symbol::new(&env, "deposit"), lp),
            (amount, shares_minted),
        );

        shares_minted
    }

    /// Redeem `shares` for bond_token assets. Subject to withdrawal cooldown.
    ///
    /// In emergency mode the withdrawal is queued and shares are burned
    /// immediately; call `process_queued_withdrawal` once funds are available.
    ///
    /// Returns the number of assets redeemed (or queued in emergency mode).
    pub fn withdraw(env: Env, lp: Address, shares: i128) -> i128 {
        lp.require_auth();
        Self::bump_instance_ttl(&env);

        if shares <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let lp_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(lp.clone()))
            .unwrap_or(0);
        if lp_shares < shares {
            panic_with_error!(&env, Error::InsufficientShares);
        }

        // Cooldown guard.
        let now = env.ledger().timestamp();
        let cooldown_until: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::WithdrawCooldown(lp.clone()))
            .unwrap_or(0);
        if now < cooldown_until {
            panic_with_error!(&env, Error::CooldownNotExpired);
        }

        let total_assets = Self::load_total_assets(&env);
        let total_shares = Self::load_total_shares(&env);

        // assets_out = shares * total_assets / total_shares
        // Fail closed on overflow (or a zero divisor) rather than silently
        // substituting a wrong value.
        let assets_out = shares
            .checked_mul(total_assets)
            .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow))
            .checked_div(total_shares)
            .unwrap_or_else(|| panic_with_error!(&env, Error::ArithmeticOverflow))
            .max(0);

        // Emergency mode: queue the withdrawal, burn shares immediately.
        let emergency: bool = env
            .storage()
            .instance()
            .get(&DataKey::EmergencyMode)
            .unwrap_or(false);
        if emergency {
            let queued: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::WithdrawQueue(lp.clone()))
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey::WithdrawQueue(lp.clone()), &(queued + assets_out));
            let total_queued: i128 = env
                .storage()
                .instance()
                .get(&DataKey::TotalQueued)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::TotalQueued, &(total_queued + assets_out));
            // Burn shares immediately.
            env.storage()
                .persistent()
                .set(&DataKey::Shares(lp.clone()), &(lp_shares - shares));
            env.storage()
                .instance()
                .set(&DataKey::TotalShares, &(total_shares - shares));
            Self::bump_persistent_ttl(&env, &DataKey::Shares(lp.clone()));
            Self::bump_persistent_ttl(&env, &DataKey::WithdrawQueue(lp.clone()));
            env.events().publish(
                (Symbol::new(&env, "withdraw_queued"), lp),
                (shares, assets_out),
            );
            return assets_out;
        }

        if assets_out > total_assets {
            panic_with_error!(&env, Error::VaultInsolvent);
        }

        // Normal path: burn shares, update totals, transfer assets.
        env.storage()
            .persistent()
            .set(&DataKey::Shares(lp.clone()), &(lp_shares - shares));
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &(total_shares - shares));
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &(total_assets - assets_out));
        Self::bump_persistent_ttl(&env, &DataKey::Shares(lp.clone()));

        let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
        token::Client::new(&env, &bond_token)
            .transfer(&env.current_contract_address(), &lp, &assets_out);

        env.events().publish(
            (Symbol::new(&env, "withdraw"), lp),
            (shares, assets_out),
        );

        assets_out
    }

    /// Process a queued withdrawal for `lp` (emergency mode only).
    /// Pays out up to the queued amount from available assets.
    pub fn process_queued_withdrawal(env: Env, lp: Address) {
        Self::bump_instance_ttl(&env);
        let emergency: bool = env
            .storage()
            .instance()
            .get(&DataKey::EmergencyMode)
            .unwrap_or(false);
        if !emergency {
            panic_with_error!(&env, Error::EmergencyModeNotActive);
        }
        let queued: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WithdrawQueue(lp.clone()))
            .unwrap_or(0);
        if queued <= 0 {
            panic_with_error!(&env, Error::NothingQueued);
        }
        let total_assets = Self::load_total_assets(&env);
        let payout = queued.min(total_assets);
        env.storage()
            .persistent()
            .set(&DataKey::WithdrawQueue(lp.clone()), &0i128);
        let total_queued: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalQueued)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalQueued, &(total_queued.saturating_sub(queued)));
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &(total_assets - payout));

        let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
        token::Client::new(&env, &bond_token)
            .transfer(&env.current_contract_address(), &lp, &payout);

        env.events().publish(
            (Symbol::new(&env, "queued_withdrawal_processed"), lp),
            payout,
        );
    }

    // ── Settlement-only callbacks ──────────────────────────────────────────────

    /// Pay `amount` of bond_token to `recipient` as backstop compensation.
    ///
    /// Only callable by the settlement contract registered at initialization.
    /// Loss is socialized across all shares by reducing `TotalAssets`.
    pub fn cover(
        env: Env,
        intent_id: BytesN<32>,
        amount: i128,
        recipient: Address,
    ) {
        Self::bump_instance_ttl(&env);
        let settlement: Address = env
            .storage()
            .instance()
            .get(&DataKey::Settlement)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        settlement.require_auth();

        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let total_assets = Self::load_total_assets(&env);
        let payout = amount.min(total_assets);

        // Socialize loss across all shares.
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &(total_assets - payout));
        let total_cover: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalCoverPaid)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalCoverPaid, &(total_cover + payout));

        let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
        token::Client::new(&env, &bond_token)
            .transfer(&env.current_contract_address(), &recipient, &payout);

        env.events().publish(
            (Symbol::new(&env, "cover"), recipient),
            (intent_id, payout),
        );
    }

    /// Notify the vault that `amount` of bond_token income has been transferred
    /// to the contract address (fee share or slash proceeds).
    ///
    /// Increases `TotalAssets` so all existing share holders benefit
    /// proportionally. The caller (settlement) must have already transferred
    /// the tokens before calling this function.
    pub fn receive_income(env: Env, amount: i128) {
        Self::bump_instance_ttl(&env);
        let settlement: Address = env
            .storage()
            .instance()
            .get(&DataKey::Settlement)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        settlement.require_auth();

        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        let total_assets = Self::load_total_assets(&env);
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &(total_assets + amount));
        let total_income: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalIncome)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalIncome, &(total_income + amount));
        env.events()
            .publish((Symbol::new(&env, "income_received"),), amount);
    }

    // ── Admin ─────────────────────────────────────────────────────────────────

    /// Admin-only: recover non-bond tokens accidentally sent directly to the
    /// vault contract address.
    ///
    /// Explicitly refuses to move the vault's own `bond_token` so this cannot
    /// be misused to drain real LP deposits. Does not touch `TotalAssets` /
    /// `TotalShares` since rescued tokens are not part of bond-token accounting.
    pub fn rescue_tokens(env: Env, token: Address, amount: i128, recipient: Address) {
        Self::bump_instance_ttl(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        admin.require_auth();

        let bond_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::BondToken)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        if token == bond_token {
            panic_with_error!(&env, Error::ProtectedToken);
        }

        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );

        env.events().publish(
            (Symbol::new(&env, "rescue_tokens"), token, recipient),
            amount,
        );
    }

    /// Admin-only: toggle emergency withdrawal-queue mode.
    /// When enabled, `withdraw` queues redemptions instead of paying
    /// immediately, protecting the vault during high-stress periods.
    pub fn set_emergency_mode(env: Env, enabled: bool) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        if let Err(CommonError::Unauthorized) = require_admin(&env, &admin) {
            panic_with_error!(&env, Error::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::EmergencyMode, &enabled);
        env.events()
            .publish((Symbol::new(&env, "emergency_mode"),), enabled);
    }

    /// Admin-only: update the vault configuration (`min_deposit` and
    /// `withdrawal_cooldown_secs`). Validates sane bounds and rejects
    /// out-of-range values with `Error::InvalidConfig`.
    pub fn set_config(env: Env, min_deposit: i128, withdrawal_cooldown_secs: u64) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        admin.require_auth();
        Self::bump_instance_ttl(&env);

        if min_deposit < MIN_DEPOSIT_FLOOR || min_deposit > MIN_DEPOSIT_CEILING {
            panic_with_error!(&env, Error::InvalidConfig);
        }
        if withdrawal_cooldown_secs < WITHDRAWAL_COOLDOWN_SECS_MIN
            || withdrawal_cooldown_secs > WITHDRAWAL_COOLDOWN_SECS_MAX
        {
            panic_with_error!(&env, Error::InvalidConfig);
        }

        let config = BackstopConfig {
            min_deposit,
            withdrawal_cooldown_secs,
        };
        env.storage().instance().set(&DataKey::Config, &config);
        env.events()
            .publish((Symbol::new(&env, "config_updated"),), config);
    }

    // ── Views ──────────────────────────────────────────────────────────────────

    /// Returns the current admin-configurable vault configuration.
    pub fn get_config(env: Env) -> BackstopConfig {
        Self::load_config(&env)
    }

    /// Returns (total_assets, total_shares, assets_per_share_in_bps).
    /// `assets_per_share_in_bps = total_assets * 10_000 / total_shares`
    /// (i.e. 10_000 means 1:1, 12_000 means the vault has appreciated 20%).
    pub fn get_vault_state(env: Env) -> (i128, i128, i128) {
        let total_assets = Self::load_total_assets(&env);
        let total_shares = Self::load_total_shares(&env);
        let rate = total_assets
            .checked_mul(10_000)
            .unwrap_or(total_assets)
            .checked_div(total_shares)
            .unwrap_or(10_000);
        (total_assets, total_shares, rate)
    }

    /// Returns `(share_balance, queued_withdrawal_amount, withdraw_cooldown_until)`
    /// for `lp`.
    ///
    /// `share_balance` is the LP's vault shares; `queued_withdrawal_amount` is
    /// what is still queued for the LP from emergency-mode withdrawals; and
    /// `withdraw_cooldown_until` is the earliest ledger timestamp at which the
    /// LP's next `withdraw` call will be accepted.
    ///
    /// The cooldown timestamp is the value `withdraw` itself compares against
    /// (`now < cooldown_until` → `Error::CooldownNotExpired`). Surfacing it
    /// here means an LP or a dashboard can tell when a withdrawal becomes
    /// possible without reading the contract's private storage key
    /// `DataKey::WithdrawCooldown(lp)`.
    ///
    /// An LP that has never deposited has no cooldown entry, and the value
    /// reported is `0` — the same default `withdraw` uses, i.e. "no cooldown".
    ///
    /// ## ABI change
    ///
    /// This view previously returned a 2-tuple `(share_balance,
    /// queued_withdrawal_amount)`. The third element is **additive** in
    /// meaning, but it does change the tuple's shape, so a client that
    /// destructures the old 2-tuple needs updating to a 3-tuple. The contract
    /// has no in-tree consumer of this view, and it has not been deployed to
    /// mainnet, so there is no live consumer to migrate.
    pub fn get_lp_position(env: Env, lp: Address) -> (i128, i128, u64) {
        let shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(lp.clone()))
            .unwrap_or(0);
        let queued: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WithdrawQueue(lp.clone()))
            .unwrap_or(0);
        let cooldown_until: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::WithdrawCooldown(lp))
            .unwrap_or(0);
        (shares, queued, cooldown_until)
    }

    /// Returns the withdrawal cooldown expiration timestamp for `lp`.
    ///
    /// If the LP has no cooldown set, returns 0 (cooldown expired / no deposit yet).
    /// If the returned timestamp is > current ledger timestamp, the LP cannot
    /// withdraw until that timestamp passes.
    /// Issue #533: Surface the cooldown timestamp for off-chain tooling.
    pub fn get_withdrawal_cooldown(env: Env, lp: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::WithdrawCooldown(lp))
            .unwrap_or(0)
    }

    // ── Private helpers ────────────────────────────────────────────────────────

    fn load_total_assets(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalAssets)
            .unwrap_or(VIRTUAL_ASSETS)
    }

    fn load_total_shares(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalShares)
            .unwrap_or(VIRTUAL_SHARES)
    }

    fn load_config(env: &Env) -> BackstopConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or(BackstopConfig {
                min_deposit: MIN_DEPOSIT,
                withdrawal_cooldown_secs: WITHDRAWAL_COOLDOWN_SECS,
            })
    }

    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

    fn bump_persistent_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO);
    }
}

#[cfg(test)]
mod bench;

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger as _};
    use soroban_sdk::{Env, IntoVal};

    /// Deploy the vault, wire up a mock bond token, and return the client,
    /// the token client, and the LP address along with the deposit timestamp.
    fn setup_deposited_lp(
        env: &Env,
        deposit_amount: i128,
    ) -> (BackstopVaultClient<'_>, token::Client<'_>, Address, u64) {
        let admin = Address::generate(env);
        let settlement = Address::generate(env);
        let lp = Address::generate(env);

        let token_admin = Address::generate(env);
        let bond_token = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();
        let token_client = token::Client::new(env, &bond_token);
        let token_admin_client = token::StellarAssetClient::new(env, &bond_token);

        let contract_id = env.register(BackstopVault, ());
        let client = BackstopVaultClient::new(env, &contract_id);

        env.mock_all_auths();
        client.initialize(&admin, &bond_token, &settlement);

        token_admin_client.mint(&lp, &deposit_amount);

        let deposit_ts: u64 = 1_000_000;
        env.ledger().set_timestamp(deposit_ts);
        client.deposit(&lp, &deposit_amount);

        (client, token_client, lp, deposit_ts)
    }

    /// Boundary: `now == cooldown_until` must succeed (inclusive boundary).
    #[test]
    fn withdraw_succeeds_at_exact_cooldown_boundary() {
        let env = Env::default();
        let deposit_amount = MIN_DEPOSIT;
        let (client, token_client, lp, deposit_ts) =
            setup_deposited_lp(&env, deposit_amount);

        let cooldown_until = deposit_ts + WITHDRAWAL_COOLDOWN_SECS;
        env.ledger().set_timestamp(cooldown_until);

        let (shares, _, _) = client.get_lp_position(&lp);
        assert!(shares > 0);

        let assets_out = client.withdraw(&lp, &shares);
        assert!(assets_out > 0);
        assert_eq!(token_client.balance(&lp), assets_out);

        let (remaining, _, _) = client.get_lp_position(&lp);
        assert_eq!(remaining, 0);
    }

    /// Boundary: `now == cooldown_until - 1` must fail with CooldownNotExpired.
    #[test]
    fn withdraw_fails_one_second_before_cooldown_boundary() {
        let env = Env::default();
        let deposit_amount = MIN_DEPOSIT;
        let (client, _token_client, lp, deposit_ts) =
            setup_deposited_lp(&env, deposit_amount);

        let cooldown_until = deposit_ts + WITHDRAWAL_COOLDOWN_SECS;
        env.ledger().set_timestamp(cooldown_until - 1);

        let (shares, _, _) = client.get_lp_position(&lp);
        assert!(shares > 0);

        let result = client.try_withdraw(&lp, &shares);
        assert_eq!(
            result,
            Err(Ok(Error::CooldownNotExpired.into_val(&env)))
        );
    }
}
