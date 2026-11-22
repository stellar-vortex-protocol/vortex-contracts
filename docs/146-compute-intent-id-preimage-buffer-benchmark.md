# `compute_intent_id` — Preimage Construction vs. Fixed-Size Buffer

**Issue:** [#146](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/146)
**Status:** Reviewed — no change recommended; hard baseline blocked on the
resource-benchmarking harness (see [docs/149](149-resource-cost-per-entrypoint.md))

---

## 1. Current implementation

> **Updated (#685).** This review was written against the original
> timestamp-based preimage. `compute_intent_id` is now the client-computable
> v1 scheme specified in
> [CLIENT_COMPUTABLE_INTENT_IDS.md](CLIENT_COMPUTABLE_INTENT_IDS.md); the
> analysis below is adjusted to it. The conclusion is unchanged.

`compute_intent_id` (`intent_settlement/src/lib.rs`) builds its SHA-256
preimage in two steps:

```rust
let mut params = Bytes::new(env);
params.append(&src_chain.clone().to_xdr(env));      // host call + to_xdr
params.extend_from_array(&src_amount.to_be_bytes());     // host call, 16 B
params.append(&dst_token.clone().to_xdr(env));      // host call + to_xdr
params.extend_from_array(&min_dst_amount.to_be_bytes()); // host call, 16 B
let params_hash: BytesN<32> = env.crypto().sha256(&params).into();

let mut preimage = Bytes::from_slice(env, b"vortex-intent-v1");
preimage.append(&env.current_contract_address().to_xdr(env)); // to_xdr
preimage.append(&env.ledger().network_id().into());           // 32 B
preimage.append(&user.clone().to_xdr(env));                   // to_xdr
preimage.extend_from_array(&nonce.to_be_bytes());             // 8 B
preimage.append(&params_hash.into());                         // 32 B
env.crypto().sha256(&preimage).into()
```

The final preimage is variable-length: 16 + 40 (contract) + 32 + 40/44
(user) + 8 + 32, about 170 bytes, plus an inner `params` hash input of about
85-100 bytes. Two SHA-256 host calls and four `to_xdr` host calls remain
regardless of how the integers are packed.

## 2. Why a fixed-size stack buffer does not cleanly apply

`Bytes` is a host object in Soroban. The two costly-looking pieces —
serializing `user` and `src_chain` — go through `to_xdr`, which **must**
call the host; there is no guest-side XDR encoder for `Address`/`String`.
So a `[u8; N]` buffer:

- still needs `user.to_xdr(env)` and `src_chain.to_xdr(env)` (2 host calls,
  unavoidable), then a guest-side `copy_from_slice` of their bytes;
- cannot be truly fixed-size because those two lengths vary — it would need
  to be over-provisioned to a worst-case `N` plus a length cursor.

The only part that genuinely collapses is the **integer appends**:
`src_amount` (16) + `min_dst_amount` (16) are separated by `dst_token`'s
XDR in `params`, so they cannot be merged into one array without reordering
the (now client-specified) preimage, which would change every id. The
`nonce` append (8 B) is the only trailing integer. The cheap win this
review originally described therefore no longer exists in the same form.

## 3. Expected effect

The dominant cost in this function is the two `sha256` calls plus the four
`to_xdr` host calls, all of which remain. Any integer packing would remove
at most one or two host boundary crossings, a sub-1% change, and
`compute_intent_id` is itself a small fraction of `submit_intent` (which
also does a persistent `IntentRecord` write, nonce read/write, and instance
bookkeeping).

## 4. Baseline measurement

This issue asks for a measured baseline first. The repo has **no
resource-benchmarking harness** (no `benches/`, no `Budget`/instruction-count
usage in `intent_settlement`) — the same blocker documented in
[docs/149](149-resource-cost-per-entrypoint.md). A CPU-instruction baseline
and A/B comparison cannot be produced on this branch.

## 5. Recommendation

**No change.** The candidate optimisation (packing integer appends) is small, its benefit is unmeasurable without the
harness, and it trades the current straightforwardly-readable preimage build
for a length-cursor buffer. Revisit only if the harness lands **and** a
measurement shows a real instruction-count win on `submit_intent`.
