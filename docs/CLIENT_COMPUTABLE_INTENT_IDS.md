# Client-Computable, Timestamp-Free Intent IDs (Issue #354)

## Problem

Current intent ID scheme:
```
intent_id = sha256(user || src_chain || src_amount || timestamp || nonce)
```

**Issue**: `timestamp` is the current Soroban ledger time, unknown until tx is included on-chain. This forces the cross-chain flow to:
1. Submit intent to Stellar
2. Wait for finality
3. **Then** generate the intent ID
4. Embed ID in source-chain deposit payload

This adds ~20s latency and requires the user to wait.

## Solution

Define a **client-computable** intent ID that the user can compute before submitting on Stellar:

```
intent_id = sha256(
  DOMAIN_SEP ||
  contract_id ||
  network_id ||
  user ||
  nonce ||
  sha256(intent_params)
)

intent_params = (
  src_chain ||
  src_amount ||
  dst_token ||
  min_dst_amount ||
  trigger_threshold ||
  duration_days
)

DOMAIN_SEP = "vortex-intent-v1"
network_id = sha256(network_passphrase)  // e.g., "Test SDF Network" or "Public Global Stellar Network"
```

## Benefits

1. **Deterministic**: Client computes same ID every time
2. **Offline-safe**: No RPC calls needed before submitting
3. **Cross-network safe**: Network ID in hash prevents replay across Stellar networks
4. **Contract-safe**: Contract ID prevents accidental reuse on new contracts
5. **Nonce-based**: Sequential nonce prevents collisions for same user

## Implementation

### 1. Nonce Tracking

```rust
// Storage: per user nonce (pre-commitment counter)
pub fn get_user_nonce(env: Env, user: Address) -> u64 {
  env.storage()
    .persistent()
    .get(&DataKey::UserNonce(user.clone()))
    .unwrap_or(0)
}

pub fn increment_user_nonce(env: Env, user: Address) {
  let nonce = get_user_nonce(&env, &user);
  env.storage()
    .persistent()
    .set(&DataKey::UserNonce(user), &(nonce + 1));
}
```

### 2. Intent ID Computation

```rust
pub fn compute_intent_id_v1(
  env: &Env,
  user: &Address,
  src_chain: &String,
  src_amount: i128,
  dst_token: &Address,
  min_dst_amount: i128,
  trigger_threshold: i128,
  duration_days: u32,
) -> BytesN<32> {
  const DOMAIN_SEP: &[u8] = b"vortex-intent-v1";
  
  let contract_id = env.current_contract_address().to_xdr();
  let network_id = sha256(env.ledger().network_id());
  
  let intent_params = (
    src_chain.clone(),
    src_amount,
    dst_token.clone(),
    min_dst_amount,
    trigger_threshold,
    duration_days,
  );
  let params_hash = sha256_contract_data(&intent_params);
  
  let nonce = get_user_nonce(env, user);
  
  let preimage = (
    Bytes::from_slice(env, DOMAIN_SEP),
    contract_id,
    network_id,
    user.clone(),
    nonce,
    params_hash,
  );
  
  sha256_contract_data(&preimage)
}
```

### 3. Submit Intent with Optional Client-Supplied ID

```rust
pub fn submit_intent(
  env: Env,
  user: Address,
  src_chain: String,
  src_amount: i128,
  dst_token: Address,
  min_dst_amount: i128,
  trigger_threshold: i128,
  duration_days: u32,
  client_supplied_id: Option<BytesN<32>>,  // NEW
) -> Result<BytesN<32>, SubmitError> {
  user.require_auth();
  
  // Compute the canonical ID
  let canonical_id = compute_intent_id_v1(
    &env,
    &user,
    &src_chain,
    src_amount,
    &dst_token,
    min_dst_amount,
    trigger_threshold,
    duration_days,
  );
  
  // If client supplied an ID, verify it matches
  if let Some(supplied) = client_supplied_id {
    if supplied != canonical_id {
      return Err(SubmitError::InvalidIntentId);
    }
  }
  
  // Create intent with canonical ID
  let intent = IntentRecord {
    id: canonical_id.clone(),
    user: user.clone(),
    // ... other fields ...
  };
  
  increment_user_nonce(&env, &user);
  env.storage()
    .persistent()
    .set(&DataKey::Intent(canonical_id.clone()), &intent);
  
  Ok(canonical_id)
}
```

### 4. Client-Side Computation

JavaScript client can compute the ID without touching RPC:

```typescript
async function computeIntentId(params: {
  contractId: string;
  networkPassphrase: string;
  user: string;
  nonce: number;
  srcChain: string;
  srcAmount: i128;
  dstToken: string;
  minDstAmount: i128;
  triggerThreshold: i128;
  durationDays: number;
}): Promise<BytesN<32>> {
  const DOMAIN_SEP = Buffer.from('vortex-intent-v1');
  const networkId = sha256(params.networkPassphrase);
  
  const intentParams = {
    srcChain: params.srcChain,
    srcAmount: params.srcAmount,
    dstToken: params.dstToken,
    minDstAmount: params.minDstAmount,
    triggerThreshold: params.triggerThreshold,
    durationDays: params.durationDays,
  };
  const paramsHash = sha256(XDR.stringify(intentParams));
  
  const preimage = Buffer.concat([
    DOMAIN_SEP,
    contractId.toBuffer(),  // XDR-encoded
    networkId,
    user.toBuffer(),        // XDR-encoded
    nonce.toBuffer(),
    paramsHash,
  ]);
  
  return sha256(preimage);
}
```

## Backward Compatibility

Old contracts using timestamp-derived IDs keep using them. New contracts/callers can opt into v1:

```rust
// Legacy (keep for now)
fn compute_intent_id_legacy(
  env: &Env,
  user: &Address,
  src_chain: &String,
  src_amount: i128,
  timestamp: u64,
  nonce: u64,
) -> BytesN<32> {
  // ... old logic ...
}

// New
fn compute_intent_id_v1(...) -> BytesN<32> {
  // ... new logic ...
}
```

In `submit_intent`, try the canonical (v1) computation first; if the supplied ID doesn't match, it's an old (legacy) caller, so validate via legacy scheme.

## Cross-Chain Flow

With client-computable IDs:

1. User generates intent params locally
2. User computes `intent_id` client-side (offline)
3. User creates source-chain deposit with `intent_id` in payload
4. User submits intent to Stellar
5. Proof registry relays the source-chain data with `intent_id`
6. Stellar settlement verifies `intent_id` matches proof

**Result**: Either "deposit first, then submit" OR "submit first, then deposit" — both now work!
