# @vortex-protocol/types

TypeScript type definitions for Vortex Protocol smart contracts, automatically derived from Rust struct/enum definitions to guarantee type parity.

**Issue #138**

## Installation

```bash
npm install @vortex-protocol/types
```

Or from the monorepo:

```bash
cd types && npm install
```

## Usage

### Importing Types

```typescript
import {
  IntentRecord,
  SolverRecord,
  IntentState,
  ErrorCode,
  getErrorMessage,
} from "@vortex-protocol/types";

// Use types in your application
const intent: IntentRecord = {
  intent_id: "0x...",
  user: "GAAA...",
  src_chain: "ethereum",
  src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", // USDC
  src_amount: "1000000000", // 1000 USDC
  dst_token: "CDLM3....",
  min_dst_amount: "3000000000",
  // ... other fields
  state: IntentState.Open,
};

// Handle errors
try {
  // Contract call
} catch (error) {
  const code = error.code as ErrorCode;
  console.error(getErrorMessage(code));
}
```

### Type Definitions Included

- **`IntentRecord`** — A user's cross-chain swap intent
- **`SolverRecord`** — A registered solver (market maker)
- **`ReputationSnapshot`** — Solver reputation snapshot (Issue #272)
- **`IntentState`** — Enum of intent lifecycle states
- **`DisputeResolution`** — Enum for dispute outcomes (Issue #188)
- **`ErrorCode`** — Complete error code enumeration
- **`ErrorMessages`** — Map of error codes to human-readable messages
- **Per-contract error enums** — `CommonError`, `SolverRegistryError`, `ProofRegistryError`, `ReputationBadgeError`, `ArbiterPanelError`, `DirectoryError`, `TimelockError` (alongside the existing governor, treasury_streams, fee_router, backstop_vault and intent_periphery enums)
- **Event payload interfaces** — `IntentSubmittedEvent`, `IntentAcceptedEvent`, `IntentFilledEvent`, `ExtensionGrantedEvent`, `SolverSlashedEvent`, `BondWithdrawnEvent`, `TokensRescuedEvent`, `BondMultiplierSetEvent`, `ConfigUpdatedEvent`, `BackstopDepositEvent`, `BackstopWithdrawEvent`, `BackstopCoverEvent`, `StreamCreatedEvent`, `StreamWithdrawEvent`, `StreamCancelledEvent`, `PeripheryInitializedEvent` (decoded event data; see `docs/event-schema.md`)

#### Governor (`governor`)

- **`Proposal`** — A governance proposal with its votes and execution state
- **`ProposalState`** — Enum of proposal lifecycle states
- **`VoteType`** — Enum of vote choices (for/against/abstain)
- **`GovernorConfig`** — Governor configuration (voting period, quorum, thresholds)

#### Backstop Vault (`backstop_vault`)

- **`BackstopVaultConfig`** — Vault configuration (capacity, cooldown, share pricing)
- **`BackstopDeposit`** — A depositor's position in the backstop vault
- **`BackstopVaultState`** — Enum of vault lifecycle states

#### Fee Router (`fee_router`)

- **`Sink`** — A fee recipient with its routing weight
- **`FeeRouterConfig`** — Fee router configuration (sinks and split parameters)
- **`FeeDistribution`** — A recorded fee distribution across sinks

#### Treasury Streams (`treasury_streams`)

- **`Stream`** — A token stream (recipient, rate, start/end, claimed amount)
- **`StreamState`** — Enum of stream lifecycle states
- **`TreasuryStreamsConfig`** — Treasury streams configuration (limits, cliff defaults)

#### Intent Periphery (`intent_periphery`)

- **`PeripheryIntent`** — An intent submitted through the periphery contract
- **`PeripheryConfig`** — Periphery configuration (allowed tokens, fee parameters)
- **`PeripheryIntentState`** — Enum of periphery intent lifecycle states

### Error Handling

```typescript
import { ErrorCode, getErrorMessage } from "@vortex-protocol/types";

// Map contract error codes to messages
const message = getErrorMessage(ErrorCode.SolverBondTooLow);
console.log(message); // "Solver bond is below minimum"
```

## Drift Detection

Types are derived from the Rust `#[contracttype]` definitions in each contract's `lib.rs`. A CI check (`check-drift`) ensures TypeScript types stay synchronized across all six contracts:

- `intent_settlement`
- `governor`
- `backstop_vault`
- `fee_router`
- `treasury_streams`
- `intent_periphery`

```bash
npm run check-drift
```

This verifies, for every covered contract:
- All struct fields exist in both Rust and TypeScript
- All enum variants are present
- Type names match exactly

If drift is detected, the check fails and shows which fields are missing. Always update both Rust and TypeScript definitions together.

## Building

Build TypeScript to JavaScript:

```bash
npm run build
```

Output goes to `dist/`:

```
types/
├── dist/
│   ├── index.js
│   ├── index.d.ts        # Type definitions
│   ├── index.js.map
│   └── index.d.ts.map
└── src/
    └── index.ts
```

## Versioning

Types version tracks the `intent_settlement` contract version. When contract types change, increment the types version.

## Architecture Notes

**Number Precision:** Large numeric fields (`src_amount`, `min_dst_amount`, `bond_amount`, `total_volume`, `fill_amount`, `total_filled`) are stored as strings to preserve precision. JavaScript `number` cannot safely represent i128 values; strings avoid rounding errors.

**Addresses:** Stellar addresses are plain strings. Contract code should validate address format at the Soroban boundary.

**Timestamps:** Stored as `number` (u64 in Rust), safe up to year 286,000 AD.

## Contributing

When updating contract types in Rust:

1. Modify the relevant contract's `lib.rs` (`intent_settlement`, `governor`, `backstop_vault`, `fee_router`, `treasury_streams`, or `intent_periphery`)
2. Update corresponding types in `types/src/index.ts`
3. Run `npm run check-drift` to verify parity
4. Update `types/` version in `package.json`
5. Commit both changes together

## See Also

- [Vortex Protocol](https://github.com/vortex-protocol)
- [Intent Settlement Contract](../intent_settlement/)
- [Governor Contract](../governor/)
- [Backstop Vault Contract](../backstop_vault/)
- [Fee Router Contract](../fee_router/)
- [Treasury Streams Contract](../treasury_streams/)
- [Intent Periphery Contract](../intent_periphery/)
- [Solver Integration Guide](../docs/solver-integration-guide.md)

## License

MIT
