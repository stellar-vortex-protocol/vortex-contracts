# Issue #351: Module Refactoring Plan for intent_settlement

## Overview

Split `intent_settlement/src/lib.rs` (4989 lines) into 13 cohesive modules while maintaining **byte-for-byte identical ABI**.

## Module Structure

```
intent_settlement/src/
├── lib.rs                          # Entry point: types, #[contractimpl], constants
├── modules/
│   ├── mod.rs                      # Module declarations
│   ├── types.rs                    # DataKey, structs, enums
│   ├── errors.rs                   # Error enum, definitions
│   ├── events.rs                   # Event publishing utilities
│   ├── storage.rs                  # Storage helpers (bump_ttl, helpers)
│   ├── admin.rs                    # Admin transfer, fee recipient, pauser
│   ├── config.rs                   # Config management (set_config, get_config)
│   ├── allowlist.rs                # Dst/Src token allowlists, management
│   ├── bonds.rs                    # Bond registration, withdrawal, tracking
│   ├── intents.rs                  # submit_intent, accept_intent, fill_intent, cancel_intent
│   ├── bids.rs                     # Bid-window mode (settle_bids, etc)
│   ├── disputes.rs                 # begin_fill, open_dispute, resolve_dispute
│   ├── backstop.rs                 # backstop_fill, slash_solver functions
│   └── views.rs                    # Query functions (pool_stats, get_solver, etc)
```

## Module Breakdown

### types.rs (lines 229–587)
**Content**: DataKey enum, Policy/Intent structs, PoolStats, OracleData, etc.
**Exports**: All #[contracttype] definitions

### errors.rs (lines 588–760)
**Content**: PoolError enum, error definitions and descriptions
**Exports**: PoolError enum

### events.rs (new module)
**Content**: Helper functions for emitting events (INTENT_SUBMITTED, FILLED, CLAIMED, etc.)
**Exports**: Event emission wrappers

### storage.rs (new module)
**Content**: TTL management (bump_instance_ttl, bump_intent_ttl, bump_solver_ttl)
**Exports**: TTL helpers

### admin.rs (lines ~1300–1400)
**Functions**:
- propose_admin_transfer
- accept_admin_transfer
- propose_fee_recipient
- accept_fee_recipient
- cancel_pending_fee_recipient
- propose_upgrade
- execute_upgrade
- get_pending_upgrade
- set_pauser
- get_pauser
- rescue_tokens

**Exports**: All admin-gated operations

### config.rs (lines ~500–700)
**Functions**:
- get_config
- set_config
- set_dst_allowlist_enabled
- is_dst_allowlist_enabled
- set_src_chain_allowlist_enabled
- is_src_chain_allowlist_enabled
- set_min_bond_multiplier
- get_min_bond_multiplier

**Exports**: Config queries and setters

### allowlist.rs (new module)
**Functions**:
- propose_add_dst_token
- execute_add_dst_token
- cancel_pending_dst_token_add
- propose_remove_dst_token
- execute_remove_dst_token
- cancel_pending_dst_token_remove
- is_dst_token_allowed
- list_allowed_dst_tokens
- add_allowed_src_chain
- remove_allowed_src_chain
- is_src_chain_allowed

**Exports**: Allowlist management

### bonds.rs (lines ~1400–1700)
**Functions**:
- register_solver
- register_solver_with_token
- deregister_solver
- withdraw_bond
- withdraw_bond_token
- get_solver
- get_solver_by_index
- get_solver_count

**Exports**: Bond and solver registration

### intents.rs (lines ~1700–2500)
**Functions**:
- submit_intent
- accept_intent
- accept_intent_with_bond
- fill_intent
- cancel_intent
- expire_intent
- get_intent
- get_holder_active_policy_ids
- get_holder_policy_ids

**Exports**: Core intent lifecycle

### bids.rs (new module)
**Functions**:
- is_bid_window_enabled
- set_bid_window_enabled
- bid_intent (if applicable)
- settle_bids

**Exports**: Bid-window mode operations

### disputes.rs (new module)
**Functions**:
- begin_fill
- open_dispute
- resolve_dispute
- process_backstop

**Exports**: Dispute and escrow operations

### backstop.rs (new module)
**Functions**:
- backstop_fill
- slash_solver
- get_claim_rejection_window

**Exports**: Slashing and backstop fills

### views.rs (lines ~2700–4989)
**Functions**:
- pool_stats
- get_protocol_params
- get_reputation_score
- is_solver_eligible
- get_intent_state
- pool_utilization
- available_capacity

**Exports**: All read-only views

## ABI Preservation Strategy

### Single #[contractimpl] Block
Keep all exported `pub fn` in main `lib.rs` wrapped in a single `#[contractimpl]` that delegates to module implementations:

```rust
#[contractimpl]
impl VortexIntentSettlement {
    // Admin functions delegate to admin module
    pub fn set_config(env: Env, caller: Address, config: ProtocolConfig) -> Result<(), PoolError> {
        admin::set_config(env, caller, config)
    }
    
    // Intent functions delegate to intents module
    pub fn submit_intent(env: Env, user: Address, ...) -> Result<BytesN<32>, PoolError> {
        intents::submit_intent(env, user, ...)
    }
    
    // ... all other exports ...
}
```

### Shared Helpers
Internal helpers (require_admin, require_not_paused, etc.) live in storage.rs or utility functions inline in the implementing modules.

## Implementation Checklist

- [ ] Create module directory structure (modules/*)
- [ ] Move types.rs, errors.rs, events.rs to modules/
- [ ] Extract admin functions → admin.rs
- [ ] Extract config functions → config.rs
- [ ] Extract allowlist functions → allowlist.rs
- [ ] Extract bond functions → bonds.rs
- [ ] Extract intent functions → intents.rs
- [ ] Extract bid functions → bids.rs
- [ ] Extract dispute functions → disputes.rs
- [ ] Extract backstop functions → backstop.rs
- [ ] Extract view functions → views.rs
- [ ] Extract storage helpers → storage.rs
- [ ] Update lib.rs with single #[contractimpl] delegating to modules
- [ ] Verify ABI: `stellar contract inspect` produces identical spec
- [ ] Verify wasm size within ±1% of original
- [ ] Update Makefile (if any per-module builds needed)

## Testing & Verification

After refactoring:

```bash
# Generate ABI before refactor
stellar contract inspect path/to/original.wasm > before.json

# Generate ABI after refactor
cargo build --release
stellar contract inspect target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm > after.json

# Verify identical
diff before.json after.json  # Should be empty

# Check wasm size
ls -lh target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm
# Should be within ±1% of original
```

## Benefits

1. **Readability**: Each module is <500 lines, focused on one concern
2. **Maintainability**: Reduced merge conflicts; local changes don't affect other modules
3. **Testing**: Unit tests can import single modules without full contract
4. **Onboarding**: New contributors can understand one module at a time
5. **ABI Stable**: Single #[contractimpl] ensures spec doesn't drift

## Out of Scope

- Behavior changes (migrations, new features)
- Consolidating admin patterns (two-step admin is kept as-is)
- Optimizing function implementations

## Notes

- vortex-common (issue #352) is already providing shared TTL, tier, and math helpers
- Workspace (issue #352) allows easy cross-crate imports if future refactoring needs them
- Lazy migration framework (issue #353) ensures schema_version is tracked separately from this refactor
