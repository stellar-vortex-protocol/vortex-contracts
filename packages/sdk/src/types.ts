/**
 * Vortex Protocol Intent Settlement Contract Types
 *
 * This file defines TypeScript types that correspond to the Rust contract's
 * Soroban types, enabling type-safe interaction with the contract.
 */

import { Address, i128, u32, u64, BytesN } from 'stellar-sdk';

/**
 * Core Intent Lifecycle States
 */
export enum IntentState {
  Open = 'Open',
  Accepted = 'Accepted',
  PartiallyFilled = 'PartiallyFilled',
  Filled = 'Filled',
  Cancelled = 'Cancelled',
  Expired = 'Expired',
  Slashed = 'Slashed',
}

/**
 * Solver Tier / Reputation Rating
 * Used for determining fee discounts and fill-window bonuses
 */
export enum SolverTier {
  Unranked = 'Unranked',
  Bronze = 'Bronze',
  Silver = 'Silver',
  Gold = 'Gold',
  Platinum = 'Platinum',
}

/**
 * Complete record of an on-chain intent
 *
 * Maps to the contract's `IntentRecord` struct.
 * Represents a single user's swap request with all metadata.
 */
export interface IntentRecord {
  /**
   * Unique 32-byte identifier for this intent
   */
  id: BytesN<32>;

  /**
   * User's Stellar address (initiator of the swap)
   */
  user: Address;

  /**
   * Source blockchain name (e.g. "ethereum", "base", "solana")
   * Validated against optional allowlist if enabled
   */
  src_chain: string;

  /**
   * Source token address (on the source chain)
   * E.g. WETH address on Ethereum, mint on Solana
   */
  src_token: string;

  /**
   * Amount of source token, in source token's smallest unit
   * E.g. 1e18 for 1 ETH (18 decimal places)
   * Maximum bounded at `MAX_AMOUNT` (10^30)
   */
  src_amount: i128;

  /**
   * Destination token (on Stellar)
   * A Soroban contract address (SAC) or SEP-41 contract
   */
  dst_token: Address;

  /**
   * Minimum acceptable destination amount (in dst_token's smallest unit)
   * Protection against slippage: solver must deliver at least this much
   */
  min_dst_amount: i128;

  /**
   * User's absolute deadline (Unix timestamp in seconds)
   * Intent expires and becomes re-auctionable after this time
   * Typically set to now + 30 minutes (INTENT_EXPIRY)
   */
  deadline: u64;

  /**
   * Current state of this intent
   */
  state: IntentState;

  /**
   * Address of the solver that accepted this intent
   * Only set when state is Accepted, PartiallyFilled, or Filled
   */
  accepted_by?: Address;

  /**
   * Timestamp when accept_intent was called
   * Starts the 5-minute fill window (FILL_WINDOW)
   */
  accepted_at?: u64;

  /**
   * Timestamp when fill_intent was called
   * For partially-filled intents, the most recent fill time
   */
  filled_at?: u64;

  /**
   * Total amount delivered by solver in previous fills
   * For multi-part fills (PartiallyFilled state)
   */
  total_filled?: i128;

  /**
   * True if the user has opened a dispute (issue #188)
   * During DISPUTE_WINDOW after first fill, user may contest via dispute_fill
   */
  has_dispute?: boolean;

  /**
   * Timestamp when user opened a dispute (if any)
   */
  dispute_opened_at?: u64;

  /**
   * Amount the user is contesting as disputed
   * Held in escrow until resolved
   */
  dispute_amount?: i128;

  /**
   * Amount released to the user after dispute resolution
   */
  dispute_resolved_amount?: i128;

  /**
   * User's anti-griefing dispute bond
   * 1 USDC, refunded if dispute is resolved in their favor
   */
  dispute_bond?: i128;
}

/**
 * Solver's registration and bond record
 *
 * Maps to the contract's `SolverRecord` struct.
 * Tracks solver identity, bond, and performance history.
 */
export interface SolverRecord {
  /**
   * Solver's Stellar address
   */
  address: Address;

  /**
   * Total USDC bond posted (minimum 50 USDC / MIN_BOND)
   */
  bond_amount: i128;

  /**
   * Whether the solver is active
   * Set to false when bond drops below MIN_BOND (due to slash)
   * Top up bond to reactivate
   */
  is_active: boolean;

  /**
   * Number of intents successfully filled (submitted via fill_intent)
   */
  fills_completed: u32;

