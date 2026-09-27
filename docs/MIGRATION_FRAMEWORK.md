# Vortex Storage Migration Framework (Issue #353)

## Overview

Soroban cannot iterate persistent storage, making eager bulk migrations impossible. This framework enables:
1. **Eager migrations** for instance data (ledger timestamp, contract state)
2. **Lazy migrations** for persistent records via schema versioning
3. **Idempotent, ordered** migration steps that are safe to retry

## Architecture

### Migration Versions

All contract code must define a `CURRENT_SCHEMA_VERSION` constant (e.g., `1`). Each version bump represents a migration step that:
- Is idempotent (safe to run multiple times)
- Runs in strict order (v1 → v2 → v3)
- Is tagged with a description and expected ledger time

```rust
const CURRENT_SCHEMA_VERSION: u32 = 1;  // Increment for each migration

// Future example:
// const CURRENT_SCHEMA_VERSION: u32 = 2;  // After adding solver_tier to IntentRecord
```

### Admin-Gated Migrate Entrypoint

```rust
pub fn migrate(env: Env, caller: Address) -> Result<(), MigrationError> {
  require_admin(&env, caller)?;
  
  let current: u32 = env.storage().instance().get(&DataKey::SchemaVersion).unwrap_or(0);
  let target = CURRENT_SCHEMA_VERSION;
  
  // Run migrations in order
  for version in (current + 1)..=target {
    match version {
      1 => migrate_to_v1(&env)?,
      2 => migrate_to_v2(&env)?,  // Future
      _ => return Err(MigrationError::UnknownVersion),
    }
  }
  
  env.storage().instance().set(&DataKey::SchemaVersion, &target);
  env.events().publish((Symbol::new(&env, "migrated"),), (current, target));
  Ok(())
}
```

### Lazy Persistent Record Migration

Each persistent record type (IntentRecord, SolverRecord) carries a schema version:

```rust
#[contracttype]
#[derive(Clone)]
pub struct IntentRecord {
  pub schema_version: u32,  // Track record version
  pub id: u64,
  pub user: Address,
  // ... other fields ...
}
```

On load, upcast from old schema to current:

```rust
fn load_intent(env: &Env, id: u64) -> Result<IntentRecord, IntentError> {
  let stored: IntentRecord = env
    .storage()
    .persistent()
    .get(&DataKey::Intent(id))
    .ok_or(IntentError::NotFound)?;
  
  // Upcast if needed
  let current = match stored.schema_version {
    0 => migrate_intent_v0_to_v1(stored)?,  // Add new field with default
    1 => stored,  // Already current
    _ => return Err(IntentError::UnknownSchemaVersion),
  };
  
  // Re-save if we upcasted (ensures future loads are fast)
  if current.schema_version > stored.schema_version {
    env.storage().persistent().set(&DataKey::Intent(id), &current);
  }
  
  Ok(current)
}
```

### Example Migration: Adding a Field to SolverRecord

**Before** (schema v1):
```rust
#[contracttype]
pub struct SolverRecord {
  pub address: Address,
  pub bond_amount: i128,
  // ...
}
```

**After** (schema v2):
```rust
#[contracttype]
pub struct SolverRecord {
  pub schema_version: u32,
  pub address: Address,
  pub bond_amount: i128,
  pub tier: u32,  // NEW FIELD
  // ...
}
```

**Migration function**:
```rust
fn migrate_to_v2_solver_records(env: &Env) -> Result<(), MigrationError> {
  // Note: Can't iterate persistent storage, so this is a no-op.
  // Individual records will upcast lazily when loaded.
  env.storage().instance().set(&DataKey::SchemaVersion, &2);
  Ok(())
}
```

## Safety Guarantees

- **Idempotent**: Running `migrate()` twice is safe (checks current version, skips completed steps)
- **Ordered**: Migrations run in increasing version order
- **Lossy-safe**: Old record fields persist; new fields default gracefully
- **Deterministic**: Same migration produces same result every run

## Per-Crate Makefile Integration

The workspace `Makefile` ensures all contracts build together:

```makefile
CONTRACTS := intent_settlement solver_registry proof_registry reputation_badge

.PHONY: build
build: vortex-common
	@for contract in $(CONTRACTS); do \
		$(MAKE) -C $$contract build; \
	done

.PHONY: test
test: vortex-common
	@for contract in $(CONTRACTS); do \
		$(MAKE) -C $$contract test; \
	done
```

Each crate's `Makefile` invokes `cargo build --release` with workspace dependencies.

## Future Work

- Versioned enum wrappers for zero-copy upcast in some scenarios
- Automated schema generation from #[contracttype] macros
- Proof registry integration for cross-contract migration ordering
