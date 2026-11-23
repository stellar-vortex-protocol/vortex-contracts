# Client-Computable, Timestamp-Free Intent IDs (Issues #354, #565, #685)

> **Status: implemented.** This document specifies the preimage that
> `intent_settlement` hashes today, so off-chain clients can derive the id of
> an intent before submitting it. It replaces the original design text, which
> described a different argument list (`trigger_threshold`, `duration_days`,
> `client_supplied_id`, a legacy fallback) that was never adopted.
>
> Source of truth: `intent_settlement/src/lib.rs`, `fn compute_intent_id`
> (the 7-argument form taking `dst_token` and `min_dst_amount`), called from
> `submit_intent` and `preview_intent_id`. Note: `lib.rs` currently also
> contains a second, older `compute_intent_id` (`user, src_chain, amount,
> timestamp, nonce`) from a pre-existing duplicated block. `submit_intent`
> and `preview_intent_id` use the arguments of the v1 form below; the older
> timestamp form is not part of the live scheme.

## Why

The legacy id mixed in the ledger timestamp, unknown until inclusion, so the
source-chain deposit payload could not carry the id until after the Stellar
transaction landed. The v1 id uses only values the client knows in advance.

## Specification

All hashing is SHA-256. `||` is byte concatenation.

```
params      = xdr(src_chain) || be16(src_amount) || xdr(dst_token) || be16(min_dst_amount)
params_hash = sha256(params)

preimage    = "vortex-intent-v1"        (16 ASCII bytes, no length prefix)
           || xdr(contract_id)          (the intent_settlement contract address)
           || network_id                (32 bytes, raw)
           || xdr(user)                 (the submitting address)
           || be8(nonce)                (u64, big-endian)
           || params_hash               (32 bytes, raw)

intent_id   = sha256(preimage)          (BytesN<32>)
```

Field details, in order:

| # | Field | Encoding | Size |
|---|---|---|---|
| 1 | domain separator | ASCII `vortex-intent-v1` | 16 |
| 2 | contract id | `ScVal::Address` XDR of the contract (`env.current_contract_address().to_xdr()`) | 40 (contract address) |
| 3 | network id | `env.ledger().network_id()`, i.e. `sha256(network_passphrase)`, raw bytes | 32 |
| 4 | user | `ScVal::Address` XDR of the user | 44 (account) / 40 (contract) |
| 5 | nonce | `u64`, big-endian (`to_be_bytes`) | 8 |
| 6 | params hash | `sha256(params)`, raw bytes | 32 |

`params` fields:

| Field | Encoding | Size |
|---|---|---|
| `src_chain` | `ScVal::String` XDR: u32 type tag `14`, u32 length, bytes, zero-padded to a multiple of 4 | 8 + padded length |
| `src_amount` | `i128`, two's-complement big-endian (`to_be_bytes`) | 16 |
| `dst_token` | `ScVal::Address` XDR | 40 (contract) |
| `min_dst_amount` | `i128`, big-endian | 16 |

Encoding notes:

- `xdr(x)` means the Soroban host XDR of the value as an `ScVal`
  (`to_xdr`), not a bare `ScAddress`. An `ScVal::Address` is the u32 tag `18`
  followed by the `ScAddress`: for a contract, u32 `1` then 32 bytes; for an
  account, u32 `0`, u32 `0` (ed25519 public key type) then 32 bytes.
- Integers are fixed width and big-endian with no length prefix; amounts are
  signed 128-bit.
- Only these five submit arguments plus the nonce are in the preimage. Other
  `submit_intent` arguments (for example deadline or exclusivity options) do
  **not** affect the id.
- The nonce is the user's current counter, read with `get_user_nonce(user)`
  (instance storage `DataKey::UserNonce(user)`, default `0`). `submit_intent`
  uses the pre-increment value, then stores `nonce + 1`. Two submissions that
  race for the same nonce get the same id; the second one fails.
