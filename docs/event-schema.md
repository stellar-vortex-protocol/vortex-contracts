# Event Schema — `intent_settlement`

> **Issue #106** — Canonical reference for every event emitted by the
> `intent_settlement` contract. Indexer authors should treat this document as
> the authoritative source; the source of truth is the `env.events().publish()`
> call sites in `intent_settlement/src/lib.rs`.

---

## Conventions

Soroban events carry two parts:

| Part | Description |
|------|-------------|
| **Topics** | An ordered tuple encoded as a `Vec<Val>`. The first element is always a `Symbol` (the event name). Additional elements identify the principal actor (solver, user, admin address). |
| **Data / Payload** | A single `Val` — scalar, tuple, or struct — containing the event's variable data. |

All `Address` values are Stellar account or contract addresses (StrKey encoded
off-chain). All token amounts are `i128` in the bond token's smallest unit
(stroops for USDC with 7 decimal places: `1_000_000 = 0.1 USDC`).

---

## Event Catalogue

### Admin Events

---

#### `fee_recipient_proposed`

Emitted by `propose_fee_recipient` when the admin nominates a new fee recipient.
The change is **not yet active** — the nominee must call `accept_fee_recipient`.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"fee_recipient_proposed"` |
| **data** | `Address` | The proposed (not yet active) new fee recipient address |

```
topics : ("fee_recipient_proposed",)
data   : <new_fee_recipient: Address>
```

---

#### `fee_recipient_updated`

Emitted by `accept_fee_recipient` once the nominee confirms. After this event
the new address receives all future protocol fees.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"fee_recipient_updated"` |
| **data** | `Address` | The newly active fee recipient address |

```
topics : ("fee_recipient_updated",)
data   : <new_fee_recipient: Address>
```

---

#### `admin_transferred`

Emitted by `transfer_admin` after both the outgoing and incoming admin have
signed. The admin role is now held by the data address.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"admin_transferred"` |
| **data** | `Address` | The new admin address |

```
topics : ("admin_transferred",)
data   : <new_admin: Address>
```

---

#### `config_updated`

Emitted by `set_config` when the admin updates the four protocol parameters
atomically.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"config_updated"` |
| **data[0]** | `i128` | `min_bond` — minimum solver bond in bond token's smallest unit |
| **data[1]** | `u64` | `fill_window` — seconds a solver has to fill after accepting |
| **data[2]** | `u64` | `intent_expiry` — default intent lifetime in seconds |
| **data[3]** | `i128` | `protocol_fee_bps` — fee in basis points (1 bps = 0.01%) |

```
topics : ("config_updated",)
data   : (min_bond: i128, fill_window: u64, intent_expiry: u64, protocol_fee_bps: i128)
```

---

#### `paused`

Emitted by `pause` when the contract is halted for incident response.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"paused"` |
| **data** | `()` | (empty payload — topic alone is sufficient) |

```
topics : ("paused",)
data   : ()
```

---

#### `unpaused`

Emitted by `unpause` when the contract resumes normal operation after a pause.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"unpaused"` |
| **data** | `()` | (empty payload — topic alone is sufficient) |

```
topics : ("unpaused",)
data   : ()
```

---

#### `tokens_rescued`

Emitted by `rescue_tokens` when the admin recovers accidentally-sent tokens.
Only non-bond, non-active-intent tokens can be rescued.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"tokens_rescued"` |
| **topics[1]** | `Address` | `to` — recipient of the rescued tokens |
| **data[0]** | `Address` | `token` — the token contract that was transferred |
| **data[1]** | `i128` | `amount` — units transferred |

```
topics : ("tokens_rescued", <to: Address>)
data   : (token: Address, amount: i128)
```

---

### Allowlist Events

---

#### `dst_token_allowed`

Emitted by `add_allowed_dst_token` when a destination token is added to the
allowlist. Only emitted after the SEP-41 interface probe succeeds.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"dst_token_allowed"` |
| **data** | `Address` | The token address that was added |

```
topics : ("dst_token_allowed",)
data   : <token: Address>
```

---

#### `dst_token_disallowed`

Emitted by `remove_allowed_dst_token`.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"dst_token_disallowed"` |
| **data** | `Address` | The token address that was removed |

```
topics : ("dst_token_disallowed",)
data   : <token: Address>
```

---

#### `src_chain_allowed`

Emitted by `add_allowed_src_chain`.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"src_chain_allowed"` |
| **data** | `String` | The chain name added (e.g. `"ethereum"`) |

```
topics : ("src_chain_allowed",)
data   : <chain: String>
```

---

#### `src_chain_disallowed`

Emitted by `remove_allowed_src_chain`.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"src_chain_disallowed"` |
| **data** | `String` | The chain name removed |

```
topics : ("src_chain_disallowed",)
data   : <chain: String>
```

---

#### `dst_allowlist_enabled`

Emitted by `set_dst_allowlist_enabled` to signal a change in whether the
destination token allowlist is actively enforced by `submit_intent`.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"dst_allowlist_enabled"` |
| **data** | `bool` | `true` = allowlist enforcement active; `false` = disabled |

