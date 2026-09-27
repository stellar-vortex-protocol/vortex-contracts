# IntentRecord / SolverRecord — Field Type & Ordering Review

**Issue:** [#147](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/147)
**Status:** Reviewed — no type/ordering change recommended
**Scope:** `intent_settlement/src/lib.rs` — `IntentRecord` (structs section) and `SolverRecord`

---

## 1. How Soroban actually stores these structs

A `#[contracttype]` struct is serialized to an `ScVal::Map` (`ScMap`): one
entry per field, the key is `ScVal::Symbol(<field name>)`, and **the host
sorts the entries by key**. Declaration order in the Rust source is *not*
preserved on the wire.

Consequence for this issue: **reordering the fields in the struct cannot
change the stored footprint.** Any "pack the small fields together" style
optimisation that would help a C struct has no effect here — the layout is
a name-keyed, name-sorted map, not a packed record.

Integer payload widths in the `ScVal` encoding:

| Rust type | `ScVal` | Payload bytes |
|---|---|---|
| `bool` | `Bool` | 1 |
| `u32` | `U32` | 4 |
| `u64` | `U64` | 8 |
| `i128` / `u128` | `I128` / `U128` | 16 |
| `Address` | `Address` | ~32 + tag |
| `String` / `Bytes` | length-prefixed | variable |
| `Option::None` | `Void` | 0 (entry still present) |

Each entry also carries the symbol key and per-`ScVal` tags, so the fixed
overhead per field is larger than the payload for the small scalars.

## 2. IntentRecord — field by field

| Field | Type | Realistic range | Verdict |
|---|---|---|---|
| `intent_id` | `BytesN<32>` | SHA-256 output | Exact fit. |
| `user` | `Address` | — | Required. |
| `src_chain` | `String` | chain name, e.g. `"ethereum"` | Kept as `String`. Could become a `u8`/enum chain code, but `AllowedSrcChain(String)` and the public API are keyed on the string; that is an API change, not type-narrowing. Out of scope. |
| `src_token` | `String` | source-chain token address (EVM `0x…` = 42 chars; other chains differ) | Must stay variable-width `String`. |
| `src_amount` | `i128` | bounded by `MAX_AMOUNT = 1e30` | `1e30 > u64::MAX (~1.8e19)`, so 128 bits are genuinely needed. `u128` would save 0 bytes vs `i128`. Keep. |
| `min_dst_amount` | `i128` | as above | Keep (128 bits needed). |
| `solver` | `Option<Address>` | — | Required. |
| `state` | `IntentState` | 8 unit variants | Already minimal. |
| `created_at` | `u64` | unix seconds | `env.ledger().timestamp()` is `u64`; `u32` seconds overflow in 2106. Keep. |
| `deadline` | `u64` | unix seconds | Keep (SDK-native). |
| `filled_at` | `Option<u64>` | unix seconds | Keep. |
| `fill_amount` | `Option<i128>` | cumulative dst tokens | 128 bits needed. |
| `total_filled` | `i128` | cumulative dst tokens | 128 bits needed. |

## 3. SolverRecord — field by field

| Field | Type | Realistic range | Verdict |
|---|---|---|---|
| `address` | `Address` | — | **Duplicates the storage key** `DataKey::Solver(Address)`. The record is only ever loaded by address. Removing it saves a whole map entry (~36 B) but touches every construction/read site. See §4. |
| `bond_amount` | `i128` | USDC smallest unit; realistically `< 1e15` | Fits `u64`, but kept `i128` for arithmetic consistency with the token-transfer paths and headroom. Marginal 8 B. |
| `fills_completed` | `u32` | lifetime fill count | 4.2e9 ceiling — ample. Minimal. |
| `fills_failed` | `u32` | as above | Minimal. |
| `total_volume` | `i128` | cumulative dst volume over solver lifetime | Unbounded growth; 128 bits justified. |
| `is_active` | `bool` | — | Minimal. |
| `registered_at` | `u64` | unix seconds | Keep (SDK-native). |
| `active_intents` | `u32` | concurrent accepted intents | Minimal. |
| `last_slash_time` | `u64` | unix seconds | Keep. |

## 4. Findings

1. **Field ordering is a non-issue.** `#[contracttype]` structs serialize as
   a key-sorted `ScMap`; no reordering can reduce the footprint.
2. **Every integer field is already at the right width.** `u64` timestamps
   are the SDK-native type and cannot safely drop to `u32`. The `i128`
   amount fields are bounded by `MAX_AMOUNT` (`1e30`), which exceeds
   `u64::MAX`, so they need 128 bits; switching them to `u128` saves zero
   bytes. The `u32` counters are already minimal.
3. **The only real footprint wins are structural, not type-level, and are
   deliberately left out of this issue:**
   - `SolverRecord.address` duplicates the `DataKey::Solver(addr)` key.
   - `IntentRecord.fill_amount` and `IntentRecord.total_filled` appear to
     track the same quantity (cumulative dst tokens delivered).
   Each is roughly one whole map entry. Both change call-site logic and,
   for `fill_amount`, event payloads, so they belong in their own issues
   rather than a "type/ordering" pass.
4. **Net recommendation: no type or ordering change.** Nothing in the
   current definitions loses range headroom or wastes an integer width that
   a safe narrowing could reclaim. No serialization tests change because no
   field type changes.
