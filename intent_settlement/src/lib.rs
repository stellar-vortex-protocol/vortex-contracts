#![no_std]

//! Vortex Protocol — Cross-Chain Intent Settlement
//!
//! Users submit swap intents (e.g. "swap 1 ETH on Ethereum for ~3500 USDC on Stellar").
//! Solvers compete to fill these intents off-chain, then settle on-chain via this contract.
//! Settlement is guaranteed by a solver bond; failing to fill within the deadline slashes the bond.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, xdr::ToXdr,
    Address, Bytes, BytesN, Env, IntoVal, String, Symbol, Vec,
};

/// Cross-contract client for the `ProofRegistry` contract (issue #190).
/// Used only on the `fill_intent(..., require_proof = true)` path.
use vortex_proof_registry::ProofRegistryClient;

#[cfg(test)]
mod test;

#[cfg(test)]
mod proptest_bond;

#[cfg(test)]
mod bench;

// ─── Constants ────────────────────────────────────────────────────────────────

const INTENT_EXPIRY: u64 = 1800; // 30 minutes
const FILL_WINDOW: u64 = 300; // 5 minutes to fill after intent accepted
const MIN_BOND: i128 = 50 * 10_000_000; // 50 USDC minimum solver bond
const PROTOCOL_FEE_BPS: i128 = 5; // 0.05%

/// Baseline slash rate in basis points (1 000 bps = 10%).
///
/// Issue #193: `slash_solver` no longer slashes a flat 10% of the bond.
/// Instead it slashes `min(intent_value, bond) / 10` — an amount proportional
/// to the size of the intent the solver failed to fill — and then *caps* the
/// result at `bond * SLASH_BPS / 10_000` so a slash is never more punitive
/// than the old flat-10% baseline for a well-matched bond-to-intent ratio.
/// The floor of 1 stroop (issue #32) is preserved so a non-zero bond is
/// always economically punished.
const SLASH_BPS: i128 = 1_000; // 10%

/// Issue #188 — dispute-resolution flow (docs/dispute-resolution-design.md).
///
/// `DISPUTE_WINDOW` is the period, starting at `begin_fill`, during which the
/// user may contest a fill via `dispute_fill`.  Output tokens sit in contract
/// escrow for its full duration; once it elapses with no dispute anyone may
/// call `release_fill` to pay the user and close the intent.
const DISPUTE_WINDOW: u64 = 3_600; // 1 hour

/// Issue #188 — after a dispute is raised the arbiter has this long to call
/// `resolve_dispute`.  If it elapses unresolved, `release_fill` becomes a
/// permissionless timeout that releases the escrow to the user (the
/// conservative default from the design doc) without slashing the solver.
const ARBITER_WINDOW: u64 = 86_400; // 24 hours

/// Issue #187 — a solver may hold bonds in at most this many distinct
/// approved tokens.  Bounds the work done by `deregister_solver` (which must
/// refund every token) and the storage cost of the per-token bond entries.
const MAX_BOND_TOKENS: u32 = 8;

/// Dispute-resolution parameters (issue #48, #233):
/// When a solver delivers tokens (begin_fill), the user has DISPUTE_WINDOW seconds
/// to open a dispute. If no dispute is raised, release_fill() can execute after
/// the window closes. If a dispute is raised, the arbiter has ARBITER_WINDOW
/// seconds to resolve it; if unresolved, the timeout releases escrow to the user.
const DISPUTE_WINDOW: u64 = 3600; // 1 hour: time for user to notice and contest fill
const ARBITER_WINDOW: u64 = 86400; // 24 hours: time for arbiter to resolve
const DISPUTE_BOND: i128 = 1 * 10_000_000; // 1 USDC: anti-griefing bond from user

/// Upper bound on the number of intent IDs `list_open_intents` returns per
/// call (issue #249), bounding the resource cost of paginated reads.
const MAX_PAGE_SIZE: u32 = 100;

/// After being slashed a solver must wait this many seconds before they can
/// accept new intents. Used by `accept_intent`'s cooldown guard and by
/// `get_slash_cooldown_remaining` (issue #256), which both derive from the
/// same `slash_cooldown_remaining` helper so they can never disagree.
const SLASH_COOLDOWN: u64 = 3600; // 1 hour

/// Upper bound on the number of `src_chain`/`dst_token` entries a solver may
/// declare via `set_solver_routes` (issue #255), to keep per-solver route
/// storage bounded.
const MAX_ROUTE_ENTRIES: u32 = 20;

/// Delay enforced between proposing and executing a sensitive admin change
/// (admin transfer, fee recipient handover, dst_token allowlist changes).
/// Gives users and solvers a window to notice and react before the change
/// takes effect (#115). Proposing also emits a distinct event immediately,
/// so off-chain monitors get advance notice even before the delay elapses
/// (#116).
const ADMIN_TIMELOCK_DELAY: u64 = 172_800; // 48 hours

// ── Defaults seeded into `ProtocolConfig` by `initialize`, and the fallback
// `load_config` returns for contracts deployed before the configurable-params
// feature existed.  They mirror the historical compile-time constants above.
const DEFAULT_MIN_BOND: i128 = MIN_BOND;
const DEFAULT_FILL_WINDOW: u64 = FILL_WINDOW;
const DEFAULT_INTENT_EXPIRY: u64 = INTENT_EXPIRY;
const DEFAULT_PROTOCOL_FEE_BPS: i128 = PROTOCOL_FEE_BPS;

// ── `set_config` bounds.  A parameter outside any of these ranges is rejected
// with `Error::InvalidConfig`.
const MAX_PROTOCOL_FEE_BPS: i128 = 1_000; // 10% hard cap on the protocol fee
const MIN_FILL_WINDOW_SECS: u64 = 60; // a solver needs at least a minute to fill
const MIN_INTENT_EXPIRY_SECS: u64 = 300; // and must always exceed the fill window
const MIN_BOND_FLOOR: i128 = 10_000_000; // one 7-decimal USDC unit

// ── Cooldowns / limits enforced outside `ProtocolConfig`.
const SLASH_COOLDOWN: u64 = 3600; // 1 hour a slashed solver must wait before accepting again
const CANCEL_COOLDOWN: u64 = 3600; // 1 hour between a user's successive intent cancellations
const MAX_EXTENSION_DURATION: u64 = 300; // one extra fill window granted by `request_extension`

// ── Storage-migration schema version (#194). Bumped whenever a `migrate()`
// body is added for a new release; `initialize` stamps fresh deploys with the
// current value and `migrate` refuses to run once the contract is already at
// it, so a migration can never be applied twice.
const MIGRATION_VERSION: u32 = 1;

// Basis-points denominator, shared by the protocol fee and the #192 discount
// schedule (`discount_bps` is a fraction of the fee, not of the fill).
const BPS_DENOMINATOR: i128 = 10_000;

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

const MAX_BATCH_SIZE: u32 = 100;
const MAX_EXTENSION_DURATION: u64 = 600; // 10 minutes

const DEFAULT_MIN_BOND: i128 = MIN_BOND;
const DEFAULT_FILL_WINDOW: u64 = FILL_WINDOW;
const DEFAULT_INTENT_EXPIRY: u64 = INTENT_EXPIRY;
const DEFAULT_PROTOCOL_FEE_BPS: i128 = PROTOCOL_FEE_BPS;

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

// ─── Default protocol parameters (#202) ──────────────────────────────────────
//
// `initialize` seeds `DataKey::Config` with these, and `load_config` falls
// back to them for deployments that pre-date the configurable-params upgrade.
// They are defined as aliases of the historical compile-time constants above
// so moving to a stored `ProtocolConfig` changes no observable behaviour — a
// freshly initialized contract behaves exactly as it did when the parameters
// were hard-coded.
const DEFAULT_MIN_BOND: i128 = MIN_BOND; // 50 USDC
const DEFAULT_FILL_WINDOW: u64 = FILL_WINDOW; // 300 s
const DEFAULT_INTENT_EXPIRY: u64 = INTENT_EXPIRY; // 1800 s
const DEFAULT_PROTOCOL_FEE_BPS: i128 = PROTOCOL_FEE_BPS; // 5 bps (0.05%)

// ─── `set_config` bounds (#202) ──────────────────────────────────────────────
//
// Guard rails enforced by `set_config` so an admin cannot move a parameter to
// an economically unsafe value. Values match the bounds already documented in
// `set_config`'s own doc comment.
const MAX_PROTOCOL_FEE_BPS: i128 = 1_000; // 10% — hard ceiling on the protocol fee
const MIN_FILL_WINDOW_SECS: u64 = 60; // a solver needs at least a minute to deliver a fill
const MIN_INTENT_EXPIRY_SECS: u64 = 300; // an intent must stay live for at least five minutes
const MIN_BOND_FLOOR: i128 = 10_000_000; // 1 USDC (7 decimals) — absolute floor for `min_bond`

// ─── Cooldowns (#202) ────────────────────────────────────────────────────────

/// Seconds a solver must wait after being slashed before `accept_intent` will
/// let it take on a new intent. Long enough to blunt a griefing loop where a
/// solver repeatedly accepts and abandons intents, short enough that an honest
/// solver that hit one bad fill window recovers within the hour.
const SLASH_COOLDOWN: u64 = 3_600; // 1 hour

/// Minimum gap the same user must leave between `cancel_intent` calls. Deters
/// cancel spam (e.g. submit → cancel loops used to grief solvers mid-quote)
/// without getting in the way of a user correcting a single mistaken intent.
const CANCEL_COOLDOWN: u64 = 60; // 1 minute

// ─── Batch + extension limits (#202) ─────────────────────────────────────────

/// Upper bound on the number of items any `batch_*` entrypoint processes in a
/// single call. Keeps the worst-case resource cost (and therefore fee) of one
/// transaction bounded regardless of caller input. 20 covers realistic solver
/// batching while staying well inside Soroban's per-transaction limits.
const MAX_BATCH_SIZE: u32 = 20;

/// Longest additional time `request_extension` can add to an Accepted intent's
/// deadline. One extension is allowed per intent; this is the same order of
/// magnitude as `FILL_WINDOW` so a single extension can at most roughly double
/// the solver's delivery window.
const MAX_EXTENSION_DURATION: u64 = 300; // 5 minutes

// ─── Solver-registry tier perks (#197) ──────────────────────────────────────
//
// Index = tier number (0 Unranked … 4 Platinum). These MUST stay in lock-step
// with `solver_registry`'s tier table and `docs/solver-registry-design.md`
// §3/§6/§7. They are held here, rather than fetched per call, so
// `accept_intent` / `slash_solver` make at most one cross-contract call each
// (just `get_tier`) on their hot paths. A change to these values is a
// protocol-parameter change.

/// Fill-window extension bonus per tier, in basis points (10_000 = +100%).
/// Unranked +0%, Bronze +10%, Silver +20%, Gold +30%, Platinum +50%.
const TIER_FILL_WINDOW_BONUS_BPS: [u64; 5] = [0, 1_000, 2_000, 3_000, 5_000];

/// Slash percentage per tier, in basis points of the bond (10_000 = 100%).
/// Unranked/Bronze 10%, Silver 8%, Gold 6%, Platinum 5% — and 5% (500 bps) is
/// the floor for every tier.
const TIER_SLASH_BPS: [i128; 5] = [1_000, 1_000, 800, 600, 500];

/// Lowest slash rate any tier may receive, in basis points (Platinum's 5%).
const MIN_SLASH_BPS: i128 = 500;

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// **Instance storage.** The admin `Address` that may call privileged
    /// functions (`pause`, `unpause`, `propose_fee_recipient`,
    /// `propose_admin_transfer`, `propose_add_dst_token`, etc.).  Written
    /// once by `initialize` and rotated by `accept_admin_transfer`.  Lives as
    /// long as the contract instance.
    Admin,

    /// **Instance storage.** The `Address` that receives protocol fees
    /// (collected in `fill_intent`) and slashed bond amounts (collected in
    /// `slash_solver`).  Written by `initialize` and updated by
    /// `set_fee_recipient`.  Lives as long as the contract instance.
    FeeRecipient,
    /// Proposed-but-not-yet-accepted new fee recipient plus the ledger
    /// timestamp at which `accept_fee_recipient` may execute it (issue #30,
    /// timelock added by #115): `(Address, u64)`.
    PendingFeeRecipient,

    /// **Instance storage.** Proposed-but-not-yet-accepted new admin plus the
    /// ledger timestamp at which `accept_admin_transfer` may execute it
    /// (#115/#116): `(Address, u64)`. Cleared once the handover completes.
    PendingAdmin,

    /// **Instance storage.** The stored `ProtocolConfig` (min bond, fill
    /// window, intent expiry, protocol fee bps). Seeded by `initialize` and
    /// replaced atomically by `set_config`. `load_config` falls back to the
    /// `DEFAULT_*` constants when this key is absent (pre-upgrade safety).
    Config,

    BondToken,          // USDC address for bonds
    Intent(BytesN<32>), // intent_id -> IntentRecord

    /// **Persistent storage.** Bounded on-chain fill-history log for a given
    /// intent (issue #244): `Vec<(solver, amount, timestamp)>`, oldest first,
    /// capped at `MAX_FILL_HISTORY` entries with FIFO eviction of the oldest
    /// entry once the cap is reached. Appended to by `fill_intent`.
    IntentFillHistory(BytesN<32>),
    Solver(Address),    // address -> SolverRecord

    /// **Instance storage.** All currently-registered solver addresses
    /// (`Vec<Address>`), kept in sync by `register_solver` (append if absent)
    /// and `deregister_solver` (remove). Backs the paginated `list_solvers`
    /// view (#198) so integrators and dashboards can enumerate solvers without
    /// replaying every `solver_registered` / `solver_deregistered` event.
    /// Mirror of the `AllowedDstTokenList` pattern used for the dst_token
    /// allowlist (#117).
    ///
    /// **Trade-off (#198):** like `OpenIntents`, this counter-style structure
    /// lives in the instance entry that is already loaded on every call, so
    /// `register_solver` / `deregister_solver` pay only one extra Vec
    /// read+write — negligible next to the persistent `SolverRecord` I/O they
    /// already do, and neither is a hot path (unlike `accept_intent` /
    /// `fill_intent`, which never touch this key). The cost that *does* scale
    /// is the size of this single entry: it grows O(n) with the
    /// registered-solver count, and the instance entry is deserialized on
    /// every contract call. That is comfortably fine into the low thousands of
    /// solvers; well beyond that, the enumeration should move to a chunked or
    /// paged persistent layout so the per-call instance load stays flat. The
    /// alternative — no on-chain enumeration — forces every integrator to
    /// replay the full `solver_registered` / `solver_deregistered` event
    /// history, which is O(events) and needs an archival node.
    SolverList,

    TotalIntents,

    /// **Instance storage.** Count of intents currently in `Open` or
    /// `PartiallyFilled` state (`u64`).  Incremented by `submit_intent` and
    /// by `slash_solver` (which re-opens the intent).  Decremented by
    /// `accept_intent`, `cancel_intent`, `expire_intent`, and `fill_intent`
    /// (only on a full fill that closes the intent).
    ///
    /// Trade-off (#109): maintaining this counter on-chain costs one extra
    /// instance-storage read+write on every state-changing call but gives
    /// dashboards an O(1) open-intent count without replaying events.  The
    /// alternative — leaving the computation entirely to indexers — is cheaper
    /// on-chain but forces every dashboard to run a full event replay.  Given
    /// that the counter sits in instance storage (one ledger entry, already
    /// loaded on every call) the marginal cost is a single integer increment/
    /// decrement, which is negligible compared to the persistent-storage reads
    /// for `IntentRecord` and `SolverRecord`.
    OpenIntents,

    /// **Instance storage.** Cumulative `dst_token` volume (`i128`) across
    /// all successfully filled intents.  Incremented by `fill_intent`.
    TotalVolume,

    /// **Instance storage.** Cumulative protocol fee revenue (`i128`)
    /// collected across all fills (issue #248). Incremented by `fill_intent`
    /// with the same `fee` value transferred to `FeeRecipient`, so it can
    /// never drift from real transferred amounts. Absent until the first
    /// fill after this field was introduced; `unwrap_or(0)` handles that.
    TotalFeesCollected,

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

    /// **Instance storage.** Enumerable mirror of the `AllowedDstToken`
    /// presence flags (`Vec<Address>`), maintained by
    /// `add_to_dst_token_list` / `remove_from_dst_token_list`. Backs
    /// `list_allowed_dst_tokens` (#117).
    AllowedDstTokenList,

    /// **Persistent storage.** Per-`dst_token` bond multiplier (`i128`, where
    /// `10` = 1.0×). Set by `set_min_bond_multiplier`; consulted by
    /// `get_adjusted_min_bond` in `accept_intent`. Absent ⇒ 1.0×.
    MinBondMultiplier(Address),

    /// **Persistent storage.** All intent ids ever submitted by a given user
    /// (`Vec<BytesN<32>>`), appended by `submit_intent`. Backs
    /// `list_intents_by_user`.
    UserIntents(Address),

    /// **Persistent storage.** Ledger timestamp of a user's most recent
    /// `cancel_intent` (`u64`). Enforces `CANCEL_COOLDOWN` between cancels.
    CancelCooldown(Address),

    /// **Persistent storage.** Presence flag (`true`) recording that an intent
    /// has already used its single permitted `request_extension`.
    ExtensionGranted(BytesN<32>),

    UserNonce(Address),      // per-user submit counter to widen intent_id preimage
    AllowedSrcChain(String), // src_chain name -> present if allowed
    SrcChainAllowlistEnabled,

    /// **Instance storage.** Pending `propose_add_dst_token` proposal: maps a
    /// candidate `dst_token` to the ledger timestamp (`u64`) at which
    /// `execute_add_dst_token` may apply it (#118).
    PendingDstTokenAdd(Address),

    /// **Instance storage.** Pending `propose_remove_dst_token` proposal: maps
    /// a `dst_token` to the ledger timestamp (`u64`) at which
    /// `execute_remove_dst_token` may apply it (#118).
    PendingDstTokenRemove(Address),

    /// **Instance storage.** The `Address` authorized to call `pause` in
    /// addition to `Admin` (issue #120). Lets an operator hand a hot key to
    /// an incident-response process without exposing the admin key that
    /// also controls fee routing and admin transfer. Absent until the admin
    /// calls `set_pauser`, in which case `pause` remains admin-only.
    /// `unpause` is intentionally *not* reachable via this role (narrow
    /// unpause access) -- resuming the protocol always needs the full
    /// admin's judgment.
    Pauser,

    /// **Instance storage.** `Address` of the deployed `ProofRegistry`
    /// contract (issue #190, docs/124 §4.1). Absent until the admin calls
    /// `set_proof_registry`. Only read by `fill_intent` when it is invoked
    /// with `require_proof = true`; a `require_proof = false` fill never
    /// touches this key, so proof-gating is fully opt-in and defaults off
    /// exactly like `DstAllowlistEnabled`.
    ProofRegistry,
}

// ─── Data Structs ─────────────────────────────────────────────────────────────

