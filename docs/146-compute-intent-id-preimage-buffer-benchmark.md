# `compute_intent_id` — Preimage Construction vs. Fixed-Size Buffer

**Issue:** [#146](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/146)
**Status:** Reviewed — no change recommended; hard baseline blocked on the
resource-benchmarking harness (see [docs/149](149-resource-cost-per-entrypoint.md))

---

## 1. Current implementation

`compute_intent_id` (`intent_settlement/src/lib.rs`) builds its SHA-256
preimage as:

```rust
let mut preimage = Bytes::new(env);
preimage.append(&user.clone().to_xdr(env));       // host call + to_xdr
preimage.append(&src_chain.clone().to_xdr(env));  // host call + to_xdr
preimage.extend_from_array(&amount.to_be_bytes());     // host call, 16 B
preimage.extend_from_array(&timestamp.to_be_bytes());  // host call, 8 B
preimage.extend_from_array(&nonce.to_be_bytes());      // host call, 8 B
env.crypto().sha256(&preimage).into()                  // host call
```

Preimage size ≈ 40 (`Address` XDR) + ~12–20 (`String` XDR) + 32 (the three
integers) ≈ **~85–95 bytes**, and it is **variable-length** because the
`Address` and `String` XDR encodings vary.

## 2. Why a fixed-size stack buffer does not cleanly apply

`Bytes` is a host object in Soroban. The two costly-looking pieces —
serializing `user` and `src_chain` — go through `to_xdr`, which **must**
call the host; there is no guest-side XDR encoder for `Address`/`String`.
So a `[u8; N]` buffer:

- still needs `user.to_xdr(env)` and `src_chain.to_xdr(env)` (2 host calls,
  unavoidable), then a guest-side `copy_from_slice` of their bytes;
- cannot be truly fixed-size because those two lengths vary — it would need
  to be over-provisioned to a worst-case `N` plus a length cursor.

The only part that genuinely collapses is the **three integer appends**:
`amount` (16) + `timestamp` (8) + `nonce` (8) can be packed into one
`[u8; 32]` guest-side and appended with a single `extend_from_array`,
removing **2 host calls** out of roughly 8.

## 3. Expected effect

The dominant cost in this function is `sha256` over ~90 bytes plus the two
`to_xdr` host calls, all of which remain. Removing 2 of ~8 host boundary
crossings for the integer packing is a sub-1% change to the function, and
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

**No change.** The candidate optimisation (packing the three trailing
integers into one append) is small, its benefit is unmeasurable without the
harness, and it trades the current straightforwardly-readable preimage build
for a length-cursor buffer. Revisit only if the harness lands **and** a
measurement shows a real instruction-count win on `submit_intent`.

## 6. Follow-up (issue #666): buffer reuse across calls

Re-examined the request to reuse one preimage `Bytes` across calls. Not
applied, for these reasons:

- **Nothing survives between calls.** Each contract invocation runs with a
  fresh `Env`; a `Bytes` is a host handle that cannot be kept in guest memory
  or instance storage without costing more than rebuilding it.
- **There is no repeated preimage to reuse.** `batch_submit_intent` calls
  `submit_intent_inner` once per item, and each call bumps the user's nonce
  (`UserNonce`), so every item hashes a different preimage. A single
  `submit_intent` computes the id exactly once.
- **The live preimage is not packable.** In the current `compute_intent_id`
  (`intent_settlement/src/lib.rs`), `src_amount` and `min_dst_amount` are
  separated by the `dst_token` XDR, and `nonce` sits after the `user` XDR, so
  the three integers never form one contiguous run. The packing in §2 applies
  to the legacy timestamp-based variant, which is no longer called.

Result: no code change. The `Bytes` buffers stay per-call, and the id bytes
are unchanged. No benchmark numbers were recorded because no harness exists
(see §4).
