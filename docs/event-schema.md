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

---

## Issue #377 — SDK upgrade migration note

All crates currently pin `soroban-sdk = "21.0.0"`. When upgrading to the
next major release:

1. **Typed events** (`#[contractevent]`): replace raw
   `env.events().publish((Symbol::new(...),), data)` calls with typed structs
   annotated `#[contractevent]`. The encoding of topics and data does not
   change for simple `(symbol, address)` patterns, but compound topics may
   need verification against the indexer fixtures.

2. **Test registration**: `env.register_contract(None, T)` becomes
   `env.register(T, ())` in sdk ≥ 22. The bench and test files already use
   `register_stellar_asset_contract_v2` (the newer API).

3. **TTL/auth APIs**: no breaking changes expected for the patterns used here
   (`extend_ttl`, `require_auth`, `mock_all_auths`).

4. **Event compatibility**: run the reference-indexer test suite
   (`indexer/reference-indexer.test.js`) against fixtures generated with both
   SDK versions before merging. Any encoding change must be documented here.

---

## Issue #378 — Policy hook events

#### `policy_proposed`

Emitted by `propose_set_policy`.

| Field | Type | Description |
|-------|------|-------------|
| topics[0] | `Symbol("policy_proposed")` | event name |
| data | `(Option<Address>, u64)` | proposed policy address (None = clear), ETA timestamp |

#### `policy_updated`

Emitted by `execute_set_policy`.

| Field | Type | Description |
|-------|------|-------------|
| topics[0] | `Symbol("policy_updated")` | event name |
| data | `Option<Address>` | new active policy address (None = cleared) |

---

## Current `intent_settlement` additions

The following events are emitted by the current `intent_settlement` contract
and supplement the catalogue above. Every row has `emitting crate =
intent_settlement`; topic tuples and payloads are shown exactly as published.
This table deliberately does not restate the passive-expiry behaviour covered
by [111-expire-intent-event-coverage.md](111-expire-intent-event-coverage.md).

| Topic tuple | Data shape | Emitting entrypoint |
|---|---|---|
| `("fee_recipient_proposal_cancelled",)` | `pending_recipient: Address` | `cancel_pending_fee_recipient` |
| `("admin_transfer_proposed",)` | `(new_admin: Address, eta: u64)` | `propose_admin_transfer` |
| `("upgrade_proposed",)` | `(wasm_hash: BytesN<32>, eta: u64)` | `propose_upgrade` |
| `("upgraded",)` | `wasm_hash: BytesN<32>` | `execute_upgrade` |
| `("migrated",)` | `(previous_version: u32, current_version: u32)` | `migrate` |
| `("dst_token_add_proposed",)` | `(token: Address, eta: u64)` | `propose_add_dst_token` |
| `("dst_token_add_proposal_cancelled",)` | `token: Address` | `cancel_pending_dst_token_add` |
| `("dst_token_remove_proposed",)` | `(token: Address, eta: u64)` | `propose_remove_dst_token` |
| `("dst_token_remove_proposal_cancelled",)` | `token: Address` | `cancel_pending_dst_token_remove` |
| `("chain_onboarding_proposed",)` | `(name: String, wormhole_id: u32, eta: u64)` | `propose_chain_onboarding` |
| `("chain_onboarding_executed",)` | `(name: String, wormhole_id: u32)` | `execute_chain_onboarding` |
| `("chain_offboarding_executed",)` | `name: String` | `execute_chain_offboarding` |
| `("pauser_updated",)` | `pauser: Address` | `set_pauser` |
| `("proof_registry_set",)` | `registry: Address` | `set_proof_registry` |
| `("intent_amended", user: Address)` | `(intent_id: BytesN<32>, old_min: i128, new_min: i128, deadline: u64)` | `amend_intent` |
| `("fill_begun",)` | `(intent_id: BytesN<32>, solver: Address, amount: i128)` | `begin_fill` |
| `("fill_begun", solver: Address)` | `(intent_id: BytesN<32>, amount: i128, dispute_deadline: u64)` | `begin_fill_with_escrow` |
| `("bid_window_enabled",)` | `enabled: bool` | `set_bid_window_enabled` |
| `("bid_submitted", solver: Address)` | `(intent_id: BytesN<32>, quoted_dst_amount: i128)` | `bid_intent` |
| `("intent_accepted", solver: Address)` | `(intent_id: BytesN<32>, deadline: u64)` | `settle_bids` |
| `("bids_settled",)` | `(intent_id: BytesN<32>, winning_quote: i128)` | `settle_bids` |
| `("bids_settled_no_winner",)` | `intent_id: BytesN<32>` | `settle_bids` |
| `("arbiter_updated",)` | `arbiter: Address` | `set_arbiter` |
| `("arbiter_panel_set",)` | `panel: Address` | `set_arbiter_panel` |
| `("fill_disputed", user: Address)` | `intent_id: BytesN<32>` | `dispute_fill` |
| `("dispute_resolved", solver: Address)` | `(intent_id: BytesN<32>, solver_payout: i128, slash_amount: i128)` | `resolve_dispute` |
| `("fill_released", solver: Address)` | `(intent_id: BytesN<32>, solver_payout: i128)` | `release_fill` |
| `("bond_token_allowed",)` | `token: Address` | `add_allowed_bond_token` |
| `("bond_token_disallowed",)` | `token: Address` | `remove_allowed_bond_token` |
| `("bond_token_min_set",)` | `(token: Address, amount: i128)` | `set_min_bond_for_token` |
| `("outbound_intent_submitted", user: Address)` | `outbound_intent_id: BytesN<32>` | `submit_outbound_intent` |
| `("outbound_intent_accepted", solver: Address)` | `outbound_intent_id: BytesN<32>` | `accept_outbound_intent` |
| `("extension_granted", solver: Address)` | `(intent_id: BytesN<32>, deadline: u64)` | `request_extension` |
| `("backstop_claimed", user: Address)` | `(intent_id: BytesN<32>, payout: i128, remaining_pool: i128)` | `claim_backstop` |
| `("intent_retention_updated",)` | `retention_secs: u64` | `set_intent_retention` |
| `("intent_closed",)` | `(intent_id: BytesN<32>, user: Address, state: IntentState)` | `close_intent` |
| `("oracle_proposed",)` | `(oracle: Address, eta: u64)` | `propose_oracle` |
| `("oracle_set",)` | `oracle: Address` | `execute_oracle` |

