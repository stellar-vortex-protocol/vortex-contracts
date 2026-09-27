# Integration Test Scenarios: Mock Source-Chain Event Feed

**Issue:** [#131](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/131)  
**Branch:** `test/mock-source-chain-scenarios`  
**Depends on:**
- [#124](./124-proof-verification-interface.md) — ProofRegistry interface and VAA relay
- [#129](./129-proof-mismatch-fallback.md) — proof-mismatch fallback behavior

**Status:** Scenarios defined — implement once #124 lands

---

## 1. Overview

Once `ProofRegistry` exists, `fill_intent` can gate fills on a verified
source-chain event. These integration tests simulate the full relay pipeline
using a mock oracle in place of the live Wormhole Guardian network.

The mock setup replaces the real `ProofRegistry` with a test-controlled version
that injects `ProofRecord` entries on demand, letting tests simulate any
source-chain event sequence without a live cross-chain network.

---

## 2. Test Environment Setup

### 2.1 Contracts under test

```
┌───────────────────────┐        ┌──────────────────────────┐
│  intent_settlement    │──────► │  MockProofRegistry       │
│  (real contract)      │        │  (test double)           │
└───────────────────────┘        └──────────────────────────┘
```

The `MockProofRegistry` contract exposes one extra admin-only function not
present in the real registry:

```rust
/// Inject a proof directly — simulates a relayed VAA arriving on-chain.
fn inject_proof(env: Env, record: ProofRecord);

/// Remove a proof — simulates a proof never arriving.
fn remove_proof(env: Env, intent_id: BytesN<32>);
```

All other trait methods (`get_proof`, `has_proof`) behave identically to the
real registry.

### 2.2 Shared test fixture

```rust
struct ProofTestEnv {
    env: Env,
    contract_id: Address,
    mock_registry_id: Address,
    admin: Address,
    solver: Address,
    user: Address,
    bond_token: Address,
    dst_token: Address,
    // A submitted, solver-accepted intent ready for fill
    intent_id: BytesN<32>,
}

impl ProofTestEnv {
    fn new() -> Self { /* deploy both contracts, register solver, submit + accept intent */ }

    /// Convenience: inject a proof that exactly matches the accepted intent.
    fn inject_matching_proof(&self) { ... }

    /// Convenience: inject a proof with a different src_amount.
    fn inject_underfunded_proof(&self, amount: i128) { ... }

    /// Convenience: inject a proof with a wrong chain ID.
    fn inject_wrong_chain_proof(&self, chain_id: u16) { ... }
}
```

---

## 3. Scenario Catalogue

### Scenario A — Successful Transfer (Happy Path)

**Description:** The source-chain deposit matches the intent exactly. The VAA
arrives before the fill window, the solver relays it, and calls `fill_intent`
with `require_proof = true`.

**Event sequence:**
1. User submits intent (ETH → USDC, `src_amount = 1e18`, `src_chain = "ethereum"`).
2. Solver accepts intent.
3. Source-chain deposit confirmed; VAA generated with matching fields.
4. VAA relayed → `ProofRegistry.receive_message()` stores `ProofRecord`.
5. Solver calls `fill_intent(solver, intent_id, fill_amount=35_000_000_000, require_proof=true)`.

**Expected outcome:**
- `fill_intent` returns successfully.
- Intent state transitions to `Filled`.
- `dst_token` balance of `user` increases by `fill_amount - fee`.
- `ProofRegistry.has_proof(intent_id)` returns `true`.

**Test assertion checklist:**
```rust
assert_eq!(intent.state, IntentState::Filled);
assert_eq!(dst_token_balance(user), initial_balance + fill_amount - fee);
assert!(mock_registry.has_proof(&intent_id));
```

---

### Scenario B — Delayed Transfer (Proof Arrives Late but Within Window)

**Description:** The source-chain confirmation is slow (e.g., Ethereum in
high-gas conditions). The proof arrives close to the fill window deadline but
still in time.

**Event sequence:**
1. Intent submitted and accepted. Fill window = 300 s.
2. Time advanced to T+250 s (50 s before deadline).
3. Proof injected at T+250 s.
4. Solver calls `fill_intent` at T+270 s (30 s remaining).

**Expected outcome:**
- Fill succeeds. Intent state = `Filled`.
- No slash is possible (fill completed before deadline).

**Test assertion checklist:**
```rust
assert_eq!(intent.state, IntentState::Filled);
// slash_solver should now panic with FillWindowNotExpired
assert_panics_with!(
    client.slash_solver(&intent_id),
    Error::IntentNotAccepted
);
```

---

### Scenario C — No Transfer (Proof Never Arrives)

**Description:** The source-chain deposit never happened, or the VAA was never
relayed. The solver optimistically calls `fill_intent` with `require_proof = true`
but the registry has no record.

**Event sequence:**
1. Intent submitted and accepted.
2. Solver calls `fill_intent(…, require_proof=true)` immediately — no proof injected.

**Expected outcome:**
- `fill_intent` panics with `Error::ProofNotFound`.
- Intent remains in `Accepted` state.
- Solver does not receive any tokens.
- User does not receive any tokens (transfer not executed).

**Test assertion checklist:**
```rust
assert_panics_with!(
    client.fill_intent(&solver, &intent_id, fill_amount, true),
    Error::ProofNotFound
);
assert_eq!(intent.state, IntentState::Accepted);
```

---

### Scenario D — No Transfer, Fill Window Expires → Slash

**Description:** Extends Scenario C. After the proof-gated fill fails, the
fill window elapses and `slash_solver` is called by a third party.

**Event sequence:**
1. Intent submitted and accepted.
2. Solver attempts `fill_intent` with `require_proof=true` — fails with `ProofNotFound`.
3. Time advanced past the fill window deadline.
4. Third-party calls `slash_solver(intent_id)`.

**Expected outcome:**
- `slash_solver` succeeds.
- Solver's bond decremented by 10%.
- Intent re-opened (`IntentState::Open`) with a fresh deadline.
- `slash_event` emitted.

**Test assertion checklist:**
```rust
let bond_before = solver_record.bond_amount;
client.slash_solver(&intent_id);
let bond_after = solver_record.bond_amount;
assert_eq!(bond_before - bond_after, bond_before / 10);
assert_eq!(intent.state, IntentState::Open);
```

---

### Scenario E — Amount Mismatch (Source Deposit Underfunded)

**Description:** The source-chain deposit was smaller than `intent.src_amount`
(e.g., user transferred 0.9 ETH for a 1 ETH intent). The proof records the
actual deposited amount.

**Event sequence:**
1. Intent: `src_amount = 1_000_000_000_000_000_000` (1 ETH).
2. Source deposit: `0.9 ETH = 900_000_000_000_000_000`.
3. VAA relayed with `src_amount = 900_000_000_000_000_000`.
4. Solver calls `fill_intent(…, require_proof=true)`.

**Expected outcome:**
- `fill_intent` panics with `Error::ProofAmountInsufficient`.
- Intent remains `Accepted`.

**Test assertion checklist:**
```rust
mock_registry.inject_proof(&env, ProofRecord {
    intent_id,
    src_amount: 900_000_000_000_000_000,  // underfunded
    src_chain_id: 2,  // Ethereum
    ..
});
assert_panics_with!(
    client.fill_intent(&solver, &intent_id, fill_amount, true),
    Error::ProofAmountInsufficient
);
assert_eq!(intent.state, IntentState::Accepted);
```

---

### Scenario F — Chain Mismatch (Proof from Wrong Chain)

**Description:** The solver relayed a VAA from the wrong chain — e.g., the
deposit was on Polygon but the intent specified Ethereum. This could be solver
error or an attempted attack reusing a proof from a different deposit.

**Event sequence:**
1. Intent: `src_chain = "ethereum"` (Wormhole ID 2).
2. Proof injected with `src_chain_id = 5` (Polygon).
3. Solver calls `fill_intent(…, require_proof=true)`.

**Expected outcome:**
- `fill_intent` panics with `Error::ProofChainMismatch`.
- Intent remains `Accepted`.

**Test assertion checklist:**
```rust
mock_registry.inject_proof(&env, ProofRecord {
    intent_id,
    src_chain_id: 5,  // Polygon, not Ethereum
    src_amount: 1_000_000_000_000_000_000,
    ..
});
assert_panics_with!(
    client.fill_intent(&solver, &intent_id, fill_amount, true),
    Error::ProofChainMismatch
);
```

---

### Scenario G — Proof Registry Not Configured

**Description:** Admin forgot to call `set_proof_registry()` before deploying
but a solver tries a proof-gated fill anyway.

**Event sequence:**
1. `intent_settlement` deployed without calling `set_proof_registry`.
2. Intent submitted and accepted.
3. Solver calls `fill_intent(…, require_proof=true)`.

**Expected outcome:**
- Panics with `Error::ProofRegistryNotSet`.
- Intent remains `Accepted`.

---

### Scenario H — Replay Protection (Same Proof, Two Intents)

**Description:** A solver attempts to reuse the same VAA to fill two different
intents — e.g., a crafted intent with the same `intent_id` as the first.

**Note:** Replay protection lives in `ProofRegistry` (VAA sequence
deduplication) and in `intent_settlement` (intent uniqueness via `UserNonce`).
This test verifies the end-to-end chain holds.

**Event sequence:**
1. Intent A submitted and filled successfully with a valid proof.
2. Attacker creates a second intent with the same parameters.
3. Attacker calls `fill_intent` for Intent B using the same VAA.

**Expected outcome:**
- `ProofRegistry.receive_message` for the duplicate VAA panics with
  `Error::ProofAlreadyRegistered` (registry-level guard).
- Even if the proof were somehow in the registry for Intent B, `intent_settlement`
  checks `intent_id`-scoped uniqueness, so the second fill would be blocked.

---

### Scenario I — Legacy Fill (require_proof = false)

**Description:** Confirms backward compatibility. An intent filled without proof
gating still works after the proof-registry feature is deployed.

**Event sequence:**
1. Intent submitted and accepted.
2. Solver calls `fill_intent(…, require_proof=false)` — no proof needed.

**Expected outcome:**
- Fill succeeds (economic-trust mode, same as pre-proof behavior).
- `ProofRegistry` not consulted.

**Test assertion checklist:**
```rust
// No proof injected
assert!(!mock_registry.has_proof(&intent_id));
client.fill_intent(&solver, &intent_id, fill_amount, false);
assert_eq!(intent.state, IntentState::Filled);
```

---

## 4. Coverage Matrix

| Scenario | ProofNotFound | ProofAmountInsufficient | ProofChainMismatch | ProofRegistryNotSet | Happy path | Slash path |
|---|---|---|---|---|---|---|
| A — Successful transfer | | | | | ✓ | |
| B — Delayed transfer | | | | | ✓ | |
| C — No proof | ✓ | | | | | |
| D — No proof + slash | ✓ | | | | | ✓ |
| E — Amount mismatch | | ✓ | | | | |
| F — Chain mismatch | | | ✓ | | | |
| G — Registry not set | | | | ✓ | | |
| H — Replay protection | | | | | | |
| I — Legacy fill | | | | | ✓ | |

---

## 5. File Placement

```
intent_settlement/src/
├── lib.rs
├── test.rs          ← add scenarios A–I here (under #[cfg(test)])
├── test_proof.rs    ← optionally split into a dedicated proof-test module
└── proptest_bond.rs
```

Each scenario maps to one or more `#[test]` functions prefixed
`test_proof_scenario_<letter>_<short_description>`, e.g.:
`test_proof_scenario_a_successful_transfer`.

---

## 6. Dependencies and Sequencing

These tests cannot be written until:

1. `ProofRegistry` contract exists (`proof_registry/src/lib.rs`) — blocked on #124.
2. `MockProofRegistry` test double is written (same crate as the tests, or a
   `test-utils` crate).
3. `fill_intent` accepts the `require_proof` parameter — blocked on #124.
4. Error codes 24–27 are defined — blocked on #129.

Once #124 and #129 land, all nine scenarios above can be implemented in a
single PR targeting this branch.

---

*Closes #131*
