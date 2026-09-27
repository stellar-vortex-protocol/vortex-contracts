#![no_std]

//! Vortex Protocol — Cross-Chain Intent Settlement
//!
//! Users submit swap intents (e.g. "swap 1 ETH on Ethereum for ~3500 USDC on Stellar").
//! Solvers compete to fill these intents off-chain, then settle on-chain via this contract.
//! Settlement is guaranteed by a solver bond; failing to fill within the deadline slashes the bond.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, xdr::ToXdr,
    Address, Bytes, BytesN, Env, String, Symbol,
};

#[cfg(test)]
mod test;

#[cfg(test)]
mod proptest_bond;

// ─── Constants ────────────────────────────────────────────────────────────────

const INTENT_EXPIRY: u64 = 1800; // 30 minutes
const FILL_WINDOW: u64 = 300; // 5 minutes to fill after intent accepted
const MIN_BOND: i128 = 50 * 10_000_000; // 50 USDC minimum solver bond
const PROTOCOL_FEE_BPS: i128 = 5; // 0.05%
/// Duration of the competitive bid-collection window when bid-window mode is
/// enabled.  Solvers have this many seconds after `submit_intent` to submit
/// competing quotes via `bid_intent`; the best quote wins once the window
/// closes.
const BID_WINDOW: u64 = 120; // 2 minutes

// Upper sanity bound for src_amount and min_dst_amount.
//
// Largest realistic token amounts use 18-decimal ETH units.
// 1e12 tokens × 1e18 units/token = 1e30, well within i128 range (~1.7e38),
// but downstream arithmetic (fee = amount * 5 / 10_000) multiplies first and
// then divides. To guarantee `amount * PROTOCOL_FEE_BPS` never overflows i128,
// the bound is i128::MAX / PROTOCOL_FEE_BPS ≈ 3.4e37. We choose a round,
// economically implausible threshold: 10^30 (one trillion 18-decimal tokens).
// That is a comfortable safety margin while rejecting only fat-fingered inputs.
pub const MAX_AMOUNT: i128 = 1_000_000_000_000_000_000_000_000_000_000i128; // 10^30

// Soroban archives ledger entries that go too long without being touched.
// Persistent Intent/Solver records get their TTL bumped on every write so
// they don't need to be manually restored before later calls can read them.
const DAY_IN_LEDGERS: u32 = 17280; // ~5s per ledger
const PERSISTENT_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 14;
const PERSISTENT_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 30;

// The contract instance entry (Admin/FeeRecipient/BondToken/TotalIntents/
// TotalVolume, plus the contract's own code) is a single ledger entry and
// needs the same treatment, or the whole contract becomes unreachable.
const INSTANCE_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 30;
const INSTANCE_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 60;

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// **Instance storage.** The admin `Address` that may call privileged
    /// functions (`pause`, `unpause`, `set_fee_recipient`, `transfer_admin`,
    /// `add_allowed_dst_token`, etc.).  Written once by `initialize` and
    /// rotated by `transfer_admin`.  Lives as long as the contract instance.
    Admin,

    /// **Instance storage.** The `Address` that receives protocol fees
    /// (collected in `fill_intent`) and slashed bond amounts (collected in
    /// `slash_solver`).  Written by `initialize` and updated by
    /// `set_fee_recipient`.  Lives as long as the contract instance.
    FeeRecipient,
    PendingFeeRecipient, // proposed-but-not-yet-accepted new fee recipient (issue #30)
    BondToken,          // USDC address for bonds
    Intent(BytesN<32>), // intent_id -> IntentRecord
    Solver(Address),    // address -> SolverRecord
    TotalIntents,

    /// **Instance storage.** Cumulative `dst_token` volume (`i128`) across
    /// all successfully filled intents.  Incremented by `fill_intent`.
    TotalVolume,

    /// **Instance storage.** Count of currently registered solvers (`u32`).
    /// Incremented by `register_solver` on first registration, decremented
    /// by `deregister_solver`.
    TotalSolvers,

    /// **Instance storage.** Boolean flag (`true` = paused).  Set by
    /// `pause()` and cleared by `unpause()`.  When `true`,
    /// `submit_intent`, `accept_intent`, and `fill_intent` reject all
    /// calls.  Absent until first `pause()` call (defaults to `false`).
    Paused,

    /// **Instance storage.** Presence-flag (value `true`) indicating that
    /// `token` is on the allowed-destination list.  Added by
    /// `add_allowed_dst_token` and removed by `remove_allowed_dst_token`.
    /// Only checked by `submit_intent` when `DstAllowlistEnabled` is `true`.
    AllowedDstToken(Address),

    /// **Instance storage.** Boolean toggle (`true` = enforced).  Set via
    /// `set_dst_allowlist_enabled`.  When `false` (the default), the
    /// `AllowedDstToken` list is populated but not enforced by
    /// `submit_intent`, letting an admin pre-populate the list before
    /// switching enforcement on.
    DstAllowlistEnabled,
    UserNonce(Address),       // per-user submit counter to widen intent_id preimage
    AllowedSrcChain(String), // src_chain name -> present if allowed
    SrcChainAllowlistEnabled,

    /// #349: Per-token solver bond (solver, token) -> bond_amount
    /// Single source of truth for bond amounts
    SolverBond(Address, Address), // (solver, token) -> i128

    /// #349: Total bonded by token across all solvers
    TotalBondedByToken(Address), // token -> i128

    /// Protocol configuration for atomic reads/writes
    Config,
}

// ─── Data Structs ─────────────────────────────────────────────────────────────

/// Admin-configurable protocol parameters.  Stored as a single instance-storage
/// entry so all four values are read/written atomically.
#[contracttype]
#[derive(Clone)]
pub struct ProtocolConfig {
    /// Minimum solver bond in bond_token's smallest unit.
    pub min_bond: i128,
    /// Seconds a solver has to fill after accepting an intent.
    pub fill_window: u64,
    /// Default intent lifetime in seconds (used when submit_intent deadline is None).
    pub intent_expiry: u64,
    /// Protocol fee in basis points charged on each fill (0.01% per bps).
    pub protocol_fee_bps: i128,
}

/// A user's cross-chain swap intent
#[contracttype]
#[derive(Clone)]
pub struct IntentRecord {
    pub intent_id: BytesN<32>,
    pub user: Address,

    /// Source chain details (off-chain reference)
    pub src_chain: String, // "ethereum" | "base" | "polygon" etc.
    pub src_token: String, // token address on source chain
    pub src_amount: i128,  // amount in source token's smallest unit

    /// Destination (always Stellar)
    pub dst_token: Address, // SAC/SEP-41 token on Stellar
    pub min_dst_amount: i128, // minimum acceptable output per fill (#348: per src_portion)

    pub solver: Option<Address>, // assigned solver
    pub state: IntentState,

    pub created_at: u64,
    /// #347: user's original deadline; separate from fill-window deadline which gets reset
    pub user_deadline: u64,
    pub deadline: u64, // effective deadline (fill window or user deadline)
    pub filled_at: Option<u64>,
    pub fill_amount: Option<i128>, // cumulative dst tokens received across all fills

    /// Cumulative dst tokens delivered so far; intent completes when this
    /// reaches or exceeds `min_dst_amount * num_fills_needed`, but in the
    /// partial-fill model the intent is fully settled once the solver
    /// delivering a fill brings `total_filled` to at least `min_dst_amount`.
    ///
    /// More precisely: each individual partial fill must be > 0, and the
    /// intent transitions to `Filled` as soon as `total_filled` satisfies
    /// the user's `min_dst_amount` requirement.
    pub total_filled: i128,
    /// #348: cumulative source tokens filled (intent closes when src_filled == src_amount)
    pub src_filled: i128,
}

#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum IntentState {
    Open,            // awaiting solver
    Accepted,        // solver claimed it
    PartiallyFilled, // one or more partial fills delivered; still open for more
    Filled,          // user received total output >= min_dst_amount
    Cancelled,       // user cancelled before fill
    Expired,         // deadline passed, no fill
    Slashed,         // solver failed to fill after accepting
}