- `preview_intent_id(user, src_chain, src_amount, dst_token, min_dst_amount)`
  returns the id the next submission will get at the current nonce, which is
  a cross-check for client code.

## Properties

1. **Deterministic and offline:** every input is known before submission.
2. **Cross-network and cross-contract safe:** `network_id` and `contract_id`
   are in the preimage.
3. **Unique per user submission:** the strictly increasing nonce separates
   otherwise identical intents, including in the same ledger.
4. **No timestamp** is used.

## Reference computation

JavaScript with `@stellar/stellar-sdk` (XDR of `ScVal` values):

```typescript
import { createHash } from "crypto";
import { Address, nativeToScVal, xdr } from "@stellar/stellar-sdk";

const sha256 = (b: Buffer) => createHash("sha256").update(b).digest();
const be = (v: bigint, bytes: number) => {
  const out = Buffer.alloc(bytes);
  let x = BigInt.asUintN(bytes * 8, v);
  for (let i = bytes - 1; i >= 0; i--) { out[i] = Number(x & 0xffn); x >>= 8n; }
  return out;
};
const scAddr = (a: string) => new Address(a).toScVal().toXDR();

export function computeIntentId(p: {
  contractId: string; networkPassphrase: string; user: string; nonce: bigint;
  srcChain: string; srcAmount: bigint; dstToken: string; minDstAmount: bigint;
}): Buffer {
  const params = Buffer.concat([
    xdr.ScVal.scvString(p.srcChain).toXDR(),
    be(p.srcAmount, 16),
    scAddr(p.dstToken),
    be(p.minDstAmount, 16),
  ]);
  const preimage = Buffer.concat([
    Buffer.from("vortex-intent-v1", "ascii"),
    scAddr(p.contractId),
    sha256(Buffer.from(p.networkPassphrase, "utf8")),
    scAddr(p.user),
    be(p.nonce, 8),
    sha256(params),
  ]);
  return sha256(preimage);
}
```

## Worked example

Inputs (illustrative; addresses are 32-byte patterns, not funded accounts):

| Input | Value |
|---|---|
| network passphrase | `Test SDF Network ; September 2015` |
| contract id | contract address, 32 bytes of `0x11` |
| user | account address, ed25519 key of 32 bytes of `0x22` |
| nonce | `0` |
| `src_chain` | `ethereum` |
| `src_amount` | `1000000000` |
| `dst_token` | contract address, 32 bytes of `0x33` |
| `min_dst_amount` | `3000000000` |

Derived (`params` is 88 bytes, `preimage` is 172 bytes):

```
network_id  = cee0302d59844d32bdca915c8203dd44b33fbb7edc19051ea37abedf28ecd472
params_hash = 16d377f75af605c607cde3e298fc5dde82853ed340024aadc25f2a330d670df4
intent_id   = e76ae380c8bcc6b5b38e299381162198a74ce9f0a18f9c66abc68ef0c836b18b
```

Caveat: these values were computed by hand-encoding the layout above with a
scratch SHA-256 script, **not** by running the contract or a unit test; the
workspace does not currently build on `main`, so `cargo test -p
intent_settlement` could not be run. Before relying on the vector, confirm it
by calling `preview_intent_id` on a deployed contract (or adding a test to
`intent_settlement/src/test.rs`) with the same inputs.

## Cross-chain flow

1. Client reads `get_user_nonce(user)` and computes `intent_id` offline.
2. Client places `intent_id` in the source-chain deposit payload.
3. Client submits the intent on Stellar; the contract assigns the same id.
4. The proof registry relays the source-chain data with `intent_id`, and
   settlement matches it to the proof.

Either order (deposit first, or submit first) works as long as the nonce has
not advanced in between.

## Related documents

- [146-compute-intent-id-preimage-buffer-benchmark.md](146-compute-intent-id-preimage-buffer-benchmark.md)
- [267-intent-id-collision-predictability-audit.md](267-intent-id-collision-predictability-audit.md)
