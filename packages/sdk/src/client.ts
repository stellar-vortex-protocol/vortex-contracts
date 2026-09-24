/**
 * Vortex Protocol Intent Settlement Contract Client
 *
 * Provides a type-safe interface to interact with the intent_settlement contract.
 * Methods map directly to the contract's public functions defined in lib.rs.
 *
 * This client handles:
 * - Transaction construction and submission
 * - Type conversion between TypeScript and Soroban types
 * - Read-only view function calls via Soroban RPC
 * - Event parsing and filtering
 *
 * Usage:
 *   import { VortexClient } from '@vortex-protocol/sdk';
 *   import { Server, Keypair } from 'stellar-sdk';
 *
 *   const horizon = new Server('https://horizon-testnet.stellar.org');
 *   const client = new VortexClient({
 *     contractId: 'CAKJ2RNHMBT74VJLGVLZ6SCOBRQMDQ6PU5CCYKP4IJMVKFAACJKBRMM',
 *     horizon,
 *     rpcUrl: 'https://soroban-testnet.stellar.org',
 *   });
 *
 *   // User submits an intent
 *   await client.submitIntent({
 *     user: userAddress,
 *     src_chain: 'ethereum',
 *     src_token: '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
 *     src_amount: 1000000000000000000n,
 *     dst_token: usdc_sac,
 *     min_dst_amount: 3500000000n,
 *   }, sourceKeyPair);
 */

import { Address, Contract, Keypair, Server, TransactionBuilder, BASE_FEE, Networks } from 'stellar-sdk';
import * as types from './types';

export interface VortexClientOptions {
  /**
   * The deployed intent_settlement contract ID
   */
  contractId: string;

  /**
   * Stellar Horizon server instance (for account fetching, transaction submission)
   */
  horizon: Server;

  /**
   * Soroban RPC URL (for contract invocations)
   * E.g. 'https://soroban-testnet.stellar.org'
   */
  rpcUrl?: string;

  /**
   * Network passphrase (default: 'Test SDF Network ; September 2015')
   */
  networkPassphrase?: string;

  /**
   * Base fee in stroops (default: 100)
   */
  baseFee?: string;

  /**
   * Timeout for RPC calls in milliseconds (default: 30000)
   */
  timeout?: number;
}

/**
 * Main client for interacting with the Vortex Protocol contract
 *
 * Provides methods for:
 * - Intent lifecycle (submit, accept, fill, cancel, expire)
 * - Solver management (register, deregister, withdraw)
 * - Read-only queries (get_intent, get_solver, list_intents, etc.)
 * - Admin functions (propose/execute fee recipient transfer, dst_token allowlist, etc.)
 */
export class VortexClient {
  private contractId: string;
  private horizon: Server;
  private rpcUrl: string;
  private networkPassphrase: string;
  private baseFee: string;
  private timeout: number;

  constructor(options: VortexClientOptions) {
    this.contractId = options.contractId;
    this.horizon = options.horizon;
    this.rpcUrl = options.rpcUrl || 'https://soroban-testnet.stellar.org';
    this.networkPassphrase = options.networkPassphrase || Networks.TESTNET_NETWORK_PASSPHRASE;
    this.baseFee = options.baseFee || BASE_FEE;
    this.timeout = options.timeout || 30000;
  }

  /**
   * Submit a new swap intent
   *
   * @param options Intent parameters
   * @param signingKeypair Keypair for transaction signing
   * @returns Intent ID as hex string
   *
   * Emits: intent_submitted event
   */
  async submitIntent(options: types.SubmitIntentOptions, signingKeypair: Keypair): Promise<string> {
    // Implementation would construct Soroban invocation for submit_intent
    // and return the intent ID from the result

    throw new Error('submitIntent not yet implemented in reference SDK');
  }

  /**
   * Accept an open intent as a solver
   *
   * Grants exclusive fill rights for FILL_WINDOW (5 minutes).
   * Solver must have registered bond >= MIN_BOND.
   *
   * @param options Intent and solver details
   * @param signingKeypair Keypair for transaction signing
   *
   * Emits: intent_accepted event
   */
  async acceptIntent(options: types.AcceptIntentOptions, signingKeypair: Keypair): Promise<void> {
    throw new Error('acceptIntent not yet implemented in reference SDK');
  }