```
topics : ("dst_allowlist_enabled",)
data   : <enabled: bool>
```

---

#### `src_chain_allowlist_enabled`

Emitted by `set_src_chain_allowlist_enabled` to signal a change in whether
the source-chain allowlist is actively enforced by `submit_intent`.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"src_chain_allowlist_enabled"` |
| **data** | `bool` | `true` = allowlist enforcement active; `false` = disabled |

```
topics : ("src_chain_allowlist_enabled",)
data   : <enabled: bool>
```

---

#### `bond_multiplier_set`

Emitted by `set_min_bond_multiplier` when the admin configures a per-token
bond requirement. A multiplier of `10` = 1.0×, `15` = 1.5×, `20` = 2.0×.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"bond_multiplier_set"` |
| **data[0]** | `Address` | The destination token this multiplier applies to |
| **data[1]** | `i128` | The multiplier value (10 = 1.0×) |

```
topics : ("bond_multiplier_set",)
data   : (token: Address, multiplier: i128)
```

---

### Solver Management Events

---

#### `solver_registered`

Emitted by `register_solver` after the bond transfer succeeds. Covers both
first-time registration and subsequent top-ups.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"solver_registered"` |
| **topics[1]** | `Address` | The solver address |
| **data** | `i128` | `bond_amount` — the incremental deposit (not cumulative total) |

```
topics : ("solver_registered", <solver: Address>)
data   : <bond_amount: i128>
```

---

#### `solver_deregistered`

Emitted by `deregister_solver` after the full bond refund transfer.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"solver_deregistered"` |
| **topics[1]** | `Address` | The solver address |
| **data** | `i128` | The bond amount refunded (was `solver_record.bond_amount` before removal) |

```
topics : ("solver_deregistered", <solver: Address>)
data   : <bond_refunded: i128>
```

---

#### `bond_withdrawn`

Emitted by `withdraw_bond` after a partial bond withdrawal. The payload
includes both the withdrawn amount and the **remaining bond balance** after
the withdrawal, so indexers can track a solver's current bond without a
separate `get_solver` call.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"bond_withdrawn"` |
| **topics[1]** | `Address` | The solver address |
| **data[0]** | `i128` | `amount` — bond units withdrawn |
| **data[1]** | `i128` | `remaining` — solver's bond balance after the withdrawal |

```
topics : ("bond_withdrawn", <solver: Address>)
data   : (amount: i128, remaining: i128)
```

> **Note:** `remaining` is the post-withdrawal `SolverRecord.bond_amount`.
> This field was added in issue #108 to enable balance-parity checks without
> a `get_solver` round-trip.

---

#### `solver_slashed`

Emitted by `slash_solver` after the slash amount is transferred to the fee
recipient and the intent is re-opened.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"solver_slashed"` |
| **topics[1]** | `Address` | The solver address that was slashed |
| **data[0]** | `BytesN<32>` | `intent_id` — the intent the solver failed to fill |
| **data[1]** | `i128` | `slash_amount` — bond units deducted and sent to fee recipient |

```
topics : ("solver_slashed", <solver: Address>)
data   : (intent_id: BytesN<32>, slash_amount: i128)
```

---

### Intent Lifecycle Events

---

#### `intent_submitted`

Emitted by `submit_intent` when a user creates a new swap intent.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"intent_submitted"` |
| **topics[1]** | `Address` | `user` — the intent owner |
| **data[0]** | `BytesN<32>` | `intent_id` — deterministic 32-byte identifier |
| **data[1]** | `i128` | `min_dst_amount` — minimum acceptable output in dst token units |
| **data[2]** | `u64` | `expiry` — Unix timestamp after which the intent can be expired |

```
topics : ("intent_submitted", <user: Address>)
data   : (intent_id: BytesN<32>, min_dst_amount: i128, expiry: u64)
```

---

#### `intent_accepted`

Emitted by `accept_intent` when a solver claims exclusive fill rights. The
deadline in the payload is the **fill-window deadline** (`now + fill_window`),
not the original intent expiry.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"intent_accepted"` |
| **topics[1]** | `Address` | `solver` — the solver that accepted |
| **data[0]** | `BytesN<32>` | `intent_id` |
| **data[1]** | `u64` | `fill_deadline` — Unix timestamp; solver must fill before this |

```
topics : ("intent_accepted", <solver: Address>)
data   : (intent_id: BytesN<32>, fill_deadline: u64)
```

---

#### `intent_filled`

