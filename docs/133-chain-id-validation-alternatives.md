# Chain ID Validation: String Enum vs Numeric IDs

## Overview

This document compares two design approaches for validating the `src_chain` parameter
in `submit_intent()`:

1. **String Enum Approach** (current implementation)  
   Validator accepts free-text chain names ("ethereum", "base", "solana") against an
   allowlist of strings.

2. **Numeric Chain ID Approach** (this proposal)  
   Validator accepts numeric chain identifiers (EVM chain IDs, Wormhole chain IDs) for
   less ambiguity in programmatic matching.

## Motivation

**Solver bot integration friction:**

- String names are ambiguous: is "ethereum" the network, mainnet only, or testnet?
- Solver bots must parse and validate strings, prone to typos and inconsistencies.
- No standard way to map string names across different bridges/protocols.

**Numeric IDs advantages:**

- Solver bots work directly with EVM chain IDs (already used by providers like Infura).
- Wormhole uses standardized numeric chain IDs across 50+ networks.
- Numeric validation is deterministic: no ambiguity about "which Ethereum?"

**Trade-offs to evaluate:**

- Developer ergonomics: is "ethereum" or 1 (Ethereum mainnet) more intuitive?
- Backwards compatibility: can we support both simultaneously?
- Proof of intent: does the cross-chain proof carry a numeric or string chain identifier?

---

## Design Option 1: String Enum (Current)

### Contract Interface

```rust
pub fn submit_intent(
    env: Env,
    user: Address,
    src_chain: String,        // "ethereum", "base", "solana", ...
    src_token: String,
    src_amount: i128,
    dst_token: Address,
    min_dst_amount: i128,
) -> BytesN<32> { ... }

pub fn add_allowed_src_chain(env: Env, chain: String) { ... }

pub fn is_src_chain_allowed(env: Env, chain: String) -> bool { ... }
```

### Solver Bot Usage

```python
# Solver discovers an intent via event or RPC
intent = {"src_chain": "ethereum", ...}

# Validate the chain name matches local config
if intent["src_chain"] not in ["ethereum", "base", "polygon"]:
    log.warn(f"Unknown chain: {intent['src_chain']}")
    return

# Proceed with cross-chain fill
tx = uniswap_swap("ethereum", "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")
```

**Issues:**

- Typo risk: "ethereum" vs "Ethereum" vs "ETH" all different
- Ambiguity: does "ethereum" mean mainnet, testnet (Goerli, Sepolia), or any EVM chain
- No standard: bridge A calls it "ethereum", bridge B calls it "eth", bridge C calls it "eth_mainnet"

### Storage

```
storage["allowed_src_chains"] = Set<String>
  "ethereum", "base", "polygon", "arbitrum", ...
```

---

## Design Option 2: Numeric Chain IDs

### Contract Interface

```rust
pub fn submit_intent(
    env: Env,
    user: Address,
    src_chain_id: u32,         // 1 = Ethereum, 8453 = Base, 137 = Polygon, ...
    src_token: String,
    src_amount: i128,
    dst_token: Address,
    min_dst_amount: i128,
) -> BytesN<32> { ... }

pub fn add_allowed_src_chain_id(env: Env, chain_id: u32) { ... }

pub fn is_src_chain_id_allowed(env: Env, chain_id: u32) -> bool { ... }

// Helper: map numeric ID to string name for logging/events
pub fn chain_id_to_name(env: Env, chain_id: u32) -> String {
    match chain_id {
        1 => "ethereum",
        8453 => "base",
        137 => "polygon",
        _ => "unknown",
    }
}
```

### Solver Bot Usage

```python
# Solver discovers an intent via event or RPC
intent = {"src_chain_id": 1, ...}  # Numeric ID

# Validate using standard chain IDs
SUPPORTED_CHAINS = {1, 8453, 137, 42161, 10}  # Ethereum, Base, Polygon, Arbitrum, Optimism

if intent["src_chain_id"] not in SUPPORTED_CHAINS:
    log.warn(f"Unsupported chain ID: {intent['src_chain_id']}")
    return

# Proceed with cross-chain fill — chain ID maps directly to RPC provider config
tx = uniswap_swap(
    chain_id=1,
    rpc_url=RPC_PROVIDERS[1],  # eth-mainnet provider
    token=intent["src_token"]
)
```

**Advantages:**

- No ambiguity: chain ID 1 always means Ethereum mainnet
- Directly usable in solver configs (RPC providers index by chain ID)
- Deterministic: numeric comparison is never wrong
- Standard across protocols: Wormhole, EVM, bridges all use the same IDs

### Storage

```
storage["allowed_src_chain_ids"] = Set<u32>
  1, 8453, 137, 42161, 10, ...  (Ethereum, Base, Polygon, Arbitrum, Optimism)
```

### Mapping Table (Embedded in Contract)