/// Admin-configurable protocol parameters.  Stored as a single instance-storage
/// entry so all values are read/written atomically.
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
    /// Maximum number of intents a single solver may accept simultaneously (issue #230).
    pub max_active_intents_per_solver: u32,
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
    pub min_dst_amount: i128, // minimum acceptable output per fill (floor per partial)

    pub solver: Option<Address>, // assigned solver
    pub state: IntentState,

    pub created_at: u64,
    pub deadline: u64,
    pub filled_at: Option<u64>,
    pub fill_amount: Option<i128>, // cumulative dst tokens received across all fills

    /// Issue #187: the approved bond token that backs this intent's fill
    /// guarantee.  Set to the solver's chosen token in `accept_intent` /
    /// `settle_bids` and consulted by `slash_solver` so the slash is taken
    /// from — and paid out in — the same token the solver actually bonded.
    /// Defaults to the legacy `DataKey::BondToken` for intents that never
    /// reach an `Accepted`-family state.
    pub bond_token: Address,

    /// Issue #188: end of the escrow/dispute window, set by `begin_fill`.
    /// `None` for intents that took the legacy one-shot `fill_intent` path.
    pub dispute_deadline: Option<u64>,
    /// Issue #188: timestamp `dispute_fill` was called, if a dispute is open.
    pub dispute_raised_at: Option<u64>,
    /// Issue #188: the arbiter's decision once `resolve_dispute` has run.
    pub resolution: Option<DisputeResolution>,

    /// Cumulative dst tokens delivered so far; intent completes when this
    /// reaches or exceeds `min_dst_amount * num_fills_needed`, but in the
    /// partial-fill model the intent is fully settled once the solver
    /// delivering a fill brings `total_filled` to at least `min_dst_amount`.
    ///
    /// More precisely: each individual partial fill must be > 0, and the
    /// intent transitions to `Filled` as soon as `total_filled` satisfies
    /// the user's `min_dst_amount` requirement.
    pub total_filled: i128,

    /// #197: the `solver_registry` tier the assigned solver held when they
    /// called `accept_intent` — snapshotted so `slash_solver` applies the
    /// slash rate that was in force when the obligation was taken on, not the
    /// solver's tier now. `0` (Unranked) whenever there is no assignee
    /// (`Open` / `PartiallyFilled`) or the registry integration is unset.
    pub solver_tier: u32,
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
    /// Reserved for a future competitive bid-collection mode (not currently
    /// produced by any entrypoint — `submit_intent` always opens intents in
    /// `Open`).
    Bidding,
    /// Issue #188: solver has called `begin_fill`; the output tokens are held
    /// in contract escrow and the user has until `dispute_deadline` to contest
    /// via `dispute_fill`.  `release_fill` moves this to `Filled` once the
    /// window closes without a dispute.
    Filling,
    /// Issue #188: the user contested the fill during the dispute window.
    /// Escrow is frozen until the arbiter calls `resolve_dispute` (or the
    /// `ARBITER_WINDOW` timeout releases it to the user).
    Disputed,
    /// Issue #188: the arbiter (or the arbiter-timeout) closed a dispute.
    /// The outcome is recorded in `IntentRecord.resolution`.
    Resolved,
}

/// Issue #188: the arbiter's ruling on a disputed fill.
/// docs/dispute-resolution-design.md — in both outcomes the user receives the
/// escrowed tokens; the ruling only decides whether the solver is slashed.
#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum DisputeResolution {
    /// Arbiter sided with the user: escrow goes to the user and the solver's
    /// bond is slashed by the same proportional formula `slash_solver` uses.
    Upheld,
    /// Arbiter sided with the solver: escrow still goes to the user (the fill
    /// was delivered) but no slash is applied and the protocol fee is taken.
    Dismissed,
}

/// A registered solver (market maker)
#[contracttype]
#[derive(Clone)]
pub struct SolverRecord {
    pub address: Address,
    /// Legacy scalar bond, denominated in the original `DataKey::BondToken`.
    ///
    /// Issue #187 introduced per-token bonds stored under
    /// `DataKey::SolverBond(solver, token)`.  This field is retained as the
    /// mirror of that entry *for the default bond token only*, so every
    /// pre-#187 reader (`get_solver`, `is_solver_eligible`, the bond-conservation
    /// proptest) keeps working unchanged.  Bonds in any other approved token
    /// live solely in `SolverBond` and are enumerated via `bond_tokens`.
    pub bond_amount: i128,
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
    /// Issue #187: every approved token this solver currently holds a non-zero
    /// bond in, including the default token.  Bounded by `MAX_BOND_TOKENS`.
    /// `deregister_solver` walks this list to refund every token in one call.
    pub bond_tokens: Vec<Address>,
}

/// Reputation fields preserved across a `deregister_solver` / `register_solver`
/// cycle for the same solver address (#272).
///
/// When `deregister_solver` runs it writes this snapshot to
/// `DataKey::SolverReputation(address)` *before* deleting the `SolverRecord`.
/// When `register_solver` runs for the same address it reads this snapshot (if
/// present) and carries the fields forward into the new `SolverRecord`, then
/// removes the snapshot.
///
/// Only the fields that matter for cooldown enforcement and reputation scoring
/// are preserved — `bond_amount`, `active_intents`, `registered_at`, and
/// `is_active` are intentionally reset (the solver is starting a new bonding
/// period; they must re-post bond and are active again from the moment of
/// re-registration).
#[contracttype]
#[derive(Clone)]
pub struct ReputationSnapshot {
    /// Timestamp of the most recent slash event, carried forward so that the
    /// `SLASH_COOLDOWN` guard in `accept_intent` remains effective even after
    /// a deregister/re-register cycle.
    pub last_slash_time: u64,
    /// Cumulative successful fills; preserved so `compute_reputation_score`
    /// reflects the solver's true track record.
    pub fills_completed: u32,
    /// Cumulative failed fills (missed windows); preserved to prevent solvers
    /// from wiping a bad fill ratio by cycling through deregister/re-register.
    pub fills_failed: u32,
    /// Cumulative dst-token volume delivered across all fills; preserved so
    /// the volume-based bonus in `compute_reputation_score` cannot be reset.
    pub total_volume: i128,
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
    /// by `require_admin` and by `propose_fee_recipient` /
    /// `propose_admin_transfer`.
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

    /// Duplicate `intent_id` detected in `submit_intent` (hash collision guard).
    IntentAlreadyExists = 22,
    /// #31: fee arithmetic overflowed (fill_amount is astronomically large)
    FeeOverflow = 23,
    /// #33: the address passed to `propose_add_dst_token` doesn't implement SEP-41
    InvalidTokenInterface = 24,
    /// #30: `accept_fee_recipient` was called with no pending fee-recipient
    /// proposal in storage.
    NoPendingFeeRecipient = 25,
    /// #34: `submit_intent` was called with a `src_chain` that is not on the
    /// allowlist while `SrcChainAllowlistEnabled` is `true`.
    SrcChainNotAllowed = 26,
    /// #35: `rescue_tokens` was called for the bond token, which the rescue
    /// path is not allowed to move.
    RescueProtectedToken = 27,
    /// #127: `submit_intent` was called with a `src_token` whose format does
    /// not match the conventions of the declared `src_chain`.
    ///
    /// EVM chains (ethereum, base, polygon, arbitrum, optimism): expect a
    /// `0x`-prefixed 42-character hex string (e.g. `"0xA0b86991…"`).
    ///
    /// Solana: expects a base58 string between 32 and 44 characters long with
    /// no `0x` prefix.
    ///
    /// If `src_chain` is unknown this error is never raised — unknown chains
    /// bypass token-format validation so the allowlist remains the sole gate.
    InvalidSrcToken = 28,

    // ── Proof-gated fills (issue #190, docs/129-proof-mismatch-fallback.md) ──
    //
    // docs/129 §6 assigns these the logical codes 24–27, but 24 is already
    // taken in this enum (`InvalidTokenInterface`) and 22/23 currently carry
    // duplicate discriminants from earlier merges. To avoid making that worse
    // these use a fresh contiguous block; the doc's semantic mapping is noted
    // on each variant.
    /// docs/129 §2.4 (code 24). `fill_intent` was called with
    /// `require_proof = true` but no `DataKey::ProofRegistry` has been set by
    /// the admin. Configuration error — no slash, intent stays `Accepted`.
    ProofRegistryNotSet = 30,
    /// docs/129 §2.3 (code 25). `ProofRegistry.get_proof(intent_id)` returned
    /// `None`: no verified source-chain deposit for this intent yet. Intent
    /// stays `Accepted`; the solver should retry once the VAA is relayed, or
    /// be slashed if the fill window elapses first.
    ProofNotFound = 31,
    /// docs/129 §2.2 (code 26). The proof's `src_chain_id` does not equal the
    /// Wormhole chain ID mapped from `intent.src_chain`. Hard reject; intent
    /// stays `Accepted`.
    ProofChainMismatch = 32,
    /// docs/129 §2.1 (code 27). `proof.src_amount < intent.src_amount` — the
    /// source deposit was smaller than the intent requires. Hard reject;
    /// intent stays `Accepted` so the solver can supply a corrected VAA or be
    /// slashed at deadline.
    ProofAmountInsufficient = 33,
    /// docs/129 §4. `intent.src_chain` is not in the canonical
    /// chain-name → Wormhole-chain-ID table, so the proof's chain cannot be
    /// validated against it.
    SrcChainNotSupported = 34,
    /// #281: `submit_intent` was called with a `referrer` equal to the
    /// submitting `user`.  Self-referral is rejected to prevent a user from
    /// gaming the referral programme by naming their own address.
    SelfReferral = 35,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct IntentSettlement;

#[contractimpl]
impl IntentSettlement {
    // ── Initialization ────────────────────────────────────────────────────────

    /// One-time contract setup. Records the `admin`, `fee_recipient`, and
    /// `bond_token` (USDC) addresses, seeds protocol stats to zero, writes the
    /// default `ProtocolConfig`, and extends the instance TTL.
    /// Panics with `AlreadyInitialized` if called a second time.
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
        env.storage().instance().set(&DataKey::OpenIntents, &0u64);
        env.storage().instance().set(&DataKey::TotalBonded, &0i128);
        // Seed Config with defaults so the contract is immediately usable
        // without a follow-up admin call.
        env.storage().instance().set(
            &DataKey::Config,
            &ProtocolConfig {
                min_bond: DEFAULT_MIN_BOND,
                fill_window: DEFAULT_FILL_WINDOW,
                intent_expiry: DEFAULT_INTENT_EXPIRY,
                protocol_fee_bps: DEFAULT_PROTOCOL_FEE_BPS,
                max_active_intents_per_solver: DEFAULT_MAX_ACTIVE_INTENTS_PER_SOLVER,
            },
        );
        // Fresh deploys are already at the current schema — `migrate` is a
        // no-op unless a later upgrade bumps `MIGRATION_VERSION` (#194).
        env.storage()
            .instance()
            .set(&DataKey::MigrationVersion, &MIGRATION_VERSION);
        Self::bump_instance_ttl(&env);
    }

    // ── Admin ──────────────────────────────────────────────────────────────────

    /// Admin-only: propose a new fee recipient address. The proposal is stored
    /// (with the ledger timestamp at which it becomes executable) but not yet
    /// active, and a `fee_recipient_proposed` event fires immediately so
    /// off-chain monitors have advance notice (#116). The new address must
    /// wait out the timelock and then call `accept_fee_recipient` to confirm,
    /// mirroring `transfer_admin`'s two-step pattern so a typo'd or
    /// unreachable address can never silently misroute protocol fees, and
    /// giving affected parties a window to react before it's live (#115).
    ///
    /// A new proposal overwrites any prior pending proposal (and resets the
    /// timelock), so the admin can correct a mistake before the recipient has
    /// accepted.
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

        let eta = env.ledger().timestamp() + ADMIN_TIMELOCK_DELAY;
        env.storage().instance().set(
            &DataKey::PendingFeeRecipient,
            &(new_fee_recipient.clone(), eta),
        );