Emitted by `fill_intent` on each (partial or full) fill. Emitted even when the
intent transitions to `PartiallyFilled` rather than `Filled` — the cumulative
`fill_amount` in the `IntentRecord` tracks overall progress.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"intent_filled"` |
| **topics[1]** | `Address` | `solver` — the solver that delivered this fill |
| **data[0]** | `BytesN<32>` | `intent_id` |
| **data[1]** | `i128` | `fill_amount` — dst token units delivered in this fill |
| **data[2]** | `i128` | `fee` — protocol fee units paid by the solver on this fill |

```
topics : ("intent_filled", <solver: Address>)
data   : (intent_id: BytesN<32>, fill_amount: i128, fee: i128)
```

---

#### `intent_cancelled`

Emitted by `cancel_intent` when the intent owner cancels an Open intent.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"intent_cancelled"` |
| **topics[1]** | `Address` | `user` — the intent owner who cancelled |
| **data** | `BytesN<32>` | `intent_id` |

```
topics : ("intent_cancelled", <user: Address>)
data   : <intent_id: BytesN<32>>
```

---

#### `intent_expired`

Emitted by `expire_intent` when an Open intent's deadline is permissionlessly
materialized as `Expired`.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"intent_expired"` |
| **data** | `BytesN<32>` | `intent_id` |

```
topics : ("intent_expired",)
data   : <intent_id: BytesN<32>>
```

---

#### `extension_granted`

Emitted by `request_extension` when a solver is granted a one-time fill-window
extension. Each intent can only trigger this event once.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"extension_granted"` |
| **topics[1]** | `Address` | `solver` — the solver that requested the extension |
| **data[0]** | `BytesN<32>` | `intent_id` |
| **data[1]** | `u64` | `new_deadline` — the updated fill deadline after extension |

```
topics : ("extension_granted", <solver: Address>)
data   : (intent_id: BytesN<32>, new_deadline: u64)
```

---

## State Transition Summary

The table below shows which event drives each `IntentState` transition. An
indexer can fully reconstruct intent state by replaying these events in
ledger order.

| From state | To state | Driving event |
|------------|----------|---------------|
| _(none)_ | `Open` | `intent_submitted` |
| `Open` | `Accepted` | `intent_accepted` |
| `Open` | `Cancelled` | `intent_cancelled` |
| `Open` | `Expired` | `intent_expired` |
| `Accepted` | `PartiallyFilled` → re-opens as `Open` | `intent_filled` (partial) |
| `Accepted` | `Filled` | `intent_filled` (cumulative ≥ min_dst_amount) |
| `Accepted` | `Open` / `PartiallyFilled` (re-opened) | `solver_slashed` (`slash_cycles < max_slash_cycles`) |
| `Accepted` | `Abandoned` | `solver_slashed` + `intent_abandoned` (`slash_cycles >= max_slash_cycles`) |
| `PartiallyFilled` | `Accepted` | `intent_accepted` |
| `PartiallyFilled` | `Expired` | `intent_expired` |
| `PartiallyFilled` | `Cancelled` | `intent_cancelled` |

`Abandoned` is terminal: an intent that hits `ProtocolConfig.max_slash_cycles`
repeated `Accepted → Slashed` cycles no longer re-opens; the user must
resubmit a fresh intent (issue #241).

> **Bidding mode:** If bid-window mode is active, `intent_submitted` opens the
> intent in `Bidding` state. `bid_intent` events (not yet emitted as named
> events) track competing quotes; `settle_bids` transitions to `Accepted`.
> This path is omitted above as the bid-event schema will be specified
> separately once the feature is finalized.

---

## Indexer Notes

1. **Topic layout matters.** Events with a solver/user in `topics[1]` are
   queryable by address using the Horizon `/accounts/{id}/effects` or
   Soroban RPC `getEvents` filter `{ "topics": [["*", "<address>"]] }`.

2. **`bond_withdrawn` balance parity.** The `remaining` field in the
   `bond_withdrawn` payload (`data[1]`) lets an indexer maintain a real-time
   solver bond ledger without ever calling `get_solver`. Combine with
   `solver_registered` (incremental deposit) and `solver_slashed` (slash
   amount) and `solver_deregistered` (full refund) to reconstruct the full
   bond history:
   - `bond` += `bond_amount` on `solver_registered`
   - `bond` -= `slash_amount` on `solver_slashed`
   - `bond`  = `remaining` on `bond_withdrawn` (authoritative snapshot)
   - `bond`  = 0 on `solver_deregistered`

3. **`open_intents` counter.** `get_stats` now returns a third value,
   `open_intents`, tracking intents currently in `Open` state. See issue #109
   and the trade-off note in `get_stats` for details. Indexers may use this as
   a cross-check against their own event-replayed count.

4. **Partial fills.** A single intent may emit `intent_filled` multiple times
   before reaching `Filled`. Each emission carries that fill's incremental
   `fill_amount` (not cumulative). Sum all `intent_filled.data[1]` for the
   same `intent_id` to get total volume for that intent.

   As of #244, this no longer strictly requires event replay:
   `get_intent_fill_history(intent_id)` returns an on-chain
   `Vec<(solver, amount, timestamp)>` log of each fill for that intent
   directly, bounded at `MAX_FILL_HISTORY` (20) entries with the oldest
   entry evicted first once the cap is reached. For intents with more than
   20 partial fills, event replay is still required for the full history.