  /**
   * Number of times this solver was slashed for missing fill window
   * Each slash reduces bond by 10% (or intent-proportional amount, capped at 10%)
   */
  fills_failed: u32;

  /**
   * Number of intents currently in Accepted state
   * Incremented by accept_intent, decremented by fill_intent
   * Capped by get_max_active_intents_per_solver
   */
  active_intents: u32;

  /**
   * Cumulative volume filled (sum of all successful fill_amount values)
   * Used to determine fee discount tier
   */
  total_volume: i128;

  /**
   * Solver's current reputation tier (if reputation_badge contract is linked)
   */
  tier?: SolverTier;

  /**
   * Unix timestamp of most recent slash event (if any)
   * Used for slash cooldown: solver may not accept new intents for 1 hour after slash
   */
  last_slashed_at?: u64;
}

/**
 * Protocol configuration (tunable parameters)
 *
 * Maps to the contract's `ProtocolConfig` struct.
 */
export interface ProtocolConfig {
  /**
   * Minimum USDC bond required to register as a solver
   * Default: 50 USDC (50 * 10_000_000 stroops)
   */
  min_bond: i128;

  /**
   * Time window for solver to fill after accept_intent
   * Default: 300 seconds (5 minutes)
   */
  fill_window: u64;

  /**
   * Time an Open intent remains available before expiring
   * Default: 1800 seconds (30 minutes)
   */
  intent_expiry: u64;

  /**
   * Protocol fee as basis points (1 bps = 0.01%)
   * Taken from each fill_amount and transferred to fee_recipient
   * Default: 5 bps (0.05%), hard-capped at 1000 bps (10%)
   */
  protocol_fee_bps: i128;

  /**
   * Paused state: if true, state-changing calls revert with ContractPaused
   * Emergency lever for incident response
   */
  is_paused: boolean;
}

/**
 * Protocol statistics and health metrics
 */
export interface ProtocolHealth {
  /**
   * Total number of intents ever submitted
   */
  total_intents: u64;

  /**
   * Current total value locked in active intents (sum of min_dst_amount)
   */
  total_tvl: i128;

  /**
   * Number of registered solvers (active and inactive)
   */
  total_solvers: u64;

  /**
   * Sum of all solver bonds posted (active + inactive)
   */
  total_bonds: i128;

  /**
   * Total protocol fees collected (sum of all fill fees)
   */
  total_fees_collected: i128;

  /**
   * Number of intents in each state
   */
  state_counts: Record<IntentState, u64>;
}

/**
 * Decoded event emitted by the contract
 */
export interface ContractEvent {
  type: 'IntentSubmitted' | 'IntentAccepted' | 'IntentFilled' | 'SolverSlashed' | string;
  timestamp: u64;
  data: Record<string, any>;
}

/**
 * Options for submitting an intent
 */
export interface SubmitIntentOptions {
  user: Address;
  src_chain: string;
  src_token: string;
  src_amount: i128;
  dst_token: Address;
  min_dst_amount: i128;
  deadline?: u64; // Defaults to now + INTENT_EXPIRY if not provided
}

/**
 * Options for accepting an intent
 */
export interface AcceptIntentOptions {
  solver: Address;
  intent_id: BytesN<32>;
}

/**
 * Options for filling an intent
 */
export interface FillIntentOptions {
  solver: Address;
  intent_id: BytesN<32>;
  fill_amount: i128;
  require_proof?: boolean; // Requires proof_registry if true
}

/**
 * Options for registering a solver
 */
export interface RegisterSolverOptions {
  solver: Address;
  bond_amount: i128;
  bond_token?: Address; // Defaults to primary bond token if not provided
}

/**
 * Options for querying intents
 */
export interface ListIntentsOptions {
  offset?: u32;
  limit?: u32; // Capped at MAX_PAGE_SIZE (100)
}

/**
 * Pair of (src_chain, dst_token) describing a supported route
 */
export interface SolverRoute {
  src_chain: string;
  dst_token: Address;
}

/**
 * Fee discount tier configuration
 */
export interface FeeDiscountTier {
  min_volume: i128;
  discount_bps: i128; // Discount as % of fee (in basis points)
}

/**
 * Fee schedule for a solver (base rate + applicable discount tier)
 */
export interface FeeSchedule {
  base_fee_bps: i128;
  effective_fee_bps: i128; // After applying highest applicable discount tier
  tiers: FeeDiscountTier[];
}
