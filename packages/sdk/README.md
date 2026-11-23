# @vortex-protocol/sdk

TypeScript SDK for the [Vortex Protocol](https://github.com/vortex-protocol) intent-settlement contract.

Provides a type-safe interface to interact with the contract on Stellar / Soroban, reducing integration friction for solver bots and frontend applications.

## Installation

```bash
npm install @vortex-protocol/sdk stellar-sdk
```

## Quick Start

```typescript
import { VortexClient } from '@vortex-protocol/sdk';
import { Server, Keypair } from 'stellar-sdk';

// Initialize client
const horizon = new Server('https://horizon-testnet.stellar.org');
const client = new VortexClient({
  contractId: 'CAKJ2RNHMBT74VJLGVLZ6SCOBRQMDQ6PU5CCYKP4IJMVKFAACJKBRMM',
  horizon,
  rpcUrl: 'https://soroban-testnet.stellar.org',
});

// Check if solver is eligible
const isEligible = await client.isSolverEligible(solverAddress);

// List open intents
const openIntents = await client.listOpenIntents({ limit: 10 });

// Submit an intent
await client.submitIntent(
  {
    user: userAddress,
    src_chain: 'ethereum',
    src_token: '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', // WETH
    src_amount: 1_000_000_000_000_000_000n, // 1 ETH
    dst_token: usdc_sac_address,
    min_dst_amount: 3_500_000_000n, // 3500 USDC
  },
  signingKeypair
);

// Accept an intent
await client.acceptIntent(
  {
    solver: solverAddress,
    intent_id: intentId,
  },
  solverKeypair
);

// Fill an intent
await client.fillIntent(
  {
    solver: solverAddress,
    intent_id: intentId,
    fill_amount: 3_500_000_000n,
  },
  solverKeypair
);
```

## API Overview

### User Operations

- `submitIntent()` - Create a new swap intent
- `cancelIntent()` - Cancel an open intent (user-only)

### Solver Operations

- `registerSolver()` - Register with a bond
- `deregisterSolver()` - Unregister and withdraw bond
- `acceptIntent()` - Accept an open intent
- `fillIntent()` - Deliver output tokens and close intent
- `isSolverEligible()` - Check registration and bond status

### Read-Only Queries

- `getIntent()` - Fetch a single intent by ID
- `getSolver()` - Fetch a solver's record
- `listOpenIntents()` - List currently open intents (paginated)
- `listIntentsByUser()` - List intents submitted by a user
- `getConfig()` - Fetch protocol configuration
- `getProtocolHealth()` - Get statistics and TVL
- `getFeeSchedule()` - Get effective fee rate for a solver
- `isPaused()` - Check if contract is paused

### Admin Operations

- `proposeFeeRecipient()` - Propose fee recipient change (timelocked)
- `acceptFeeRecipient()` - Accept pending fee recipient change
- `proposeAdminTransfer()` - Propose admin transfer (timelocked)
- `acceptAdminTransfer()` - Accept pending admin transfer
- `proposeAddDstToken()` - Add destination token (timelocked)
- `addAllowedSrcChain()` - Add source chain to allowlist
- `isSrcChainAllowed()` - Check if source chain is allowed
- `slashSolver()` - Slash a solver for missing fill window (permissionless)
- `rescueTokens()` - Recover tokens sent by mistake (admin-only)

## Types

The SDK exports all contract data types:

```typescript
import {
  IntentRecord,
  SolverRecord,
  ProtocolConfig,
  ProtocolHealth,
  IntentState,
  SolverTier,
  SubmitIntentOptions,
  AcceptIntentOptions,
  FillIntentOptions,
} from '@vortex-protocol/sdk';
```

See [`src/types.ts`](./src/types.ts) for full documentation.

## Features

✅ **Type-safe** — Full TypeScript support with JSDoc comments  
✅ **Comprehensive** — All 100+ contract functions exposed  
✅ **Soroban-native** — Built on `stellar-sdk` for Soroban contracts  
✅ **Efficient** — The contract spec is parsed once, on first use, and cached on the client instance, so reuse one `VortexClient` for many calls  
✅ **Well-documented** — Every method and type has detailed comments  

## Status

This SDK is **functional**: every method performs a real call against the `intent_settlement` contract via Soroban RPC.

- **Read-only queries** (`getIntent`, `getSolver`, `listOpenIntents`, `getConfig`, `isPaused`, `isSolverEligible`, etc.) are executed as simulated contract invocations and return decoded results.
- **State-changing operations** (`submitIntent`, `acceptIntent`, `fillIntent`, `registerSolver`, admin operations, etc.) build a Soroban `Contract` invocation, prepare it via RPC (fee and footprint estimation), sign it with the supplied keypair, submit it, and poll for the result.
- Failures surface as real, meaningful errors (simulation failures, submission errors, and contract errors) rather than unconditional stub exceptions.

Potential future enhancements:
- Retry logic and backoff for transient RPC failures
- Event subscription and filtering helpers
- Additional coverage for other contracts (`governor`, `backstop_vault`, etc.)

## Development

```bash
# Install dependencies
npm install

# Build
npm run build

# Run tests
npm test

# Lint
npm run lint
```

## Contributing

See the main [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

## License

MIT — see [LICENSE](../../LICENSE)

## References

- [Vortex Protocol GitHub](https://github.com/stellar-vortex-protocol/vortex-contracts)
- [Solver Integration Guide](../../docs/solver-integration-guide.md)
- [Stellar SDK Documentation](https://developers.stellar.org/docs/tools/js-stellar-sdk)
- [Soroban Documentation](https://developers.stellar.org/docs/build/smart-contracts)