```rust
pub fn chain_id_to_wormhole_id(env: Env, chain_id: u32) -> u16 {
    match chain_id {
        1 => 2,           // Ethereum mainnet
        8453 => 30,       // Base
        137 => 5,         // Polygon
        42161 => 23,      // Arbitrum One
        10 => 24,         // Optimism
        900 => 1,         // Solana (Wormhole chain ID)
        _ => panic!()
    }
}

pub fn wormhole_id_to_chain_id(env: Env, wormhole_id: u16) -> u32 {
    match wormhole_id {
        2 => 1,
        30 => 8453,
        5 => 137,
        23 => 42161,
        24 => 10,
        1 => 900,  // Solana
        _ => panic!()
    }
}
```

---

## Comparison Matrix

| Criterion | String Enum | Numeric Chain ID |
|---|---|---|
| **Unambiguous?** | ❌ "ethereum" is vague | ✅ 1 is always Ethereum mainnet |
| **Solver-friendly?** | ❌ Requires manual parsing | ✅ Matches RPC provider APIs |
| **Typo-safe?** | ❌ "etherum" silently fails | ✅ 1 is unambiguous |
| **Standards-based?** | ⚠️ Custom per protocol | ✅ Wormhole, EVM, bridges agree |
| **Backend friendly?** | ⚠️ Requires string matching | ✅ Works directly with indexers |
| **Event compatibility** | ✅ Matches current events | ⚠️ Requires migration |
| **Extensible?** | ❌ New chains → new strings | ✅ ID space is infinite |

---

## Hybrid Approach (Recommended)

To minimize friction, deploy both systems in parallel during a transition period:

### Phase 1: Launch with Strings (Current)

- `submit_intent(src_chain: String)` with allowlist validation
- Backwards compatible with existing solver bots
- Solvers can voluntarily provide numeric IDs via `set_solver_routes()`

### Phase 2: Add Numeric Alternative (v1.1)

```rust
pub fn submit_intent_with_chain_id(
    env: Env,
    user: Address,
    src_chain_id: u32,        // New parameter
    src_token: String,
    src_amount: i128,
    dst_token: Address,
    min_dst_amount: i128,
) -> BytesN<32> { ... }
```

- Both endpoints coexist
- Internally stored as numeric ID
- Events emit both formats for a transition period
- Deprecate string version after 6-month notice

### Phase 3: String Deprecation (v2.0)

- Remove `submit_intent(src_chain: String)`
- Consolidate to single `submit_intent(src_chain_id: u32)` endpoint
- Non-breaking for anyone using Phase 2

---

## Proof Integration Considerations

**If using cross-chain proofs (issue #190):**

The `ProofRegistry` contract may carry either:
- A string chain name from the source-chain proof
- A numeric chain ID (if the proof system uses Wormhole IDs)

**Resolution:**

- Proof → chain ID conversion happens once (in contract or off-chain)
- Store proofs by numeric ID internally for consistency
- Map to string only for event logging/API compatibility

---

## Recommendation

**Adopt the numeric chain ID approach** for new intent submissions:

1. **Ship in v1.1**: Add `submit_intent_with_chain_id()` as opt-in
2. **Market**: Solver bots adopt numeric IDs in v1.1 (easier, matches their RPC config)
3. **Stabilize**: After 6 months, deprecate string version for v2.0 cleanup

This balances:
- ✅ Immediate solver bot ergonomics improvement
- ✅ Backwards compatibility (strings still work via parallel endpoint)
- ✅ Clear migration path (phase timeline)
- ✅ Standards alignment (Wormhole, EVM, bridges)

---

## Implementation Notes

### Reference Implementation: Numeric Chain IDs

See the contract's existing `src_chain_to_wormhole_id()` function in `lib.rs` —
it already contains the string→number mapping. Reversing this to numeric→string
is trivial:

```rust
pub fn is_src_chain_id_allowed(env: Env, chain_id: u32) -> bool {
    let allowed: Set<u32> = env
        .storage()
        .instance()
        .get(&DataKey::AllowedSrcChainIds)
        .unwrap_or_else(Set::new);
    allowed.contains(chain_id)
}

pub fn add_allowed_src_chain_id(env: Env, chain_id: u32) {
    require_admin(&env);
    let mut allowed: Set<u32> = env
        .storage()
        .instance()
        .get(&DataKey::AllowedSrcChainIds)
        .unwrap_or_else(Set::new);
    allowed.insert(chain_id);
    env.storage().instance().set(&DataKey::AllowedSrcChainIds, &allowed);
}
```

### Solver Bot Migration (Python Example)

```python
# OLD (string-based)
submit_intent(..., src_chain="ethereum", ...)

# NEW (numeric)
submit_intent_with_chain_id(..., src_chain_id=1, ...)

# Solver config now uses Wormhole IDs directly:
RPC_ENDPOINTS = {
    1: "https://eth-rpc.example.com",      # Ethereum mainnet
    8453: "https://base-rpc.example.com",  # Base
}

# Fill operation uses the same ID:
execute_fill(chain_id=intent.src_chain_id, rpc=RPC_ENDPOINTS[intent.src_chain_id])
```

---

## References

- **Wormhole Network**: [Chain IDs](https://docs.wormhole.com/wormhole/reference/glossary#chain-id)
- **EVM Chain IDs**: [chainlist.org](https://chainlist.org)
- **Issue #34**: Original `src_chain` allowlist design
- **Issue #133**: This proposal
