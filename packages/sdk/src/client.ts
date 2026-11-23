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

import {
  Address,
  Contract,
  Keypair,
  Server,
  TransactionBuilder,
  BASE_FEE,
  Networks,
  SorobanRpc,
  nativeToScVal,
  scValToNative,
  xdr,
} from 'stellar-sdk';
import * as types from './types';
import { ContractSpecError } from './types';

/**
 * Convert a 32-byte intent ID (hex string, with or without 0x prefix) to an ScVal BytesN<32>.
 */
function intentIdToScVal(intentId: string): xdr.ScVal {
  const hex = intentId.startsWith('0x') ? intentId.slice(2) : intentId;
  if (!/^[0-9a-fA-F]{64}$/.test(hex)) {
    throw new Error(`Invalid intent ID: expected 32-byte hex string, got "${intentId}"`);
  }
  return nativeToScVal(Buffer.from(hex, 'hex'), { type: 'bytes' });
}

/**
 * Convert an ScVal BytesN<32> (or Buffer) to a 0x-prefixed hex string.
 */
function scValToIntentId(value: unknown): string {
  if (value instanceof Uint8Array || Buffer.isBuffer(value)) {
    return '0x' + Buffer.from(value as Uint8Array).toString('hex');
  }
  if (typeof value === 'string') {
    return value.startsWith('0x') ? value : '0x' + value;
  }
  throw new Error(`Unexpected intent ID value from contract: ${String(value)}`);
}

/**
 * Extract a meaningful error message from a Soroban RPC error response.
 */
