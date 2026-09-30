# Batch 33 Implementation Plan: Issues #355-358

## Overview
This batch implements 4 high-priority enhancements to the Vortex Protocol:
- #356: Add explicit recipient field to intents
- #358: Amend intent price/deadline in-place
- #357: Post-fill hooks for recipient contracts
- #355: Reverse-direction intents with escrow

## #356: Recipient Field

### Changes to IntentRecord
- Add `recipient: Option<Address>` field (defaults to user if None)
- Include recipient in intent ID preimage
- Update payout paths to use recipient

### Functions Modified
- `submit_intent`: Add optional `recipient` parameter
- `fill_intent`: Use recipient for output payout
- `begin_fill`: Use recipient for escrow target
- `release_fill`: Pay recipient
- `resolve_dispute`: Pay recipient
- `claim_backstop_compensation`: Pay recipient

### Error Types
- (none new required for recipient field)

---

## #358: Amend Intent

### New Function
```rust
pub fn amend_intent(
    env: Env,
    user: Address,
    intent_id: BytesN<32>,
    new_min_dst_amount: i128,
    new_deadline: u64,
) -> AmendmentResult
```

### Rules
- Only user can amend (auth required)
- Only allowed in Open or Bidding states
- Loosening always allowed: lower min_dst_amount OR later deadline
- Tightening (increase min or earlier deadline) only allowed while Open
- New deadline must stay within max intent lifetime
- Separate rate-limiting from cancellation
- Emits `intent_amended` event with old/new values
- Updates bids if Bidding state

### Storage
- Add `DataKey::AmendmentLastTime(user)` for rate-limiting amendments
- Add `AMENDMENT_COOLDOWN` constant (separate from CANCEL_COOLDOWN)

### Error Types
- `IntentNotAmendable` = 36
- `AmendmentTooTightening` = 37
- `InvalidAmendmentDeadline` = 38
- `AmendmentCooldownActive` = 39

---

## #357: Post-Fill Hooks

### New Types
```rust
#[contracttype]
#[derive(Clone)]
pub struct Hook {
    pub contract: Address,       // recipient contract to invoke
    pub symbol: Symbol,          // function to call
    pub data: Bytes,             // encoded arguments
    pub fail_policy: HookFailPolicy,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq)]
pub enum HookFailPolicy {
    Revert,                      // failing hook reverts the fill
    Isolate,                     // failing hook emits event but doesn't revert
}
```

### Changes to IntentRecord
- Add `hook: Option<Hook>` field

### Changes to submit_intent
- Add optional `hook` parameter
- Validate hook gas budget

### Changes to fill_intent
- After `release_fill` or full fill, invoke hook if present
- Handle hook failures per fail_policy
- Emit `hook_executed` or `hook_failed` event

### Error Types
- `HookInvocationFailed` = 40
- `HookBudgetExceeded` = 41

---

## #355: Reverse-Direction Intents (Outbound)

### New Types
```rust
#[contracttype]
#[derive(Clone)]
pub struct OutboundIntentRecord {
    pub outbound_intent_id: BytesN<32>,
    pub user: Address,
    pub token: Address,           // Stellar token
    pub amount: i128,
    pub dst_chain: String,
    pub dst_token: String,
    pub dst_recipient: String,
    pub min_dst_amount: i128,
    
    pub solver: Option<Address>,
    pub state: OutboundIntentState,
    
    pub created_at: u64,
    pub deadline: u64,
    pub proof_delivered_at: Option<u64>,
    
    pub bond_token: Address,
    pub escrow_released_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum OutboundIntentState {
    EscrowLocked,        // user locked tokens, awaiting solver
    SolverAccepted,      // solver accepted, in flight
    ProofVerified,       // destination proof verified
    EscrowReleased,      // escrow paid to solver
    UserRefunded,        // deadline passed, refunded to user
    SolverSlashed,       // solver failed to deliver, slashed
}
```

### New Functions
```rust
pub fn submit_outbound_intent(
    env: Env,
    user: Address,
    token: Address,
    amount: i128,
    dst_chain: String,
    dst_token: String,
    dst_recipient: String,
    min_dst_amount: i128,
    deadline: Option<u64>,
) -> BytesN<32>

pub fn accept_outbound_intent(
    env: Env,
    solver: Address,
    outbound_intent_id: BytesN<32>,
    bond_amount: i128,
    bond_token: Address,
)

pub fn claim_outbound_intent(
    env: Env,
    solver: Address,
    outbound_intent_id: BytesN<32>,
    proof: Bytes,  // versioned proof from proof_registry
) -> i128        // payout amount
```

### Storage
- `DataKey::OutboundIntent(intent_id)` for escrow-locked intents
- `DataKey::OutboundIntentsByUser(user)` for pagination
- `DataKey::OutboundIntentsByState(state)` for solver discovery

### Error Types
- `OutboundIntentNotFound` = 42
- `InsufficientEscrow` = 43
- `ProofMismatch` = 44
- `RecipientMismatch` = 45

---

## Implementation Order

1. **Phase 1**: Extend Error enum + storage DataKey variants
2. **Phase 2**: Extend IntentRecord with recipient + hook fields
3. **Phase 3**: Implement amend_intent (#358)
4. **Phase 4**: Implement Hook infrastructure + hook invocation in fill_intent (#357)
5. **Phase 5**: Create OutboundIntentRecord + implement reverse functions (#355)
6. **Phase 6**: Update submit_intent signature to include recipient + hook
7. **Phase 7**: Update all payout paths to use recipient
8. **Phase 8**: Add comprehensive tests
9. **Phase 9**: Update events with new fields

---

## Event Changes

New events to emit:
- `intent_amended { intent_id, user, old_min_dst_amount, new_min_dst_amount, old_deadline, new_deadline }`
- `hook_executed { intent_id, hook_contract, success }`
- `hook_failed { intent_id, hook_contract, reason }`
- `outbound_intent_submitted { outbound_intent_id, user, dst_chain }`
- `outbound_intent_accepted { outbound_intent_id, solver }`
- `outbound_intent_claimed { outbound_intent_id, solver, proof_chain }`

---

## Backward Compatibility

- recipient defaults to user (None) for old intents
- hook defaults to None (no invocation if missing)
- outbound intents are separate storage tree, don't affect existing flow
- amend_intent is new public function, doesn't change existing flow
- All existing tests should pass unchanged

---

## Testing Strategy

1. Unit tests for amend_intent state transitions and validation
2. Unit tests for hook invocation and failure policies
3. Integration tests for outbound intent lifecycle
4. Fuzz tests for amount/deadline/recipient combinations
5. Regression tests to ensure existing flows still work
