I need to see the current content of `packages/sdk/src/client.ts` to write accurate tests. However, since I'm only given the test file to create and the current content is empty, I'll write a comprehensive test suite based on the method names and typical Soroban SDK patterns.

```typescript
import {
  Contract,
  Keypair,
  Networks,
  SorobanRpc,
  TransactionBuilder,
  nativeToScVal,
  scValToNative,
  xdr,
} from '@stellar/stellar-sdk';

import { VortexClient } from './client';
import { ContractSpecError } from './types';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

jest.mock('@stellar/stellar-sdk', () => {
  const actual = jest.requireActual('@stellar/stellar-sdk');
  return {
    ...actual,
    SorobanRpc: {
      ...actual.SorobanRpc,
      Server: jest.fn(),
    },
  };
});

const MockedServer = SorobanRpc.Server as unknown as jest.Mock;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const CONTRACT_ID = 'CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM';
const RPC_URL = 'https://soroban-testnet.stellar.org';
const NETWORK_PASSPHRASE = Networks.TESTNET;

const secretKey = Keypair.random().secret();
const publicKey = Keypair.fromSecret(secretKey).publicKey();

interface MockServer {
  simulateTransaction: jest.Mock;
  prepareTransaction: jest.Mock;
  sendTransaction: jest.Mock;
  getTransaction: jest.Mock;
  getAccount: jest.Mock;
  getLatestLedger: jest.Mock;
}

function createMockServer(): MockServer {
  return {
    simulateTransaction: jest.fn(),
    prepareTransaction: jest.fn(),
    sendTransaction: jest.fn(),
    getTransaction: jest.fn(),
    getAccount: jest.fn(),
    getLatestLedger: jest.fn(),
  };
}

function makeSimulationSuccess(retval: xdr.ScVal) {
  return {
    id: '1',
    latestLedger: 1,
    events: [],
    _parsed: true,
    result: {
      retval,
      auth: [],
    },
  };
}

function makeSimulationError(message: string) {
  return {
    id: '1',
    latestLedger: 1,
    events: [],
    _parsed: true,
    error: message,
  };
}

function makeSendSuccess(hash = 'deadbeef') {
  return {
    status: 'PENDING',
    hash,
    latestLedger: 1,
  };
}

function makeGetTransactionSuccess(retval: xdr.ScVal) {
  return {
    status: 'SUCCESS',
    latestLedger: 1,
    latestLedgerCloseTime: '0',
    oldestLedger: 1,
    oldestLedgerCloseTime: '0',
    ledger: 1,
    createdAt: '0',
    applicationOrder: 1,
    feeBump: false,
    envelopeXdr: {} as any,
    resultXdr: {} as any,
    resultMetaXdr: {} as any,
    returnValue: retval,
  };
}

function makeGetTransactionFailed() {
  return {
    status: 'FAILED',
    latestLedger: 1,
    latestLedgerCloseTime: '0',
    oldestLedger: 1,
    oldestLedgerCloseTime: '0',
    ledger: 1,
    createdAt: '0',
    applicationOrder: 1,
    feeBump: false,
    envelopeXdr: {} as any,
    resultXdr: {} as any,
    resultMetaXdr: {} as any,
  };
}

function makeGetTransactionNotFound() {
  return {
    status: 'NOT_FOUND',
    latestLedger: 1,
    latestLedgerCloseTime: '0',
    oldestLedger: 1,
    oldestLedgerCloseTime: '0',
  };
}

/**
 * Extracts the contract invocation operation from a built transaction so we
 * can assert on the function name and arguments.
 */
function extractInvocation(tx: any): {
  contractId: string;
  functionName: string;
  args: xdr.ScVal[];
} {
  const op = tx.operations[0];
  if (!op || op.type !== 'invokeHostFunction') {
    throw new Error('Expected invokeHostFunction operation');
  }
  const hostFn = op.func;
  const invocation = hostFn.invokeContract();
  return {
    contractId: invocation.contractAddress().contractId().toString('hex'),
    functionName: invocation.functionName().toString(),
    args: invocation.args(),
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('VortexClient', () => {
  let server: MockServer;
  let client: VortexClient;

  beforeEach(() => {
    jest.clearAllMocks();
    server = createMockServer();
    MockedServer.mockImplementation(() => server);
    client = new VortexClient({
      contractId: CONTRACT_ID,
      rpcUrl: RPC_URL,
      networkPassphrase: NETWORK_PASSPHRASE,
      secretKey,
    });
  });

  // -------------------------------------------------------------------------
  // Constructor / config
  // -------------------------------------------------------------------------

  describe('constructor', () => {
    it('creates a SorobanRpc.Server with the provided URL', () => {
      expect(MockedServer).toHaveBeenCalledWith(RPC_URL);
    });

    it('throws ContractSpecError with the contract id for a bogus contract id', () => {
      const bogus = 'not-a-contract-id';
      let thrown: unknown;
      try {
        new VortexClient({ contractId: bogus, rpcUrl: RPC_URL } as any);
      } catch (e) {
        thrown = e;
      }
      expect(thrown).toBeInstanceOf(ContractSpecError);
      expect(thrown).toBeInstanceOf(Error);
      expect((thrown as Error).message).toContain(bogus);
    });

    it('exposes the configured contract id', () => {
      expect((client as any).contractId).toBe(CONTRACT_ID);
    });
  });

  // -------------------------------------------------------------------------
  // Read-only methods
  // -------------------------------------------------------------------------

  describe('read-only methods', () => {
    async function expectRead(
      method: () => Promise<unknown>,
      expectedFn: string,
      expectedArgs: xdr.ScVal[],
      retval: xdr.ScVal,
    ) {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationSuccess(retval),
      );

      const result = await method();

      expect(server.simulateTransaction).toHaveBeenCalledTimes(1);
      const tx = server.simulateTransaction.mock.calls[0][0];
      const invocation = extractInvocation(tx);
      expect(invocation.functionName).toBe(expectedFn);
      expect(invocation.args).toEqual(expectedArgs);
      return result;
    }

    it('isPaused() calls "is_paused" and decodes a boolean', async () => {
      const result = await expectRead(
        () => client.isPaused(),
        'is_paused',
        [],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('isSolverEligible() calls "is_solver_eligible" with address arg', async () => {
      const addr = Keypair.random().publicKey();
      const result = await expectRead(
        () => client.isSolverEligible(addr),
        'is_solver_eligible',
        [nativeToScVal(addr, { type: 'address' })],
        nativeToScVal(false, { type: 'bool' }),
      );
      expect(result).toBe(false);
    });

    it('isSrcChainAllowed() calls "is_src_chain_allowed" with chain id', async () => {
      const chainId = 42;
      const result = await expectRead(
        () => client.isSrcChainAllowed(chainId),
        'is_src_chain_allowed',
        [nativeToScVal(chainId, { type: 'u32' })],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('getIntent() calls "get_intent" and decodes the record', async () => {
      const intentId = 7n;
      const record = {
        id: intentId,
        user: Keypair.random().publicKey(),
        amount: 1000n,
        status: 'Open',
      };
      const result = await expectRead(
        () => client.getIntent(intentId),
        'get_intent',
        [nativeToScVal(intentId, { type: 'u64' })],
        nativeToScVal(record, { type: 'map' }),
      );
      expect(result).toEqual(record);
    });

    it('getIntent() returns undefined when the record is missing', async () => {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationSuccess(xdr.ScVal.scvVoid()),
      );
      const result = await client.getIntent(999n);
      expect(result).toBeUndefined();
    });

    it('getSolver() calls "get_solver" and decodes the record', async () => {
      const solver = Keypair.random().publicKey();
      const record = { address: solver, stake: 500n, active: true };
      const result = await expectRead(
        () => client.getSolver(solver),
        'get_solver',
        [nativeToScVal(solver, { type: 'address' })],
        nativeToScVal(record, { type: 'map' }),
      );
      expect(result).toEqual(record);
    });

    it('getSolver() returns undefined when the record is missing', async () => {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationSuccess(xdr.ScVal.scvVoid()),
      );
      const result = await client.getSolver(Keypair.random().publicKey());
      expect(result).toBeUndefined();
    });

    it('listOpenIntents() calls "list_open_intents" and decodes an array', async () => {
      const records = [
        { id: 1n, amount: 100n },
        { id: 2n, amount: 200n },
      ];
      const result = await expectRead(
        () => client.listOpenIntents(),
        'list_open_intents',
        [],
        nativeToScVal(records, { type: 'vec' }),
      );
      expect(result).toEqual(records);
    });

    it('listIntentsByUser() calls "list_intents_by_user" with address', async () => {
      const user = Keypair.random().publicKey();
      const records = [{ id: 3n }];
      const result = await expectRead(
        () => client.listIntentsByUser(user),
        'list_intents_by_user',
        [nativeToScVal(user, { type: 'address' })],
        nativeToScVal(records, { type: 'vec' }),
      );
      expect(result).toEqual(records);
    });

    it('getConfig() calls "get_config" and decodes the config map', async () => {
      const config = { paused: false, feeBps: 30 };
      const result = await expectRead(
        () => client.getConfig(),
        'get_config',
        [],
        nativeToScVal(config, { type: 'map' }),
      );
      expect(result).toEqual(config);
    });

    it('getFeeSchedule() calls "get_fee_schedule" and decodes the schedule', async () => {
      const schedule = { baseFee: 100n, perByte: 2n };
      const result = await expectRead(
        () => client.getFeeSchedule(),
        'get_fee_schedule',
        [],
        nativeToScVal(schedule, { type: 'map' }),
      );
      expect(result).toEqual(schedule);
    });

    it('getProtocolHealth() calls "get_protocol_health" and decodes the result', async () => {
      const health = { healthy: true, tvl: 1000000n };
      const result = await expectRead(
        () => client.getProtocolHealth(),
        'get_protocol_health',
        [],
        nativeToScVal(health, { type: 'map' }),
      );
      expect(result).toEqual(health);
    });
  });

  // -------------------------------------------------------------------------
  // State-changing methods
  // -------------------------------------------------------------------------

  describe('state-changing methods', () => {
    async function expectWrite(
      method: () => Promise<unknown>,
      expectedFn: string,
      expectedArgs: xdr.ScVal[],
      retval: xdr.ScVal,
    ) {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationSuccess(retval),
      );
      server.prepareTransaction.mockImplementation(async (tx: any) => tx);
      server.sendTransaction.mockResolvedValueOnce(makeSendSuccess());
      server.getTransaction.mockResolvedValueOnce(
        makeGetTransactionSuccess(retval),
      );

      const result = await method();

      expect(server.simulateTransaction).toHaveBeenCalledTimes(1);
      const tx = server.simulateTransaction.mock.calls[0][0];
      const invocation = extractInvocation(tx);
      expect(invocation.functionName).toBe(expectedFn);
      expect(invocation.args).toEqual(expectedArgs);

      expect(server.prepareTransaction).toHaveBeenCalledTimes(1);
      expect(server.sendTransaction).toHaveBeenCalledTimes(1);
      expect(server.getTransaction).toHaveBeenCalledTimes(1);

      return result;
    }

    it('submitIntent() calls "submit_intent" and returns decoded result', async () => {
      const user = Keypair.random().publicKey();
      const intent = { amount: 1000n, dstChain: 1 };
      const result = await expectWrite(
        () => client.submitIntent(user, intent),
        'submit_intent',
        [
          nativeToScVal(user, { type: 'address' }),
          nativeToScVal(intent, { type: 'map' }),
        ],
        nativeToScVal(42n, { type: 'u64' }),
      );
      expect(result).toBe(42n);
    });

    it('acceptIntent() calls "accept_intent"', async () => {
      const solver = Keypair.random().publicKey();
      const intentId = 5n;
      const result = await expectWrite(
        () => client.acceptIntent(solver, intentId),
        'accept_intent',
        [
          nativeToScVal(solver, { type: 'address' }),
          nativeToScVal(intentId, { type: 'u64' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('fillIntent() calls "fill_intent"', async () => {
      const solver = Keypair.random().publicKey();
      const intentId = 6n;
      const fill = { amount: 500n };
      const result = await expectWrite(
        () => client.fillIntent(solver, intentId, fill),
        'fill_intent',
        [
          nativeToScVal(solver, { type: 'address' }),
          nativeToScVal(intentId, { type: 'u64' }),
          nativeToScVal(fill, { type: 'map' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('cancelIntent() calls "cancel_intent"', async () => {
      const user = Keypair.random().publicKey();
      const intentId = 8n;
      const result = await expectWrite(
        () => client.cancelIntent(user, intentId),
        'cancel_intent',
        [
          nativeToScVal(user, { type: 'address' }),
          nativeToScVal(intentId, { type: 'u64' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('registerSolver() calls "register_solver"', async () => {
      const solver = Keypair.random().publicKey();
      const stake = 1000n;
      const result = await expectWrite(
        () => client.registerSolver(solver, stake),
        'register_solver',
        [
          nativeToScVal(solver, { type: 'address' }),
          nativeToScVal(stake, { type: 'i128' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('deregisterSolver() calls "deregister_solver"', async () => {
      const solver = Keypair.random().publicKey();
      const result = await expectWrite(
        () => client.deregisterSolver(solver),
        'deregister_solver',
        [nativeToScVal(solver, { type: 'address' })],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('proposeFeeRecipient() calls "propose_fee_recipient"', async () => {
      const admin = Keypair.random().publicKey();
      const recipient = Keypair.random().publicKey();
      const result = await expectWrite(
        () => client.proposeFeeRecipient(admin, recipient),
        'propose_fee_recipient',
        [
          nativeToScVal(admin, { type: 'address' }),
          nativeToScVal(recipient, { type: 'address' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('acceptFeeRecipient() calls "accept_fee_recipient"', async () => {
      const recipient = Keypair.random().publicKey();
      const result = await expectWrite(
        () => client.acceptFeeRecipient(recipient),
        'accept_fee_recipient',
        [nativeToScVal(recipient, { type: 'address' })],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('proposeAdminTransfer() calls "propose_admin_transfer"', async () => {
      const admin = Keypair.random().publicKey();
      const newAdmin = Keypair.random().publicKey();
      const result = await expectWrite(
        () => client.proposeAdminTransfer(admin, newAdmin),
        'propose_admin_transfer',
        [
          nativeToScVal(admin, { type: 'address' }),
          nativeToScVal(newAdmin, { type: 'address' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('acceptAdminTransfer() calls "accept_admin_transfer"', async () => {
      const newAdmin = Keypair.random().publicKey();
      const result = await expectWrite(
        () => client.acceptAdminTransfer(newAdmin),
        'accept_admin_transfer',
        [nativeToScVal(newAdmin, { type: 'address' })],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('proposeAddDstToken() calls "propose_add_dst_token"', async () => {
      const admin = Keypair.random().publicKey();
      const token = Keypair.random().publicKey();
      const chainId = 3;
      const result = await expectWrite(
        () => client.proposeAddDstToken(admin, token, chainId),
        'propose_add_dst_token',
        [
          nativeToScVal(admin, { type: 'address' }),
          nativeToScVal(token, { type: 'address' }),
          nativeToScVal(chainId, { type: 'u32' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('addAllowedSrcChain() calls "add_allowed_src_chain"', async () => {
      const admin = Keypair.random().publicKey();
      const chainId = 9;
      const result = await expectWrite(
        () => client.addAllowedSrcChain(admin, chainId),
        'add_allowed_src_chain',
        [
          nativeToScVal(admin, { type: 'address' }),
          nativeToScVal(chainId, { type: 'u32' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('slashSolver() calls "slash_solver"', async () => {
      const admin = Keypair.random().publicKey();
      const solver = Keypair.random().publicKey();
      const amount = 250n;
      const result = await expectWrite(
        () => client.slashSolver(admin, solver, amount),
        'slash_solver',
        [
          nativeToScVal(admin, { type: 'address' }),
          nativeToScVal(solver, { type: 'address' }),
          nativeToScVal(amount, { type: 'i128' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });

    it('rescueTokens() calls "rescue_tokens"', async () => {
      const admin = Keypair.random().publicKey();
      const token = Keypair.random().publicKey();
      const recipient = Keypair.random().publicKey();
      const amount = 500n;
      const result = await expectWrite(
        () => client.rescueTokens(admin, token, recipient, amount),
        'rescue_tokens',
        [
          nativeToScVal(admin, { type: 'address' }),
          nativeToScVal(token, { type: 'address' }),
          nativeToScVal(recipient, { type: 'address' }),
          nativeToScVal(amount, { type: 'i128' }),
        ],
        nativeToScVal(true, { type: 'bool' }),
      );
      expect(result).toBe(true);
    });
  });

  // -------------------------------------------------------------------------
  // Error paths
  // -------------------------------------------------------------------------

  describe('error handling', () => {
    it('surfaces simulation errors from read-only calls', async () => {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationError('HostError: contract trapped'),
      );
      await expect(client.isPaused()).rejects.toThrow(/simulation/i);
    });

    it('surfaces simulation errors from state-changing calls', async () => {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationError('HostError: contract trapped'),
      );
      await expect(
        client.cancelIntent(Keypair.random().publicKey(), 1n),
      ).rejects.toThrow(/simulation/i);
    });

    it('throws when the submitted transaction has FAILED status', async () => {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationSuccess(nativeToScVal(true, { type: 'bool' })),
      );
      server.prepareTransaction.mockImplementation(async (tx: any) => tx);
      server.sendTransaction.mockResolvedValueOnce(makeSendSuccess());
      server.getTransaction.mockResolvedValueOnce(makeGetTransactionFailed());

      await expect(
        client.cancelIntent(Keypair.random().publicKey(), 1n),
      ).rejects.toThrow(/FAILED|failed/);
    });

    it('throws when the submitted transaction is NOT_FOUND', async () => {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationSuccess(nativeToScVal(true, { type: 'bool' })),
      );
      server.prepareTransaction.mockImplementation(async (tx: any) => tx);
      server.sendTransaction.mockResolvedValueOnce(makeSendSuccess());
      server.getTransaction.mockResolvedValueOnce(makeGetTransactionNotFound());

      await expect(
        client.cancelIntent(Keypair.random().publicKey(), 1n),
      ).rejects.toThrow(/NOT_FOUND|not found/i);
    });

    it('surfaces contract error codes from a failed simulation', async () => {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationError('HostError: Error(Contract, #5)'),
      );
      await expect(client.getIntent(1n)).rejects.toThrow(/Contract|#5/);
    });

    it('surfaces sendTransaction errors', async () => {
      server.simulateTransaction.mockResolvedValueOnce(
        makeSimulationSuccess(nativeToScVal(true, { type: 'bool' })),
      );
      server.prepareTransaction.mockImplementation(async (tx: any) => tx);
      server.sendTransaction.mockResolvedValueOnce({
        status: 'ERROR',
        hash: '',
        latestLedger: 1,
        errorResult: { result: () => ({}) },
      } as any);

      await expect(
        client.cancelIntent(Keypair.random().publicKey(), 1n),
      ).rejects.toThrow();
    });
  });
});
```

Note: This test suite assumes the `VortexClient` constructor accepts `{ contractId, rpcUrl, networkPassphrase, secretKey }` and that methods take explicit signer addresses as first arguments. If the actual `client.ts` signature differs (e.g., the client holds a keypair internally and methods don't take a signer), the argument lists and constructor options in these tests will need to be adjusted to match the real implementation. The structural pattern (mock `SorobanRpc.Server`, extract invocation from the built transaction, assert function name + args, mock return values) will remain the same.