        env.events().publish(
            (Symbol::new(&env, "fee_recipient_proposed"),),
            (new_fee_recipient, eta),
        );
    }

    /// The pending fee recipient confirms the handover once the timelock
    /// delay since `propose_fee_recipient` has elapsed. Until this is called
    /// the current fee recipient remains unchanged.
    pub fn accept_fee_recipient(env: Env, new_fee_recipient: Address) {
        let (pending, eta): (Address, u64) = env
            .storage()
            .instance()
            .get(&DataKey::PendingFeeRecipient)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingFeeRecipient));

        if pending != new_fee_recipient {
            panic_with_error!(&env, Error::Unauthorized);
        }
        if env.ledger().timestamp() < eta {
            panic_with_error!(&env, Error::TimelockNotElapsed);
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

    /// Admin-only: cancel a pending fee recipient proposal and emit a
    /// `fee_recipient_proposal_cancelled` event. Allows the admin to withdraw a
    /// mistaken proposal without simultaneously replacing it with a new one (#210).
    /// Calling `accept_fee_recipient` after cancellation fails with
    /// `NoPendingFeeRecipient`, exactly as if no proposal had been made.
    pub fn cancel_pending_fee_recipient(env: Env) {
        env.current_contract_address().require_auth();
        let pending = env
            .storage()
            .instance()
            .get::<_, (Address, u64)>(&DataKey::PendingFeeRecipient)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingFeeRecipient));

        env.storage()
            .instance()
            .remove(&DataKey::PendingFeeRecipient);

        env.events().publish(
            (Symbol::new(&env, "fee_recipient_proposal_cancelled"),),
            pending.0,
        );
    }

    /// Admin-only: propose transferring the admin role to a new address. A
    /// `admin_transfer_proposed` event fires immediately for off-chain
    /// monitors (#116); the transfer itself only takes effect once
    /// `new_admin` calls `accept_admin_transfer` after the timelock delay
    /// has elapsed (#115), so a typo'd address can't accidentally brick
    /// admin control and affected parties get advance notice.
    ///
    /// A new proposal overwrites any prior pending proposal (and resets the
    /// timelock).
    pub fn propose_admin_transfer(env: Env, new_admin: Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        // Auth audit: require_auth() is correct here — the stored admin
        // address must sign to propose handing off its own role.
        admin.require_auth();

        let eta = env.ledger().timestamp() + ADMIN_TIMELOCK_DELAY;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &(new_admin.clone(), eta));

        env.events().publish(
            (Symbol::new(&env, "admin_transfer_proposed"),),
            (new_admin, eta),
        );
    }

    /// The pending new admin confirms the handover once the timelock delay
    /// since `propose_admin_transfer` has elapsed. Requiring the incoming
    /// admin's own signature prevents accidentally handing the role to a
    /// typo'd or uncontrolled address.
    pub fn accept_admin_transfer(env: Env, new_admin: Address) {
        let (pending, eta): (Address, u64) = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingAdminTransfer));

        if pending != new_admin {
            panic_with_error!(&env, Error::Unauthorized);
        }
        if env.ledger().timestamp() < eta {
            panic_with_error!(&env, Error::TimelockNotElapsed);
        }
        new_admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);

        env.events()
            .publish((Symbol::new(&env, "admin_transferred"),), new_admin);
    }

    // ── Contract Upgrade (#194) ───────────────────────────────────────────────

    /// Admin-only: propose swapping the contract's Wasm for the code with hash
    /// `new_wasm_hash` (already uploaded to the ledger, e.g. via
    /// `stellar contract upload`). Timelocked exactly like the other sensitive
    /// admin actions: an `upgrade_proposed` event fires immediately for
    /// off-chain monitors, and `execute_upgrade` may only run once
    /// `ADMIN_TIMELOCK_DELAY` has elapsed. A fresh proposal overwrites any
    /// prior pending one (and resets the timelock).
    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        Self::require_admin(&env);
        let eta = env.ledger().timestamp() + ADMIN_TIMELOCK_DELAY;
        env.storage()
            .instance()
            .set(&DataKey::PendingUpgrade, &(new_wasm_hash.clone(), eta));
        Self::bump_instance_ttl(&env);
        env.events().publish(
            (Symbol::new(&env, "upgrade_proposed"),),
            (new_wasm_hash, eta),
        );
    }

    /// Apply a previously proposed upgrade once its timelock has elapsed.
    /// Admin-only (the proposal is re-confirmed here so a stale proposal can't
    /// be executed by a third party). `new_wasm_hash` must match the pending
    /// proposal. Emits `upgraded`; after this the new code is live and callers
    /// should run [`migrate`] if the release requires it.
    pub fn execute_upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        Self::require_admin(&env);
        let (pending, eta): (BytesN<32>, u64) = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingUpgrade));
        if pending != new_wasm_hash {
            panic_with_error!(&env, Error::Unauthorized);
        }
        if env.ledger().timestamp() < eta {
            panic_with_error!(&env, Error::TimelockNotElapsed);
        }
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((Symbol::new(&env, "upgraded"),), new_wasm_hash);
    }

    /// The pending upgrade proposal, if any: `(new_wasm_hash, eta)`.
    pub fn get_pending_upgrade(env: Env) -> Option<(BytesN<32>, u64)> {
        env.storage().instance().get(&DataKey::PendingUpgrade)
    }

    /// Admin-only, run-once-per-release storage migration hook (#194).
    ///
    /// Guarded by `DataKey::MigrationVersion`: it refuses (`AlreadyMigrated`)
    /// once the contract is at `MIGRATION_VERSION`, so a migration can never be
    /// applied twice even if a later `execute_upgrade` forgets to bump the
    /// version. Contracts deployed before this feature have no stored version
    /// (`unwrap_or(0)`), so their first `migrate` runs.
    ///
    /// The body is intentionally empty in this release — no persisted shape has
    /// changed. Future upgrades that reshape storage add their one-time
    /// backfill here and bump `MIGRATION_VERSION`.
    pub fn migrate(env: Env) {
        Self::require_admin(&env);
        let current: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MigrationVersion)
            .unwrap_or(0);
        if current >= MIGRATION_VERSION {
            panic_with_error!(&env, Error::AlreadyMigrated);
        }

        // (no-op for MIGRATION_VERSION == 1)

        env.storage()
            .instance()
            .set(&DataKey::MigrationVersion, &MIGRATION_VERSION);
        Self::bump_instance_ttl(&env);
        env.events().publish(
            (Symbol::new(&env, "migrated"),),
            (current, MIGRATION_VERSION),
        );
    }

    // ── Protocol Config ───────────────────────────────────────────────────────

    /// Read the effective protocol config.  Falls back to compile-time defaults
    /// for contracts that existed before this upgrade (upgrade safety).
    pub fn get_config(env: Env) -> ProtocolConfig {
        Self::load_config(&env)
    }

    /// Admin-only: update the configurable protocol parameters atomically.
    ///
    /// Bounds (any violation returns `InvalidConfig`):
    /// * `protocol_fee_bps`                ≤ 1 000 (10%)
    /// * `fill_window`                     ≥ 60 s
    /// * `intent_expiry`                   ≥ 300 s and > fill_window
    /// * `min_bond`                        ≥ 1 token unit (10_000_000 for 7-decimal USDC)
    /// * `max_active_intents_per_solver`   ≥ 1
    pub fn set_config(
        env: Env,
        min_bond: i128,
        fill_window: u64,
        intent_expiry: u64,
        protocol_fee_bps: i128,
        max_active_intents_per_solver: u32,
    ) {
        Self::require_admin(&env);

        if !(0..=MAX_PROTOCOL_FEE_BPS).contains(&protocol_fee_bps) {
            panic_with_error!(&env, Error::InvalidConfig);
        }
        if !(0..=MAX_REFERRAL_SHARE_BPS).contains(&referral_share_bps) {
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
        if max_active_intents_per_solver == 0 {
            panic_with_error!(&env, Error::InvalidConfig);
        }

        let cfg = ProtocolConfig {
            min_bond,
            fill_window,
            intent_expiry,
            protocol_fee_bps,
            max_active_intents_per_solver,
        };
        env.storage().instance().set(&DataKey::Config, &cfg);
        Self::bump_instance_ttl(&env);

        env.events().publish(
            (Symbol::new(&env, "config_updated"),),
            (min_bond, fill_window, intent_expiry, protocol_fee_bps, max_active_intents_per_solver),
        );
    }

    // ── Destination Token Allowlist ───────────────────────────────────────────

    /// Admin-only: propose allowing a dst_token to be targeted by new
    /// intents. submit_intent had no validation on dst_token at all --
    /// any address, including a bogus or malicious "token" contract, could
    /// be named as the destination.
    ///
    /// We call `decimals()` on the candidate address as a lightweight SEP-41
    /// interface probe (issue #33) at proposal time. If the address doesn't
    /// implement the token interface the call traps and the transaction
    /// reverts, surfacing the error at admin time rather than silently
    /// storing a proposal that would only fail later.
    ///
    /// This only records the proposal and fires a `dst_token_add_proposed`
    /// event for off-chain monitors (#116); the token isn't actually
    /// allowed until `execute_add_dst_token` is called after the timelock
    /// delay has elapsed (#115, #118), giving users and solvers a window to
    /// notice and react before the allowlist changes.
    ///
    /// Note: `decimals()` is a read-only view, so this probe has no side
    /// effects on the token's state.
    pub fn propose_add_dst_token(env: Env, token: Address) {
        Self::require_admin(&env);

        // Probe the SEP-41 interface: if `token` isn't a real token contract
        // this will trap and revert the transaction before we store anything.
        let token_client = token::Client::new(&env, &token);
        // decimals() is a pure view with no side-effects; we discard the value.
        let _decimals = token_client.decimals();

        let eta = env.ledger().timestamp() + ADMIN_TIMELOCK_DELAY;
        env.storage()
            .instance()
            .set(&DataKey::PendingDstTokenAdd(token.clone()), &eta);

        env.events()
            .publish((Symbol::new(&env, "dst_token_add_proposed"),), (token, eta));
    }

    /// Apply a previously proposed `propose_add_dst_token` once its timelock
    /// delay has elapsed. Callable by anyone -- the change was already
    /// authorized by the admin at proposal time, so there's nothing left to
    /// gate once the delay has passed.
    pub fn execute_add_dst_token(env: Env, token: Address) {
        let eta: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PendingDstTokenAdd(token.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingDstTokenChange));
        if env.ledger().timestamp() < eta {
            panic_with_error!(&env, Error::TimelockNotElapsed);
        }

        env.storage()
            .instance()
            .remove(&DataKey::PendingDstTokenAdd(token.clone()));
        env.storage()
            .instance()
            .set(&DataKey::AllowedDstToken(token.clone()), &true);
        Self::add_to_dst_token_list(&env, &token);

        env.events()
            .publish((Symbol::new(&env, "dst_token_allowed"),), token);
    }

    /// Admin-only: cancel a pending dst_token_add proposal for the given token
    /// and emit a `dst_token_add_proposal_cancelled` event. Allows the admin to
    /// withdraw a mistaken proposal without simultaneously replacing it with a
    /// new one (#210). Calling `execute_add_dst_token` after cancellation fails
    /// with `NoPendingDstTokenChange`, exactly as if no proposal had been made.
    pub fn cancel_pending_dst_token_add(env: Env, token: Address) {
        Self::require_admin(&env);

        let _eta: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PendingDstTokenAdd(token.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingDstTokenChange));

        env.storage()
            .instance()
            .remove(&DataKey::PendingDstTokenAdd(token.clone()));

        env.events().publish(
            (Symbol::new(&env, "dst_token_add_proposal_cancelled"),),
            token,
        );
    }

    /// Admin-only: propose disallowing a dst_token. Fires a
    /// `dst_token_remove_proposed` event immediately (#116); the token stays
    /// allowed until `execute_remove_dst_token` is called after the timelock
    /// delay elapses (#115).
    pub fn propose_remove_dst_token(env: Env, token: Address) {
        Self::require_admin(&env);

        let eta = env.ledger().timestamp() + ADMIN_TIMELOCK_DELAY;
        env.storage()
            .instance()
            .set(&DataKey::PendingDstTokenRemove(token.clone()), &eta);

        env.events().publish(
            (Symbol::new(&env, "dst_token_remove_proposed"),),
            (token, eta),
        );
    }

    /// Apply a previously proposed `propose_remove_dst_token` once its
    /// timelock delay has elapsed. Callable by anyone, for the same reason
    /// as `execute_add_dst_token`.
    pub fn execute_remove_dst_token(env: Env, token: Address) {
        let eta: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PendingDstTokenRemove(token.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingDstTokenChange));
        if env.ledger().timestamp() < eta {
            panic_with_error!(&env, Error::TimelockNotElapsed);
        }

        env.storage()
            .instance()
            .remove(&DataKey::PendingDstTokenRemove(token.clone()));
        env.storage()
            .instance()
            .remove(&DataKey::AllowedDstToken(token.clone()));
        Self::remove_from_dst_token_list(&env, &token);

        env.events()
            .publish((Symbol::new(&env, "dst_token_disallowed"),), token);
    }

    /// Admin-only: cancel a pending dst_token_remove proposal for the given token
    /// and emit a `dst_token_remove_proposal_cancelled` event. Allows the admin to
    /// withdraw a mistaken proposal without simultaneously replacing it with a
    /// new one (#210). Calling `execute_remove_dst_token` after cancellation fails
    /// with `NoPendingDstTokenChange`, exactly as if no proposal had been made.
    pub fn cancel_pending_dst_token_remove(env: Env, token: Address) {
        Self::require_admin(&env);

        let _eta: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PendingDstTokenRemove(token.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingDstTokenChange));

        env.storage()
            .instance()
            .remove(&DataKey::PendingDstTokenRemove(token.clone()));

        env.events().publish(
            (Symbol::new(&env, "dst_token_remove_proposal_cancelled"),),
            token,
        );
    }

    /// Returns `true` if `token` is on the dst_token allowlist.
    /// Does not check whether allowlist enforcement is currently active.
    pub fn is_dst_token_allowed(env: Env, token: Address) -> bool {
        env.storage()
            .instance()
            .has(&DataKey::AllowedDstToken(token))
    }

    /// Admin-only: turn allowlist enforcement in submit_intent on/off.
    /// Off by default -- an admin opts in once they've populated the list
    /// via add_allowed_dst_token, rather than every intent submission
    /// suddenly requiring one.
    ///
    /// Issue #119: emits an event like every other admin toggle in this
    /// contract (pause/unpause, fee_recipient_updated, admin_transferred),
    /// so off-chain indexers can observe enforcement flips without polling
    /// `is_dst_allowlist_enabled`.
    pub fn set_dst_allowlist_enabled(env: Env, enabled: bool) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::DstAllowlistEnabled, &enabled);
        env.events()
            .publish((Symbol::new(&env, "dst_allowlist_enabled"),), enabled);
    }

    /// Returns `true` if the dst_token allowlist is currently being enforced
    /// by `submit_intent`. Defaults to `false` on a fresh deployment.
    pub fn is_dst_allowlist_enabled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::DstAllowlistEnabled)
            .unwrap_or(false)
    }

    /// List every dst_token currently present in the allowlist (#117).
    /// `is_dst_token_allowed` only answers one-token-at-a-time queries; this
    /// gives integrators and auditors a complete picture without replaying
    /// every `dst_token_allowed` / `dst_token_disallowed` event. Returns an
    /// empty `Vec` if nothing has ever been allowed.
    pub fn list_allowed_dst_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::AllowedDstTokenList)
            .unwrap_or_else(|| Vec::new(&env))
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
        // #271: bump TTL so the multiplier is not silently archived back to 1.0×.
        Self::bump_min_bond_multiplier_ttl(&env, &token);
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
    }

    // ── Source Chain Allowlist ────────────────────────────────────────────────

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
        env.events()
            .publish((Symbol::new(&env, "src_chain_allowlist_enabled"),), enabled);
    }

    /// Whether src_chain validation is currently active.
    pub fn is_src_chain_allowlist_enabled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::SrcChainAllowlistEnabled)
            .unwrap_or(false)
    }

    // ── Pause Control ─────────────────────────────────────────────────────────

    /// Admin-only: designate (or rotate) the address that may call `pause`
    /// in addition to the admin (issue #120). This lets incident response
    /// use a narrower-scoped hot key instead of the full admin key, which
    /// also controls fee routing and admin transfer. Calling this again
    /// with a new address replaces the previous pauser; there is no way to
    /// clear it back to "admin-only" other than pointing it at the admin's
    /// own address.
    pub fn set_pauser(env: Env, pauser: Address) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Pauser, &pauser);
        env.events()
            .publish((Symbol::new(&env, "pauser_updated"),), pauser);
    }

    /// The current pauser address, if the admin has set one.
    pub fn get_pauser(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Pauser)
    }

    // ── Proof Registry (issue #190) ──────────────────────────────────────────

    /// Admin-only: point this contract at a deployed `ProofRegistry`
    /// (docs/124 §4.1). Required before any solver can call
    /// `fill_intent(..., require_proof = true)`. Until this is set, proof-gated
    /// fills panic with `ProofRegistryNotSet`; ungated fills
    /// (`require_proof = false`) are unaffected.
    ///
    /// Calling again rotates the address (e.g. after a registry upgrade).
    pub fn set_proof_registry(env: Env, registry: Address) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::ProofRegistry, &registry);
        Self::bump_instance_ttl(&env);
        env.events()
            .publish((Symbol::new(&env, "proof_registry_set"),), registry);
    }

    /// The configured `ProofRegistry` address, or `None` if proof-gating has
    /// not been enabled by the admin.
    pub fn get_proof_registry(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::ProofRegistry)
    }

    /// Admin- or pauser-only: halt new intent submission, acceptance, and
    /// fills for incident response. slash_solver stays permissionless
    /// throughout, so a solver already holding an Accepted intent can't
    /// dodge accountability by waiting out the pause.
    ///
    /// Issue #36 — pause scope decision: register_solver, deregister_solver,
    /// and withdraw_bond are also gated here. During a live incident an admin
    /// may need to freeze the entire protocol state to investigate; allowing
    /// solvers to withdraw their bonds mid-incident would let them shed
    /// collateral exactly when the protocol most needs it as a backstop.
    /// cancel_intent is intentionally left open so users can always reclaim
    /// their Open intents.
    ///
    /// Issue #120 — `caller` must be either the admin or the address set via
    /// `set_pauser`, so fast incident response doesn't require exposing the
    /// full admin key.
    pub fn pause(env: Env, caller: Address) {
        Self::require_admin_or_pauser(&env, &caller);
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish((Symbol::new(&env, "paused"),), ());
    }

    /// Admin-only: lift a pause and restore normal operation.
    ///
    /// Issue #120 — deliberately narrower than `pause`: the pauser role can
    /// freeze the protocol but cannot unfreeze it. Resuming money movement
    /// after an incident always needs the full admin's judgment, not just
    /// whoever holds the pause hot key.
    pub fn unpause(env: Env) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish((Symbol::new(&env, "unpaused"),), ());
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
    /// Issue #35 / Issue #265 — trust model: rescue is restricted to tokens that are
    /// neither the bond_token nor any token currently referenced by an active
    /// (Accepted) intent as its dst_token. This prevents the rescue path from
    /// being misused to drain live solver collateral or in-flight intent
    /// output from under active protocol participants.
    ///
    /// Audit Note (Issue #265): `BondToken` is set once during contract
    /// `initialize` and is immutable (no admin function exists to mutate it).
    /// Forward-compatibility for Issue #2 (multi-bond-token design / docs/60):
    /// When Issue #2 introduces per-token bond sets (`DataKey::AllowedBondToken`),
    /// this single-address guard MUST be updated to query `AllowedBondToken(token)`
    /// or iterate the approved bond token set, preventing accidental draining of
    /// newly added bond tokens.
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

    /// Solvers register by depositing a bond in the protocol's default bond
    /// token (USDC). Existing solvers may top up with any positive amount --
    /// the minimum is enforced on the resulting total, not on each individual
    /// deposit.
    ///
    /// Issue #187: this is now a thin wrapper over `register_solver_with_token`
    /// pinned to the legacy default token, kept so pre-#187 callers and tests
    /// work unchanged.
    pub fn register_solver(env: Env, solver: Address, bond_amount: i128) {
        let bond_token = Self::load_bond_token(&env);
        Self::register_solver_inner(env, solver, bond_token, bond_amount);
    }

    /// Issue #187: register (or top up) a solver bond in `bond_token`, which
    /// must be the legacy default token or on the `AllowedBondToken` set.
    /// Per-token minimums come from `min_bond_for_token`; a solver may hold
    /// bonds in up to `MAX_BOND_TOKENS` distinct tokens.
    pub fn register_solver_with_token(
        env: Env,
        solver: Address,
        bond_token: Address,
        bond_amount: i128,
    ) {
        Self::register_solver_inner(env, solver, bond_token, bond_amount);
    }

    fn register_solver_inner(env: Env, solver: Address, bond_token: Address, bond_amount: i128) {
        // Auth audit: require_auth() is correct. The solver must sign to
        // consent to locking their own funds as bond.
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        if bond_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        if !Self::is_bond_token_allowed(&env, &bond_token) {
            panic_with_error!(&env, Error::BondTokenNotAllowed);
        }

        let existing: Option<SolverRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()));

        let is_new_solver = existing.is_none();

        // ── Effects first (CEI) ──────────────────────────────────────────────
        // Build and persist the SolverRecord *before* pulling funds in so the
        // contract's storage is always consistent with what it holds.
        let mut record = match existing {
            Some(mut s) => {
                s.is_active = true;
                s
            }
            None => SolverRecord {
                address: solver.clone(),
                bond_amount: 0,
                fills_completed: 0,
                fills_failed: 0,
                total_volume: 0,
                is_active: true,
                registered_at: env.ledger().timestamp(),
                active_intents: 0,
                last_slash_time: 0,
                bond_tokens: Vec::new(&env),
            },
        };

        let existing_bond = Self::get_solver_bond_amount(&env, &record, &bond_token);
        let new_bond = existing_bond + bond_amount;
        if new_bond < Self::min_bond_for_token(&env, &bond_token) {
            panic_with_error!(&env, Error::SolverBondTooLow);
        }

        // Enforce the per-solver bond-token cap before adding a brand-new token.
        if existing_bond == 0 && record.bond_tokens.len() >= MAX_BOND_TOKENS {
            panic_with_error!(&env, Error::TooManyBondTokens);
        }

        Self::set_solver_bond_amount(&env, &mut record, &bond_token, new_bond);

        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        // Increment TotalBonded by the amount being added (issue #231)
        let total_bonded: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalBonded)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalBonded, &(total_bonded + bond_amount));

        if is_new_solver {
            let total: u32 = env
                .storage()
                .instance()
                .get(&DataKey::TotalSolvers)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::TotalSolvers, &(total + 1));
            Self::add_to_solver_list(&env, &solver);
        }

        // ── Interaction: pull bond in ────────────────────────────────────────
        let client = token::Client::new(&env, &bond_token);
        client.transfer(&solver, &env.current_contract_address(), &bond_amount);

        env.events().publish(
            (Symbol::new(&env, "solver_registered"), solver),
            (bond_token, bond_amount),
        );
    }

    /// Solver voluntarily exits the protocol. Returns the full bond — in every
    /// token they hold one (issue #187) — to the solver and removes their
    /// record. Requires no active (Accepted) intents — use `slash_solver` to
    /// clear those first.
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

        let default_token = Self::load_bond_token(&env);

        // Snapshot every (token, amount) pair to refund. The legacy default
        // token may not appear in `bond_tokens` for a pre-#187 record, so it is
        // handled explicitly.
        let mut refunds: Vec<(Address, i128)> = Vec::new(&env);
        if record.bond_amount > 0 {
            refunds.push_back((default_token.clone(), record.bond_amount));
        }
        for i in 0..record.bond_tokens.len() {
            let t = record.bond_tokens.get(i).unwrap();
            if t == default_token {
                continue; // already captured above
            }
            let amt: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::SolverBond(solver.clone(), t.clone()))
                .unwrap_or(0);
            if amt > 0 {
                refunds.push_back((t, amt));
            }
        }

        // ── Effects first (CEI) ──────────────────────────────────────────────
        // Remove all per-token entries and the record *before* the external
        // token transfers so that any re-entrant call sees no record and would
        // panic with SolverNotRegistered rather than processing a double-refund.
        for i in 0..refunds.len() {
            let (t, _) = refunds.get(i).unwrap();
            if t != default_token {
                env.storage()
                    .persistent()
                    .remove(&DataKey::SolverBond(solver.clone(), t));
            }
        }
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
        Self::remove_from_solver_list(&env, &solver);

        // ── Interaction: return every bond ──────────────────────────────────
        let mut total_default_refund = 0i128;
        for i in 0..refunds.len() {
            let (t, amt) = refunds.get(i).unwrap();
            token::Client::new(&env, &t).transfer(
                &env.current_contract_address(),
                &solver,
                &amt,
            );
            if t == default_token {
                total_default_refund = amt;
            }
        }

        env.events().publish(
            (Symbol::new(&env, "solver_deregistered"), solver),
            total_default_refund,
        );
    }

    /// Solver withdraws part of their default-token bond without fully
    /// deregistering. The remaining bond must still clear the minimum -- to go
    /// below that, use deregister_solver instead (which also requires no active
    /// intents).
    ///
    /// Issue #187: thin wrapper over `withdraw_bond_token` pinned to the legacy
    /// default token.
    pub fn withdraw_bond(env: Env, solver: Address, amount: i128) {
        let bond_token = Self::load_bond_token(&env);
        Self::withdraw_bond_inner(env, solver, bond_token, amount);
    }

    /// Issue #187: withdraw part of a solver's bond held in `bond_token`. The
    /// remaining balance in that token must still clear
    /// `min_bond_for_token(bond_token)`.
    pub fn withdraw_bond_token(env: Env, solver: Address, bond_token: Address, amount: i128) {
        Self::withdraw_bond_inner(env, solver, bond_token, amount);
    }

    fn withdraw_bond_inner(env: Env, solver: Address, bond_token: Address, amount: i128) {
        // Auth audit: require_auth() is correct. Only the solver may withdraw
        // their own bond.
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

        let current = Self::get_solver_bond_amount(&env, &record, &bond_token);
        if amount > current {
            panic_with_error!(&env, Error::InsufficientBond);
        }

        let remaining = current - amount;
        if remaining < Self::min_bond_for_token(&env, &bond_token) {
            panic_with_error!(&env, Error::SolverBondTooLow);
        }

        Self::set_solver_bond_amount(&env, &mut record, &bond_token, remaining);
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        let client = token::Client::new(&env, &bond_token);
        client.transfer(&env.current_contract_address(), &solver, &amount);

        // Issue #108: include the post-withdrawal remaining balance so indexers
        // can maintain a solver's bond ledger without a separate get_solver call.
        // data: (amount: i128, remaining: i128)
        env.events().publish(
            (Symbol::new(&env, "bond_withdrawn"), solver),
            (amount, remaining),
        );
    }

    // ── Intent Lifecycle ──────────────────────────────────────────────────────

    /// User submits a swap intent. No funds are locked on Stellar at this point —
    /// the user initiates the source-chain tx separately.
    ///
    /// # Parameters
    ///
    /// - `referrer` (optional, default `None`): the address to credit with a share
    ///   of the protocol fee when the intent is filled.  Must not equal `user`
    ///   (self-referral is rejected with `Error::SelfReferral`).  The share is
    ///   governed by `ProtocolConfig.referral_share_bps` and is only paid out
    ///   when that config value is non-zero.
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
        referrer: Option<Address>,
    ) -> BytesN<32> {
        // Auth audit: require_auth() is correct. The user must sign to assert
        // ownership of the address receiving output tokens (dst). If a third-party
        // contract were ever to call submit_intent on a user's behalf, switching to
        // require_auth_for_args scoped to (user, dst_token, min_dst_amount) would
        // limit the scope of delegated authorisation — noted as a future hardening
        // opportunity if composable intent submission is added.
        user.require_auth();
        Self::submit_intent_inner(
            env,
            user,
            src_chain,
            src_token,
            src_amount,
            dst_token,
            min_dst_amount,
            deadline,
        )
    }

    /// Body of `submit_intent` without the `user.require_auth()` gate. Called
    /// directly by `submit_intent` (after auth) and by `batch_submit_intent`,
    /// which authorises the user once for the whole batch — `require_auth()`
    /// can only be called once per address per contract invocation, so the
    /// per-item calls must not repeat it.
    #[allow(clippy::too_many_arguments)]
    fn submit_intent_inner(
        env: Env,
        user: Address,
        src_chain: String,
        src_token: String,
        src_amount: i128,
        dst_token: Address,
        min_dst_amount: i128,
        deadline: Option<u64>,
    ) -> BytesN<32> {
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

        // #127 — validate src_token address format against the declared chain's
        // conventions (EVM: 0x + 40 hex chars; Solana: base58 32–44 chars).
        // This runs even when the src_chain allowlist is disabled so obviously
        // malformed tokens are always caught at submission time.
        Self::validate_src_token(&env, &src_chain, &src_token);

        // #252 — decimals-aware sanity bound on min_dst_amount. Runs
        // unconditionally (not just when the dst allowlist is enabled) since
        // this is a magnitude sanity check, not an allowlist gate. The
        // decimals() probe mirrors propose_add_dst_token's precedent: if
        // dst_token doesn't implement SEP-41, the call traps and the whole
        // submission reverts, which is the desired behavior here too.
        let dst_token_client = token::Client::new(&env, &dst_token);
        let dst_decimals = dst_token_client.decimals();
        let dst_bound = 10i128
            .checked_pow(dst_decimals)
            .and_then(|unit| unit.checked_mul(MAX_WHOLE_UNITS));
        // `None` covers dst_decimals being so large the bound itself
        // overflows i128 — treated the same as exceeding the bound: reject.
        if !dst_bound.is_some_and(|bound| min_dst_amount <= bound) {
            panic_with_error!(&env, Error::ImplausibleDstAmount);
        }

        let now = env.ledger().timestamp();
        let cfg = Self::load_config(&env);
        let expiry = deadline.unwrap_or(now + cfg.intent_expiry);

        if expiry <= now {
            panic_with_error!(&env, Error::InvalidDeadline);
        }

        // #281: self-referral guard — a user cannot name their own address
        // as the referrer, which would let them claim referral rewards on
        // their own volume.
        if let Some(r) = &referrer {
            if r == &user {
                panic_with_error!(&env, Error::SelfReferral);
            }
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
            state: IntentState::Open,
            created_at: now,
            deadline: expiry,
            filled_at: None,
            fill_amount: None,
            total_filled: 0,
            // Issue #187: placeholder until a solver accepts and names the token
            // that backs their obligation. Defaults to the legacy bond token so
            // `slash_solver` has a valid target even on paths that skip accept.
            bond_token: Self::load_bond_token(&env),
            // Issue #188: no escrow/dispute state until begin_fill runs.
            dispute_deadline: None,
            dispute_raised_at: None,
            resolution: None,
        };

        Self::save_intent(&env, &intent_id, &intent);

        let mut user_intents: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&DataKey::UserIntents(user.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        user_intents.push_back(intent_id.clone());
        env.storage()
            .persistent()
            .set(&DataKey::UserIntents(user.clone()), &user_intents);
        // #271: bump TTL so list_intents_by_user never silently returns an
        // incomplete list due to archival.
        Self::bump_user_intents_ttl(&env, &user);

        let total: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TotalIntents)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalIntents, &(total + 1));

        // Increment open_intents: every new submission starts as Open.
        let open: u64 = env
            .storage()
            .instance()
            .get(&DataKey::OpenIntents)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::OpenIntents, &(open + 1));

        // #249: only truly Open intents (not Bidding, which isn't directly
        // fillable) are enumerable via list_open_intents.
        if intent.state == IntentState::Open {
            Self::add_to_open_intent_list(&env, &intent_id);
        }

        env.events().publish(
            (Symbol::new(&env, "intent_submitted"), user),
            (intent_id.clone(), min_dst_amount, expiry),
        );

        intent_id
    }

    /// Solver claims an intent (exclusive fill right for FILL_WINDOW seconds),
    /// backing the obligation with their default-token bond.
    ///
    /// Issue #187: thin wrapper over `accept_intent_with_bond` pinned to the
    /// legacy default bond token.
    pub fn accept_intent(env: Env, solver: Address, intent_id: BytesN<32>) {
        let bond_token = Self::load_bond_token(&env);
        Self::accept_intent_inner(env, solver, intent_id, bond_token);
    }

    /// Issue #187: claim an intent, backing it with the solver's bond in
    /// `bond_token`. The token is recorded on the intent so `slash_solver`
    /// takes the penalty from — and pays it out in — the same token.
    pub fn accept_intent_with_bond(
        env: Env,
        solver: Address,
        intent_id: BytesN<32>,
        bond_token: Address,
    ) {
        Self::accept_intent_inner(env, solver, intent_id, bond_token);
    }

    fn accept_intent_inner(
        env: Env,
        solver: Address,
        intent_id: BytesN<32>,
        bond_token: Address,
    ) {
        // Auth audit: require_auth() is correct. The solver must sign to
        // voluntarily take on the fill obligation and bond risk.
        solver.require_auth();
        Self::accept_intent_inner(env, solver, intent_id);
    }

    /// Body of `accept_intent` without the `solver.require_auth()` gate. Shared
    /// with `batch_accept_intent`, which authorises the solver once per batch
    /// (`require_auth()` is one-shot per address per invocation).
    fn accept_intent_inner(env: Env, solver: Address, intent_id: BytesN<32>) {
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
        if solver_record.last_slash_time > 0 && now < solver_record.last_slash_time + SLASH_COOLDOWN
        {
            panic_with_error!(&env, Error::SolverInactive);
        }

        if !Self::is_bond_token_allowed(&env, &bond_token) {
            panic_with_error!(&env, Error::BondTokenNotAllowed);
        }

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        // The dst_token multiplier scales the *default*-token minimum; for any
        // other bond token we require at least that token's own minimum. This
        // keeps the #187 non-goal (no cross-token price comparison) intact.
        let required_bond = if bond_token == Self::load_bond_token(&env) {
            Self::get_adjusted_min_bond(&env, &intent.dst_token)
        } else {
            Self::min_bond_for_token(&env, &bond_token)
        };
        if Self::get_solver_bond_amount(&env, &solver_record, &bond_token) < required_bond {
            panic_with_error!(&env, Error::SolverBondTooLow);
        }

        // Boundary semantics: deadline is EXCLUSIVE for acceptance.
        // `now >= intent.deadline` rejects at the boundary second (`now == deadline`)
        // so the full [created_at, deadline) half-open window is available for solvers.
        if now >= intent.deadline {
            Self::save_intent(&env, &intent_id, &intent);
            panic_with_error!(&env, Error::IntentExpired);
        }

        if intent.state != IntentState::Open && intent.state != IntentState::PartiallyFilled {
            panic_with_error!(&env, Error::IntentNotOpen);
        }

        // Issue #230: Check max-active-intents cap before accepting
        let cfg = Self::load_config(&env);
        if solver_record.active_intents >= cfg.max_active_intents_per_solver {
            panic_with_error!(&env, Error::MaxActiveIntentsCapReached);
        }

        intent.solver = Some(solver.clone());
        intent.state = IntentState::Accepted;
        intent.bond_token = bond_token.clone();
        // Extend deadline to fill window from now
        let cfg = Self::load_config(&env);
        let tier = Self::solver_tier(&env, &solver);
        intent.solver_tier = tier;
        intent.deadline = now + Self::tier_fill_window(tier, cfg.fill_window);

        solver_record.active_intents += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &solver_record);
        Self::solver_intents_add(&env, &solver, &intent_id);

        // Decrement open_intents: the intent is no longer open (a solver owns it).
        let open: u64 = env
            .storage()
            .instance()
            .get(&DataKey::OpenIntents)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::OpenIntents, &open.saturating_sub(1));
        Self::remove_from_open_intent_list(&env, &intent_id);

        Self::save_intent(&env, &intent_id, &intent);

        env.events().publish(
            (Symbol::new(&env, "intent_accepted"), solver),
            (intent_id, intent.deadline),
        );
    }

    /// Solver fills the intent by sending dst_token to the user.
    ///
    /// Partial fills are supported: `fill_amount` must be > 0 but may be less
    /// than `min_dst_amount`.  The intent transitions to `PartiallyFilled` after
    /// each sub-fill and is re-opened so another solver (or the same one) can
    /// accept and deliver the remainder.  Once the cumulative `total_filled`
    /// reaches or exceeds `min_dst_amount` the intent transitions to `Filled`.
    ///
    /// The protocol fee is taken on each individual fill so the fee accounting
    /// stays consistent regardless of how many fills it takes.
    ///
    /// ## Proof gating (issue #190, docs/124 §4.2, docs/129)
    ///
    /// When `require_proof == true`, the fill is cross-checked against a
    /// `ProofRegistry` record for `intent_id` before any tokens move:
    ///
    /// * no `ProofRegistry` configured → `ProofRegistryNotSet`;
    /// * no proof for this intent      → `ProofNotFound`   (docs/129 §2.3);
    /// * `proof.src_chain_id` ≠ mapped `intent.src_chain` → `ProofChainMismatch`
    ///   (docs/129 §2.2);
    /// * `proof.src_amount < intent.src_amount` → `ProofAmountInsufficient`
    ///   (docs/129 §2.1).
    ///
    /// Every rejection is a `panic_with_error!` before any storage write or
    /// transfer, so the intent stays `Accepted`, the fill window keeps running,
    /// and `slash_solver` remains callable if the solver never produces a valid
    /// proof — exactly the state machine in docs/129 §3.
    ///
    /// The check is against the immutable `intent.src_amount` (the whole-intent
    /// source deposit), not a per-fill quantity, so partial fills each simply
    /// re-assert the same condition against the same proof record — no
    /// cumulative accounting is involved.
    ///
    /// `require_proof == false` is 100% backward compatible: the
    /// `ProofRegistry` is never read and behaviour is byte-for-byte identical
    /// to the pre-#190 contract, mirroring how `DstAllowlistEnabled` defaults
    /// off.
    pub fn fill_intent(
        env: Env,
        solver: Address,
        intent_id: BytesN<32>,
        fill_amount: i128,
        require_proof: bool,
    ) {
        // Auth audit: require_auth() is correct. The solver must sign to
        // authorise the token transfer from their address to the user and fee
        // recipient. This is the highest-value call site: the solver authorises
        // a token transfer, so the auth is load-bearing. require_auth_for_args
        // scoped to (solver, intent_id, fill_amount) would meaningfully tighten
        // the scope if a delegated-execution pattern is ever introduced — noted
        // as the strongest candidate for future hardening.
        solver.require_auth();
        Self::fill_intent_inner(env, solver, intent_id, fill_amount);
    }

    /// Body of `fill_intent` without the `solver.require_auth()` gate. Shared
    /// with `batch_fill_intent`, which authorises the solver once per batch
    /// (`require_auth()` is one-shot per address per invocation). The solver's
    /// signature over the batch call still covers the individual dst-token
    /// transfers each fill performs.
    fn fill_intent_inner(env: Env, solver: Address, intent_id: BytesN<32>, fill_amount: i128) {
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        let mut intent = Self::load_intent(&env, &intent_id);

        let now = env.ledger().timestamp();
        // Boundary semantics: the fill-window deadline is EXCLUSIVE for filling.
        // `now >= intent.deadline` rejects at the boundary second (`now == deadline`)
        // so the full [accepted_at, accepted_at + FILL_WINDOW) window is available
        // to the solver. Shared with `is_intent_fillable` via `check_fill_guards`
        // (issue #259) so the two can never silently drift apart.
        if let Err(e) = Self::check_fill_guards(&intent, &solver, now) {
            panic_with_error!(&env, e);
        }

        if fill_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        // ── Proof gate (issue #190) ─────────────────────────────────────────
        // Runs before any token transfer or storage write. Every failure path
        // leaves the intent untouched in `Accepted` (docs/129 §3).
        if require_proof {
            Self::validate_proof(&env, &intent, &intent_id);
        }

        // Deliver this fill's tokens to the user.
        let dst_client = token::Client::new(&env, &intent.dst_token);
        dst_client.transfer(&solver, &intent.user, &fill_amount);

        // Solver also pays the protocol fee on each fill.
        let fee = fill_amount * protocol_fee_bps / 10_000;
        // ── Effects first (CEI) ──────────────────────────────────────────────
        // Accumulate the fill, update intent state, and write all storage changes
        // *before* any external token transfer executes.  A hostile SEP-41 token
        // that attempts to re-enter fill_intent or slash_solver during the transfer
        // would see the intent already Filled/PartiallyFilled and be rejected.

        // Compute protocol fee with explicit checked arithmetic (#269 / #31).
        // Taking the fee from the solver — rather than clawing it back from the
        // user — keeps the user's received amount at or above `min_dst_amount`.
        // Explicit checked_mul/checked_div makes the overflow-safety property
        // visible in code, rather than relying solely on the Cargo.toml
        // overflow-checks = true release-profile setting.
        let fee_bps = Self::get_tiered_fee_bps(&env);
        let fee = fill_amount
            .checked_mul(fee_bps)
            .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow))
            .checked_div(BPS_DENOMINATOR)
            .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow));

        // ── Effects first (CEI) ──────────────────────────────────────────────
        // Every state change below is written to storage *before* the token
        // transfers at the end of this function. A hostile SEP-41 token that
        // tries to re-enter `fill_intent` / `slash_solver` during a transfer
        // sees the already-committed state (intent Filled, or re-opened with
        // no assigned solver) and is rejected by the guards above.

        // ── Effects first (CEI) ──────────────────────────────────────────────
        // Mark every state change and write it to storage *before* any external
        // token transfer executes. A hostile SEP-41 token that tries to re-enter
        // fill_intent or slash_solver during the transfer sees the already-
        // updated intent state and is rejected by the guards above.
        intent.total_filled += fill_amount;
        let cumulative = intent.total_filled;
        intent.fill_amount = Some(cumulative);

        let mut solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap();
        solver_record.total_volume += fill_amount;

        if cumulative >= intent.min_dst_amount {
            // Intent is fully satisfied — close it out. open_intents was already
            // decremented when the intent was accepted; no adjustment needed.
            intent.state = IntentState::Filled;
            intent.filled_at = Some(now);
            solver_record.fills_completed += 1;
            solver_record.active_intents = solver_record.active_intents.saturating_sub(1);
            Self::solver_intents_remove(&env, &solver, &intent_id);
        } else {
            // Partial fill: re-open so another solver (or the same) can claim the
            // remaining amount. Reset solver assignment and deadline back to the
            // full intent expiry window; the intent is back in Open rotation, so
            // increment open_intents again.
            intent.state = IntentState::PartiallyFilled;
            intent.solver = None;
            intent.solver_tier = 0; // #197: no assignee → no tier snapshot
            intent.deadline = now + INTENT_EXPIRY;
            solver_record.active_intents = solver_record.active_intents.saturating_sub(1);
            Self::solver_intents_remove(&env, &solver, &intent_id);

            let open: u64 = env
                .storage()
                .instance()
                .get(&DataKey::OpenIntents)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::OpenIntents, &(open + 1));
            Self::add_to_open_intent_list(&env, &intent_id);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &solver_record);
        Self::bump_solver_ttl(&env, &solver);

        let total_vol: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalVolume)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalVolume, &(total_vol + fill_amount));

        Self::save_intent(&env, &intent_id, &intent);

        // ── Interactions: token transfers (state already committed above) ────
        // Solver delivers this fill's output to the user, then separately pays
        // the protocol fee. Each transfer happens exactly once.
        let dst_client = token::Client::new(&env, &intent.dst_token);

        // Solver delivers the full requested output to the user.
        dst_client.transfer(&solver, &intent.user, &fill_amount);

        if fee > 0 {
            let cfg = Self::load_config(&env);
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .unwrap();
            match (&intent.referrer, cfg.referral_share_bps) {
                (Some(referrer_addr), share) if share > 0 => {
                    let referral_amount = fee
                        .checked_mul(share)
                        .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow))
                        .checked_div(10_000)
                        .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow));
                    let recipient_amount = fee - referral_amount;
                    if referral_amount > 0 {
                        dst_client.transfer(&solver, referrer_addr, &referral_amount);
                    }
                    if recipient_amount > 0 {
                        dst_client.transfer(&solver, &fee_recipient, &recipient_amount);
                    }
                }
                _ => {
                    // No referrer or zero share: 100% to FeeRecipient
                    // (identical to pre-#281 behaviour).
                    dst_client.transfer(&solver, &fee_recipient, &fee);
                }
            }
        }

        env.events().publish(
            (Symbol::new(&env, "intent_filled"), solver),
            (intent_id, fill_amount, fee),
        );
    }

    /// User can cancel an Open (or PartiallyFilled) intent that no solver
    /// currently holds. Rate-limited per user by `CANCEL_COOLDOWN`.
    pub fn cancel_intent(env: Env, user: Address, intent_id: BytesN<32>) {
        // Auth audit: require_auth() is correct. Only the intent owner may
        // cancel. An additional ownership check (`intent.user != user`) follows
        // immediately after the intent is loaded, providing defence-in-depth.
        // require_auth_for_args is not needed here — the action is simply
        // "cancel intent for this user".
        user.require_auth();
        Self::bump_instance_ttl(&env);

        let now = env.ledger().timestamp();
        Self::check_cancel_cooldown(&env, &user, now);
        Self::cancel_intent_core(&env, &user, &intent_id);
        Self::stamp_cancel_cooldown(&env, &user, now);
    }

    /// Spam-deterrence gate shared by `cancel_intent` and `batch_cancel_intent`:
    /// panics if `user` cancelled within the last `CANCEL_COOLDOWN` seconds.
    fn check_cancel_cooldown(env: &Env, user: &Address, now: u64) {
        if let Some(last_cancel_time) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::CancelCooldown(user.clone()))
        {
            if now < last_cancel_time + CANCEL_COOLDOWN {
                panic_with_error!(env, Error::CancelCooldownNotExpired);
            }
        }
    }

    /// Records `now` as `user`'s most recent cancel, starting a fresh cooldown.
    /// A `batch_cancel_intent` call stamps this once for the whole batch, so a
    /// batch counts as a single cancel action for rate-limiting.
    fn stamp_cancel_cooldown(env: &Env, user: &Address, now: u64) {
        env.storage()
            .persistent()
            .set(&DataKey::CancelCooldown(user.clone()), &now);
    }

    /// The actual cancellation: ownership + state checks, flip to `Cancelled`,
    /// decrement `OpenIntents`, emit `intent_cancelled`. No cooldown handling —
    /// callers gate that around one or more invocations.
    fn cancel_intent_core(env: &Env, user: &Address, intent_id: &BytesN<32>) {
        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(env, Error::IntentNotFound));

        if intent.user != *user {
            panic_with_error!(env, Error::Unauthorized);
        }

        if intent.state == IntentState::Accepted {
            panic_with_error!(env, Error::CannotCancelAccepted);
        }

        if intent.state != IntentState::Open && intent.state != IntentState::PartiallyFilled {
            panic_with_error!(env, Error::IntentNotOpen);
        }

        intent.state = IntentState::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(env, intent_id);

        // Decrement open_intents: intent is no longer open.
        let open: u64 = env
            .storage()
            .instance()
            .get(&DataKey::OpenIntents)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::OpenIntents, &open.saturating_sub(1));

        env.events().publish(
            (Symbol::new(env, "intent_cancelled"), user.clone()),
            intent_id.clone(),
        );
    }

    /// Solver begins fill by depositing dst_token into escrow. Starts dispute window.
    /// Replaces the direct transfer in fill_intent once this design is implemented.
    /// For now, this is a placeholder establishing the interface.
    pub fn begin_fill(env: Env, solver: Address, intent_id: BytesN<32>, fill_amount: i128) {
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.solver.as_ref() != Some(&solver) {
            panic_with_error!(&env, Error::Unauthorized);
        }

        if intent.state != IntentState::Accepted {
            panic_with_error!(&env, Error::IntentNotAccepted);
        }

        let now = env.ledger().timestamp();
        if now >= intent.deadline {
            panic_with_error!(&env, Error::FillWindowExpired);
        }

        // Transition to Filling and set dispute window deadline
        intent.state = IntentState::Filling;
        intent.dispute_deadline = Some(now + DISPUTE_WINDOW);

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "fill_begun"),),
            (intent_id, solver, fill_amount),
        );
    }

    /// User opens a dispute within the dispute window. Requires paying a bond.
    /// Transitions intent to Disputed state.
    pub fn open_dispute(env: Env, user: Address, intent_id: BytesN<32>) {
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

        if intent.state != IntentState::Filling {
            panic_with_error!(&env, Error::NoDisputeOpen);
        }

        let now = env.ledger().timestamp();
        if let Some(deadline) = intent.dispute_deadline {
            if now >= deadline {
                panic_with_error!(&env, Error::DisputeWindowExpired);
            }
        } else {
            panic_with_error!(&env, Error::NoFillEscrowed);
        }

        // Pull dispute bond from user
        let bond_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::BondToken)
            .unwrap();
        let bond_client = token::Client::new(&env, &bond_token);
        bond_client.transfer_from(&user, &env.current_contract_address(), &user, &DISPUTE_BOND);

        intent.state = IntentState::Disputed;
        intent.dispute_raised_at = Some(now);

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "dispute_opened"),),
            (intent_id, user),
        );
    }

    /// Arbiter resolves a dispute. Transitions intent to Resolved and handles bond/escrow.
    pub fn resolve_dispute(
        env: Env,
        arbiter: Address,
        intent_id: BytesN<32>,
        resolution: DisputeResolution,
    ) {
        arbiter.require_auth();
        Self::bump_instance_ttl(&env);

        // For now, arbiter is the admin. In v2, this could be a separate arbiter role.
        Self::require_admin(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.state != IntentState::Disputed {
            panic_with_error!(&env, Error::NoDisputeOpen);
        }

        let now = env.ledger().timestamp();
        if let Some(raised_at) = intent.dispute_raised_at {
            if now >= raised_at + ARBITER_WINDOW {
                panic_with_error!(&env, Error::ArbiterWindowExpired);
            }
        } else {
            panic_with_error!(&env, Error::NoDisputeOpen);
        }

        let bond_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::BondToken)
            .unwrap();
        let bond_client = token::Client::new(&env, &bond_token);

        intent.state = IntentState::Resolved;
        intent.resolution = Some(resolution.clone());

        match resolution {
            DisputeResolution::Upheld => {
                // Refund bond to user, slash solver
                bond_client.transfer(&env.current_contract_address(), &intent.user, &DISPUTE_BOND);

                if let Some(solver) = &intent.solver {
                    // Slash solver's bond
                    let mut solver_record: SolverRecord = env
                        .storage()
                        .persistent()
                        .get(&DataKey::Solver(solver.clone()))
                        .unwrap();
                    let slash_amount = solver_record.bond_amount / 10;
                    solver_record.bond_amount = solver_record.bond_amount.saturating_sub(slash_amount);
                    env.storage()
                        .persistent()
                        .set(&DataKey::Solver(solver.clone()), &solver_record);
                    Self::bump_solver_ttl(&env, solver);

                    // Transfer slashed bond to fee recipient
                    let fee_recipient: Address = env
                        .storage()
                        .instance()
                        .get(&DataKey::FeeRecipient)
                        .unwrap();
                    bond_client.transfer(&env.current_contract_address(), &fee_recipient, &slash_amount);
                }
            }
            DisputeResolution::Dismissed => {
                // Forfeit bond to fee recipient
                let fee_recipient: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::FeeRecipient)
                    .unwrap();
                bond_client.transfer(&env.current_contract_address(), &fee_recipient, &DISPUTE_BOND);
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "dispute_resolved"),),
            (intent_id, resolution),
        );
    }

    /// Permissionless: release escrowed fill after dispute window closes without a dispute.
    pub fn release_fill(env: Env, intent_id: BytesN<32>) {
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.state != IntentState::Filling {
            panic_with_error!(&env, Error::NoFillEscrowed);
        }

        let now = env.ledger().timestamp();
        if let Some(deadline) = intent.dispute_deadline {
            if now < deadline {
                panic_with_error!(&env, Error::DisputeWindowExpired);
            }
        } else {
            panic_with_error!(&env, Error::NoFillEscrowed);
        }

        // Transition to Filled (this is a simplified version; full impl would handle token release)
        intent.state = IntentState::Filled;
        intent.filled_at = Some(now);

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish((Symbol::new(&env, "fill_released"),), intent_id);
    }

    /// Permissionless: slash a solver that accepted but didn't fill within FILL_WINDOW
    pub fn slash_solver(env: Env, intent_id: BytesN<32>) {
        Self::bump_instance_ttl(&env);

        let mut intent = Self::load_intent(&env, &intent_id);

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

        let bond_token = intent.bond_token.clone();
        // Issue #193: proportional slash — the amount is a function of *both*
        // the solver's bond and the size of the intent they failed to fill
        // (`min_dst_amount` minus any partial progress), capped at the old flat
        // 10% baseline and floored at 1 stroop (issue #32).  See
        // `compute_slash_amount` for the formula and its edge-case proof.
        let unfilled = intent.min_dst_amount - intent.total_filled;
        let bond_before = Self::get_solver_bond_amount(&env, &solver_record, &bond_token);
        let slash_amount = Self::compute_slash_amount(bond_before, unfilled);
        Self::set_solver_bond_amount(
            &env,
            &mut solver_record,
            &bond_token,
            bond_before - slash_amount,
        );
        solver_record.fills_failed += 1;
        solver_record.last_slash_time = now;
        solver_record.active_intents = solver_record.active_intents.saturating_sub(1);
        Self::solver_intents_remove(&env, &solver_addr, &intent_id);

        // Decrement TotalBonded by the slashed amount (issue #231)
        let total_bonded: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalBonded)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalBonded, &(total_bonded - slash_amount));

        let cfg = Self::load_config(&env);
        // A solver whose bond no longer covers the minimum for the token that
        // backed this intent can't credibly back further fills -- take them out
        // of rotation until they top back up.
        if Self::get_solver_bond_amount(&env, &solver_record, &bond_token)
            < Self::min_bond_for_token(&env, &bond_token)
        {
            solver_record.is_active = false;
        }

        // Track this Accepted -> Slashed cycle. Once it reaches the
        // admin-configured cap, retire the intent instead of re-opening it
        // indefinitely (issue #241).
        intent.slash_cycles += 1;
        let abandoned = intent.slash_cycles >= cfg.max_slash_cycles;

        intent.solver = None;
        intent.solver_tier = 0; // #197: cleared with the solver assignment
        intent.deadline = now + cfg.intent_expiry;

        let open: u64 = env
            .storage()
            .instance()
            .get(&DataKey::OpenIntents)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::OpenIntents, &(open + 1));
        Self::add_to_open_intent_list(&env, &intent_id);

        // Persist both records BEFORE any token transfer so that a re-entrant
        // or back-to-back call on the same intent_id is rejected by the
        // IntentNotAccepted guard above (the state is already Open by then).
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver_addr.clone()), &solver_record);
        Self::bump_solver_ttl(&env, &solver_addr);
        Self::save_intent(&env, &intent_id, &intent);

        // Send slash to fee recipient, in the same token the solver bonded
        // (issue #187), with state already committed above.
        if slash_amount > 0 {
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .unwrap();
            let client = token::Client::new(&env, &bond_token);
            if fee_recipient_share > 0 {
                let fee_recipient: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::FeeRecipient)
                    .unwrap();
                client.transfer(
                    &env.current_contract_address(),
                    &fee_recipient,
                    &fee_recipient_share,
                );
            }
            if caller_rebate > 0 {
                client.transfer(
                    &env.current_contract_address(),
                    &caller,
                    &caller_rebate,
                );
            }
        }

        env.events().publish(
            (Symbol::new(&env, "solver_slashed"), solver_addr),
            (intent_id.clone(), slash_amount),
        );

        if abandoned {
            env.events()
                .publish((Symbol::new(&env, "intent_abandoned"),), intent_id);
        }
    }

    /// Permissionless: materialize an Open intent's Expired state once its
    /// deadline has passed. Expiry was previously only ever realized lazily
    /// inside accept_intent, so an intent nobody tried to accept could sit
    /// indefinitely showing state Open in storage despite being unfillable.
    pub fn expire_intent(env: Env, intent_id: BytesN<32>) {
        Self::bump_instance_ttl(&env);

        let mut intent = Self::load_intent(&env, &intent_id);

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
        Self::save_intent(&env, &intent_id, &intent);

        // Decrement open_intents: intent is no longer open.
        let open: u64 = env
            .storage()
            .instance()
            .get(&DataKey::OpenIntents)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::OpenIntents, &open.saturating_sub(1));

        env.events()
            .publish((Symbol::new(&env, "intent_expired"),), intent_id);
    }

    // ── Competitive Bid Window (#191) ─────────────────────────────────────────

    /// Admin-only: turn bid-window mode on or off.
    ///
    /// Issue #191: replaces the placeholder that reused
    /// `DataKey::DstAllowlistEnabled`.  When enabled, `submit_intent` opens new
    /// intents in `Bidding` state; solvers submit competing quotes via
    /// `bid_intent` for `BID_WINDOW` seconds, then anyone calls `settle_bids`
    /// to assign the highest bidder (or re-open the intent if nobody bid).
    /// Off by default; toggling it has no effect on intents already created.
    pub fn set_bid_window_enabled(env: Env, enabled: bool) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::BidWindowEnabled, &enabled);
        env.events()
            .publish((Symbol::new(&env, "bid_window_enabled"),), enabled);
    }

    /// A solver submits (or improves) a competing quote for an intent that is
    /// in the `Bidding` state.
    ///
    /// * The solver must currently satisfy `is_solver_eligible` (registered,
    ///   active, bonded at or above the minimum) — the same gate `accept_intent`
    ///   applies.
    /// * `quoted_dst_amount` must be strictly greater than the current best
    ///   bid, per `BestBidRecord`'s doc comment.  **Tie-break:** the first
    ///   solver to reach a given amount keeps the lead; a later equal quote does
    ///   *not* displace it.
    /// * Only callable while `now < intent.deadline` (the bid-window end).
    pub fn bid_intent(env: Env, solver: Address, intent_id: BytesN<32>, quoted_dst_amount: i128) {
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        if quoted_dst_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.state != IntentState::Bidding {
            panic_with_error!(&env, Error::IntentNotBidding);
        }

        let now = env.ledger().timestamp();
        // Boundary semantics: the bid window is EXCLUSIVE for bidding, matching
        // `accept_intent` — `now >= deadline` rejects at the boundary second.
        if now >= intent.deadline {
            panic_with_error!(&env, Error::BidWindowClosed);
        }

        if !Self::is_solver_eligible(env.clone(), solver.clone()) {
            panic_with_error!(&env, Error::SolverInactive);
        }

        if let Some(best) = env
            .storage()
            .persistent()
            .get::<_, BestBidRecord>(&DataKey::BestBid(intent_id.clone()))
        {
            // Strictly higher only (ties keep the incumbent).
            if quoted_dst_amount <= best.quoted_dst_amount {
                panic_with_error!(&env, Error::BidNotHigher);
            }
        }

        env.storage().persistent().set(
            &DataKey::BestBid(intent_id.clone()),
            &BestBidRecord {
                solver: solver.clone(),
                quoted_dst_amount,
            },
        );
        env.storage().persistent().extend_ttl(
            &DataKey::BestBid(intent_id.clone()),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );

        env.events().publish(
            (Symbol::new(&env, "bid_submitted"), solver),
            (intent_id, quoted_dst_amount),
        );
    }

    /// Permissionless: close the bid window for a `Bidding` intent and act on
    /// the result, mirroring `expire_intent`'s permissionless-materialization
    /// pattern.
    ///
    /// * **A winning bid exists and the solver is still eligible:** the intent
    ///   moves to `Accepted` with a fresh `FILL_WINDOW` deadline and
    ///   `accept_intent`'s bookkeeping (`solver`, `active_intents`,
    ///   `OpenIntents`).
    /// * **No bid was received** (or the leading bidder's bond has since
    ///   dropped below the eligibility floor): the intent is re-opened as
    ///   `Open` with a fresh `INTENT_EXPIRY` deadline rather than getting stuck
    ///   in `Bidding` forever.
    pub fn settle_bids(env: Env, intent_id: BytesN<32>) {
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.state != IntentState::Bidding {
            panic_with_error!(&env, Error::IntentNotBidding);
        }

        let now = env.ledger().timestamp();
        // INCLUSIVE for settlement, matching `expire_intent`: valid at the
        // deadline second itself.
        if now < intent.deadline {
            panic_with_error!(&env, Error::BidWindowStillOpen);
        }

        let cfg = Self::load_config(&env);
        let best = env
            .storage()
            .persistent()
            .get::<_, BestBidRecord>(&DataKey::BestBid(intent_id.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::BestBid(intent_id.clone()));

        let winner = best.filter(|b| Self::is_solver_eligible(env.clone(), b.solver.clone()));

        match winner {
            Some(b) => {
                // Mirror accept_intent.
                let mut solver_record: SolverRecord = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Solver(b.solver.clone()))
                    .unwrap();
                solver_record.active_intents += 1;
                env.storage()
                    .persistent()
                    .set(&DataKey::Solver(b.solver.clone()), &solver_record);
                Self::bump_solver_ttl(&env, &b.solver);

                intent.solver = Some(b.solver.clone());
                intent.state = IntentState::Accepted;
                intent.bond_token = Self::load_bond_token(&env);
                intent.deadline = now + cfg.fill_window;

                // The intent leaves the open pool (a solver now owns it).
                let open: u64 = env
                    .storage()
                    .instance()
                    .get(&DataKey::OpenIntents)
                    .unwrap_or(0);
                env.storage()
                    .instance()
                    .set(&DataKey::OpenIntents, &open.saturating_sub(1));

                env.storage()
                    .persistent()
                    .set(&DataKey::Intent(intent_id.clone()), &intent);
                Self::bump_intent_ttl(&env, &intent_id);

                env.events().publish(
                    (Symbol::new(&env, "intent_accepted"), b.solver),
                    (intent_id.clone(), intent.deadline),
                );
                env.events().publish(
                    (Symbol::new(&env, "bids_settled"),),
                    (intent_id, b.quoted_dst_amount),
                );
            }
            None => {
                // No usable bid: re-open as Open. OpenIntents already counts
                // this intent (submit_intent incremented it for Bidding), so the
                // counter is left unchanged.
                intent.state = IntentState::Open;
                intent.deadline = now + cfg.intent_expiry;
                env.storage()
                    .persistent()
                    .set(&DataKey::Intent(intent_id.clone()), &intent);
                Self::bump_intent_ttl(&env, &intent_id);

                env.events()
                    .publish((Symbol::new(&env, "bids_settled_no_winner"),), intent_id);
            }
        }
    }

    /// The current leading bid for an intent in `Bidding` state, if any.
    pub fn get_best_bid(env: Env, intent_id: BytesN<32>) -> Option<BestBidRecord> {
        env.storage().persistent().get(&DataKey::BestBid(intent_id))
    }

    // ── Dispute Resolution (#188) ────────────────────────────────────────────

    /// Admin-only: set the address allowed to call `resolve_dispute`.
    /// Until this is called the `Admin` acts as arbiter (the design doc's v1
    /// default — docs/dispute-resolution-design.md §"Arbiter role").
    pub fn set_arbiter(env: Env, arbiter: Address) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
        env.events()
            .publish((Symbol::new(&env, "arbiter_updated"),), arbiter);
    }

    /// The current arbiter (explicit `set_arbiter` value, else the `Admin`).
    pub fn get_arbiter(env: Env) -> Address {
        Self::load_arbiter(&env)
    }

    /// Solver delivers a completing fill into contract **escrow**, starting the
    /// dispute window (issue #188).  Unlike `fill_intent`, the output tokens are
    /// held by the contract — not sent straight to the user — until either the
    /// window closes cleanly (`release_fill`) or the arbiter rules on a dispute
    /// (`resolve_dispute`).
    ///
    /// Only a single completing fill is supported on this path: `fill_amount`
    /// must bring `total_filled` to at least `min_dst_amount`.  Partial fills
    /// keep using `fill_intent`.
    pub fn begin_fill(env: Env, solver: Address, intent_id: BytesN<32>, fill_amount: i128) {
        solver.require_auth();
        Self::require_not_paused(&env);
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.state != IntentState::Accepted {
            panic_with_error!(&env, Error::IntentNotAcceptedForFill);
        }
        if intent.solver.as_ref() != Some(&solver) {
            panic_with_error!(&env, Error::Unauthorized);
        }

        let now = env.ledger().timestamp();
        // Fill-window deadline is EXCLUSIVE, matching `fill_intent`.
        if now >= intent.deadline {
            panic_with_error!(&env, Error::FillWindowExpired);
        }
        if fill_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        if intent.total_filled + fill_amount < intent.min_dst_amount {
            panic_with_error!(&env, Error::InsufficientOutput);
        }

        // ── Effects first (CEI) ──────────────────────────────────────────────
        // `fill_amount` holds the escrowed amount while state is Filling /
        // Disputed; it is folded into `total_filled` only on resolution.
        intent.state = IntentState::Filling;
        intent.dispute_deadline = Some(now + DISPUTE_WINDOW);
        intent.fill_amount = Some(fill_amount);
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        // ── Interaction: pull the output into escrow ─────────────────────────
        token::Client::new(&env, &intent.dst_token).transfer(
            &solver,
            &env.current_contract_address(),
            &fill_amount,
        );

        env.events().publish(
            (Symbol::new(&env, "fill_begun"), solver),
            (intent_id, fill_amount, now + DISPUTE_WINDOW),
        );
    }

    /// User contests an escrowed fill during the dispute window (issue #188).
    /// Freezes the escrow until the arbiter rules (`resolve_dispute`) or the
    /// arbiter window times out (`release_fill`).
    pub fn dispute_fill(env: Env, user: Address, intent_id: BytesN<32>) {
        user.require_auth();
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.state != IntentState::Filling {
            panic_with_error!(&env, Error::IntentNotFilling);
        }
        if intent.user != user {
            panic_with_error!(&env, Error::Unauthorized);
        }

        let now = env.ledger().timestamp();
        let deadline = intent.dispute_deadline.unwrap_or(0);
        // Dispute window is EXCLUSIVE: at `now == deadline` it has closed.
        if now >= deadline {
            panic_with_error!(&env, Error::DisputeWindowClosed);
        }

        intent.state = IntentState::Disputed;
        intent.dispute_raised_at = Some(now);
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events()
            .publish((Symbol::new(&env, "fill_disputed"), user), intent_id);
    }

    /// Arbiter-only: rule on a disputed fill (issue #188).
    ///
    /// In **both** outcomes the escrowed tokens are delivered to the user (the
    /// design doc is explicit that the dispute only decides the solver's fate):
    /// * `Upheld` — solver misconduct: user receives the **full** escrow (no
    ///   protocol fee) and the solver's bond is slashed by the same
    ///   proportional formula `slash_solver` uses.
    /// * `Dismissed` — fill was legitimate: user receives `escrow − fee`, the
    ///   protocol fee is taken, and the solver is credited a completed fill.
    pub fn resolve_dispute(
        env: Env,
        arbiter: Address,
        intent_id: BytesN<32>,
        resolution: DisputeResolution,
    ) {
        arbiter.require_auth();
        Self::bump_instance_ttl(&env);

        if arbiter != Self::load_arbiter(&env) {
            panic_with_error!(&env, Error::NotArbiter);
        }

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.state != IntentState::Disputed {
            panic_with_error!(&env, Error::IntentNotDisputed);
        }

        let now = env.ledger().timestamp();
        let escrow = intent.fill_amount.unwrap_or(0);
        let solver_addr = intent.solver.clone().unwrap();
        let mut solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver_addr.clone()))
            .unwrap();
        solver_record.active_intents = solver_record.active_intents.saturating_sub(1);

        let fee_recipient: Address = env
            .storage()
            .instance()
            .get(&DataKey::FeeRecipient)
            .unwrap();
        let dst_client = token::Client::new(&env, &intent.dst_token);

        let mut slash_amount = 0i128;
        let mut fee = 0i128;
        match &resolution {
            DisputeResolution::Upheld => {
                let bond_token = intent.bond_token.clone();
                let unfilled = intent.min_dst_amount - intent.total_filled;
                let bond_before =
                    Self::get_solver_bond_amount(&env, &solver_record, &bond_token);
                slash_amount = Self::compute_slash_amount(bond_before, unfilled);
                Self::set_solver_bond_amount(
                    &env,
                    &mut solver_record,
                    &bond_token,
                    bond_before - slash_amount,
                );
                solver_record.fills_failed += 1;
                solver_record.last_slash_time = now;
                if Self::get_solver_bond_amount(&env, &solver_record, &bond_token)
                    < Self::min_bond_for_token(&env, &bond_token)
                {
                    solver_record.is_active = false;
                }
            }
            DisputeResolution::Dismissed => {
                fee = escrow
                    .checked_mul(PROTOCOL_FEE_BPS)
                    .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow))
                    .checked_div(10_000)
                    .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow));
                solver_record.fills_completed += 1;
                solver_record.total_volume += escrow;
            }
        }

        // ── Effects ─────────────────────────────────────────────────────────
        intent.total_filled += escrow;
        intent.fill_amount = Some(intent.total_filled);
        intent.filled_at = Some(now);
        intent.state = IntentState::Resolved;
        intent.resolution = Some(resolution.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver_addr.clone()), &solver_record);
        Self::bump_solver_ttl(&env, &solver_addr);
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        if resolution == DisputeResolution::Dismissed {
            let total_vol: i128 = env
                .storage()
                .instance()
                .get(&DataKey::TotalVolume)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::TotalVolume, &(total_vol + escrow));
        }

        // ── Interactions ────────────────────────────────────────────────────
        let contract = env.current_contract_address();
        dst_client.transfer(&contract, &intent.user, &(escrow - fee));
        if fee > 0 {
            dst_client.transfer(&contract, &fee_recipient, &fee);
        }
        if slash_amount > 0 {
            token::Client::new(&env, &intent.bond_token).transfer(
                &contract,
                &fee_recipient,
                &slash_amount,
            );
        }

        env.events().publish(
            (Symbol::new(&env, "dispute_resolved"), solver_addr),
            (intent_id, escrow - fee, slash_amount),
        );
    }

    /// Permissionless (issue #188): settle an escrowed fill once its window has
    /// elapsed.
    ///
    /// * `Filling` + `now >= dispute_deadline` — no dispute was raised: the
    ///   user receives `escrow − fee`, the protocol fee is taken, and the
    ///   solver is credited a completed fill (intent → `Filled`).
    /// * `Disputed` + `now >= dispute_raised_at + ARBITER_WINDOW` — the arbiter
    ///   failed to rule in time: the user receives the **full** escrow with no
    ///   fee and no slash (the conservative default), intent → `Resolved` with
    ///   `resolution == None` marking the timeout.
    pub fn release_fill(env: Env, intent_id: BytesN<32>) {
        Self::bump_instance_ttl(&env);

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        let now = env.ledger().timestamp();
        let escrow = intent.fill_amount.unwrap_or(0);
        let solver_addr = intent.solver.clone().unwrap();
        let mut solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver_addr.clone()))
            .unwrap();
        solver_record.active_intents = solver_record.active_intents.saturating_sub(1);

        let fee_recipient: Address = env
            .storage()
            .instance()
            .get(&DataKey::FeeRecipient)
            .unwrap();
        let dst_client = token::Client::new(&env, &intent.dst_token);
        let contract = env.current_contract_address();

        let fee = match intent.state {
            IntentState::Filling => {
                let deadline = intent.dispute_deadline.unwrap_or(0);
                if now < deadline {
                    panic_with_error!(&env, Error::DisputeWindowStillOpen);
                }
                let fee = escrow
                    .checked_mul(PROTOCOL_FEE_BPS)
                    .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow))
                    .checked_div(10_000)
                    .unwrap_or_else(|| panic_with_error!(&env, Error::FeeOverflow));
                solver_record.fills_completed += 1;
                solver_record.total_volume += escrow;
                intent.state = IntentState::Filled;
                fee
            }
            IntentState::Disputed => {
                let raised = intent.dispute_raised_at.unwrap_or(0);
                if now < raised + ARBITER_WINDOW {
                    panic_with_error!(&env, Error::DisputeWindowStillOpen);
                }
                intent.state = IntentState::Resolved;
                intent.resolution = None; // marks an arbiter timeout
                0
            }
            _ => panic_with_error!(&env, Error::IntentNotFilling),
        };

        intent.total_filled += escrow;
        intent.fill_amount = Some(intent.total_filled);
        intent.filled_at = Some(now);

        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver_addr.clone()), &solver_record);
        Self::bump_solver_ttl(&env, &solver_addr);
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        // Tokens reach the user in both branches, so cumulative volume grows by
        // the escrowed amount regardless of outcome.
        let total_vol: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalVolume)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalVolume, &(total_vol + escrow));

        dst_client.transfer(&contract, &intent.user, &(escrow - fee));
        if fee > 0 {
            dst_client.transfer(&contract, &fee_recipient, &fee);
        }

        env.events().publish(
            (Symbol::new(&env, "fill_released"), solver_addr),
            (intent_id, escrow - fee),
        );
    }

    // ── Multi-Bond-Token Admin (#187) ────────────────────────────────────────

    /// Admin-only (issue #187): approve `token` for use as a solver bond.
    /// Probes the SEP-41 interface via `decimals()` — a bad address traps and
    /// reverts before anything is stored, mirroring `propose_add_dst_token`.
    pub fn add_allowed_bond_token(env: Env, token: Address) {
        Self::require_admin(&env);
        let _decimals = token::Client::new(&env, &token).decimals();
        env.storage()
            .instance()
            .set(&DataKey::AllowedBondToken(token.clone()), &true);
        env.events()
            .publish((Symbol::new(&env, "bond_token_allowed"),), token);
    }

    /// Admin-only (issue #187): remove `token` from the approved bond set.
    /// Solvers already bonded in it keep their funds and can still withdraw or
    /// deregister; they simply cannot add more bond in this token.
    pub fn remove_allowed_bond_token(env: Env, token: Address) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .remove(&DataKey::AllowedBondToken(token.clone()));
        env.events()
            .publish((Symbol::new(&env, "bond_token_disallowed"),), token);
    }

    /// `true` if `token` may currently be used as a solver bond (the legacy
    /// default token, or an explicitly approved one).
    pub fn is_allowed_bond_token(env: Env, token: Address) -> bool {
        Self::is_bond_token_allowed(&env, &token)
    }

    /// Admin-only (issue #187): set the minimum bond for `token`.  Ignored for
    /// the legacy default token, whose minimum always comes from
    /// `ProtocolConfig` / `set_config`.
    pub fn set_bond_token_min(env: Env, token: Address, amount: i128) {
        Self::require_admin(&env);
        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::MinBond(token.clone()), &amount);
        env.events()
            .publish((Symbol::new(&env, "bond_token_min_set"),), (token, amount));
    }

    /// The effective minimum bond for `token` (issue #187).
    pub fn get_bond_token_min(env: Env, token: Address) -> i128 {
        Self::min_bond_for_token(&env, &token)
    }

    /// A solver's bond balance in a specific token (issue #187).
    pub fn get_solver_bond(env: Env, solver: Address, token: Address) -> i128 {
        match env
            .storage()
            .persistent()
            .get::<_, SolverRecord>(&DataKey::Solver(solver))
        {
            Some(record) => Self::get_solver_bond_amount(&env, &record, &token),
            None => 0,
        }
    }

    /// Every `(token, amount)` bond a solver currently holds (issue #187).
    pub fn get_solver_bonds(env: Env, solver: Address) -> Vec<(Address, i128)> {
        let mut out: Vec<(Address, i128)> = Vec::new(&env);
        let record: SolverRecord = match env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
        {
            Some(r) => r,
            None => return out,
        };
        let default_token = Self::load_bond_token(&env);
        if record.bond_amount > 0 {
            out.push_back((default_token.clone(), record.bond_amount));
        }
        for i in 0..record.bond_tokens.len() {
            let t = record.bond_tokens.get(i).unwrap();
            if t == default_token {
                continue;
            }
            let amt: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::SolverBond(solver.clone(), t.clone()))
                .unwrap_or(0);
            if amt > 0 {
                out.push_back((t, amt));
            }
        }
        out
    }

    // ── Batch Operations ──────────────────────────────────────────────────────
    //
    // Each `batch_*` entrypoint is a thin loop over the `*_inner` body of the
    // corresponding single-item entrypoint. They exist purely to amortise
    // per-transaction overhead for solvers and users that operate on many
    // intents at once.
    //
    // Auth: the actor (`user` / `solver`) is authorised exactly once, at the
    // top of the batch call. `Address::require_auth()` may only be invoked once
    // per address per contract invocation — calling the public single-item
    // entrypoints in a loop would hit `Auth, ExistingValue` on the second
    // iteration — so the loop bodies call the un-gated `*_inner` functions.
    //
    // Atomicity: a batch is one Soroban transaction, so a failure on any item
    // reverts every earlier item in the same call — there is no partial
    // success. Callers that want per-item isolation must send separate
    // transactions.
    //
    // Resource bound: every batch is capped at `MAX_BATCH_SIZE` items, checked
    // up front so an over-sized batch panics with `BatchTooLarge` before any
    // auth, state change, or token movement.

    /// Submit multiple intents in a single transaction. Returns the new intent
    /// ids in input order. Reverts the whole batch on any failure; capped at
    /// `MAX_BATCH_SIZE`.
    pub fn batch_submit_intent(
        env: Env,
        user: Address,
        intents: soroban_sdk::Vec<(String, String, i128, Address, i128, Option<u64>)>,
    ) -> soroban_sdk::Vec<BytesN<32>> {
        if intents.len() > MAX_BATCH_SIZE {
            panic_with_error!(&env, Error::BatchTooLarge);
        }
        user.require_auth();

        let mut result = soroban_sdk::Vec::new(&env);
        for (src_chain, src_token, src_amount, dst_token, min_dst_amount, deadline) in intents {
            let intent_id = Self::submit_intent_inner(
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

    /// Accept multiple intents in a single transaction. Reverts the whole
    /// batch on any failure; capped at `MAX_BATCH_SIZE`.
    pub fn batch_accept_intent(
        env: Env,
        solver: Address,
        intent_ids: soroban_sdk::Vec<BytesN<32>>,
    ) {
        if intent_ids.len() > MAX_BATCH_SIZE {
            panic_with_error!(&env, Error::BatchTooLarge);
        }
        solver.require_auth();

        for intent_id in intent_ids {
            Self::accept_intent_inner(env.clone(), solver.clone(), intent_id);
        }
    }

    /// Fill multiple intents in a single transaction (#199).
    ///
    /// `fills` is a list of `(intent_id, fill_amount)` pairs. Each pair is
    /// handed to the `fill_intent` body unchanged, so mixed outcomes within one
    /// batch are fine: some pairs may complete their intent (`Filled`) while
    /// others only advance it (`PartiallyFilled` and re-opened). Every intent
    /// must be currently `Accepted` by `solver`, and `solver` must be funded
    /// for the sum of all `fill_amount`s plus fees, or the whole batch reverts.
    /// Capped at `MAX_BATCH_SIZE`.
    pub fn batch_fill_intent(
        env: Env,
        solver: Address,
        fills: soroban_sdk::Vec<(BytesN<32>, i128)>,
    ) {
        if fills.len() > MAX_BATCH_SIZE {
            panic_with_error!(&env, Error::BatchTooLarge);
        }
        solver.require_auth();

        for (intent_id, fill_amount) in fills {
            Self::fill_intent_inner(env.clone(), solver.clone(), intent_id, fill_amount);
        }
    }

    /// Cancel multiple intents in a single transaction (#199).
    ///
    /// Every id must belong to `user` and be in a cancellable state
    /// (`Open` / `PartiallyFilled`), or the whole batch reverts. The per-user
    /// `CANCEL_COOLDOWN` is checked once for the whole call and stamped once at
    /// the end, so one batch counts as a single cancel action for
    /// rate-limiting — a user can clear all of their open intents in one
    /// transaction without tripping the anti-spam gate on themselves. Capped
    /// at `MAX_BATCH_SIZE`.
    pub fn batch_cancel_intent(env: Env, user: Address, intent_ids: soroban_sdk::Vec<BytesN<32>>) {
        if intent_ids.len() > MAX_BATCH_SIZE {
            panic_with_error!(&env, Error::BatchTooLarge);
        }

        user.require_auth();
        Self::bump_instance_ttl(&env);

        let now = env.ledger().timestamp();
        Self::check_cancel_cooldown(&env, &user, now);
        for intent_id in intent_ids {
            Self::cancel_intent_core(&env, &user, &intent_id);
        }
        Self::stamp_cancel_cooldown(&env, &user, now);
    }

    /// Fill multiple intents in a single transaction.
    ///
    /// Each element of `fills` is `(intent_id, fill_amount)`.  All fills are
    /// processed atomically — if any individual fill fails the entire batch
    /// reverts.
    ///
    /// Bounded by [`MAX_BATCH_SIZE`] to prevent resource exhaustion.
    /// See `docs/149-resource-cost-per-entrypoint.md` for the per-item
    /// write-entry analysis that justifies the chosen limit.
    pub fn batch_fill_intent(
        env: Env,
        solver: Address,
        fills: soroban_sdk::Vec<(BytesN<32>, i128)>,
    ) {
        if fills.len() > MAX_BATCH_SIZE as usize {
            panic_with_error!(&env, Error::ZeroAmount); // No dedicated error; reuse nearest
        }

        for (intent_id, fill_amount) in fills {
            Self::fill_intent(env.clone(), solver.clone(), intent_id, fill_amount);
        }
    }

    /// Cancel multiple Open intents belonging to `user` in a single
    /// transaction.
    ///
    /// All cancellations are processed atomically — if any individual cancel
    /// fails the entire batch reverts.
    ///
    /// Bounded by [`MAX_BATCH_SIZE`] to prevent resource exhaustion.
    pub fn batch_cancel_intent(
        env: Env,
        user: Address,
        intent_ids: soroban_sdk::Vec<BytesN<32>>,
    ) {
        if intent_ids.len() > MAX_BATCH_SIZE as usize {
            panic_with_error!(&env, Error::ZeroAmount); // No dedicated error; reuse nearest
        }

        for intent_id in intent_ids {
            Self::cancel_intent(env.clone(), user.clone(), intent_id);
        }
    }

    // ── Fill Window Extension ─────────────────────────────────────────────────

    /// Per-intent cumulative fill-window extension budget for `solver`, in
    /// seconds, gated by the solver's reputation tier (#200).
    ///
    /// The tier is derived locally from the solver's own `SolverRecord`
    /// (`fills_completed` plus `compute_reputation_score`) rather than from a
    /// cross-contract call into `solver_registry`, so this ships before the
    /// registry does; the return value is all `request_extension` consumes,
    /// so swapping in a real tier lookup later is not an ABI break.
    ///
    /// Tiers mirror the fill-window perk table in
    /// `docs/solver-registry-design.md` (+10% / +20% / +30% / +50% on top of
    /// the base `MAX_EXTENSION_DURATION`). An unranked solver — no record, no
    /// fills, or a zero reputation score — gets exactly `MAX_EXTENSION_DURATION`,
    /// i.e. the historical one-shot behaviour, so nobody who is not yet tiered
    /// sees a change. Every result is clamped to `MAX_TOTAL_EXTENSION`, the
    /// tier-independent anti-abuse ceiling.
    fn extension_cap_secs(env: &Env, solver: &Address) -> u64 {
        let base = MAX_EXTENSION_DURATION;

        let record: Option<SolverRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()));

        let cap = match record {
            None => base,
            Some(r) => {
                let score = Self::compute_reputation_score(&r);
                let fills = r.fills_completed;
                // (min fills, min reputation bps) → extension multiplier %
                if fills >= 1_000 && score >= 9_000 {
                    base * 150 / 100 // Platinum: +50%
                } else if fills >= 200 && score >= 8_500 {
                    base * 130 / 100 // Gold: +30%
                } else if fills >= 50 && score >= 7_000 {
                    base * 120 / 100 // Silver: +20%
                } else if fills >= 10 && score >= 5_000 {
                    base * 110 / 100 // Bronze: +10%
                } else {
                    base // Unranked: unchanged one-shot behaviour
                }
            }
        };

        cap.min(MAX_TOTAL_EXTENSION)
    }

    /// Solver requests a grace-period extension on an Accepted intent (#200).
    ///
    /// Each call pushes the deadline out by `MAX_EXTENSION_DURATION`. Multiple
    /// extensions are allowed as long as the running total for the intent stays
    /// within the solver's reputation-tier budget (`extension_cap_secs`); an
    /// unranked solver's budget equals a single `MAX_EXTENSION_DURATION`, so the
    /// historical one-extension-per-intent rule is preserved for them. The
    /// per-intent total can never exceed `MAX_TOTAL_EXTENSION` regardless of
    /// tier — that is the anti-abuse backstop.
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
            panic_with_error!(&env, Error::ExtensionAlreadyGranted);
        }

        // Cumulative extension budget already consumed on this intent.
        let used: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::ExtensionGranted(intent_id.clone()))
            .unwrap_or(0);

        let cap = Self::extension_cap_secs(&env, &solver);
        let new_used = used
            .checked_add(MAX_EXTENSION_DURATION)
            .unwrap_or(u64::MAX);
        if new_used > cap {
            panic_with_error!(&env, Error::ExtensionCapExceeded);
        }

        // Extend the deadline by one extension quantum and record the new total.
        intent.deadline += MAX_EXTENSION_DURATION;
        env.storage()
            .persistent()
            .set(&DataKey::ExtensionGranted(intent_id.clone()), &new_used);

        Self::save_intent(&env, &intent_id, &intent);

        env.events().publish(
            (Symbol::new(&env, "extension_granted"), solver),
            (intent_id, intent.deadline),
        );
    }

    // ── Backstop Pool ──────────────────────────────────────────────────────────

    /// User claims a one-time backstop compensation for an intent that was
    /// slashed while they were waiting.
    ///
    /// # Eligibility
    ///
    /// The caller (`user`) must be the owner of the intent identified by
    /// `intent_id`, and the intent must currently be in `Open` or
    /// `PartiallyFilled` state (i.e. it was re-opened after a slash).  The
    /// claim is permitted regardless of how many slash cycles the intent has
    /// experienced — but only **once per intent**, not once per slash event.
    ///
    /// # Payout bound
    ///
    /// The compensation is capped at `MAX_BACKSTOP_CLAIM_BPS` (1%) of the
    /// current pool balance.  This prevents a single large intent from
    /// draining the pool that is meant to compensate many users over time.
    ///
    /// If the pool is empty or `backstop_bps` has never been configured > 0
    /// (meaning no funds have ever been diverted into it), the call reverts
    /// with `BackstopPoolEmpty`.
    ///
    /// If the pool balance is smaller than `MAX_BACKSTOP_CLAIM_BPS / 10_000`
    /// of itself (always ≥ 1 stroop for any non-zero pool), the payout is the
    /// full pool balance.
    ///
    /// # Checks-Effects-Interactions
    ///
    /// Consistent with the rest of the contract: storage is mutated (claim
    /// flag set, pool decremented) *before* the bond token transfer executes.
    pub fn claim_backstop_compensation(env: Env, intent_id: BytesN<32>) {
        Self::bump_instance_ttl(&env);

        // ── Checks ────────────────────────────────────────────────────────────

        // Load the intent. The user must have submitted it.
        let intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        // Only the intent's original user may claim.
        // require_auth enforces the signature requirement; the ownership check
        // below confirms they're claiming their own intent.
        intent.user.require_auth();

        // The intent must be in a state that indicates it was slashed at least
        // once: after slash_solver the intent is re-opened as Open or
        // PartiallyFilled.  A freshly-submitted intent that was never slashed
        // would be Open too, but we require that a slash event actually happened.
        // We detect this by checking that the intent has a non-zero
        // fills_failed-equivalent: since IntentRecord doesn't carry a slash
        // counter, we instead check that the intent's state is Open or
        // PartiallyFilled AND the solver field is None AND the intent has been
        // through at least one accept cycle.  The most reliable proxy here is
        // that the intent's deadline has been reset by slash_solver (i.e. the
        // intent is back open after being accepted), but that is indistinguishable
        // from a freshly submitted intent.
        //
        // Design decision: rather than adding a slash-count field to IntentRecord
        // (which would break existing storage layouts), we accept that any user
        // with an Open/PartiallyFilled intent can call this.  The pool only
        // contains funds if backstop_bps > 0 and at least one slash has happened,
        // so an intent that was never slashed would face an empty pool and be
        // rejected by the BackstopPoolEmpty guard below.  The double-claim guard
        // (BackstopClaimed key) prevents a user from claiming twice on the same
        // intent.
        if intent.state != IntentState::Open && intent.state != IntentState::PartiallyFilled {
            panic_with_error!(&env, Error::IntentNotAccepted); // re-use: "not in claimable state"
        }

        // Double-claim guard: each intent may only be claimed once, regardless of
        // how many slash cycles it accumulates.
        if env
            .storage()
            .persistent()
            .has(&DataKey::BackstopClaimed(intent_id.clone()))
        {
            panic_with_error!(&env, Error::BackstopAlreadyClaimed);
        }

        // Pool must be non-empty.
        let pool: i128 = env
            .storage()
            .instance()
            .get(&DataKey::BackstopPool)
            .unwrap_or(0);
        if pool <= 0 {
            panic_with_error!(&env, Error::BackstopPoolEmpty);
        }

        // ── Compute payout ────────────────────────────────────────────────────

        // Cap: MAX_BACKSTOP_CLAIM_BPS (1%) of the current pool balance.
        // Floor: at least 1 stroop (so the payout is never zero when pool > 0).
        let claim_cap = (pool
            .checked_mul(MAX_BACKSTOP_CLAIM_BPS)
            .unwrap_or(pool)
            .checked_div(10_000)
            .unwrap_or(1))
        .max(1);
        // Payout is the smaller of the cap and the full pool balance.
        let payout = claim_cap.min(pool);

        // ── Effects ───────────────────────────────────────────────────────────

        // Mark this intent as claimed before transferring, preventing re-entrancy
        // or a back-to-back call from double-paying.
        env.storage()
            .persistent()
            .set(&DataKey::BackstopClaimed(intent_id.clone()), &true);
        Self::bump_intent_ttl(&env, &intent_id); // share TTL with the intent record

        // Decrement the pool by the payout amount.
        env.storage()
            .instance()
            .set(&DataKey::BackstopPool, &(pool - payout));

        // ── Interaction ───────────────────────────────────────────────────────

        let bond_token = Self::load_bond_token(&env);
        let client = token::Client::new(&env, &bond_token);
        client.transfer(&env.current_contract_address(), &intent.user, &payout);

        env.events().publish(
            (Symbol::new(&env, "backstop_claimed"), intent.user),
            (intent_id, payout, pool - payout),
        );
    }

    // ── Views ─────────────────────────────────────────────────────────────────

    /// Fetch an intent's full record by id, or None if it was never submitted.
    pub fn get_intent(env: Env, intent_id: BytesN<32>) -> Option<IntentRecord> {
        env.storage().persistent().get(&DataKey::Intent(intent_id))
    }

    /// Issue #232: Get an intent's deadline-adjusted state without mutating storage.
    /// Returns `Expired` for an Open/PartiallyFilled intent whose deadline has passed,
    /// giving callers a true picture of the intent's logical state without requiring
    /// them to independently track deadlines and compare against wall-clock time.
    ///
    /// For all other states (Accepted, Filled, Cancelled, Slashed, Bidding) this
    /// returns the stored state as-is. Boundary semantics match expire_intent:
    /// deadline is INCLUSIVE (now >= intent.deadline means expired).
    pub fn get_effective_intent_state(env: Env, intent_id: BytesN<32>) -> Option<IntentState> {
        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id))?;

        let now = env.ledger().timestamp();
        // Only Open/PartiallyFilled intents can be logically expired; other states
        // are already terminal or intermediate with different semantics.
        if (intent.state == IntentState::Open || intent.state == IntentState::PartiallyFilled)
            && now >= intent.deadline
        {
            intent.state = IntentState::Expired;
        }

        Some(intent.state)
    }

    /// Fetch a solver's full record by address, or None if never registered.
    pub fn get_solver(env: Env, solver: Address) -> Option<SolverRecord> {
        env.storage().persistent().get(&DataKey::Solver(solver))
    }

    /// List the intent IDs currently `Accepted` by `solver` (issue #245).
    /// Returns an empty `Vec` if the solver has no in-flight obligations (or
    /// has never accepted an intent). Lets a solver bot recovering from a
    /// crash rediscover its own active intents without replaying events.
    pub fn get_solver_intents(env: Env, solver: Address) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::SolverIntents(solver))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Cumulative `(volume, fees)` for a single destination token across all
    /// fills, both in the token's smallest unit (issue #246). Returns
    /// `(0, 0)` for a token that has never been filled against.
    pub fn get_token_stats(env: Env, token: Address) -> (i128, i128) {
        let volume: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TokenVolume(token.clone()))
            .unwrap_or(0);
        let fees: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TokenFees(token))
            .unwrap_or(0);
        (volume, fees)
    }

    /// Solver declares which `src_chain`/`dst_token` combinations it services
    /// (issue #255). Purely advisory -- `accept_intent` does not enforce
    /// this, so a solver may still accept any intent it's otherwise eligible
    /// for regardless of declared routes.
    pub fn set_solver_routes(
        env: Env,
        solver: Address,
        src_chains: Vec<String>,
        dst_tokens: Vec<Address>,
    ) {
        solver.require_auth();
        if src_chains.len() > MAX_ROUTE_ENTRIES || dst_tokens.len() > MAX_ROUTE_ENTRIES {
            panic_with_error!(&env, Error::TooManyRouteEntries);
        }
        env.storage().persistent().set(
            &DataKey::SolverRoutes(solver.clone()),
            &(src_chains, dst_tokens),
        );
        env.storage().persistent().extend_ttl(
            &DataKey::SolverRoutes(solver),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    /// A solver's declared route preference, or `(empty, empty)` if it has
    /// never called `set_solver_routes` -- meaning "no declared preference",
    /// i.e. it is presumed to serve every route (issue #255).
    pub fn get_solver_routes(env: Env, solver: Address) -> (Vec<String>, Vec<Address>) {
        env.storage()
            .persistent()
            .get(&DataKey::SolverRoutes(solver))
            .unwrap_or_else(|| (Vec::new(&env), Vec::new(&env)))
    }

    /// Returns the reputation score (0–10_000 basis points) for `solver`,
    /// or None if the solver has never registered.
    ///
    /// Callers that only need the numeric value and already hold the
    /// SolverRecord can call `compute_reputation_score` directly.
    pub fn get_reputation_score(env: Env, solver: Address) -> Option<u32> {
        let record: SolverRecord = env.storage().persistent().get(&DataKey::Solver(solver))?;
        Some(Self::compute_reputation_score(&record))
    }

    /// Whether `solver` currently meets accept_intent's requirements
    /// (registered, active, bonded above MIN_BOND, below max_active_intents cap).
    /// Lets off-chain solver bots self-check eligibility without independently
    /// reimplementing the same logic accept_intent enforces.
    pub fn is_solver_eligible(env: Env, solver: Address) -> bool {
        let cfg = Self::load_config(&env);
        match env
            .storage()
            .persistent()
            .get::<_, SolverRecord>(&DataKey::Solver(solver))
        {
            Some(record) => {
                record.is_active
                    && record.bond_amount >= cfg.min_bond
                    && record.active_intents < cfg.max_active_intents_per_solver
            }
            None => false,
        }
    }

    /// Get the current max-active-intents cap per solver (issue #230).
    pub fn get_max_active_intents_per_solver(env: Env) -> u32 {
        Self::load_config(&env).max_active_intents_per_solver
    }

    /// Returns the current fee recipient address, or `None` before initialization.
    pub fn get_fee_recipient(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::FeeRecipient)
    }

    /// Pending fee-recipient proposal, if any: `(new_fee_recipient, eta)`
    /// where `eta` is the ledger timestamp at which `accept_fee_recipient`
    /// may execute it.
    pub fn get_pending_fee_recipient(env: Env) -> Option<(Address, u64)> {
        env.storage().instance().get(&DataKey::PendingFeeRecipient)
    }

    /// Pending admin-transfer proposal, if any: `(new_admin, eta)` where `eta`
    /// is the ledger timestamp at which `accept_admin_transfer` may execute it
    /// (#115/#116).
    pub fn get_pending_admin(env: Env) -> Option<(Address, u64)> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    /// Returns the bond token address (USDC SAC), or `None` before initialization.
    pub fn get_bond_token(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::BondToken)
    }

    /// Returns the current admin address, or `None` before initialization.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Admin)
    }

    /// Pending admin-transfer proposal, if any: `(new_admin, eta)` where `eta`
    /// is the ledger timestamp at which `accept_admin_transfer` may execute it.
    pub fn get_pending_admin(env: Env) -> Option<(Address, u64)> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }


    ///
    /// - `total_intents` — cumulative count of intents ever submitted.
    /// - `total_volume`  — cumulative dst-token units delivered across all fills.
    /// - `open_intents`  — intents currently in `Open` or `PartiallyFilled` state.
    ///
    /// **Trade-off (#109):** `open_intents` is maintained as an on-chain
    /// counter in instance storage (the same ledger entry that already holds
    /// `TotalIntents` and `TotalVolume`).  This means every state-changing
    /// call (`submit_intent`, `accept_intent`, `fill_intent`,
    /// `cancel_intent`, `expire_intent`, `slash_solver`) pays one extra
    /// integer read + write inside the instance entry, which is already
    /// loaded on every call.  The marginal cost is negligible compared to the
    /// persistent-storage I/O for `IntentRecord` and `SolverRecord`.
    ///
    /// The alternative — leaving `open_intents` entirely to indexers — would
    /// keep on-chain logic simpler, but would force every dashboard to replay
    /// the full event history for an O(N) count.  Storing the counter on-chain
    /// makes it O(1) for any caller.
    ///
    /// Note: the counter can transiently under-count if the contract is
    /// upgraded from a version that did not track it (pre-#109 deployments
    /// will have `OpenIntents` absent, which `unwrap_or(0)` handles gracefully
    /// — the counter will be accurate from the upgrade ledger forward).
    pub fn get_stats(env: Env) -> (u64, i128, u64) {
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
        let open: u64 = env
            .storage()
            .instance()
            .get(&DataKey::OpenIntents)
            .unwrap_or(0);
        (intents, volume, open)
    }

    /// List all intent IDs for a given user. Returns empty Vec if user has no intents.
    pub fn list_intents_by_user(env: Env, user: Address) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::UserIntents(user))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Number of currently-registered solvers.
    pub fn get_solver_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSolvers)
            .unwrap_or(0)
    }

    /// Enumerate registered solver addresses, paginated (#198).
    ///
    /// `start` is a 0-based offset into the registration-ordered list and
    /// `limit` is clamped to `MAX_BATCH_SIZE` so a single call stays
    /// resource-bounded as the solver set grows. Returns an empty `Vec` once
    /// `start` is past the end. Pair with `get_solver` to fetch each record, or
    /// `get_solver_count` to size the pagination loop.
    ///
    /// This is the on-chain alternative to reconstructing the solver set from
    /// `solver_registered` / `solver_deregistered` event replay. It mirrors the
    /// `list_allowed_dst_tokens` enumerable-list pattern (#117); see the
    /// `DataKey::SolverList` doc comment for the storage-cost trade-off.
    pub fn list_solvers(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let all: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::SolverList)
            .unwrap_or_else(|| Vec::new(&env));

        let capped_limit = limit.min(MAX_BATCH_SIZE);
        let mut page = Vec::new(&env);
        if start >= all.len() || capped_limit == 0 {
            return page;
        }
        let end = start.saturating_add(capped_limit).min(all.len());
        for i in start..end {
            page.push_back(all.get(i).unwrap());
        }
        page
    }

    /// Aggregate health snapshot combining `is_paused`, `get_stats`, and
    /// `get_solver_count` into a single call, for dashboard/monitoring
    /// integrations that would otherwise need multiple separate round-trips.
    pub fn get_protocol_health(env: Env) -> ProtocolHealth {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        let total_intents: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TotalIntents)
            .unwrap_or(0);
        let total_volume: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalVolume)
            .unwrap_or(0);
        let total_solvers: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSolvers)
            .unwrap_or(0);
        let total_bonded: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalBonded)
            .unwrap_or(0);

        ProtocolHealth {
            paused,
            total_intents,
            total_volume,
            total_solvers,
            total_bonded,
        }
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
    ///
    /// Not a contract entrypoint: it takes `&SolverRecord` by reference, which
    /// is not a valid Soroban ABI parameter, so it is `pub(crate)` (callable
    /// from tests and from `get_reputation_score`) rather than `pub`.
    pub(crate) fn compute_reputation_score(record: &SolverRecord) -> u32 {
        let total_fills = record.fills_completed as u64 + record.fills_failed as u64;
        if total_fills == 0 {
            return 0;
        }

        // base_bps ∈ [0, 10_000]
        let base_bps = (record.fills_completed as u64 * 10_000) / total_fills;

        // Volume scale: 1 000 fills × 100 dst tokens (7 dp) is the knee of
        // the curve. Only the shape matters — the constant can be tuned later.
        const VOLUME_SCALE: i128 = 1_000 * 100 * 10_000_000;

        // decay_bps = VOLUME_SCALE / (VOLUME_SCALE + vol) × 10_000
        // ∈ (0, 10_000].  High volume → low decay_bps.  `VOLUME_SCALE` is a
        // positive constant and `vol >= 0`, so the denominator is never zero.
        let vol = record.total_volume.max(0);
        let decay_bps = ((VOLUME_SCALE as u64) * 10_000) / ((VOLUME_SCALE + vol + 1) as u64);

        // volume_multiplier_bps ∈ [9_000, 10_000)
        // At zero volume: decay_bps = ~10_000, multiplier = 9_000
        // At high  volume: decay_bps → 0,      multiplier → 10_000
        let multiplier_bps = 10_000u64 - decay_bps / 10;

        let score = base_bps * multiplier_bps / 10_000;
        score as u32
    }

    /// #127: Validate `src_token` address format against the conventions of
    /// `src_chain`.
    ///
    /// Rules:
    /// * EVM chains (`"ethereum"`, `"base"`, `"polygon"`, `"arbitrum"`,
    ///   `"optimism"`): token must be a `0x`-prefixed 42-character ASCII string
    ///   (2 + 40 hex digits).
    /// * `"solana"`: token must be a base58-encoded public key — ASCII, no `0x`
    ///   prefix, between 32 and 44 characters inclusive.
    /// * Any other `src_chain` value: validation is skipped (forward-compatible).
    ///
    /// Called from `submit_intent` unconditionally so that even when the
    /// src_chain allowlist is disabled, obviously malformed tokens are rejected
    /// early.
    fn validate_src_token(env: &Env, src_chain: &String, src_token: &String) {
        // `soroban_sdk::String` is not byte-indexable; copy both values into
        // fixed ASCII buffers so the format checks can work on raw bytes.
        // Any `src_chain` longer than the longest name we recognise, or any
        // `src_token` longer than the longest address format we accept, cannot
        // be a match — treat over-long inputs as an unknown chain (chain) or a
        // rejected token (token) without touching the buffers.
        const MAX_CHAIN_LEN: usize = 16;
        const MAX_TOKEN_LEN: usize = 64;

        let chain_len = src_chain.len() as usize;
        let token_len = src_token.len() as usize;

        let mut chain_buf = [0u8; MAX_CHAIN_LEN];
        let chain_bytes: &[u8] = if chain_len <= MAX_CHAIN_LEN {
            src_chain.copy_into_slice(&mut chain_buf[..chain_len]);
            &chain_buf[..chain_len]
        } else {
            &chain_buf[..0]
        };

        let chain_is = |literal: &[u8]| -> bool { chain_bytes == literal };

        let is_evm = chain_is(b"ethereum")
            || chain_is(b"base")
            || chain_is(b"polygon")
            || chain_is(b"arbitrum")
            || chain_is(b"optimism");
        let is_solana = chain_is(b"solana");

        if !is_evm && !is_solana {
            // Unknown chain: skip validation — forward-compatible with future
            // chains, and keeps the src_chain allowlist as the sole gate.
            return;
        }

        if token_len > MAX_TOKEN_LEN {
            panic_with_error!(env, Error::InvalidSrcToken);
        }
        let mut token_buf = [0u8; MAX_TOKEN_LEN];
        src_token.copy_into_slice(&mut token_buf[..token_len]);
        let token = &token_buf[..token_len];

        if is_evm {
            // EVM token address: exactly "0x" + 40 hex chars = 42 characters.
            if token_len != 42 || token[0] != b'0' || token[1] != b'x' {
                panic_with_error!(env, Error::InvalidSrcToken);
            }
            // Remaining 40 characters must all be hex digits [0-9a-fA-F].
            for &ch in &token[2..] {
                let is_hex = ch.is_ascii_digit()
                    || (b'a'..=b'f').contains(&ch)
                    || (b'A'..=b'F').contains(&ch);
                if !is_hex {
                    panic_with_error!(env, Error::InvalidSrcToken);
                }
            }
            return;
        }

        // Solana token (SPL mint): base58-encoded 32-byte public key. Mint
        // addresses are 32–44 characters (a 32-byte value is at most 44 base58
        // digits, at least 32) with no "0x" prefix. Alphabet is Bitcoin base58
        // — 123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz — which
        // excludes 0, I, O and l. Verified against the published SPL mints in
        // `docs/132-supported-chains.md` §4.8 (e.g. USDC
        // `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`, 44 chars).
        if !(32..=44).contains(&token_len) {
            panic_with_error!(env, Error::InvalidSrcToken);
        }
        if token_len >= 2 && token[0] == b'0' && token[1] == b'x' {
            panic_with_error!(env, Error::InvalidSrcToken);
        }
        for &ch in token {
            let is_b58 = (b'1'..=b'9').contains(&ch)
                || (b'A'..=b'H').contains(&ch)
                || (b'J'..=b'N').contains(&ch)
                || (b'P'..=b'Z').contains(&ch)
                || (b'a'..=b'k').contains(&ch)
                || (b'm'..=b'z').contains(&ch);
            if !is_b58 {
                panic_with_error!(env, Error::InvalidSrcToken);
            }
        }
    }

    // ── Proof gating (issue #190) ────────────────────────────────────────────

    /// Cross-check an `Accepted` intent against its `ProofRegistry` record.
    /// Called from `fill_intent` only when `require_proof == true`. Panics —
    /// leaving the intent untouched — on any of the four docs/129 failure
    /// modes; returns normally when the proof matches.
    fn validate_proof(env: &Env, intent: &IntentRecord, intent_id: &BytesN<32>) {
        let registry_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ProofRegistry)
            .unwrap_or_else(|| panic_with_error!(env, Error::ProofRegistryNotSet));

        // One extra cross-contract read per gated fill (docs/124 §4.3). This is
        // the resource-fee cost the issue calls out; it is only paid when the
        // caller opts in via `require_proof = true`.
        let registry = ProofRegistryClient::new(env, &registry_addr);
        let proof = registry
            .get_proof(intent_id)
            .unwrap_or_else(|| panic_with_error!(env, Error::ProofNotFound));

        let want_chain_id = Self::wormhole_chain_id(env, &intent.src_chain);
        if proof.src_chain_id != want_chain_id {
            panic_with_error!(env, Error::ProofChainMismatch);
        }

        if proof.src_amount < intent.src_amount {
            panic_with_error!(env, Error::ProofAmountInsufficient);
        }
    }

    /// Map a canonical `src_chain` name to its Wormhole chain ID
    /// (docs/129 §4, kept in sync with docs/132-supported-chains.md). Panics
    /// with `SrcChainNotSupported` for any name not in the table.
    fn wormhole_chain_id(env: &Env, src_chain: &String) -> u32 {
        let len = src_chain.len() as usize;
        // Longest supported name ("avalanche") is 9 bytes.
        if len == 0 || len > 16 {
            panic_with_error!(env, Error::SrcChainNotSupported);
        }
        let mut buf = [0u8; 16];
        src_chain.copy_into_slice(&mut buf[..len]);
        match &buf[..len] {
            b"solana" => 1,
            b"ethereum" => 2,
            b"bsc" => 4,
            b"polygon" => 5,
            b"avalanche" => 6,
            b"arbitrum" => 23,
            b"optimism" => 24,
            b"base" => 30,
            _ => panic_with_error!(env, Error::SrcChainNotSupported),
        }
    }

    /// Translates a canonical `src_chain` string (per
    /// `docs/132-supported-chains.md` §2) to its numeric Wormhole chain ID,
    /// for comparison against `proof.src_chain_id` once proof-gated fills
    /// (issue #5) are wired up. Single source of truth for this mapping —
    /// kept in sync with `docs/129-proof-mismatch-fallback.md` §4 (issue #253).
    ///
    /// Fails closed: an unmapped/future `src_chain` string panics with
    /// `Error::SrcChainNotSupported` rather than defaulting to chain ID 0.
    pub fn src_chain_to_wormhole_id(env: Env, src_chain: String) -> u32 {
        let chain_len = src_chain.len();
        let chain_is = |literal: &[u8]| -> bool {
            if chain_len as usize != literal.len() {
                return false;
            }
            let mut i = 0u32;
            while i < chain_len {
                if src_chain.get(i) != literal[i as usize] as u32 {
                    return false;
                }
                i += 1;
            }
            true
        };

        if chain_is(b"ethereum") {
            2
        } else if chain_is(b"base") {
            30
        } else if chain_is(b"polygon") {
            5
        } else if chain_is(b"arbitrum") {
            23
        } else if chain_is(b"optimism") {
            24
        } else if chain_is(b"avalanche") {
            6
        } else if chain_is(b"bsc") {
            4
        } else if chain_is(b"solana") {
            1
        } else {
            panic_with_error!(&env, Error::SrcChainNotSupported)
        }
    }

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        // Auth audit: require_auth() is correct. All callers of require_admin
        // are admin-only functions (unpause, set_pauser,
        // add/remove_allowed_dst_token, set_dst_allowlist_enabled). The admin
        // is a single address with uniform authority over these functions;
        // require_auth_for_args would add no meaningful scope reduction.
        admin.require_auth();
    }

    /// Issue #120: `pause` accepts either the admin or the address set via
    /// `set_pauser`. `caller` is an explicit argument (rather than looked up
    /// implicitly, as `require_admin` does for the single-admin case)
    /// because there are now two addresses that could legitimately be the
    /// signer, so the contract needs to know which one is authorizing this
    /// call before it can require that specific address's auth.
    fn require_admin_or_pauser(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        let is_pauser = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::Pauser)
            .map(|pauser| pauser == *caller)
            .unwrap_or(false);
        if *caller != admin && !is_pauser {
            panic_with_error!(env, Error::Unauthorized);
        }
        caller.require_auth();
    }

    fn require_not_paused(env: &Env) {
        if Self::is_paused(env.clone()) {
            panic_with_error!(env, Error::ContractPaused);
        }
    }

    /// The pre-transfer guard sequence shared between `fill_intent` and
    /// `is_intent_fillable` (issue #259): intent state is `Accepted`, `solver`
    /// matches `intent.solver`, and `now` is before the fill-window deadline.
    /// Extracted so the two call sites can never silently drift apart.
    fn check_fill_guards(intent: &IntentRecord, solver: &Address, now: u64) -> Result<(), Error> {
        // Boundary semantics: the fill-window deadline is EXCLUSIVE for filling
        // (issue #26) — `now >= intent.deadline` rejects at the boundary second.
        if now >= intent.deadline {
            return Err(Error::FillWindowExpired);
        }
        match &intent.state {
            IntentState::Accepted => {}
            IntentState::Filled => return Err(Error::IntentAlreadyFilled),
            _ => return Err(Error::IntentNotAccepted),
        }
        if intent.solver.as_ref() != Some(solver) {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    /// Add `token` to the enumerable allowlist (#117), if not already present.
    fn add_to_dst_token_list(env: &Env, token: &Address) {
        let mut list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedDstTokenList)
            .unwrap_or_else(|| Vec::new(env));
        let mut already_present = false;
        for i in 0..list.len() {
            if list.get(i).unwrap() == *token {
                already_present = true;
                break;
            }
        }
        if !already_present {
            list.push_back(token.clone());
            env.storage()
                .instance()
                .set(&DataKey::AllowedDstTokenList, &list);
        }
    }

    /// Remove `token` from the enumerable allowlist (#117), if present.
    fn remove_from_dst_token_list(env: &Env, token: &Address) {
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedDstTokenList)
            .unwrap_or_else(|| Vec::new(env));
        let mut new_list: Vec<Address> = Vec::new(env);
        for i in 0..list.len() {
            let item = list.get(i).unwrap();
            if item != *token {
                new_list.push_back(item);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::AllowedDstTokenList, &new_list);
    }

    /// Append `solver` to the enumerable solver list (#198) if not already
    /// present. Called from `register_solver` only on a first registration, so
    /// a solver that deregisters and re-registers gets exactly one entry — the
    /// same "already present" guard the dst_token list uses.
    fn add_to_solver_list(env: &Env, solver: &Address) {
        let mut list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::SolverList)
            .unwrap_or_else(|| Vec::new(env));
        for i in 0..list.len() {
            if list.get(i).unwrap() == *solver {
                return;
            }
        }
        list.push_back(solver.clone());
        env.storage().instance().set(&DataKey::SolverList, &list);
    }

    /// Remove `solver` from the enumerable solver list (#198), if present.
    /// Called from `deregister_solver`.
    fn remove_from_solver_list(env: &Env, solver: &Address) {
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::SolverList)
            .unwrap_or_else(|| Vec::new(env));
        let mut new_list: Vec<Address> = Vec::new(env);
        for i in 0..list.len() {
            let item = list.get(i).unwrap();
            if item != *solver {
                new_list.push_back(item);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::SolverList, &new_list);
    }

    fn get_adjusted_min_bond(env: &Env, dst_token: &Address) -> i128 {
        let base_bond = Self::load_config(env).min_bond;
        let multiplier = env
            .storage()
            .persistent()
            .get::<_, i128>(&DataKey::MinBondMultiplier(dst_token.clone()))
            .unwrap_or(10);
        (base_bond * multiplier) / 10
    }

    /// #197: resolve `solver`'s registry tier for perk calculation.
    ///
    /// Makes a single cross-contract call — `solver_registry.get_tier(solver)`
    /// — via `try_invoke_contract` (rather than a generated `#[contractclient]`,
    /// to keep the settlement wasm small). Returns `0` (Unranked) — the
    /// pre-integration behaviour — whenever the registry address is unset, or
    /// the call reverts, or the return value doesn't decode as a `u32`. The
    /// result is clamped to a known tier so the perk tables index safely.
    fn solver_tier(env: &Env, solver: &Address) -> u32 {
        let Some(registry) = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::SolverRegistry)
        else {
            return 0;
        };
        let args: Vec<soroban_sdk::Val> = (solver.clone(),).into_val(env);
        let result: Result<Result<u32, _>, Result<soroban_sdk::Error, _>> =
            env.try_invoke_contract(&registry, &Symbol::new(env, "get_tier"), args);
        match result {
            Ok(Ok(tier)) => tier.min(TIER_SLASH_BPS.len() as u32 - 1),
            _ => 0,
        }
    }

    /// Fill-window seconds a solver on `tier` gets when accepting: the base
    /// `fill_window` plus the tier's `TIER_FILL_WINDOW_BONUS_BPS` extension.
    fn tier_fill_window(tier: u32, base_fill_window: u64) -> u64 {
        let bonus_bps = TIER_FILL_WINDOW_BONUS_BPS
            .get(tier as usize)
            .copied()
            .unwrap_or(0);
        base_fill_window.saturating_mul(10_000 + bonus_bps) / 10_000
    }

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
                max_active_intents_per_solver: DEFAULT_MAX_ACTIVE_INTENTS_PER_SOLVER,
            })
    }

    fn load_bond_token(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::BondToken)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized))
    }

    /// Returns `true` when bid-window mode is active.
    ///
    /// Issue #191: this now reads a dedicated `DataKey::BidWindowEnabled` flag
    /// set via `set_bid_window_enabled`.  It previously reused
    /// `DataKey::DstAllowlistEnabled` "as a placeholder", which meant toggling
    /// the destination-token allowlist would silently also toggle bidding mode —
    /// a storage-key collision that is now closed.  Defaults to `false` so
    /// first-accept-wins behaviour is preserved on every deployment that
    /// pre-dates this feature.
    ///
    /// Bid-window mode changes `submit_intent` so newly created intents start
    /// in the `Bidding` state instead of `Open`, giving solvers a fixed
    /// `BID_WINDOW`-second window to submit competing quotes via `bid_intent`
    /// before `settle_bids` assigns the winner.
    pub fn is_bid_window_enabled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::BidWindowEnabled)
            .unwrap_or(false)
    }

    /// Issue #187 — the minimum bond required for a given bond token.
    ///
    /// * Legacy default token → the effective `min_bond` from `ProtocolConfig`
    ///   (so pre-#187 behaviour and any admin `set_config` override are
    ///   preserved).
    /// * Any other approved token → an admin-set `DataKey::MinBond(token)`
    ///   entry, falling back to the compile-time `MIN_BOND` constant when the
    ///   admin has not set one.
    fn min_bond_for_token(env: &Env, token: &Address) -> i128 {
        if *token == Self::load_bond_token(env) {
            return Self::load_config(env).min_bond;
        }
        env.storage()
            .instance()
            .get(&DataKey::MinBond(token.clone()))
            .unwrap_or(MIN_BOND)
    }

    /// Issue #187 — `true` if `token` may be used as a solver bond: either it
    /// is the legacy default token (always allowed) or it has an explicit
    /// `DataKey::AllowedBondToken` entry.
    fn is_bond_token_allowed(env: &Env, token: &Address) -> bool {
        *token == Self::load_bond_token(env)
            || env
                .storage()
                .instance()
                .has(&DataKey::AllowedBondToken(token.clone()))
    }

    /// Issue #187 — read a solver's bond in a specific token.
    ///
    /// For the legacy default token the source of truth is
    /// `SolverRecord.bond_amount` (kept mirrored for pre-#187 readers); for
    /// every other token it is the `DataKey::SolverBond(solver, token)` entry.
    fn get_solver_bond_amount(env: &Env, record: &SolverRecord, token: &Address) -> i128 {
        if *token == Self::load_bond_token(env) {
            record.bond_amount
        } else {
            env.storage()
                .persistent()
                .get(&DataKey::SolverBond(record.address.clone(), token.clone()))
                .unwrap_or(0)
        }
    }

    /// Issue #187 — write a solver's bond in a specific token, keeping the
    /// legacy `bond_amount` mirror and the `bond_tokens` enumeration in sync.
    /// A zero balance drops the token from `bond_tokens` (and, for non-default
    /// tokens, removes the storage entry entirely).
    fn set_solver_bond_amount(
        env: &Env,
        record: &mut SolverRecord,
        token: &Address,
        amount: i128,
    ) {
        let default_token = Self::load_bond_token(env);
        if *token == default_token {
            record.bond_amount = amount;
        } else if amount > 0 {
            env.storage().persistent().set(
                &DataKey::SolverBond(record.address.clone(), token.clone()),
                &amount,
            );
        } else {
            env.storage()
                .persistent()
                .remove(&DataKey::SolverBond(record.address.clone(), token.clone()));
        }

        let mut present = false;
        for i in 0..record.bond_tokens.len() {
            if record.bond_tokens.get(i).unwrap() == *token {
                present = true;
                break;
            }
        }
        if amount > 0 && !present {
            record.bond_tokens.push_back(token.clone());
        } else if amount == 0 && present {
            // Rebuild without `token`, mirroring `remove_from_dst_token_list`.
            let mut rebuilt: Vec<Address> = Vec::new(env);
            for i in 0..record.bond_tokens.len() {
                let t = record.bond_tokens.get(i).unwrap();
                if t != *token {
                    rebuilt.push_back(t);
                }
            }
            record.bond_tokens = rebuilt;
        }
    }

    /// Issue #193 — proportional bond slash.
    ///
    /// Returns the amount to slash from `bond` for a solver that failed to
    /// deliver an intent whose outstanding output is `unfilled_amount`
    /// (`min_dst_amount - total_filled`, floored at 0):
    ///
    /// ```text
    ///   exposure   = min(unfilled_amount, bond)   // same-token comparability
    ///   proportional = exposure / 10              // 10% of what was at stake
    ///   cap          = bond * SLASH_BPS / 10_000  // never worse than flat 10%
    ///   slash        = clamp(proportional, 1, min(cap, bond))
    /// ```
    ///
    /// Properties (mirroring `compute_reputation_score`'s edge-case discipline):
    /// * Integer-only, cannot panic (all operands ≥ 0, no division by zero).
    /// * Floor of 1 stroop preserves issue #32's "non-zero bond is always
    ///   punished" guarantee.
    /// * Cap at `bond * 10%` means a well-matched bond is never slashed harder
    ///   than the old flat rate; a solver who over-bonds relative to the intent
    ///   is slashed *less*, and a solver who under-bonds is still capped at
    ///   100% of bond (via `exposure ≤ bond`) and never panics.
    /// * `unfilled_amount == 0` (shouldn't happen for an Accepted intent, but
    ///   guarded) still yields the floor of 1.
    fn compute_slash_amount(bond: i128, unfilled_amount: i128) -> i128 {
        if bond <= 0 {
            return 0;
        }
        let exposure = unfilled_amount.max(0).min(bond);
        let proportional = exposure / 10;
        let cap = (bond / 10_000) * SLASH_BPS;
        let cap = cap.min(bond).max(1);
        proportional.max(1).min(cap)
    }

    /// Issue #188 — the address allowed to call `resolve_dispute`: the
    /// `DataKey::Arbiter` entry if set, otherwise the `Admin` (the design
    /// doc's v1 default).
    fn load_arbiter(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Arbiter)
            .unwrap_or_else(|| {
                env.storage()
                    .instance()
                    .get(&DataKey::Admin)
                    .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized))
            })
    }

    /// Returns the effective protocol fee in basis points from the stored
    /// `ProtocolConfig`.
    ///
    /// Future work (tiered-fee feature): this can be extended to take a solver
    /// address and apply volume-tier discounts from the solver's historical
    /// `total_volume`. It is retained as the single intended lookup point for
    /// that logic; `#[allow(dead_code)]` because `fill_intent` still reads the
    /// flat `PROTOCOL_FEE_BPS` constant directly today.
    #[allow(dead_code)]
    fn get_tiered_fee_bps(env: &Env) -> i128 {
        Self::load_config(env).protocol_fee_bps
    }

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

    /// Load an intent record, or panic `IntentNotFound`.
    fn load_intent(env: &Env, intent_id: &BytesN<32>) -> IntentRecord {
        env.storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(env, Error::IntentNotFound))
    }

    /// Persist an intent record and bump its TTL.
    fn save_intent(env: &Env, intent_id: &BytesN<32>, intent: &IntentRecord) {
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), intent);
        Self::bump_intent_ttl(env, intent_id);
    }

    fn bump_solver_ttl(env: &Env, solver: &Address) {
        env.storage().persistent().extend_ttl(
            &DataKey::Solver(solver.clone()),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    /// Seconds remaining until a `SLASH_COOLDOWN` starting at `last_slash_time`
    /// clears, given the current ledger time `now`. Returns `0` for a solver
    /// that has never been slashed (`last_slash_time == 0`) or whose cooldown
    /// has already elapsed. Shared by `accept_intent` and
    /// `get_slash_cooldown_remaining` so both can never disagree (issue #256).
    fn slash_cooldown_remaining(last_slash_time: u64, now: u64) -> u64 {
        if last_slash_time == 0 {
            return 0;
        }
        let cooldown_end = last_slash_time + SLASH_COOLDOWN;
        cooldown_end.saturating_sub(now)
    }

    /// Appends `intent_id` to `solver`'s `SolverIntents` list (issue #245).
    fn solver_intents_add(env: &Env, solver: &Address, intent_id: &BytesN<32>) {
        let mut list: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&DataKey::SolverIntents(solver.clone()))
            .unwrap_or_else(|| Vec::new(env));
        list.push_back(intent_id.clone());
        env.storage()
            .persistent()
            .set(&DataKey::SolverIntents(solver.clone()), &list);
        env.storage().persistent().extend_ttl(
            &DataKey::SolverIntents(solver.clone()),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    /// Removes `intent_id` from `solver`'s `SolverIntents` list, if present
    /// (issue #245). A no-op if the list or the entry doesn't exist.
    fn solver_intents_remove(env: &Env, solver: &Address, intent_id: &BytesN<32>) {
        let key = DataKey::SolverIntents(solver.clone());
        if let Some(list) = env.storage().persistent().get::<_, Vec<BytesN<32>>>(&key) {
            if let Some(idx) = list.iter().position(|id| &id == intent_id) {
                let mut list = list;
                let _ = list.remove(idx as u32);
                env.storage().persistent().set(&key, &list);
                env.storage().persistent().extend_ttl(
                    &key,
                    PERSISTENT_TTL_THRESHOLD,
                    PERSISTENT_TTL_EXTEND_TO,
                );
            }
        }
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

    fn validate_proof(env: &Env, intent_id: &BytesN<32>, intent: &IntentRecord) {
        let _registry_addr = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::ProofRegistry)
            .unwrap_or_else(|| panic_with_error!(env, Error::ProofRegistryNotSet));

        // In production, this would call:
        // - registry.has_proof(intent_id) to check existence
        // - registry.get_proof(intent_id) to retrieve the proof record
        // - Validate proof.src_chain matches intent.src_chain
        // - Validate proof.src_amount >= intent.src_amount
        //
        // For now, the proof logic is deferred to issue #5's fill_intent integration.
        // This function serves as the proof-validation checkpoint in the fill flow.
        // Tests will inject mock proofs and verify this gate works correctly.
    }
}
