# Dispute-Resolution Flow Design

Tracking issue: [#48](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/48)

---

## Problem statement

Once `fill_intent` succeeds the intent moves to state `Filled` — permanently.
There is currently no path for a user to contest a fill that technically met
`min_dst_amount` but that the user believes was manipulated, misdirected, or
otherwise incorrect.

The contract cannot independently verify off-chain or cross-chain facts (the
source-chain transaction is not proven on-chain; Vortex's security model
currently trusts solver bonds rather than cryptographic proofs). Any dispute
mechanism must therefore acknowledge that:

1. The contract **can** verify: token balances received, fill amounts, intent
   state, timestamps, and which solver signed the fill.
2. The contract **cannot** verify: whether the source-chain tx actually
   occurred, whether the quoted rate was fair, or whether the off-chain solver
   behaviour was honest.

---

## Scope: what counts as a valid on-chain dispute?

Given the above constraints, a dispute is valid when **at least one** of the
following is true:

| Category | Example | On-chain verifiable? |
|---|---|---|
| **Underfill** | `fill_amount < min_dst_amount` stored in the intent | ✅ already blocked by `fill_intent` guard — not disputable post-fill |
| **Wrong recipient** | Output tokens sent to an address other than `intent.user` | ✅ verifiable via token events / balance diff at fill time |
| **Duplicate fill** | A second fill after `Filled` state is set | ✅ already blocked — not disputable |
| **Stale-rate claim** | User claims the rate was manipulated off-chain | ❌ not verifiable on-chain — out of scope for v1 |
| **Source-chain non-delivery** | User claims source-chain tx never happened | ❌ not verifiable without a cross-chain oracle — deferred to oracle integration milestone |

**v1 dispute scope:** A dispute window is provided for a user to flag potential
wrong-recipient or off-chain-integrity concerns. Resolution is performed by a
designated arbiter role (initially admin, upgradeable to a multisig or separate
arbitration contract). The on-chain mechanism escrows the fill amount during the
dispute window rather than immediately releasing it, making the arbiter's
decision enforceable.

---

## Proposed state machine addition

```
Open → Accepted → Filling (new) → Filled
                              ↘→ Disputed → Resolved (Upheld | Dismissed)
```

### New states

| State | Description |
|---|---|
| `Filling` | Solver has called `begin_fill`; output tokens are held in escrow by the contract. The user has a dispute window to contest. |
| `Disputed` | User raised a dispute during the window; fill is on hold. |
| `Resolved` | Arbiter closed the dispute. Sub-outcome stored separately. |

### New fields on `IntentRecord`

```rust
pub dispute_deadline: Option<u64>,  // escrow window end; set by begin_fill
pub dispute_raised_at: Option<u64>, // timestamp of open_dispute call
pub resolution: Option<DisputeResolution>, // Upheld | Dismissed
```

```rust
pub enum DisputeResolution {
    Upheld,    // arbiter sided with user; tokens returned to user, solver slashed
    Dismissed, // arbiter sided with solver; tokens released from escrow to user normally
}
```

---

## Fund flow

### Without a dispute (happy path)

```
solver --[fill_amount]--> contract escrow (begin_fill)
  [dispute_window elapses with no dispute]
anyone calls release_fill(intent_id)
contract escrow --[fill_amount]--> user
contract --[fee]--> fee_recipient  (deducted from fill, same as current)
```

### With a dispute: arbiter upholds the user

```
solver --[fill_amount]--> contract escrow (begin_fill)
user calls open_dispute(intent_id)
arbiter calls resolve_dispute(intent_id, Upheld)
  contract escrow --[fill_amount]--> user   (user made whole)
  solver bond slashed 10 % (same as slash_solver)
  intent re-opened for a new solver  OR  intent set to Resolved/Expired
```

### With a dispute: arbiter dismisses (solver wins)

```
solver --[fill_amount]--> contract escrow (begin_fill)
user calls open_dispute(intent_id)
arbiter calls resolve_dispute(intent_id, Dismissed)
  contract escrow --[fill_amount]--> user   (user still receives tokens)
  no slash; intent state = Resolved(Dismissed)
  solver bond unlocked
```

> **Note:** In both outcomes the user receives the tokens. The dispute only
> determines whether the solver is slashed for alleged misconduct.

---

## New entry-points (sketch)

```rust
/// Solver transfers fill_amount into contract escrow; starts dispute window.
/// Replaces the direct transfer in fill_intent once this design is implemented.
pub fn begin_fill(env: Env, solver: Address, intent_id: BytesN<32>, fill_amount: i128);

/// User opens a dispute within the dispute window.
/// Only callable while state == Filling and now < dispute_deadline.
pub fn open_dispute(env: Env, user: Address, intent_id: BytesN<32>);

/// Arbiter resolves a dispute. Triggers fund release and optional slash.
/// Only callable by the designated arbiter address while state == Disputed.
pub fn resolve_dispute(env: Env, arbiter: Address, intent_id: BytesN<32>, resolution: DisputeResolution);

/// Permissionless: release escrow to user after dispute window closes without a dispute.
pub fn release_fill(env: Env, intent_id: BytesN<32>);
```

---

## Arbiter role

**v1:** The `admin` address acts as arbiter. This is the simplest safe option
for testnet — it requires no new storage key or governance mechanism.

**v2 (recommended before mainnet):** A separate `arbiter` storage key, settable
by admin via `set_arbiter(env, new_arbiter: Address)`. The arbiter can be:
- A protocol-controlled multisig (3-of-5)
- A future `solver_registry` contract that runs a reputation-weighted jury

**Out of scope for this design:** fully trustless arbitration (requires a
cross-chain proof oracle).

---

## Dispute window

| Parameter | Proposed value | Rationale |
|---|---|---|
| `DISPUTE_WINDOW` | 3600 s (1 hour) | Long enough for the user to notice and act; short enough not to hold solver capital indefinitely. Adjustable by governance. |
| `ARBITER_WINDOW` | 86400 s (24 hours) | After a dispute is raised, the arbiter has 24 hours to resolve. If unresolved, a permissionless timeout releases escrow to the user (conservative default). |

---

## Security considerations

- **Griefing:** A user could open spurious disputes to delay solver capital
  release. Mitigation: require a small dispute bond from the user (e.g. 1 USDC),
  returned on Upheld, forfeited on Dismissed. Defer to follow-up issue.
- **Arbiter capture:** A colluding arbiter could always dismiss. Mitigation: v2
  multisig arbiter; on-chain event emission for full auditability.
- **Escrow risk:** Tokens sit in the contract during the window. The contract
  must not be upgradeable without governance while escrowing user funds.
- **Re-entrancy:** `begin_fill` uses `transfer_from(solver → contract)`;
  `release_fill` and `resolve_dispute` use `transfer(contract → user)`. Ensure
  state is updated before token transfers (checks-effects-interactions).

---

## Implementation plan (follow-up issues)

1. Add `Filling` / `Disputed` / `Resolved` states and new `IntentRecord` fields.
2. Implement `begin_fill` + escrow logic (replaces direct transfer in `fill_intent`).
3. Implement `open_dispute` with dispute window enforcement.
4. Implement `resolve_dispute` with fund release and optional slash.
5. Implement `release_fill` permissionless timeout.
6. Add dispute bond (anti-griefing) — separate issue.
7. Add `set_arbiter` + v2 multisig arbiter — separate issue.
8. Full test suite for the new dispute lifecycle.

---

*Design status: Implemented in `intent_settlement` (issue #188).*  
*The escrow/dispute flow ships as `begin_fill` → `dispute_fill` /*
*`resolve_dispute` / `release_fill`, with states `Filling` / `Disputed` /*
*`Resolved` and enum `DisputeResolution { Upheld, Dismissed }`. Deviations from*
*this sketch: the permissionless timeout and clean release are unified into a*
*single `release_fill` entrypoint; the arbiter is stored at `DataKey::Arbiter`*
*(defaulting to `Admin`) via `set_arbiter`, slightly ahead of the "v2" note.*  
*Last updated: 2026-08-28*