function describeRpcError(context: string, error: unknown): Error {
  if (error instanceof Error) {
    return new Error(`${context}: ${error.message}`);
  }
  if (typeof error === 'string') {
    return new Error(`${context}: ${error}`);
  }
  try {
    return new Error(`${context}: ${JSON.stringify(error)}`);
  } catch {
    return new Error(`${context}: ${String(error)}`);
  }
}

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
  private server: SorobanRpc.Server;
  private contract: Contract;

  constructor(options: VortexClientOptions) {
    this.contractId = options.contractId;
    this.horizon = options.horizon;
    this.rpcUrl = options.rpcUrl || 'https://soroban-testnet.stellar.org';
    this.networkPassphrase = options.networkPassphrase || Networks.TESTNET_NETWORK_PASSPHRASE;
    this.baseFee = options.baseFee || BASE_FEE;
    this.timeout = options.timeout || 30000;
    this.server = new SorobanRpc.Server(this.rpcUrl, { allowHttp: this.rpcUrl.startsWith('http://') });
    try {
      this.contract = new Contract(this.contractId);
    } catch (err) {
      throw new ContractSpecError(
        String(this.contractId),
        err instanceof Error ? err.message : String(err)
      );
    }
  }

  /**
   * Build a Soroban contract invocation operation for the given method and args.
   */
  private buildInvokeOperation(method: string, args: xdr.ScVal[]): xdr.Operation {
    return this.contract.call(method, ...args);
  }

  /**
   * Simulate a read-only contract call and decode its return value.
   * Throws a meaningful error if simulation fails.
   */
  private async simulateCall(method: string, args: xdr.ScVal[]): Promise<unknown> {
    const account = await this.server.getAccount(this.contractId).catch(() => undefined);
    // Read-only simulations still need a valid source account; use the contract ID's
    // account only if it exists, otherwise fall back to a random funded-free source.
    const source = account ? account.accountId() : Keypair.random().publicKey();

    let builder: TransactionBuilder;
    try {
      const sourceAccount = account ?? new (await import('stellar-sdk')).Account(source, '0');
      builder = new TransactionBuilder(sourceAccount, {
        fee: this.baseFee,
        networkPassphrase: this.networkPassphrase,
      });
    } catch (err) {
      throw describeRpcError(`Failed to build simulation transaction for ${method}`, err);
    }

    builder.addOperation(this.buildInvokeOperation(method, args)).setTimeout(this.timeout / 1000);
    const tx = builder.build();

    let sim: SorobanRpc.SimulateTransactionResponse;
    try {
      sim = await this.server.simulateTransaction(tx);
    } catch (err) {
      throw describeRpcError(`Simulation of ${method} failed`, err);
    }

    if (SorobanRpc.Api.isSimulationError(sim)) {
      throw new Error(`Simulation of ${method} failed: ${sim.error}`);
    }
    if (!SorobanRpc.Api.isSimulationSuccess(sim) || !sim.result) {
      throw new Error(`Simulation of ${method} returned no result`);
    }
    return scValToNative(sim.result.retval);
  }

  /**
   * Prepare, sign, submit, and await a state-changing contract invocation.
   * Returns the decoded return value of the contract call.
   */
  private async invokeAndSubmit(
    method: string,
    args: xdr.ScVal[],
    signingKeypair: Keypair,
  ): Promise<unknown> {
    const account = await this.server.getAccount(signingKeypair.publicKey()).catch((err) => {
      throw describeRpcError(`Failed to load account ${signingKeypair.publicKey()}`, err);
    });

    const tx = new TransactionBuilder(account, {
      fee: this.baseFee,
      networkPassphrase: this.networkPassphrase,
    })
      .addOperation(this.buildInvokeOperation(method, args))
      .setTimeout(this.timeout / 1000)
      .build();

    let prepared: SorobanRpc.Api.PrepareTransactionResponse;
    try {
      prepared = await this.server.prepareTransaction(tx);
    } catch (err) {
      throw describeRpcError(`Failed to prepare ${method} transaction`, err);
    }

    const preparedTx = prepared as unknown as { sign: (...kps: Keypair[]) => void };
    preparedTx.sign(signingKeypair);

    let sendResponse: SorobanRpc.Api.SendTransactionResponse;
    try {
      sendResponse = await this.server.sendTransaction(prepared as unknown as Parameters<SorobanRpc.Server['sendTransaction']>[0]);
    } catch (err) {
      throw describeRpcError(`Failed to submit ${method} transaction`, err);
    }

    if (sendResponse.status === 'ERROR') {
      throw new Error(
        `Submission of ${method} failed: ${JSON.stringify(sendResponse.errorResult ?? sendResponse)}`,
      );
    }

    const hash = sendResponse.hash;
    const deadline = Date.now() + this.timeout;
    let getResponse: SorobanRpc.Api.GetTransactionResponse | undefined;
    while (Date.now() < deadline) {
      try {
        getResponse = await this.server.getTransaction(hash);
      } catch (err) {
        throw describeRpcError(`Failed to poll ${method} transaction ${hash}`, err);
      }
      if (getResponse.status !== SorobanRpc.Api.GetTransactionStatus.NOT_FOUND) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }

    if (!getResponse || getResponse.status === SorobanRpc.Api.GetTransactionStatus.NOT_FOUND) {
      throw new Error(`Timed out waiting for ${method} transaction ${hash} to confirm`);
    }
    if (getResponse.status === SorobanRpc.Api.GetTransactionStatus.FAILED) {
      throw new Error(
        `Transaction ${hash} for ${method} failed: ${JSON.stringify(getResponse.resultXdr ?? getResponse)}`,
      );
    }

    const success = getResponse as SorobanRpc.Api.GetSuccessfulTransactionResponse;
    if (success.returnValue) {
      return scValToNative(success.returnValue);
    }
    return undefined;
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
    const args: xdr.ScVal[] = [
      new Address(options.user).toScVal(),
      nativeToScVal(options.src_chain, { type: 'string' }),
      nativeToScVal(options.src_token, { type: 'string' }),
      nativeToScVal(options.src_amount, { type: 'i128' }),
      new Address(options.dst_token).toScVal(),
      nativeToScVal(options.min_dst_amount, { type: 'i128' }),
    ];
    const result = await this.invokeAndSubmit('submit_intent', args, signingKeypair);
    return scValToIntentId(result);
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
    const args: xdr.ScVal[] = [
      new Address(options.solver).toScVal(),
      intentIdToScVal(options.intent_id),
    ];
    await this.invokeAndSubmit('accept_intent', args, signingKeypair);
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
    const args: xdr.ScVal[] = [
      new Address(options.solver).toScVal(),
      intentIdToScVal(options.intent_id),
      nativeToScVal(options.fill_amount, { type: 'i128' }),
      nativeToScVal(Boolean(options.require_proof), { type: 'bool' }),
    ];
    await this.invokeAndSubmit('fill_intent', args, signingKeypair);
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
    const args: xdr.ScVal[] = [
      new Address(signingKeypair.publicKey()).toScVal(),
      intentIdToScVal(intentId),
    ];
    await this.invokeAndSubmit('cancel_intent', args, signingKeypair);
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
    const args: xdr.ScVal[] = [
      new Address(options.solver).toScVal(),
      new Address(options.bond_token).toScVal(),
      nativeToScVal(options.bond_amount, { type: 'i128' }),
    ];
    await this.invokeAndSubmit('register_solver', args, signingKeypair);
  }

  /**
   * Deregister as a solver and withdraw all bonded tokens
   *
   * @param solver Solver address
   * @param signingKeypair Keypair for transaction signing (must match solver)
   */
  async deregisterSolver(solver: Address, signingKeypair: Keypair): Promise<void> {
    const args: xdr.ScVal[] = [new Address(solver.toString()).toScVal()];
    await this.invokeAndSubmit('deregister_solver', args, signingKeypair);
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
    const result = await this.simulateCall('is_solver_eligible', [new Address(solver.toString()).toScVal()]);
    return Boolean(result);
  }

  /**
   * Fetch a single intent by ID
   *
   * @param intentId The intent's 32-byte ID
   * @returns The intent record, or undefined if not found
   */
  async getIntent(intentId: string): Promise<types.IntentRecord | undefined> {
    const result = await this.simulateCall('get_intent', [intentIdToScVal(intentId)]);
    if (result === undefined || result === null) {
      return undefined;
    }
    return result as types.IntentRecord;
  }

  /**
   * Fetch a solver's registration record
   *
   * @param solver Solver address
   * @returns The solver record, or undefined if not registered
   */
  async getSolver(solver: Address): Promise<types.SolverRecord | undefined> {
    const result = await this.simulateCall('get_solver', [new Address(solver.toString()).toScVal()]);
    if (result === undefined || result === null) {
      return undefined;
    }
    return result as types.SolverRecord;
  }

  /**
   * List all currently open and partially-filled intents
   *
   * @param options Pagination options (offset, limit)
   * @returns Array of open intent IDs (paginated, up to MAX_PAGE_SIZE per call)
   */
  async listOpenIntents(options?: types.ListIntentsOptions): Promise<string[]> {
    const args: xdr.ScVal[] = [
      nativeToScVal(options?.offset ?? 0, { type: 'u32' }),
      nativeToScVal(options?.limit ?? 0, { type: 'u32' }),
    ];
    const result = await this.simulateCall('list_open_intents', args);
    return this.decodeIntentIdList(result);
  }

  /**
   * List intents submitted by a specific user
   *
   * @param user User address
   * @returns Array of intent IDs submitted by this user
   */
  async listIntentsByUser(user: Address): Promise<string[]> {
    const result = await this.simulateCall('list_intents_by_user', [new Address(user.toString()).toScVal()]);
    return this.decodeIntentIdList(result);
  }

  /**
   * Decode a contract return value into an array of 0x-prefixed intent ID hex strings.
   */
  private decodeIntentIdList(result: unknown): string[] {
    if (!Array.isArray(result)) {
      return [];
    }
    return result.map((value) => scValToIntentId(value));
  }

  /**
   * Check if the contract is currently paused
   *
   * @returns Whether pause() has been called and unpause() has not been called since
   */
  async isPaused(): Promise<boolean> {
    const result = await this.simulateCall('is_paused', []);
    return Boolean(result);
  }

  /**
   * Get current protocol configuration
   *
   * @returns Protocol config (min_bond, fill_window, intent_expiry, protocol_fee_bps, etc.)
   */
  async getConfig(): Promise<types.ProtocolConfig> {
    const result = await this.simulateCall('get_config', []);
    return result as types.ProtocolConfig;
  }

  /**
   * Get fee schedule for a solver (including any discount tier bonus)
   *
   * @param solver Solver address
   * @returns Base fee, effective fee after discount, and list of tiers
   */
  async getFeeSchedule(solver: Address): Promise<types.FeeSchedule> {
    const result = await this.simulateCall('get_fee_schedule', [new Address(solver.toString()).toScVal()]);
    return result as types.FeeSchedule;
  }

  /**
   * Get current protocol health metrics
   *
   * @returns Total intents, TVL, solvers, bonds, fees collected, state breakdown
   */
  async getProtocolHealth(): Promise<types.ProtocolHealth> {
    const result = await this.simulateCall('get_protocol_health', []);
    return result as types.ProtocolHealth;
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
    const args: xdr.ScVal[] = [
      new Address(signingKeypair.publicKey()).toScVal(),
      new Address(newFeeRecipient.toString()).toScVal(),
    ];
    await this.invokeAndSubmit('propose_fee_recipient', args, signingKeypair);
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
    const args: xdr.ScVal[] = [new Address(signingKeypair.publicKey()).toScVal()];
    await this.invokeAndSubmit('accept_fee_recipient', args, signingKeypair);
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
    const args: xdr.ScVal[] = [
      new Address(signingKeypair.publicKey()).toScVal(),
      new Address(newAdmin.toString()).toScVal(),
    ];
    await this.invokeAndSubmit('propose_admin_transfer', args, signingKeypair);
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
    const args: xdr.ScVal[] = [new Address(signingKeypair.publicKey()).toScVal()];
    await this.invokeAndSubmit('accept_admin_transfer', args, signingKeypair);
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
    const args: xdr.ScVal[] = [
      new Address(signingKeypair.publicKey()).toScVal(),
      new Address(token.toString()).toScVal(),
    ];
    await this.invokeAndSubmit('propose_add_dst_token', args, signingKeypair);
  }

  /**
   * Add a source chain to the allowlist (admin-only, no timelock)
   *
   * @param chain Chain name (e.g. "ethereum", "base")
   * @param signingKeypair Admin keypair for signing
   */
  async addAllowedSrcChain(chain: string, signingKeypair: Keypair): Promise<void> {
    const args: xdr.ScVal[] = [
      new Address(signingKeypair.publicKey()).toScVal(),
      nativeToScVal(chain, { type: 'string' }),
    ];
    await this.invokeAndSubmit('add_allowed_src_chain', args, signingKeypair);
  }

  /**
   * Check if a source chain is in the allowlist (if enabled)
   *
   * @param chain Chain name
   * @returns Whether this chain is allowed
   */
  async isSrcChainAllowed(chain: string): Promise<boolean> {
    const result = await this.simulateCall('is_src_chain_allowed', [nativeToScVal(chain, { type: 'string' })]);
    return Boolean(result);
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
    const args: xdr.ScVal[] = [
      new Address(signingKeypair.publicKey()).toScVal(),
      intentIdToScVal(intentId),
    ];
    await this.invokeAndSubmit('slash_solver', args, signingKeypair);
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
    const args: xdr.ScVal[] = [
      new Address(signingKeypair.publicKey()).toScVal(),
      new Address(token.toString()).toScVal(),
      new Address(to.toString()).toScVal(),
      nativeToScVal(amount, { type: 'i128' }),
    ];
    await this.invokeAndSubmit('rescue_tokens', args, signingKeypair);
  }
}