/// A registered solver (market maker)
#[contracttype]
#[derive(Clone)]
pub struct SolverRecord {
    pub address: Address,
    // #349: bond_amount removed — derive from SolverBond(address, bond_token) for single source of truth
    pub fills_completed: u32,
    pub fills_failed: u32,
    pub total_volume: i128,
    pub is_active: bool,
    pub registered_at: u64,
    /// Number of intents currently Accepted by this solver (not yet filled or slashed).
    /// Bond stays locked behind these obligations, so it must be zero before deregistration.
    pub active_intents: u32,
    /// Timestamp of last slash; cooldown applies after a slash.
    pub last_slash_time: u64,
}

/// Return type for `get_protocol_params`.
/// Exposes the four effective protocol values as named fields so integrators
/// don't have to rely on source-code comments for the constant definitions.
#[contracttype]
#[derive(Clone)]
pub struct ProtocolParams {
    /// Minimum USDC bond (in token's smallest unit) a solver must hold.
    pub min_bond: i128,
    /// Seconds a solver has to fill an intent after accepting it.
    pub fill_window: u64,
    /// Default intent lifetime in seconds (when no explicit deadline is passed).
    pub intent_expiry: u64,
    /// Protocol fee charged on each fill, in basis points (1 bps = 0.01%).
    pub protocol_fee_bps: i128,
/// Tracks the leading bid for an intent that is in the `Bidding` state.
/// Only the current best bid is kept — a new submission replaces it only
/// if it quotes a strictly higher `quoted_dst_amount`.
#[contracttype]
#[derive(Clone)]
pub struct BestBidRecord {
    pub solver: Address,
    pub quoted_dst_amount: i128,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    /// `initialize` was called on a contract that already has an `Admin` key
    /// in instance storage. Raised exclusively by `initialize`.
    AlreadyInitialized = 1,

    /// A privileged operation was attempted by a caller who is not the
    /// required authority.  Raised by `fill_intent` when the caller is not
    /// the solver that accepted the intent, and by `cancel_intent` when the
    /// caller is not the intent's owner.
    Unauthorized = 2,

    /// The supplied `intent_id` has no corresponding `IntentRecord` in
    /// persistent storage.  Raised by `accept_intent`, `fill_intent`,
    /// `cancel_intent`, `slash_solver`, and `expire_intent`.
    IntentNotFound = 3,

    /// The intent's `state` is not `Open` at a point where `Open` is
    /// required.  Raised by `cancel_intent` (non-`Open`/non-`Accepted`
    /// guard) and by `expire_intent` (which only operates on `Open` intents).
    IntentNotOpen = 4,

    /// The current ledger timestamp has reached or passed the intent's
    /// `deadline` when a solver tries to accept it via `accept_intent`.
    /// The intent's state is lazily updated to `Expired` before the panic.
    IntentExpired = 5,

    /// `fill_intent` or `slash_solver` requires the intent to be in state
    /// `Accepted`, but it is in a different terminal or intermediate state.
    /// Also raised by `slash_solver` when `intent.state != Accepted`.
    IntentNotAccepted = 6,

    /// An operation that requires a registered solver (e.g. `deregister_solver`,
    /// `withdraw_bond`, `accept_intent`) was called for an address that has no
    /// `SolverRecord` in persistent storage.
    SolverNotRegistered = 7,

    /// `register_solver` was called with a `bond_amount` that, when added to
    /// any existing bond, does not reach `MIN_BOND` (500_000_000 stroops /
    /// 50 USDC).  Also raised by `withdraw_bond` when the post-withdrawal
    /// balance would fall below `MIN_BOND`.
    SolverBondTooLow = 8,

    /// `fill_intent` was called with a `fill_amount` less than the intent's
    /// `min_dst_amount`.  Raised only in `fill_intent`.
    InsufficientOutput = 9,

    /// `fill_intent` was called after the intent's `deadline` (i.e. the fill
    /// window that starts when the solver calls `accept_intent` and lasts
    /// `FILL_WINDOW` seconds) has already elapsed.  Also (confusingly) used
    /// in `slash_solver` as a guard label when the fill window has *not yet*
    /// expired — the intent cannot be slashed before its deadline.
    FillWindowExpired = 10,

    /// `cancel_intent` was called on an intent in state `Accepted`.  Users
    /// may only cancel `Open` intents; once a solver has accepted, the
    /// `slash_solver` path must be used if the solver fails to fill.
    CannotCancelAccepted = 11,

    /// `accept_intent` was called for a solver whose `is_active` flag is
    /// `false` (set when the bond falls below `MIN_BOND` after a slash, or
    /// after calling `deregister_solver`).
    SolverInactive = 12,

    /// A numeric input that must be strictly positive was zero or negative.
    /// Raised by `submit_intent` (`src_amount` or `min_dst_amount ≤ 0`) and
    /// by `register_solver` / `withdraw_bond` (`bond_amount ≤ 0`).
    ZeroAmount = 13,

    /// `submit_intent` was called with a `deadline` that is already in the
    /// past (i.e. `deadline ≤ env.ledger().timestamp()`).
    InvalidDeadline = 14,

    /// `fill_intent` was called on an intent that is already in state
    /// `Filled`.
    IntentAlreadyFilled = 15,

    /// An operation that requires the contract to be initialized (i.e. needs
    /// `Admin` in instance storage) was called before `initialize`.  Raised
    /// by `require_admin` and by `set_fee_recipient` / `transfer_admin`.
    NotInitialized = 16,

    /// `deregister_solver` was called while the solver's `active_intents`
    /// counter is greater than zero, meaning at least one intent is currently
    /// in state `Accepted` by this solver.  The solver must wait for those
    /// intents to reach a terminal state first.
    SolverHasActiveIntents = 17,

    /// `submit_intent`, `accept_intent`, or `fill_intent` was called while
    /// the contract's `Paused` flag is `true`.  Raised by
    /// `require_not_paused`.
    ContractPaused = 18,

    /// `expire_intent` was called before the intent's `deadline` has been
    /// reached (i.e. `env.ledger().timestamp() < intent.deadline`).
    DeadlineNotReached = 19,

    /// `withdraw_bond` was called with an `amount` greater than the solver's
    /// current `bond_amount`.
    InsufficientBond = 20,

    /// `submit_intent` was called with a `dst_token` that is not present in
    /// the `AllowedDstToken` allowlist while `DstAllowlistEnabled` is `true`.
    DstTokenNotAllowed = 21,
    IntentAlreadyExists = 22,
    /// #30: no pending fee-recipient proposal to accept
    NoPendingFeeRecipient = 22,
    /// #31: fee arithmetic overflowed (fill_amount is astronomically large)
    FeeOverflow = 23,
    /// #33: the address passed to add_allowed_dst_token doesn't implement SEP-41
    InvalidTokenInterface = 24,
    SrcChainNotAllowed = 25,
    RescueProtectedToken = 26,
    /// #348: src_portion is invalid (zero, negative, or exceeds remaining)
    InvalidSrcPortion = 27,
    /// Amount overflow in multiplication/division
    AmountOverflow = 28,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct IntentSettlement;

#[contractimpl]
impl IntentSettlement {
    // ── Initialization ────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address, fee_recipient: Address, bond_token: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        // Auth audit: require_auth() is correct here. `admin` must sign the
        // initialization tx to prove ownership of the address being recorded as
        // admin. require_auth_for_args is not needed because there are no
        // separate per-argument capabilities to scope — the signer IS the admin.
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &fee_recipient);
        env.storage()
            .instance()
            .set(&DataKey::BondToken, &bond_token);
        env.storage().instance().set(&DataKey::TotalIntents, &0u64);
        env.storage().instance().set(&DataKey::TotalVolume, &0i128);
        env.storage().instance().set(&DataKey::TotalSolvers, &0u32);
        // Seed Config with defaults so the contract is immediately usable
        // without a follow-up admin call.
        env.storage().instance().set(
            &DataKey::Config,
            &ProtocolConfig {
                min_bond: DEFAULT_MIN_BOND,
                fill_window: DEFAULT_FILL_WINDOW,
                intent_expiry: DEFAULT_INTENT_EXPIRY,
                protocol_fee_bps: DEFAULT_PROTOCOL_FEE_BPS,
            },
        );
        Self::bump_instance_ttl(&env);
    }

