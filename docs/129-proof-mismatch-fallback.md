# Proof-Mismatch Fallback Behavior

**Issue:** [#129](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/129)  
**Depends on:** [#124](./124-proof-verification-interface.md) (proof interface), [#48](./dispute-resolution-design.md) (dispute resolution)  
**Status:** Spec — ready for implementation once #124 lands

---

## 1. Problem Statement

Once `ProofRegistry` and proof-gated fills land (see #124), `fill_intent` will
cross-check the solver's claimed `fill_amount` against an on-chain
`ProofRecord` derived from a Wormhole VAA. Three categories of disagreement are
possible:

| Mismatch type | Description |
|---|---|
| **Amount mismatch** | `proof.src_amount < intent.src_amount` — the source deposit was smaller than the intent required |
| **Chain mismatch** | `proof.src_chain_id` does not map to `intent.src_chain` |
| **No proof** | `ProofRegistry` has no record for this `intent_id` when `require_proof = true` |

Each case needs a defined fallback so neither the user nor the solver is left in
limbo, and so the contract's state machine remains well-formed.

---

## 2. Mismatch Categories and Fallback Decisions

### 2.1 Amount Mismatch — `proof.src_amount < intent.src_amount`

**What happened:** The source deposit was smaller than the intent's `src_amount`.
This may be a partial deposit (user error), a fee deduction on the source chain,
or solver manipulation.

**Fallback — automatic partial-intent handling:**

1. `fill_intent` compares `proof.src_amount` against `intent.src_amount`.
2. If `proof.src_amount < intent.src_amount`, the call **panics with
   `Error::ProofAmountInsufficient`** — the fill is rejected.
3. The intent remains in `Accepted` state; the solver still holds fill
   obligations and the fill window is still running.
4. The solver must either:
   - Obtain a corrected VAA (if the source deposit was later topped up), or
   - Allow the deadline to elapse, triggering `slash_solver()` — the solver
     is slashed 10% and the intent is re-opened.

**Rationale:** Accepting an underfunded deposit would let solvers fill intents
for less than the user agreed to. Rejecting the fill and keeping the intent
`Accepted` gives the solver a narrow window to correct the situation without
automatic punishment, but maintains the slash backstop if they do not.

**New error code required:**

```rust
ProofAmountInsufficient = 27,
```

(Already listed in #124 §4.5.)

---

### 2.2 Chain Mismatch — `proof.src_chain_id ≠ mapped(intent.src_chain)`

**What happened:** The VAA came from a different chain than the intent specified.
This is almost certainly solver error (relayed the wrong VAA) or an attempted
attack (reusing a proof from a different deposit on a different chain).

**Fallback — hard reject, intent stays Accepted:**

1. `fill_intent` maps `intent.src_chain` (a string like `"ethereum"`) to its
   Wormhole chain ID using the canonical mapping table (see §4 below).
2. If `proof.src_chain_id` does not match, the call **panics with
   `Error::ProofChainMismatch`**.
3. Identical to the amount-mismatch path: intent stays `Accepted`, fill window
   still ticking, slash applies if the window expires.

**New error code required:**

```rust
ProofChainMismatch = 26,
```

(Already listed in #124 §4.5.)

---

### 2.3 No Proof — `ProofRegistry` has no record for `intent_id`

**What happened:** The solver called `fill_intent` with `require_proof = true`
but the `ProofRegistry` has not yet received a VAA for this intent. The
source-chain deposit may be in flight (not yet finalized), the VAA may not have
been relayed yet, or no deposit occurred at all.

**Fallback — reject fill, no state change:**

1. `fill_intent` calls `ProofRegistry.get_proof(intent_id)`.
2. If `None` is returned, the call **panics with `Error::ProofNotFound`**.
3. Intent stays `Accepted`. The solver should wait for the VAA to arrive and
   be relayed before retrying.
4. If the fill window expires before a valid proof arrives, `slash_solver()`
   is callable and the solver is slashed.

**New error code required:**

```rust
ProofNotFound = 25,
```

(Already listed in #124 §4.5.)

---

### 2.4 Proof Registry Not Configured

**What happened:** `fill_intent` was called with `require_proof = true` but the
admin has not yet called `set_proof_registry()`, so no `DataKey::ProofRegistry`
entry exists.

**Fallback — hard panic, no state change:**

```rust
ProofRegistryNotSet = 24,
```

This is a configuration error, not a mismatch. No slash. The admin must call
`set_proof_registry()` before proof-gated fills can proceed.

---

## 3. State Machine Under Proof Mismatch

```
Accepted
  │
  ├─[fill_intent, require_proof=true]──────────────────────────────────┐
  │                                                                     │
  │  ProofRegistry check                                               │
  │    ├─ proof not found            → panic(ProofNotFound)            │
  │    ├─ chain mismatch             → panic(ProofChainMismatch)        │
  │    ├─ amount insufficient        → panic(ProofAmountInsufficient)   │
  │    └─ proof valid + fill ok      → state = Filled ✓                │
  │                                                                     │
  │  [fill window expires, no successful fill]                         │
  └─[slash_solver()]──────────────────────────────────────────────────►│
                                                                        │
                                         state = Open (re-auctioned)   │
                                         10% bond slashed              ◄┘
```

All mismatch paths leave the intent in `Accepted` state and allow the fill
window to expire naturally, which means `slash_solver()` remains the
enforcement mechanism for a solver that cannot or will not produce a valid
proof.

---

## 4. Chain ID Mapping Table

`fill_intent` must translate `intent.src_chain` (a human-readable string) to
a Wormhole chain ID to compare against `proof.src_chain_id`. This mapping is
used in validation and must be kept in sync with the supported-chains list
(see [#132](./132-supported-chains.md)).

| `src_chain` string | Wormhole chain ID |
|---|---|
| `"ethereum"` | 2 |
| `"base"` | 30 |
| `"polygon"` | 5 |
| `"arbitrum"` | 23 |
| `"optimism"` | 24 |
| `"avalanche"` | 6 |
| `"bsc"` | 4 |
| `"solana"` | 1 |

Strings not in this table: `fill_intent` panics with `Error::SrcChainNotSupported`
(a new error code, separate from the allowlist variant).

**Implemented** (issue #253): this table is realized as
`IntentSettlement::src_chain_to_wormhole_id` in `intent_settlement/src/lib.rs`,
tested against every chain in the table above. `fill_intent` itself does not
yet call it — that wiring is issue #5's proof-gated fill logic.

---

## 5. Dispute Path for Contested Proofs

The cases above are all **automated** — the contract rejects or accepts based on
proof data alone, with no human intervention. However, there is one scenario
where the automated path is insufficient:

> The proof shows a valid amount and chain, but the *user* disputes the
> source-chain deposit (e.g., the VAA was generated from a reorg'd block, or
> the underlying oracle was manipulated).

This case is handled by the **dispute resolution flow** defined in
[dispute-resolution-design.md](./dispute-resolution-design.md). Specifically:

1. The fill is accepted on-chain (proof validated, tokens transferred).
2. During the dispute window, the user calls `open_dispute()`.
3. The arbiter reviews off-chain evidence (source-chain block explorer,
   reorg history).
4. If the arbiter upholds the dispute: tokens are returned to the user and
   the solver is slashed.
5. If dismissed: funds are released normally.

This is a v1 mechanism. A fully trustless path (on-chain reorg detection)
is deferred to a future research spike.

---

## 6. Summary of New Error Codes

| Code | Variant | Trigger |
|---|---|---|
| 24 | `ProofRegistryNotSet` | `fill_intent` with `require_proof=true` and no `DataKey::ProofRegistry` set |
| 25 | `ProofNotFound` | `ProofRegistry.get_proof()` returns `None` |
| 26 | `ProofChainMismatch` | `proof.src_chain_id` ≠ mapped chain ID for `intent.src_chain` |
| 27 | `ProofAmountInsufficient` | `proof.src_amount < intent.src_amount` |

---

## 7. Implementation Checklist

- [ ] Add the four error codes to `Error` enum in `intent_settlement/src/lib.rs`
- [ ] Implement chain-string → Wormhole-ID mapping function (const table or match arm)
- [ ] Add proof validation block inside `fill_intent` (gated on `require_proof`)
- [ ] Add `set_proof_registry(env, registry: Address)` admin entry-point
- [ ] Add `DataKey::ProofRegistry` to `DataKey` enum
- [ ] Write unit tests covering each of the four error paths
- [ ] Write a happy-path test: valid proof → intent transitions to `Filled`
- [ ] Verify `slash_solver()` remains callable after any mismatch-path rejection

---

*Closes #129*