  /**
   * Fill an accepted intent by delivering destination tokens to the user
   *
   * Must be called within FILL_WINDOW of accept_intent.
   * Protocol fee is deducted from fill_amount and transferred to fee_recipient.
   *
   * @param options Solver, intent ID, and fill amount
   * @param signingKeypair Keypair for transaction signing
   *
   * Emits: intent_filled event
   */
  async fillIntent(options: types.FillIntentOptions, signingKeypair: Keypair): Promise<void> {
    throw new Error('fillIntent not yet implemented in reference SDK');
  }

  /**
   * Cancel an open intent (user-only operation)
   *
   * Only the user who submitted the intent can cancel it.
   * Cannot cancel an accepted intent (in fill window).
   *
   * @param intentId The intent to cancel
   * @param signingKeypair Keypair for transaction signing (must match user)
   *
   * Emits: intent_cancelled event
   */
  async cancelIntent(intentId: string, signingKeypair: Keypair): Promise<void> {
    throw new Error('cancelIntent not yet implemented in reference SDK');
  }

  /**
   * Register as a solver with a bond
   *
   * @param options Solver address and bond amount
   * @param signingKeypair Keypair for transaction signing
   *
   * Emits: solver_registered event
   */
  async registerSolver(options: types.RegisterSolverOptions, signingKeypair: Keypair): Promise<void> {
    throw new Error('registerSolver not yet implemented in reference SDK');
  }

  /**
   * Deregister as a solver and withdraw all bonded tokens
   *
   * @param solver Solver address
   * @param signingKeypair Keypair for transaction signing (must match solver)
   */
  async deregisterSolver(solver: Address, signingKeypair: Keypair): Promise<void> {
    throw new Error('deregisterSolver not yet implemented in reference SDK');
  }

  /**
   * Check if a solver is eligible to accept new intents
   *
   * Returns true if:
   * 1. Solver is registered
   * 2. Bond >= MIN_BOND
   * 3. Solver is active (not deactivated due to low bond)
   *
   * @param solver Solver address
   * @returns Whether the solver is eligible
   */
  async isSolverEligible(solver: Address): Promise<boolean> {
    throw new Error('isSolverEligible not yet implemented in reference SDK');
  }

  /**
   * Fetch a single intent by ID
   *
   * @param intentId The intent's 32-byte ID
   * @returns The intent record, or undefined if not found
   */
  async getIntent(intentId: string): Promise<types.IntentRecord | undefined> {
    throw new Error('getIntent not yet implemented in reference SDK');
  }

  /**
   * Fetch a solver's registration record
   *
   * @param solver Solver address
   * @returns The solver record, or undefined if not registered
   */
  async getSolver(solver: Address): Promise<types.SolverRecord | undefined> {
    throw new Error('getSolver not yet implemented in reference SDK');
  }

  /**
   * List all currently open and partially-filled intents
   *
   * @param options Pagination options (offset, limit)
   * @returns Array of open intent IDs (paginated, up to MAX_PAGE_SIZE per call)
   */
  async listOpenIntents(options?: types.ListIntentsOptions): Promise<string[]> {
    throw new Error('listOpenIntents not yet implemented in reference SDK');
  }

  /**
   * List intents submitted by a specific user
   *
   * @param user User address
   * @returns Array of intent IDs submitted by this user
   */
  async listIntentsByUser(user: Address): Promise<string[]> {
    throw new Error('listIntentsByUser not yet implemented in reference SDK');
  }

  /**
   * Check if the contract is currently paused
   *
   * @returns Whether pause() has been called and unpause() has not been called since
   */
  async isPaused(): Promise<boolean> {
    throw new Error('isPaused not yet implemented in reference SDK');
  }

  /**
   * Get current protocol configuration
   *
   * @returns Protocol config (min_bond, fill_window, intent_expiry, protocol_fee_bps, etc.)
   */
  async getConfig(): Promise<types.ProtocolConfig> {
    throw new Error('getConfig not yet implemented in reference SDK');
  }

  /**
   * Get fee schedule for a solver (including any discount tier bonus)
   *
   * @param solver Solver address
   * @returns Base fee, effective fee after discount, and list of tiers
   */
  async getFeeSchedule(solver: Address): Promise<types.FeeSchedule> {
    throw new Error('getFeeSchedule not yet implemented in reference SDK');
  }