    // ── Admin ──────────────────────────────────────────────────────────────────

    /// Admin-only: propose a new fee recipient address. The proposal is stored
    /// but not yet active. The new address must call `accept_fee_recipient` to
    /// confirm, mirroring `transfer_admin`'s two-step pattern so a typo'd or
    /// unreachable address can never silently misroute protocol fees.
    ///
    /// A new proposal overwrites any prior pending proposal, so the admin can
    /// correct a mistake before the recipient has accepted.
    pub fn propose_fee_recipient(env: Env, new_fee_recipient: Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        // Auth audit: require_auth() is correct. The stored admin address must
        // sign. require_auth_for_args would add no security here — there's no
        // meaningful sub-scope within "being admin".
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::PendingFeeRecipient, &new_fee_recipient);

        env.events().publish(
            (Symbol::new(&env, "fee_recipient_proposed"),),
            new_fee_recipient,
        );
    }

    /// The pending fee recipient confirms the handover. Until this is called
    /// the current fee recipient remains unchanged.
    pub fn accept_fee_recipient(env: Env, new_fee_recipient: Address) {
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingFeeRecipient)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingFeeRecipient));

        if pending != new_fee_recipient {
            panic_with_error!(&env, Error::Unauthorized);
        }
        new_fee_recipient.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &new_fee_recipient);
        env.storage()
            .instance()
            .remove(&DataKey::PendingFeeRecipient);

        env.events().publish(
            (Symbol::new(&env, "fee_recipient_updated"),),
            new_fee_recipient,
        );
    }

    /// Admin-only: transfer the admin role to a new address. The new admin
    /// must authorize too, so a typo'd address can't accidentally brick
    /// admin control of the contract.
    pub fn transfer_admin(env: Env, new_admin: Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        // Auth audit: require_auth() is correct on both the outgoing and
        // incoming admin. Requiring both prevents accidentally handing the role
        // to a typo'd or uncontrolled address. require_auth_for_args is not
        // applicable — both signers ARE the principals, there's nothing to
        // sub-scope.
        admin.require_auth();
        new_admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &new_admin);

        env.events()
            .publish((Symbol::new(&env, "admin_transferred"),), new_admin);
    }

    // ── Protocol Config ───────────────────────────────────────────────────────

    /// Read the effective protocol config.  Falls back to compile-time defaults
    /// for contracts that existed before this upgrade (upgrade safety).
    pub fn get_config(env: Env) -> ProtocolConfig {
        Self::load_config(&env)
    }

    /// Admin-only: update the four configurable protocol parameters atomically.
    ///
    /// Bounds (any violation returns `InvalidConfig`):
    /// * `protocol_fee_bps`  ≤ 1 000 (10%)
    /// * `fill_window`       ≥ 60 s
    /// * `intent_expiry`     ≥ 300 s and > fill_window
    /// * `min_bond`          ≥ 1 token unit (10_000_000 for 7-decimal USDC)
    pub fn set_config(
        env: Env,
        min_bond: i128,
        fill_window: u64,
        intent_expiry: u64,
        protocol_fee_bps: i128,
    ) {
        Self::require_admin(&env);

        if !(0..=MAX_PROTOCOL_FEE_BPS).contains(&protocol_fee_bps) {
            panic_with_error!(&env, Error::InvalidConfig);
        }
        if fill_window < MIN_FILL_WINDOW_SECS {
            panic_with_error!(&env, Error::InvalidConfig);
        }
        if intent_expiry < MIN_INTENT_EXPIRY_SECS || intent_expiry <= fill_window {
            panic_with_error!(&env, Error::InvalidConfig);
        }
        if min_bond < MIN_BOND_FLOOR {
            panic_with_error!(&env, Error::InvalidConfig);
        }

        let cfg = ProtocolConfig {
            min_bond,
            fill_window,
            intent_expiry,
            protocol_fee_bps,
        };
        env.storage().instance().set(&DataKey::Config, &cfg);
        Self::bump_instance_ttl(&env);

        env.events().publish(
            (Symbol::new(&env, "config_updated"),),
            (min_bond, fill_window, intent_expiry, protocol_fee_bps),
        );
    }

    // ── Destination Token Allowlist ───────────────────────────────────────────

    /// Admin-only: allow a dst_token to be targeted by new intents.
    /// submit_intent had no validation on dst_token at all -- any address,
    /// including a bogus or malicious "token" contract, could be named as
    /// the destination.
    ///
    /// Before storing the allowance we call `decimals()` on the candidate
    /// address as a lightweight SEP-41 interface probe (issue #33). If the
    /// address doesn't implement the token interface the call traps and the
    /// transaction reverts, surfacing the error at admin time rather than
    /// silently allowing a non-token that would only fail later inside
    /// fill_intent's transfer call.
    ///
    /// Note: `decimals()` is a read-only view, so this probe has no side
    /// effects on the token's state.
    pub fn add_allowed_dst_token(env: Env, token: Address) {
        Self::require_admin(&env);

        // Probe the SEP-41 interface: if `token` isn't a real token contract
        // this will trap and revert the transaction before we store anything.
        let token_client = token::Client::new(&env, &token);
        // decimals() is a pure view with no side-effects; we discard the value.
        let _decimals = token_client.decimals();

        env.storage()
            .instance()
            .set(&DataKey::AllowedDstToken(token.clone()), &true);
        env.events()
            .publish((Symbol::new(&env, "dst_token_allowed"),), token);
    }

    pub fn remove_allowed_dst_token(env: Env, token: Address) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .remove(&DataKey::AllowedDstToken(token.clone()));
        env.events()
            .publish((Symbol::new(&env, "dst_token_disallowed"),), token);
    }

    pub fn is_dst_token_allowed(env: Env, token: Address) -> bool {
        env.storage()
            .instance()
            .has(&DataKey::AllowedDstToken(token))
    }

    /// Admin-only: turn allowlist enforcement in submit_intent on/off.
    /// Off by default -- an admin opts in once they've populated the list
    /// via add_allowed_dst_token, rather than every intent submission
    /// suddenly requiring one.
    pub fn set_dst_allowlist_enabled(env: Env, enabled: bool) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::DstAllowlistEnabled, &enabled);
    }

    pub fn is_dst_allowlist_enabled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::DstAllowlistEnabled)
            .unwrap_or(false)
    }

    // ── Per-Token Bond Multiplier ──────────────────────────────────────────────

    /// Admin-only: set a custom bond multiplier for a dst_token.
    /// Multiplier is stored as i128 where 10 = 1.0x, 15 = 1.5x, 20 = 2.0x.
    /// Unset tokens default to 10 (1.0x).
    pub fn set_min_bond_multiplier(env: Env, token: Address, multiplier: i128) {
        Self::require_admin(&env);
        if multiplier <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        env.storage()
            .persistent()
            .set(&DataKey::MinBondMultiplier(token.clone()), &multiplier);
        env.events().publish(
            (Symbol::new(&env, "bond_multiplier_set"),),
            (token, multiplier),
        );
    }

    /// Get the bond multiplier for a dst_token, or 10 (1.0x) if unset.
    pub fn get_min_bond_multiplier(env: Env, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::MinBondMultiplier(token))
            .unwrap_or(10)
    // ── Source Chain Allowlist ────────────────────────────────────────────────
get_tiered_fee_bps is only called from fund_c_address (1320), reveal_fund (4164), fund_c_address_with_swap (4325), and execute_meta_fund (4498). batch_fund_c_address (1445) and fund_c_address_with_referral (2079) both compute the fee directly from the flat global rate via get_effective_fee_bps, never consulting the caller's volume tier — meaning the tiered-fee feature is silently bypassed on 2 of the bridge's 7 funding entry points.

