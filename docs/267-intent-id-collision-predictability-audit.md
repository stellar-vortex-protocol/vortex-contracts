# Formal Security Audit: `compute_intent_id` Preimage & Collision/Predictability Resistance

**Issues Addressed:** #267  
**Status:** Formal Audit Completed & Property Tests Added  

---

## 1. Executive Summary

`compute_intent_id` calculates a 32-byte cryptographic identifier for each submitted intent by hashing a preimage using SHA-256. 

> **Updated (#685).** This audit was written against the original
> timestamp-based preimage `(user, src_chain, src_amount, timestamp, nonce)`.
> The live scheme is the client-computable v1 id specified in
> [CLIENT_COMPUTABLE_INTENT_IDS.md](CLIENT_COMPUTABLE_INTENT_IDS.md):
> `sha256("vortex-intent-v1" || xdr(contract_id) || network_id || xdr(user) || be8(nonce) || sha256(xdr(src_chain) || be16(src_amount) || xdr(dst_token) || be16(min_dst_amount)))`.
> There is no timestamp. The conclusions below hold and are strengthened:
> the preimage still contains `XDR(user)` and the per-user nonce, and now also
> the contract id, network id, `dst_token` and `min_dst_amount`.

This audit formally evaluates the collision resistance and predictability of this scheme, specifically analyzing whether an attacker could manipulate or predict components of the preimage to deliberately produce a hash collision and trigger a griefing attack via `DataKey::Intent(intent_id)` rejection.

---

## 2. Preimage Composition & Entropy Analysis

The preimage is serialized as:
$$\text{Preimage} = \text{"vortex-intent-v1"} \parallel \text{XDR}(contract) \parallel \text{network\_id} \parallel \text{XDR}(user) \parallel \text{be8}(nonce) \parallel \text{params\_hash}$$

where $\text{params\_hash} = \text{SHA256}(\text{XDR}(src\_chain) \parallel \text{be16}(src\_amount) \parallel \text{XDR}(dst\_token) \parallel \text{be16}(min\_dst\_amount))$.

### 1. Collision Resistance
- SHA-256 provides a 256-bit hash space with a theoretical birthday attack bound of $2^{128}$ operations.
- Accidental hash collisions across randomly or linearly generated preimages are practically impossible ($p < 10^{-30}$ for any realistic network throughput).

### 2. Same-Ledger Same-User Disambiguation (Nonce Guarantee)
- When a user submits multiple identical intents (same `src_chain`, same amounts and token) within the exact same ledger block, the per-user `nonce` (incremented sequentially per submission) guarantees that every preimage remains distinct.

---

## 3. Threat Model & Griefing Analysis

### Attack Scenario: Deliberate Collision Targeting
- **Griefer Goal:** Precompute an intent ID matching a victim's pending submission to occupy `DataKey::Intent(intent_id)` and cause the victim's submission to panic with `IntentAlreadyExists`.
- **Preimage Dependency on Victim Address:**
  The preimage explicitly includes `XDR(user)`. To generate a colliding `intent_id`, the griefer would have to compute a hash matching `(contract, network, victim_address, nonce, params)`.
- **Authorization Requirement:**
  If the griefer submits the intent using their own account address `griefer_address`, the hash becomes `SHA-256(griefer_address, ...)` which will **never** equal `SHA-256(victim_address, ...)`.
- **Conclusion:** A griefer cannot forge a collision using their own identity, nor can they submit an intent under the victim's address without valid signature/auth. Therefore, collision-based griefing targeting victim submissions is computationally infeasible.

---

## 4. Verification & Testing

- **Property-Based Fuzzing:** Extended property test coverage in `proptest_bond.rs` / `test.rs` generating randomized preimages asserting zero collisions.
- **Targeted Nonce Test:** Unit test `test_compute_intent_id_nonce_uniqueness` verifying that submissions within the same ledger timestamp with identical params yield unique intent IDs due to nonce incrementation.