# Event Schema — `backstop_vault`

> Added in issue #519. Source of truth: `backstop_vault/src/lib.rs`.

The backstop vault is an LP pool that absorbs solver-default losses and earns a
share of protocol fees and slash proceeds. All token amounts are `i128` in the
bond token's smallest unit (stroops for USDC with 7 decimal places).

---

## Event Catalogue

### LP Events

---

#### `deposit`

Emitted by `deposit` after the LP's bond-token transfer is confirmed and shares
are minted.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"deposit"` |
| **topics[1]** | `Address` | `lp` — the liquidity provider |
| **data[0]** | `i128` | `amount` — bond-token units deposited |
| **data[1]** | `i128` | `shares_minted` — vault shares issued to the LP |

```
topics : ("deposit", <lp: Address>)
data   : (amount: i128, shares_minted: i128)
```

---

#### `withdraw`

Emitted by `withdraw` on a normal (non-queued) share redemption after the bond
tokens have been transferred back to the LP.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"withdraw"` |
| **topics[1]** | `Address` | `lp` — the liquidity provider |
| **data[0]** | `i128` | `shares` — vault shares redeemed |
| **data[1]** | `i128` | `assets_out` — bond-token units returned |

```
topics : ("withdraw", <lp: Address>)
data   : (shares: i128, assets_out: i128)
```

---

#### `withdraw_queued`

Emitted by `withdraw` when the vault is in emergency mode. Shares are burned
immediately but the asset payout is deferred until `process_queued_withdrawal`
is called.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"withdraw_queued"` |
| **topics[1]** | `Address` | `lp` — the liquidity provider |
| **data[0]** | `i128` | `shares` — vault shares burned |
| **data[1]** | `i128` | `assets_out` — bond-token units queued for payout |

```
topics : ("withdraw_queued", <lp: Address>)
data   : (shares: i128, assets_out: i128)
```

---

#### `queued_withdrawal_processed`

Emitted by `process_queued_withdrawal` after the queued payout is transferred
to the LP.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"queued_withdrawal_processed"` |
| **topics[1]** | `Address` | `lp` — the liquidity provider |
| **data** | `i128` | `payout` — bond-token units actually transferred (may be less than queued amount if vault is underfunded) |

```
topics : ("queued_withdrawal_processed", <lp: Address>)
data   : <payout: i128>
```

---

### Settlement-Callback Events

---

#### `cover`

Emitted by `cover` after the vault pays out backstop compensation to a user
whose solver defaulted. Loss is socialized across all LP shares by reducing
`TotalAssets`.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"cover"` |
| **topics[1]** | `Address` | `recipient` — the party receiving the compensation |
| **data[0]** | `BytesN<32>` | `intent_id` — the intent that triggered the cover |
| **data[1]** | `i128` | `payout` — bond-token units paid out (capped at `TotalAssets`) |

```
topics : ("cover", <recipient: Address>)
data   : (intent_id: BytesN<32>, payout: i128)
```

---

#### `income_received`

Emitted by `receive_income` when the settlement contract deposits fee or slash
proceeds into the vault. Increases `TotalAssets`, appreciating all LP shares.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"income_received"` |
| **data** | `i128` | `amount` — bond-token units received |

```
topics : ("income_received",)
data   : <amount: i128>
```

---

### Admin Events

---

#### `emergency_mode`