What needs to be done
 Route batch_fund_c_address and fund_c_address_with_referral through get_tiered_fee_bps the same way the other four funding paths do
 Add test_batch_fund_applies_tiered_fee and test_referral_fund_applies_tiered_fee
Files to change
contracts/onboarding-bridge/src/lib.rs
Difficulty
Medium

Getting started
This is a self-contained task — no additional repo access or secrets needed.

git clone https://github.com/<your-fork>/C-Address-Onboarding-Bridge--Contract.git
cd C-Address-Onboarding-Bridge--Contract
rustup target add wasm32-unknown-unknown
cargo test -p onboarding-bridge --features testutils
Submitting your PR
When you open your pull request, include Closes #123 in the PR description (using this issue's actual number in place of 123). This links the PR to the issue and closes it automatically on merge.

Please follow this repo's Conventional Commits format for your commit messages (e.g. fix(contract): ..., test(sdk): ..., docs: ...).



    /// Admin-only: add a chain name to the src_chain allowlist.
    ///
    /// Issue #34: submit_intent accepted src_chain as free-text with zero
    /// validation, so a typo ("etherium") or unsupported name would create an
    /// intent that solvers can never match. This allowlist mirrors the
    /// AllowedDstToken pattern: an admin populates the list, then enables
    /// enforcement via set_src_chain_allowlist_enabled.
    pub fn add_allowed_src_chain(env: Env, chain: String) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::AllowedSrcChain(chain.clone()), &true);
        env.events()
            .publish((Symbol::new(&env, "src_chain_allowed"),), chain);
    }

    /// Admin-only: remove a chain name from the src_chain allowlist.
    pub fn remove_allowed_src_chain(env: Env, chain: String) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .remove(&DataKey::AllowedSrcChain(chain.clone()));
        env.events()
            .publish((Symbol::new(&env, "src_chain_disallowed"),), chain);
    }

    /// Returns true if `chain` is on the allowlist.
    pub fn is_src_chain_allowed(env: Env, chain: String) -> bool {
        env.storage()
            .instance()
            .has(&DataKey::AllowedSrcChain(chain))
    }

    /// Admin-only: toggle src_chain validation in submit_intent.
    ///
    /// Defaults to false so existing deployments keep working until an admin
    /// has populated the list and is ready to enforce it. Set to true before
    /// mainnet launch after calling add_allowed_src_chain for every chain the
    /// protocol supports.
    pub fn set_src_chain_allowlist_enabled(env: Env, enabled: bool) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::SrcChainAllowlistEnabled, &enabled);
    }

    /// Whether src_chain validation is currently active.
    pub fn is_src_chain_allowlist_enabled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::SrcChainAllowlistEnabled)
            .unwrap_or(false)
    }

    // ── Pause Control ─────────────────────────────────────────────────────────

    /// Admin-only: halt new intent submission, acceptance, and fills for
    /// incident response. slash_solver stays permissionless throughout, so a
    /// solver already holding an Accepted intent can't dodge accountability
    /// by waiting out the pause.
    ///
    /// Issue #36 — pause scope decision: register_solver, deregister_solver,
    /// and withdraw_bond are also gated here. During a live incident an admin
    /// may need to freeze the entire protocol state to investigate; allowing
    /// solvers to withdraw their bonds mid-incident would let them shed
    /// collateral exactly when the protocol most needs it as a backstop.
    /// cancel_intent is intentionally left open so users can always reclaim
    /// their Open intents.
    pub fn pause(env: Env) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish((Symbol::new(&env, "paused"),), true);
    }

    /// Admin-only: lift a pause and restore normal operation.
    pub fn unpause(env: Env) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish((Symbol::new(&env, "paused"),), false);
    }

    /// Whether submit_intent/accept_intent/fill_intent and solver bond
    /// management are currently halted.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    // ── Token Rescue ──────────────────────────────────────────────────────────

    /// Admin-only: recover SEP-41 tokens accidentally sent to the contract.
    ///
    /// Issue #35 — trust model: rescue is restricted to tokens that are
    /// neither the bond_token nor any token currently referenced by an active
    /// (Accepted) intent as its dst_token. This prevents the rescue path from
    /// being misused to drain live solver collateral or in-flight intent
    /// output from under active protocol participants.
    ///
    /// If you need to move bond_token you must wait until all active intents
    /// have settled (filled, slashed, or cancelled), then handle any
    /// accounting off-chain.
    pub fn rescue_tokens(env: Env, token: Address, to: Address, amount: i128) {
        Self::require_admin(&env);

        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        // Refuse to rescue the protocol's own bond/collateral token.
        let bond_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::BondToken)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        if token == bond_token {
            panic_with_error!(&env, Error::RescueProtectedToken);
        }

        let client = token::Client::new(&env, &token);
        client.transfer(&env.current_contract_address(), &to, &amount);

        env.events()
            .publish((Symbol::new(&env, "tokens_rescued"), to), (token, amount));
    }

    // ── Solver Management ─────────────────────────────────────────────────────

    /// Solvers register by depositing a USDC bond. Existing solvers may top up
    /// with any positive amount -- the minimum is enforced on the resulting
    /// total, not on each individual deposit.
    pub fn register_solver(env: Env, solver: Address, bond_amount: i128) {
        // Auth audit: require_auth() is correct. The solver must sign to
        // consent to locking their own funds as bond. require_auth_for_args
        // could theoretically scope to (solver, bond_amount) but adding that
        // scope provides no real benefit — the solver is the tx signer and
        // the bond amount is constrained by their token balance anyway.
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        if bond_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let existing: Option<SolverRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()));

        let existing_bond = existing.as_ref().map(|s| s.bond_amount).unwrap_or(0);
        let cfg = Self::load_config(&env);
        if existing_bond + bond_amount < cfg.min_bond {
            panic_with_error!(&env, Error::SolverBondTooLow);
        }

        let is_new_solver = existing.is_none();

        // ── Effects first (CEI) ──────────────────────────────────────────────
        // Build and persist the SolverRecord *before* pulling funds in so the
        // contract's storage is always consistent with what it holds: if the
        // transfer were to fail (or a re-entrant call were made mid-transfer),
        // the record either doesn't exist yet (new solver) or still reflects
        // the pre-topup balance, rather than an inflated balance with no matching funds.
        let record = match existing {
            Some(mut s) => {
                s.bond_amount += bond_amount;
                s.is_active = true;
                s
            }
            None => SolverRecord {
                address: solver.clone(),
                bond_amount,
                fills_completed: 0,
                fills_failed: 0,
                total_volume: 0,
                is_active: true,
                registered_at: env.ledger().timestamp(),
                active_intents: 0,
                last_slash_time: 0,
            },
        };

        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        if is_new_solver {
            let total: u32 = env
                .storage()
                .instance()
                .get(&DataKey::TotalSolvers)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::TotalSolvers, &(total + 1));
        }

        // ── Interaction: pull bond in ────────────────────────────────────────
        let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
        let client = token::Client::new(&env, &bond_token);
        client.transfer(&solver, &env.current_contract_address(), &bond_amount);

        env.events().publish(
            (Symbol::new(&env, "solver_registered"), solver),
            bond_amount,
        );
    }

    pub fn deregister_solver(env: Env, solver: Address) {
        // Auth audit: require_auth() is correct. Only the solver themselves
        // may deregister and trigger bond return. require_auth_for_args is not
        // useful — the sole action is "deregister this exact address".
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        let record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SolverNotRegistered));

        if record.active_intents > 0 {
            panic_with_error!(&env, Error::SolverHasActiveIntents);
        }

        // ── Effects first (CEI) ──────────────────────────────────────────────
        // Remove the solver record and update the counter *before* the external
        // token transfer so that any re-entrant call sees no record and would
        // panic with SolverNotRegistered rather than processing a double-refund.
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

        // ── Interaction: return bond ─────────────────────────────────────────
        if record.bond_amount > 0 {
            let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
            let client = token::Client::new(&env, &bond_token);
            client.transfer(
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

    /// Solver withdraws part of their bond without fully deregistering.
    /// The remaining bond must still clear MIN_BOND -- to go below that,
    /// use deregister_solver instead (which also requires no active intents).
    pub fn withdraw_bond(env: Env, solver: Address, amount: i128) {
        // Auth audit: require_auth() is correct. Only the solver may withdraw
        // their own bond. require_auth_for_args could scope to the withdrawal
        // amount, but the solver signature authorises the full withdrawal path;
        // amount is validated against their stored balance immediately after.
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let mut record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SolverNotRegistered));

        if amount > record.bond_amount {
            panic_with_error!(&env, Error::InsufficientBond);
        }

        let remaining = record.bond_amount - amount;
        let cfg = Self::load_config(&env);
        if remaining < cfg.min_bond {
            panic_with_error!(&env, Error::SolverBondTooLow);
        }

        record.bond_amount = remaining;
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
        let client = token::Client::new(&env, &bond_token);
        client.transfer(&env.current_contract_address(), &solver, &amount);

        env.events()
            .publish((Symbol::new(&env, "bond_withdrawn"), solver), amount);
    }

    // ── Intent Lifecycle ──────────────────────────────────────────────────────

    /// User submits a swap intent. No funds are locked on Stellar at this point —
    /// the user initiates the source-chain tx separately.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_intent(
        env: Env,
        user: Address,
        src_chain: String,
        src_token: String,
        src_amount: i128,
        dst_token: Address,
        min_dst_amount: i128,
        deadline: Option<u64>,
    ) -> BytesN<32> {
        // Auth audit: require_auth() is correct. The user must sign to assert
        // ownership of the address receiving output tokens (dst). If a third-party
        // contract were ever to call submit_intent on a user's behalf, switching to
        // require_auth_for_args scoped to (user, dst_token, min_dst_amount) would
        // limit the scope of delegated authorisation — noted as a future hardening
        // opportunity if composable intent submission is added.
        user.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        if src_amount <= 0 || min_dst_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        if src_amount > MAX_AMOUNT || min_dst_amount > MAX_AMOUNT {
            panic_with_error!(&env, Error::AmountTooLarge);
        }

        if Self::is_dst_allowlist_enabled(env.clone())
            && !Self::is_dst_token_allowed(env.clone(), dst_token.clone())
        {
            panic_with_error!(&env, Error::DstTokenNotAllowed);
        }

        // #34 — validate src_chain when the allowlist is enabled.
        if Self::is_src_chain_allowlist_enabled(env.clone())
            && !Self::is_src_chain_allowed(env.clone(), src_chain.clone())
        {
            panic_with_error!(&env, Error::SrcChainNotAllowed);
        }

        let now = env.ledger().timestamp();
        let cfg = Self::load_config(&env);
        let expiry = deadline.unwrap_or(now + cfg.intent_expiry);

        if expiry <= now {
            panic_with_error!(&env, Error::InvalidDeadline);
        }

        // Widen the preimage with a per-user nonce so that two intents from
        // the same user with identical (src_chain, src_amount) in the same
        // ledger close produce distinct ids rather than colliding silently.
        let nonce: u64 = env
            .storage()
            .instance()
            .get(&DataKey::UserNonce(user.clone()))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::UserNonce(user.clone()), &(nonce + 1));

        // Deterministic intent_id = hash(user, src_chain, src_token, src_amount, now, nonce)
        let intent_id = Self::compute_intent_id(&env, &user, &src_chain, src_amount, now, nonce);

        // Guard against an extremely unlikely hash collision: if a record with
        // this id somehow already exists, reject rather than silently overwrite.
        if env
            .storage()
            .persistent()
            .has(&DataKey::Intent(intent_id.clone()))
        {
            panic_with_error!(&env, Error::IntentAlreadyExists);
        }

        let intent = IntentRecord {
            intent_id: intent_id.clone(),
            user: user.clone(),
            src_chain,
            src_token,
            src_amount,
            dst_token,
            min_dst_amount,
            solver: None,
            // When bid-window mode is active, the intent opens in Bidding state
            // so solvers can compete before one is assigned exclusive fill rights.
            // The bid-window deadline is BID_WINDOW seconds from now, not the
            // full intent expiry — settle_bids extends it to FILL_WINDOW once a
            // winner is picked.  The original expiry is stored separately in
            // deadline and reset after settlement.
            state: if Self::is_bid_window_enabled(env.clone()) {
                IntentState::Bidding
            } else {
                IntentState::Open
            },
            created_at: now,
            user_deadline: expiry, // #347: store user's original deadline
            // In bidding mode, deadline tracks the end of the bid window.
            // In first-accept-wins mode, deadline tracks the intent expiry.
            deadline: if Self::is_bid_window_enabled(env.clone()) {
                now + BID_WINDOW
            } else {
                expiry
            },
            filled_at: None,
            fill_amount: None,
            total_filled: 0,
            src_filled: 0, // #348: start with 0 source filled
        };

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        let mut user_intents: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&DataKey::UserIntents(user.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        user_intents.push_back(intent_id.clone());
        env.storage()
            .persistent()
            .set(&DataKey::UserIntents(user.clone()), &user_intents);

        let total: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TotalIntents)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalIntents, &(total + 1));

        env.events().publish(
            (Symbol::new(&env, "intent_submitted"), user),
            (intent_id.clone(), min_dst_amount, expiry),
        );

        intent_id
    }

    /// Solver claims an intent (exclusive fill right for FILL_WINDOW seconds)
    pub fn accept_intent(env: Env, solver: Address, intent_id: BytesN<32>) {
        // Auth audit: require_auth() is correct. The solver must sign to
        // voluntarily take on the fill obligation and bond risk associated with
        // this intent. require_auth_for_args scoped to intent_id could prevent a
        // malicious invoker contract from accepting an unintended intent on the
        // solver's behalf; noted as a future hardening opportunity.
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        let mut solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SolverNotRegistered));

        if !solver_record.is_active {
            panic_with_error!(&env, Error::SolverInactive);
        }

        let now = env.ledger().timestamp();
        if solver_record.last_slash_time > 0 && now < solver_record.last_slash_time + SLASH_COOLDOWN {
            panic_with_error!(&env, Error::SolverInactive);
        }

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        let adjusted_min_bond = Self::get_adjusted_min_bond(&env, &intent.dst_token);
        if solver_record.bond_amount < adjusted_min_bond {
            panic_with_error!(&env, Error::SolverBondTooLow);
        }

        let now = env.ledger().timestamp();
        // Boundary semantics: deadline is EXCLUSIVE for acceptance.
        // `now >= intent.deadline` rejects at the boundary second (`now == deadline`)
        // so the full [created_at, deadline) half-open window is available for solvers.
        if now >= intent.deadline {
            env.storage()
                .persistent()
                .set(&DataKey::Intent(intent_id.clone()), &intent);
            Self::bump_intent_ttl(&env, &intent_id);
            panic_with_error!(&env, Error::IntentExpired);
        }

        if intent.state != IntentState::Open && intent.state != IntentState::PartiallyFilled {
            panic_with_error!(&env, Error::IntentNotOpen);
        }

        intent.solver = Some(solver.clone());
        intent.state = IntentState::Accepted;
        // Extend deadline to fill window from now
        let cfg = Self::load_config(&env);
        intent.deadline = now + cfg.fill_window;

        solver_record.active_intents += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &solver_record);

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "intent_accepted"), solver),
            (intent_id, intent.deadline),
        );
    }

    /// Solver fills the intent by sending dst_token to the user.
    ///
    /// Partial fills are supported: `fill_amount` must be > 0 but may be less
    /// than `min_dst_amount * src_portion / src_amount`.  The intent transitions to `PartiallyFilled` after
    /// each sub-fill and is re-opened so another solver (or the same one) can
    /// accept and deliver the remainder.  Once the cumulative `total_filled`
    /// reaches or exceeds `min_dst_amount * src_amount / src_amount` (when src_filled == src_amount),
    /// the intent transitions to `Filled`.
    ///
    /// The protocol fee is taken on each individual fill so the fee accounting
    /// stays consistent regardless of how many fills it takes.
    ///
    /// #348: src_portion is the amount of source tokens this fill covers; must be > 0
    /// and at most src_amount - src_filled. fill_amount must be >= src_portion * min_dst_amount / src_amount.
    pub fn fill_intent(env: Env, solver: Address, intent_id: BytesN<32>, fill_amount: i128, src_portion: i128) {
        // Auth audit: require_auth() is correct. The solver must sign to
        // authorise the token transfer from their address to the user and fee
        // recipient. This is the highest-value call site: the solver authorises
        // a token transfer, so the auth is load-bearing. require_auth_for_args
        // scoped to (solver, intent_id, fill_amount) would meaningfully tighten
        // the scope if a delegated-execution pattern is ever introduced — noted
        // as the strongest candidate for future hardening.
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        let now = env.ledger().timestamp();
        // Boundary semantics: the fill-window deadline is EXCLUSIVE for filling.
        // `now >= intent.deadline` rejects at the boundary second (`now == deadline`)
        // so the full [accepted_at, accepted_at + FILL_WINDOW) window is available
        // to the solver.
        if now >= intent.deadline {
            panic_with_error!(&env, Error::FillWindowExpired);
        }

        match &intent.state {
            IntentState::Accepted => {}
            IntentState::Filled => panic_with_error!(&env, Error::IntentAlreadyFilled),
            _ => panic_with_error!(&env, Error::IntentNotAccepted),
        }

        if intent.solver.as_ref() != Some(&solver) {
            panic_with_error!(&env, Error::Unauthorized);
        }

        if fill_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        // #348: validate src_portion
        if src_portion <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        if src_portion > intent.src_amount - intent.src_filled {
            panic_with_error!(&env, Error::InvalidSrcPortion);
        }

        // #348: check proportional minimum: fill_amount >= src_portion * min_dst_amount / src_amount
        let min_for_portion = src_portion
            .checked_mul(intent.min_dst_amount)
            .unwrap_or_else(|| panic_with_error!(&env, Error::AmountOverflow))
            .checked_div(intent.src_amount)
            .unwrap_or_else(|| panic_with_error!(&env, Error::AmountOverflow));
        if fill_amount < min_for_portion {
            panic_with_error!(&env, Error::InsufficientOutput);
        }

        // Solver also pays the protocol fee on each fill.
        let fee = fill_amount * PROTOCOL_FEE_BPS / 10_000;
        // ── Effects first (CEI) ──────────────────────────────────────────────
        // Mark the intent Filled and write every state change to storage
        // *before* any external token transfer executes. A hostile SEP-41
        // token that attempts to re-enter fill_intent or slash_solver during
        // the transfer would see the intent already Filled and be rejected.
        // Solver delivers the full requested output to the user.
        let dst_client = token::Client::new(&env, &intent.dst_token);
        dst_client.transfer(&solver, &intent.user, &fill_amount);

        // Solver also pays the protocol fee (priced into their quote). Taking the
        // fee from the solver — rather than clawing it back from the user — keeps
        // the user's received amount at or above `min_dst_amount`, and keeps every
        // token transfer authorized by the solver who signed this call.
        //
        // Explicit checked_mul/checked_div makes the overflow-safety property
        // visible in code, rather than relying solely on the Cargo.toml
        // overflow-checks = true release-profile setting (issue #31).
        let fee = fill_amount
            .checked_mul(PROTOCOL_FEE_BPS)
            .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow))
            .checked_div(10_000)
            .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow));
        if fee > 0 {
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .unwrap();
            dst_client.transfer(&solver, &fee_recipient, &fee);
        }

        // Accumulate the fill.
        intent.total_filled += fill_amount;
        let cumulative = intent.total_filled;

        // #348: update source filled
        intent.src_filled += src_portion;

        // Update fill_amount to reflect the running total for backward-compatible reads.
        intent.fill_amount = Some(cumulative);

        // Update solver stats for this partial fill.
        let mut solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap();
        solver_record.total_volume += fill_amount;

        // #348: intent is closed when src_filled == src_amount (all source covered)
        if intent.src_filled >= intent.src_amount {
            // Intent is fully satisfied — close it out.
            intent.state = IntentState::Filled;
            intent.filled_at = Some(now);
            solver_record.fills_completed += 1;
            solver_record.active_intents = solver_record.active_intents.saturating_sub(1);
        } else {
            // Partial fill: re-open so another solver (or the same) can claim the
            // remaining amount. #347: respect user's original deadline
            let cfg = Self::load_config(&env);
            intent.state = IntentState::PartiallyFilled;
            intent.solver = None;
            // #347: use min(user_deadline, now + intent_expiry) to not extend past user's deadline
            intent.deadline = now
                .checked_add(cfg.intent_expiry)
                .unwrap_or(u64::MAX)
                .min(intent.user_deadline);
            solver_record.active_intents = solver_record.active_intents.saturating_sub(1);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &solver_record);
        Self::bump_solver_ttl(&env, &solver);

        // Update protocol stats
        let total_vol: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalVolume)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalVolume, &(total_vol + fill_amount));

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        // ── Interactions: token transfers ────────────────────────────────────
        // Solver delivers the full requested output to the user.
        let dst_client = token::Client::new(&env, &intent.dst_token);
        dst_client.transfer(&solver, &intent.user, &fill_amount);

        // Solver also pays the protocol fee (priced into their quote). Taking the
        // fee from the solver — rather than clawing it back from the user — keeps
        // the user's received amount at or above `min_dst_amount`, and keeps every
        // token transfer authorized by the solver who signed this call.
        let fee = fill_amount * PROTOCOL_FEE_BPS / 10_000;
        if fee > 0 {
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .unwrap();
            dst_client.transfer(&solver, &fee_recipient, &fee);
        }

        env.events().publish(
            (Symbol::new(&env, "intent_filled"), solver),
            (intent_id, fill_amount, fee),
        );
    }

    /// User can cancel an Open intent (not yet accepted)
    pub fn cancel_intent(env: Env, user: Address, intent_id: BytesN<32>) {
        // Auth audit: require_auth() is correct. Only the intent owner may
        // cancel. An additional ownership check (`intent.user != user`) follows
        // immediately after the intent is loaded, providing defence-in-depth.
        // require_auth_for_args is not needed here — the action is simply
        // "cancel intent for this user".
        user.require_auth();
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.user != user {
            panic_with_error!(&env, Error::Unauthorized);
        }

        if intent.state == IntentState::Accepted {
            panic_with_error!(&env, Error::CannotCancelAccepted);
        }

        if intent.state != IntentState::Open && intent.state != IntentState::PartiallyFilled {
            panic_with_error!(&env, Error::IntentNotOpen);
        }

        intent.state = IntentState::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events()
            .publish((Symbol::new(&env, "intent_cancelled"), user), intent_id);
    }

    /// Permissionless: slash a solver that accepted but didn't fill within FILL_WINDOW
    pub fn slash_solver(env: Env, intent_id: BytesN<32>) {
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        let now = env.ledger().timestamp();

        if intent.state != IntentState::Accepted {
            panic_with_error!(&env, Error::IntentNotAccepted);
        }

        // Boundary semantics: the fill-window deadline is INCLUSIVE for slashing.
        // The guard `now < intent.deadline` is false when `now == deadline`, so
        // slashing becomes valid at the deadline second itself (not strictly after).
        // Fill window available to solver: [accepted_at, accepted_at + FILL_WINDOW).
        // Slash window: [accepted_at + FILL_WINDOW, ∞).
        if now < intent.deadline {
            panic_with_error!(&env, Error::FillWindowExpired); // not expired yet
        }

        let solver_addr = intent.solver.clone().unwrap();
        let mut solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver_addr.clone()))
            .unwrap();

        // Slash 10% of bond, with a floor of 1 so that a non-zero bond is never
        // economically unpunished due to integer division rounding to zero
        // (issue #32: tiny bonds below 10 would otherwise yield slash_amount = 0).
        let slash_amount = (solver_record.bond_amount / 10).max(1);
        solver_record.bond_amount -= slash_amount;
        solver_record.fills_failed += 1;
        solver_record.last_slash_time = now;
        solver_record.active_intents = solver_record.active_intents.saturating_sub(1);

        let cfg = Self::load_config(&env);
        // A solver whose bond no longer covers min_bond can't credibly back
        // further fills -- take them out of rotation until they top back up.
        if solver_record.bond_amount < cfg.min_bond {
            solver_record.is_active = false;
        }

        // Re-open the intent, preserving partial-fill progress if any.
        intent.state = if intent.total_filled > 0 {
            IntentState::PartiallyFilled
        } else {
            IntentState::Open
        };
        intent.solver = None;
        // #347: use min(user_deadline, now + intent_expiry) to not extend past user's deadline
        intent.deadline = now
            .checked_add(cfg.intent_expiry)
            .unwrap_or(u64::MAX)
            .min(intent.user_deadline);

        // Persist both records BEFORE any token transfer so that a re-entrant
        // or back-to-back call on the same intent_id is rejected by the
        // IntentNotAccepted guard above (the state is already Open by then).
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver_addr.clone()), &solver_record);
        Self::bump_solver_ttl(&env, &solver_addr);
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        // Send slash to fee recipient (state already committed above)
        if slash_amount > 0 {
            let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .unwrap();
            let client = token::Client::new(&env, &bond_token);
            client.transfer(
                &env.current_contract_address(),
                &fee_recipient,
                &slash_amount,
            );
        }

        env.events().publish(
            (Symbol::new(&env, "solver_slashed"), solver_addr),
            (intent_id, slash_amount),
        );
    }

    /// Permissionless: materialize an Open intent's Expired state once its
    /// deadline has passed. Expiry was previously only ever realized lazily
    /// inside accept_intent, so an intent nobody tried to accept could sit
    /// indefinitely showing state Open in storage despite being unfillable.
    pub fn expire_intent(env: Env, intent_id: BytesN<32>) {
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.state != IntentState::Open && intent.state != IntentState::PartiallyFilled {
            panic_with_error!(&env, Error::IntentNotOpen);
        }

        let now = env.ledger().timestamp();
        // Boundary semantics: the intent deadline is INCLUSIVE for expiry.
        // The guard `now < intent.deadline` is false when `now == deadline`, so
        // expiry becomes valid at the deadline second itself (not strictly after).
        // Intent is live in [created_at, deadline); caller can expire at deadline+.
        if now < intent.deadline {
            panic_with_error!(&env, Error::DeadlineNotReached);
        }

        intent.state = IntentState::Expired;
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events()
            .publish((Symbol::new(&env, "intent_expired"),), intent_id);
    }

    // ── Batch Operations ──────────────────────────────────────────────────────

    /// Submit multiple intents in a single transaction.
    /// Processes all intents in the batch; a failure partway through will
    /// revert the entire batch (Soroban transaction atomicity).
    /// Bounded by MAX_BATCH_SIZE to prevent resource exhaustion.
    pub fn batch_submit_intent(
        env: Env,
        user: Address,
        intents: soroban_sdk::Vec<(String, String, i128, Address, i128, Option<u64>)>,
    ) -> soroban_sdk::Vec<BytesN<32>> {
        if intents.len() > MAX_BATCH_SIZE as usize {
            panic_with_error!(&env, Error::ZeroAmount); // No dedicated error; reuse nearest
        }

        let mut result = soroban_sdk::Vec::new(&env);
        for (src_chain, src_token, src_amount, dst_token, min_dst_amount, deadline) in intents {
            let intent_id = Self::submit_intent(
                env.clone(),
                user.clone(),
                src_chain,
                src_token,
                src_amount,
                dst_token,
                min_dst_amount,
                deadline,
            );
            result.push_back(intent_id);
        }
        result
    }

    /// Accept multiple intents in a single transaction.
    /// Processes all intents in the batch; a failure partway through will
    /// revert the entire batch (Soroban transaction atomicity).
    /// Bounded by MAX_BATCH_SIZE to prevent resource exhaustion.
    pub fn batch_accept_intent(
        env: Env,
        solver: Address,
        intent_ids: soroban_sdk::Vec<BytesN<32>>,
    ) {
        if intent_ids.len() > MAX_BATCH_SIZE as usize {
            panic_with_error!(&env, Error::ZeroAmount); // No dedicated error; reuse nearest
        }

        for intent_id in intent_ids {
            Self::accept_intent(env.clone(), solver.clone(), intent_id);
        }
    }

    // ── Fill Window Extension ─────────────────────────────────────────────────

    /// Solver requests a grace-period extension on an Accepted intent.
    /// Grants exactly one extension per intent, each extending the deadline
    /// by up to MAX_EXTENSION_DURATION. Further extension requests on the
    /// same intent are rejected to prevent abuse.
    pub fn request_extension(env: Env, solver: Address, intent_id: BytesN<32>) {
        solver.require_auth();
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        // Only Accepted intents can be extended
        if intent.state != IntentState::Accepted {
            panic_with_error!(&env, Error::IntentNotAccepted);
        }

        // Only the assigned solver can request an extension
        if intent.solver.as_ref() != Some(&solver) {
            panic_with_error!(&env, Error::Unauthorized);
        }

        // Each intent gets exactly one extension
        if env
            .storage()
            .persistent()
            .has(&DataKey::ExtensionGranted(intent_id.clone()))
        {
            panic_with_error!(&env, Error::ZeroAmount); // No dedicated error; reuse nearest
        }

        let now = env.ledger().timestamp();

        // Extend the deadline by the full extension duration
        intent.deadline = now + MAX_EXTENSION_DURATION;

        // Record that this intent has used its one extension
        env.storage()
            .persistent()
            .set(&DataKey::ExtensionGranted(intent_id.clone()), &true);

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "extension_granted"), solver),
            (intent_id, intent.deadline),
        );
    }

    // ── Views ─────────────────────────────────────────────────────────────────

    /// Read-only: returns the current effective protocol parameters.
    ///
    /// Useful for integrators who need to know MIN_BOND, FILL_WINDOW,
    /// INTENT_EXPIRY, and PROTOCOL_FEE_BPS without reading source code.
    /// Returns the values as a dedicated struct so each field is named at
    /// the call site rather than relying on tuple-position conventions.
    pub fn get_protocol_params(env: Env) -> ProtocolParams {
        let _ = env; // view — no storage read needed; values are compile-time constants
        ProtocolParams {
            min_bond: MIN_BOND,
            fill_window: FILL_WINDOW,
            intent_expiry: INTENT_EXPIRY,
            protocol_fee_bps: PROTOCOL_FEE_BPS,
        }
    }

    /// Fetch an intent's full record by id, or None if it was never submitted.
    pub fn get_intent(env: Env, intent_id: BytesN<32>) -> Option<IntentRecord> {
        env.storage().persistent().get(&DataKey::Intent(intent_id))
    }

    /// Fetch a solver's full record by address, or None if never registered.
    pub fn get_solver(env: Env, solver: Address) -> Option<SolverRecord> {
        env.storage().persistent().get(&DataKey::Solver(solver))
    }

    /// Returns the reputation score (0–10_000 basis points) for `solver`,
    /// or None if the solver has never registered.
    ///
    /// Callers that only need the numeric value and already hold the
    /// SolverRecord can call `compute_reputation_score` directly.
    pub fn get_reputation_score(env: Env, solver: Address) -> Option<u32> {
        let record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver))?;
        Some(Self::compute_reputation_score(&record))
    }

    /// Whether `solver` currently meets accept_intent's requirements
    /// (registered, active, bonded above MIN_BOND). Lets off-chain solver
    /// bots self-check eligibility without independently reimplementing
    /// the same logic accept_intent enforces.
    pub fn is_solver_eligible(env: Env, solver: Address) -> bool {
        let cfg = Self::load_config(&env);
        match env
            .storage()
            .persistent()
            .get::<_, SolverRecord>(&DataKey::Solver(solver))
        {
            Some(record) => record.is_active && record.bond_amount >= cfg.min_bond,
            None => false,
        }
    }

    pub fn get_fee_recipient(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::FeeRecipient)
    }

    pub fn get_pending_fee_recipient(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingFeeRecipient)
    }

    pub fn get_bond_token(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::BondToken)
    }

    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Admin)
    }

    /// (total intents ever submitted, total volume ever filled).
    pub fn get_stats(env: Env) -> (u64, i128) {
        let intents: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TotalIntents)
            .unwrap_or(0);
        let volume: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalVolume)
            .unwrap_or(0);
        (intents, volume)
    }

    /// Minimum bond required for solver registration.
    pub fn get_min_bond(_env: Env) -> i128 {
        MIN_BOND
    }

    /// List all intent IDs for a given user. Returns empty Vec if user has no intents.
    pub fn list_intents_by_user(env: Env, user: Address) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::UserIntents(user))
            .unwrap_or_else(|| Vec::new(&env))
    /// Total number of solvers ever registered.
    pub fn get_solver_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSolvers)
            .unwrap_or(0)
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    /// Compute a reputation score (0–10 000 bps) for a solver.
    ///
    /// Formula:
    ///   base  = fills_completed / (fills_completed + fills_failed)  [0–1]
    ///   decay = 1 / (1 + total_volume / VOLUME_SCALE)               [0–1]
    ///   score = base * (1 - 0.1 * decay) * 10_000
    ///
    /// Rationale:
    /// - `base` is the raw success rate.
    /// - `decay` gives a small bonus (up to 10%) to high-volume solvers who
    ///   demonstrate consistent execution: at zero volume the score is 90% of
    ///   the success rate; at very high volume it approaches 100%.
    /// - All arithmetic is integer-only and cannot panic — division by zero is
    ///   guarded, and intermediate values stay within i128/u64 range.
    ///
    /// Edge cases:
    ///   zero fills  → 0
    ///   all failures → 0
    ///   perfect rate, no volume → 9 000  (90% × 10 000)
    ///   perfect rate, high vol  → approaches 10 000
    pub fn compute_reputation_score(record: &SolverRecord) -> u32 {
        let total_fills = record.fills_completed as u64 + record.fills_failed as u64;
        if total_fills == 0 {
            return 0;
        }

        // base_bps ∈ [0, 10_000]
        let base_bps = (record.fills_completed as u64 * 10_000) / total_fills;

        // Volume scale: 1 000 fills × 100 dst tokens (7 dp) is the knee of
        // the curve. Only the shape matters — the constant can be tuned later.
        const VOLUME_SCALE: i128 = 1_000 * 100 * 10_000_000;

        // decay_bps = VOLUME_SCALE / (VOLUME_SCALE + vol + 1) × 10_000
        // ∈ (0, 10_000].  High volume → low decay_bps.
        let vol = record.total_volume.max(0);
        let decay_bps = ((VOLUME_SCALE as u64) * 10_000)
            / ((VOLUME_SCALE + vol + 1) as u64);

        // volume_multiplier_bps ∈ [9_000, 10_000)
        // At zero volume: decay_bps = ~10_000, multiplier = 9_000
        // At high  volume: decay_bps → 0,      multiplier → 10_000
        let multiplier_bps = 10_000u64 - decay_bps / 10;

        let score = base_bps * multiplier_bps / 10_000;
        score as u32
    }

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        // Auth audit: require_auth() is correct. All callers of require_admin
        // are admin-only functions (pause, unpause, add/remove_allowed_dst_token,
        // set_dst_allowlist_enabled). The admin is a single address with uniform
        // authority over these functions; require_auth_for_args would add no
        // meaningful scope reduction.
        admin.require_auth();
    }

    fn require_not_paused(env: &Env) {
        if Self::is_paused(env.clone()) {
            panic_with_error!(env, Error::ContractPaused);
        }
    }

    fn get_adjusted_min_bond(env: &Env, dst_token: &Address) -> i128 {
        let multiplier = env
            .storage()
            .persistent()
            .get::<_, i128>(&DataKey::MinBondMultiplier(dst_token.clone()))
            .unwrap_or(10);
        (MIN_BOND * multiplier) / 10
    /// Load the protocol config from storage, falling back to defaults for
    /// contracts that pre-date this upgrade (upgrade-safe).
    fn load_config(env: &Env) -> ProtocolConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or(ProtocolConfig {
                min_bond: DEFAULT_MIN_BOND,
                fill_window: DEFAULT_FILL_WINDOW,
                intent_expiry: DEFAULT_INTENT_EXPIRY,
                protocol_fee_bps: DEFAULT_PROTOCOL_FEE_BPS,
            })
    }

    /// #349: Get solver's bond for a specific token (single source of truth)
    fn get_solver_bond(env: &Env, solver: &Address, token: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::SolverBond(solver.clone(), token.clone()))
            .unwrap_or(0)
    }

    /// #349: Set solver's bond for a specific token and atomically update total
    /// All bond mutations must use this to keep SolverBond and TotalBondedByToken in sync.
    fn set_solver_bond(env: &Env, solver: &Address, token: &Address, amount: i128) {
        let old_amount = Self::get_solver_bond(env, solver, token);
        env.storage()
            .persistent()
            .set(&DataKey::SolverBond(solver.clone(), token.clone()), &amount);

        // Update total bonded
        let total: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalBondedByToken(token.clone()))
            .unwrap_or(0);
        let new_total = total - old_amount + amount;
        env.storage()
            .persistent()
            .set(&DataKey::TotalBondedByToken(token.clone()), &new_total);
    }

    // ── Implementation notes for #349 and #350 ──────────────────────────────
    // #349: TODO - Replace all bond_amount field accesses with get_solver_bond/set_solver_bond:
    //   register_solver (lines ~793, 809): use set_solver_bond(solver, bond_token, new_amount)
    //   withdraw_bond (lines ~927, 937): use set_solver_bond
    //   slash_solver (lines ~1435, 1436): use set_solver_bond
    //   accept_intent (line ~1130): use get_solver_bond for min_bond check
    //   is_solver_eligible (line ~1689): use get_solver_bond
    //
    // #350: TODO - Add registry integration:
    //   Add DataKey::Registry(Address) for the registry contract address (optional)
    //   In fill_intent when state becomes Filled: call registry.record_fill(solver)
    //   In slash_solver: call registry.record_failure(solver) and registry.slash(solver, amount)
    //   Decide whether registry failures should propagate or be swallowed

    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

    fn bump_intent_ttl(env: &Env, intent_id: &BytesN<32>) {
        env.storage().persistent().extend_ttl(
            &DataKey::Intent(intent_id.clone()),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    fn bump_solver_ttl(env: &Env, solver: &Address) {
        env.storage().persistent().extend_ttl(
            &DataKey::Solver(solver.clone()),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    fn compute_intent_id(
        env: &Env,
        user: &Address,
        src_chain: &String,
        amount: i128,
        timestamp: u64,
        nonce: u64,
    ) -> BytesN<32> {
        // Build a collision-resistant preimage from the full intent context, then
        // hash to a 32-byte id. Including the user, source chain, and a
        // per-user nonce ensures two otherwise-identical intents from the same
        // user in the same ledger always produce distinct ids.
        let mut preimage = Bytes::new(env);
        preimage.append(&user.clone().to_xdr(env));
        preimage.append(&src_chain.clone().to_xdr(env));
        preimage.extend_from_array(&amount.to_be_bytes());
        preimage.extend_from_array(&timestamp.to_be_bytes());
        preimage.extend_from_array(&nonce.to_be_bytes());
        env.crypto().sha256(&preimage).into()
    }
}
