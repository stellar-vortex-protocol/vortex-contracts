# Design Doc: Stellar Oracle/Messaging Interface for Source-Chain Proof Verification

**Issue:** [#124](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/124)  
**Branch:** `docs/task-spike`  
**Status:** Design complete — ready for implementation  
**Blocked by:** [#49](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/49) (spike complete, Wormhole recommended)

---

## 1. Overview

This document defines the concrete on-chain interface for verifying that a
user's source-chain deposit transaction occurred before releasing a solver's
fill. It is the implementation follow-up to the research spike in [#49](./49-cross-chain-proof-spike.md).

**Selected transport: Wormhole generic messaging** (see #49 §6 for rationale).

---

## 2. System Architecture

```
┌────────────────────────────────────────────────────────────────────────────┐
│  SOURCE CHAIN (Ethereum / Base / Polygon / …)                              │
│                                                                            │
│  User calls VortexDeposit.deposit(intent_id, token, amount)                │
│      │                                                                     │
│      └──► emit WormholeMessage {                                           │
│               payload: encode(intent_id, user, token, amount, chain_id)   │
│           }                                                                │
└────────────────────────────────────────────────────────────────────────────┘
                          │
                          │  Wormhole Guardian quorum signs VAA
                          ▼
┌────────────────────────────────────────────────────────────────────────────┐
│  STELLAR                                                                   │
│                                                                            │
│  ProofRegistry contract                                                    │
│      receive_message(vaa: Bytes)                                           │
│          └─ verify Guardian signatures via Wormhole Core contract          │
│          └─ decode payload → ProofRecord                                   │
│          └─ store DataKey::Proof(intent_id) = ProofRecord                  │
│                                                                            │
│  intent_settlement (existing, modified)                                    │
│      fill_intent(solver, intent_id, fill_amount, proof_id)                 │
│          └─ call ProofRegistry.get_proof(intent_id)                        │
│          └─ validate: proof.user == intent.user                            │
│          └─ validate: proof.src_amount >= intent.src_amount                │
│          └─ validate: proof.src_chain == intent.src_chain                  │
│          └─ proceed with token transfer                                    │
└────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. New Contract: `ProofRegistry`

This is a standalone Soroban contract. Keeping it separate from
`intent_settlement` allows:
- Independent auditing of the verification logic.
- Reuse by other Vortex contracts in the future.
- Upgradability without touching the settlement contract's core state.

### 3.1 Storage

```rust
#[contracttype]
#[derive(Clone)]
pub enum ProofKey {
    /// Wormhole Core contract address (set in initialize)
    WormholeCore,
    /// Admin of this registry
    Admin,
    /// Set of authorized emitter addresses per chain_id.
    /// Key: chain_id → emitter_address (BytesN<32>)
    AuthorizedEmitter(u16),
    /// Verified proof records by intent_id
    Proof(BytesN<32>),
}
```

### 3.2 Proof record

```rust
#[contracttype]
#[derive(Clone)]
pub struct ProofRecord {
    /// The intent this proof corresponds to
    pub intent_id: BytesN<32>,
    /// User address on the source chain (hex string, e.g. "0xabc…")
    pub src_user: String,
    /// Source chain Wormhole chain_id (e.g. 2 = Ethereum, 30 = Base)
    pub src_chain_id: u16,
    /// Source token address on the source chain
    pub src_token: String,
    /// Amount deposited on the source chain (in that token's units)
    pub src_amount: i128,
    /// Wormhole VAA sequence number (for deduplication)
    pub vaa_sequence: u64,
    /// Ledger timestamp when this proof was registered on Stellar
    pub received_at: u64,
}
```

### 3.3 Interface

```rust
pub trait ProofRegistryTrait {

    /// Deploy-time initialization. Sets Wormhole Core contract and admin.
    fn initialize(env: Env, admin: Address, wormhole_core: Address);

    /// Admin-only: register a trusted emitter address on a given source chain.
    /// Only messages from these addresses will be accepted.
    fn set_authorized_emitter(env: Env, chain_id: u16, emitter: BytesN<32>);

    /// Remove an authorized emitter (e.g., if source contract is upgraded).
    fn remove_authorized_emitter(env: Env, chain_id: u16);

    /// Anyone can relay a signed VAA here. The contract verifies Guardian
    /// signatures via the Wormhole Core contract, decodes the payload,
    /// checks the emitter is authorized, and stores the ProofRecord.
    ///
    /// Panics if:
    ///   - VAA signature verification fails
    ///   - Emitter not authorized for the claimed chain_id
    ///   - Intent proof already registered (replay protection)
    fn receive_message(env: Env, vaa: Bytes);

    /// Read a stored proof by intent_id. Returns None if not yet received.
    fn get_proof(env: Env, intent_id: BytesN<32>) -> Option<ProofRecord>;

    /// Convenience: returns true iff a valid proof exists for this intent_id.
    fn has_proof(env: Env, intent_id: BytesN<32>) -> bool;
}
```

### 3.4 VAA payload encoding

The `VortexDeposit` source-chain contract encodes the payload as:

```
bytes32  intent_id       // Vortex intent ID (same derivation as on Stellar)
bytes20  src_user        // user's address on source chain (EVM: 20 bytes)
uint16   src_chain_id    // Wormhole chain ID of source chain
bytes32  src_token       // token address (padded to 32 bytes)
int128   src_amount      // amount in source token's smallest unit
```

Total: 32 + 20 + 2 + 32 + 16 = **102 bytes** (fixed-size, no ABI encoding
overhead, suitable for Wormhole's generic message payload).

`ProofRegistry.receive_message` deserializes this payload after VAA verification.

---

## 4. Changes to `intent_settlement`

### 4.1 New storage key

```rust
// DataKey addition:
ProofRegistry,  // Address of the deployed ProofRegistry contract
```

Set by admin via:
```rust
pub fn set_proof_registry(env: Env, registry: Address)
```

### 4.2 Updated `fill_intent` signature

```rust
pub fn fill_intent(
    env: Env,
    solver: Address,
    intent_id: BytesN<32>,
    fill_amount: i128,
    require_proof: bool,  // ← new: opt-in proof requirement
)
```

`require_proof` is `true` for proof-gated fills and `false` for the legacy
economic-trust mode. This enables a phased rollout (see §6).

### 4.3 Proof validation logic inside `fill_intent`

```rust
if require_proof {
    let registry_addr: Address = env
        .storage()
        .instance()
        .get(&DataKey::ProofRegistry)
        .unwrap_or_else(|| panic_with_error!(&env, Error::ProofRegistryNotSet));

    // Call ProofRegistry via cross-contract call
    let registry = ProofRegistryClient::new(&env, &registry_addr);

    let proof: ProofRecord = registry
        .get_proof(&intent_id)
        .unwrap_or_else(|| panic_with_error!(&env, Error::ProofNotFound));

    // Validate proof fields match intent
    if proof.src_chain_id != chain_id_for(intent.src_chain.clone()) {
        panic_with_error!(&env, Error::ProofChainMismatch);
    }
    if proof.src_amount < intent.src_amount {
        panic_with_error!(&env, Error::ProofAmountInsufficient);
    }
    // src_user vs intent.user: requires a mapping from Stellar Address
    // to source-chain address, stored in the intent (see §4.4).
}

// ... existing transfer logic continues unchanged ...
```

### 4.4 Source-chain user address in `IntentRecord`

To validate `proof.src_user == intent's source-chain address`, `IntentRecord`
gains an optional field:

```rust
pub src_user: Option<String>,  // user's address on src_chain, if provided
```

`submit_intent` gains an optional `src_user: Option<String>` parameter. When
provided, `fill_intent` validates the proof's `src_user` matches. When absent,
user validation is skipped (backward compatible, relies on `intent_id`
uniqueness for binding).

### 4.5 New error codes

```rust
ProofRegistryNotSet    = 24,
ProofNotFound          = 25,
ProofChainMismatch     = 26,
ProofAmountInsufficient = 27,
```

---

## 5. Source-Chain: `VortexDeposit` Contract (Sketch)

One contract per supported EVM chain. Not a Soroban contract.

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

interface IWormhole {
    function publishMessage(uint32 nonce, bytes calldata payload, uint8 consistencyLevel)
        external payable returns (uint64 sequence);
}

contract VortexDeposit {
    IWormhole public immutable wormhole;
    // consistencyLevel 1 = finalized on most EVM chains
    uint8 constant CONSISTENCY_FINALIZED = 1;

    event Deposited(bytes32 indexed intentId, address token, uint256 amount, uint64 wormholeSeq);

    constructor(address _wormhole) {
        wormhole = IWormhole(_wormhole);
    }

    /// User calls this after signing a Vortex intent on Stellar.
    /// intentId must match the BytesN<32> generated by intent_settlement.submit_intent.
    function deposit(
        bytes32 intentId,
        address token,
        uint256 amount
    ) external payable {
        IERC20(token).transferFrom(msg.sender, address(this), amount);

        bytes memory payload = abi.encodePacked(
            intentId,                    // 32 bytes
            bytes20(msg.sender),         // 20 bytes
            uint16(block.chainid),       // 2 bytes  (Wormhole chain ID mapping needed)
            bytes32(uint256(uint160(token))),  // 32 bytes
            int128(int256(amount))       // 16 bytes
        );

        uint64 seq = wormhole.publishMessage{value: msg.value}(0, payload, CONSISTENCY_FINALIZED);

        emit Deposited(intentId, token, amount, seq);
    }
}
```

Note: `block.chainid` is the EVM chain ID, which must be mapped to the
Wormhole chain ID on the Stellar side (Wormhole uses its own chain ID
namespace: Ethereum = 2, Base = 30, Polygon = 5, etc.).

---

## 6. Rollout Plan

### Phase 1 — Proof infrastructure, opt-in (no breaking change)

1. Deploy `ProofRegistry` on Stellar testnet.
2. Deploy `VortexDeposit` on target source chains (testnet).
3. Upgrade `intent_settlement` with `set_proof_registry` and the updated
   `fill_intent(…, require_proof: bool)` signature.
4. All existing fills use `require_proof = false`. New fills may opt in.

### Phase 2 — Proof required above threshold (configurable)

1. Admin sets a `min_proof_amount: i128` threshold.
2. `fill_intent` automatically sets `require_proof = true` when
   `fill_amount >= min_proof_amount`.
3. Existing small-intent solvers are unaffected; large-intent fills require proof.

### Phase 3 — Proof required for all fills

1. Admin removes the threshold; all fills require proof.
2. Bond can be reduced (since cryptographic proof supersedes pure economic trust).
3. `require_proof` parameter is deprecated/removed in a subsequent upgrade.

---

## 7. Impact on Trust Model

### Before proof verification

```
Trust basis: economic (solver bond + slash)
User protection: solver risks 10% of bond per failed fill
Weakness: solver with large bond can absorb slashes as a griefing cost
```

### After proof verification (Phase 3)

```
Trust basis: cryptographic (Guardian quorum) + economic (bond)
User protection:
  - fill_intent cannot succeed without a verified source-chain deposit
  - solver cannot fake a fill for an intent where no deposit occurred
  - bond still exists as a backstop for fill-window violations
Residual risk: Guardian collusion (19 signers) — same risk as all Wormhole users
```

The bond + slash mechanism is **not removed** — it still protects against
a solver accepting an intent (blocking other solvers) and failing to fill
within the 5-minute window. The proof requirement adds a second layer:
the solver cannot even attempt `fill_intent` unless the source deposit was
confirmed.

---

## 8. Open Questions and Deferred Decisions

| Question | Deferred to |
|----------|-------------|
| Who runs the VAA relay bot (solver, Vortex, or permissionless)? | Implementation |
| Chain ID namespace mapping (EVM chain ID → Wormhole chain ID) | Resolved — issue #253, `IntentSettlement::src_chain_to_wormhole_id` |
| Grace period if proof arrives after fill window but fill was honest | v2 dispute resolution |
| `ProofRegistry` upgrade authority (same Admin or separate?) | Implementation |
| Handling non-EVM source chains (Solana, Cosmos) | Future spike |
| Proof expiry (how long is a proof valid after receipt?) | Resolved — issue #254, `PROOF_VALIDITY_WINDOW` |

---

## 9. Files to Create / Modify

| File | Action |
|------|--------|
| `proof_registry/src/lib.rs` | New contract |
| `proof_registry/Cargo.toml` | New crate |
| `intent_settlement/src/lib.rs` | Add `DataKey::ProofRegistry`, update `fill_intent`, `submit_intent`, new errors |
| `intent_settlement/src/test.rs` | Add proof-gated fill tests |
| `src_chain/VortexDeposit.sol` | New (EVM, outside this repo) |
| `docs/124-proof-verification-interface.md` | This file |

---

*Closes #124*