Emitted by `set_emergency_mode` when the admin enables or disables the
emergency withdrawal-queue mode.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"emergency_mode"` |
| **data** | `bool` | `true` = emergency mode active (withdrawals queue); `false` = normal |

```
topics : ("emergency_mode",)
data   : <enabled: bool>
```

---

# Event Schema — `treasury_streams`

> Added in issue #519. Source of truth: `treasury_streams/src/lib.rs`.

`treasury_streams` manages timelocked token vesting streams for grants and
contributor payments. All amounts are `i128` in the token's smallest unit.
Stream IDs are monotonically increasing `u64` counters.

> **Note:** `treasury_streams` uses `soroban_sdk::symbol_short!` for the event
> name topic (max 9 bytes), so the symbol values are exactly `"create"`,
> `"withdraw"`, and `"cancel"` — not longer names.

---

## Event Catalogue

---

#### `create` (stream created)

Emitted by `create_stream` after the sender's tokens are escrowed.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"create"` |
| **topics[1]** | `u64` | `id` — the newly assigned stream ID |
| **data[0]** | `Address` | `sender` |
| **data[1]** | `Address` | `recipient` |
| **data[2]** | `Address` | `token` |
| **data[3]** | `i128` | `total` — total tokens locked in the stream |
| **data[4]** | `u64` | `start` — Unix timestamp when vesting begins |
| **data[5]** | `u64` | `cliff` — Unix timestamp before which nothing is withdrawable |
| **data[6]** | `u64` | `end` — Unix timestamp when vesting completes |

```
topics : ("create", <id: u64>)
data   : (sender: Address, recipient: Address, token: Address,
          total: i128, start: u64, cliff: u64, end: u64)
```

---

#### `withdraw` (stream withdrawal)

Emitted by `withdraw` after the recipient pulls currently-vested funds.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"withdraw"` |
| **topics[1]** | `u64` | `stream_id` |
| **data[0]** | `Address` | `recipient` |
| **data[1]** | `i128` | `claimable` — tokens transferred in this call |

```
topics : ("withdraw", <stream_id: u64>)
data   : (recipient: Address, claimable: i128)
```

---

#### `cancel` (stream cancelled)

Emitted by `cancel` after unvested tokens are returned to the sender. The
recipient retains the right to withdraw any already-vested remainder.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"cancel"` |
| **topics[1]** | `u64` | `stream_id` |
| **data[0]** | `Address` | `sender` — receives the unvested refund |
| **data[1]** | `i128` | `unvested` — tokens returned to the sender |

```
topics : ("cancel", <stream_id: u64>)
data   : (sender: Address, unvested: i128)
```

---

# Event Schema — `intent_periphery`

> Added in issue #519. Source of truth: `intent_periphery/src/lib.rs`.

`intent_periphery` is a read-only proxy that aggregates heavy view and
governance surfaces from the core `intent_settlement` contract. It emits only
two administrative events (it never moves funds).

---

## Event Catalogue

---

#### `periphery_initialized`

Emitted by `initialize` on first-time setup.

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"periphery_initialized"` |
| **data[0]** | `Address` | `admin` |
| **data[1]** | `Address` | `settlement` — the core contract this periphery reads from |

```
topics : ("periphery_initialized",)
data   : (admin: Address, settlement: Address)
```

---

#### `settlement_updated`

Emitted by `set_settlement` when the admin points the periphery at a new core
settlement contract (e.g. after a core upgrade).

| Field | Type | Description |
|-------|------|-------------|
| **topics[0]** | `Symbol` | `"settlement_updated"` |
| **data** | `Address` | `settlement` — the new settlement contract address |

```
topics : ("settlement_updated",)
data   : <settlement: Address>
```

---

# Event Schema — `governor`

> Source of truth: `governor/src/lib.rs`. Topics follow the conventions in
> [113-event-topic-naming-conventions.md](113-event-topic-naming-conventions.md).

| Topic tuple | Data shape | Emitting entrypoint | Emitting crate |
|---|---|---|---|
| `("init",)` | `(registry: Address, timelock: Address, voting_delay: u32, voting_period: u32, proposal_threshold: i128, quorum: i128)` | `initialize` | `governor` |
| `("cfg_upd",)` | same shape as `init` | `update_config` | `governor` |
| `("propose",)` | `(proposal_id: u32, proposer: Address, target: Address)` | `propose` | `governor` |
| `("proposal_canceled", proposal_id: u32)` | `caller: Address` | `cancel` | `governor` |
| `("vote",)` | `(proposal_id: u32, voter: Address, support: VoteType, power: i128)` | `cast_vote` | `governor` |
| `("queue",)` | `(proposal_id: u32, eta: u32)` | `queue` | `governor` |
| `("execute",)` | `(proposal_id: u32, target: Address, function: Symbol)` | `execute` | `governor` |

`expire_intent` coverage remains intentionally described in
[111-expire-intent-event-coverage.md](111-expire-intent-event-coverage.md).

---

# Event Schema — `fee_router`

> Added in issue #519. Source of truth: `fee_router/src/lib.rs`.

`fee_router` is implemented as a **pure Rust library crate** (not a Soroban
contract), so it does not emit Soroban events via `env.events().publish()`.
Revenue accounting (`on_fee_received`, `distribute`, `claim`), weight changes
(`propose_weights`, `apply_weights`), and error conditions are surfaced
exclusively through return values and the caller's own event layer.

If `fee_router` is later promoted to a standalone Soroban contract, this
section will be updated with its event schema.