  /**
   * Get current protocol health metrics
   *
   * @returns Total intents, TVL, solvers, bonds, fees collected, state breakdown
   */
  async getProtocolHealth(): Promise<types.ProtocolHealth> {
    throw new Error('getProtocolHealth not yet implemented in reference SDK');
  }

  /**
   * Propose a fee recipient change (admin-only, timelocked)
   *
   * @param newFeeRecipient New fee recipient address
   * @param signingKeypair Admin keypair for signing
   *
   * Change takes effect after ADMIN_TIMELOCK_DELAY (48 hours)
   * Emits: fee_recipient_transfer_proposed event
   */
  async proposeFeeRecipient(newFeeRecipient: Address, signingKeypair: Keypair): Promise<void> {
    throw new Error('proposeFeeRecipient not yet implemented in reference SDK');
  }

  /**
   * Accept a pending fee recipient change (admin-only)
   *
   * @param signingKeypair Admin keypair for signing
   *
   * Must be called at least ADMIN_TIMELOCK_DELAY seconds after propose_fee_recipient
   * Emits: fee_recipient_transfer_completed event
   */
  async acceptFeeRecipient(signingKeypair: Keypair): Promise<void> {
    throw new Error('acceptFeeRecipient not yet implemented in reference SDK');
  }

  /**
   * Propose an admin transfer (admin-only, timelocked)
   *
   * @param newAdmin New admin address
   * @param signingKeypair Admin keypair for signing
   *
   * Change takes effect after ADMIN_TIMELOCK_DELAY (48 hours)
   * Emits: admin_transfer_proposed event
   */
  async proposeAdminTransfer(newAdmin: Address, signingKeypair: Keypair): Promise<void> {
    throw new Error('proposeAdminTransfer not yet implemented in reference SDK');
  }

  /**
   * Accept a pending admin transfer (admin-only)
   *
   * @param signingKeypair New admin keypair for signing
   *
   * Must be called at least ADMIN_TIMELOCK_DELAY seconds after propose_admin_transfer
   * Emits: admin_transfer_completed event
   */
  async acceptAdminTransfer(signingKeypair: Keypair): Promise<void> {
    throw new Error('acceptAdminTransfer not yet implemented in reference SDK');
  }

  /**
   * Add a destination token to the allowlist (admin-only, timelocked)
   *
   * @param token Token address to allowlist
   * @param signingKeypair Admin keypair for signing
   *
   * Addition takes effect after ADMIN_TIMELOCK_DELAY (48 hours)
   */
  async proposeAddDstToken(token: Address, signingKeypair: Keypair): Promise<void> {
    throw new Error('proposeAddDstToken not yet implemented in reference SDK');
  }

  /**
   * Add a source chain to the allowlist (admin-only, no timelock)
   *
   * @param chain Chain name (e.g. "ethereum", "base")
   * @param signingKeypair Admin keypair for signing
   */
  async addAllowedSrcChain(chain: string, signingKeypair: Keypair): Promise<void> {
    throw new Error('addAllowedSrcChain not yet implemented in reference SDK');
  }

  /**
   * Check if a source chain is in the allowlist (if enabled)
   *
   * @param chain Chain name
   * @returns Whether this chain is allowed
   */
  async isSrcChainAllowed(chain: string): Promise<boolean> {
    throw new Error('isSrcChainAllowed not yet implemented in reference SDK');
  }

  /**
   * Slash a solver that accepted but missed the fill window
   *
   * Anyone can call this (permissionless) if:
   * - Solver accepted an intent
   * - FILL_WINDOW has elapsed without fill_intent
   *
   * Slashes min(intent_value, bond) / 10, capped at 10% of bond
   * Transfers slash amount to fee_recipient
   * Re-opens intent for re-auction
   *
   * @param intentId The intent the solver missed
   * @param signingKeypair Any keypair (permissionless)
   *
   * Emits: solver_slashed event
   */
  async slashSolver(intentId: string, signingKeypair: Keypair): Promise<void> {
    throw new Error('slashSolver not yet implemented in reference SDK');
  }

  /**
   * Manually rescue tokens sent to the contract (admin-only)
   *
   * @param token Token address to rescue
   * @param to Recipient address
   * @param amount Amount to transfer
   * @param signingKeypair Admin keypair for signing
   */
  async rescueTokens(token: Address, to: Address, amount: bigint, signingKeypair: Keypair): Promise<void> {
    throw new Error('rescueTokens not yet implemented in reference SDK');
  }
}
