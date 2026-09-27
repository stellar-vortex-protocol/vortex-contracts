# Solver Bot Integration Guide

This guide explains how to build an off-chain solver bot that participates in
the Vortex Protocol intent market. It covers the full operational loop: startup
eligibility checks, event-driven intent discovery, the accept/fill submission
flow, and ongoing bond health monitoring.

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Protocol Overview](#protocol-overview)
3. [Registering as a Solver](#registering-as-a-solver)
4. [Startup Eligibility Check](#startup-eligibility-check)
5. [Subscribing to Intent Events](#subscribing-to-intent-events)
6. [The Accept → Fill Loop](#the-accept--fill-loop)
7. [Handling Edge Cases and Errors](#handling-edge-cases-and-errors)
8. [Bond Health Monitoring](#bond-health-monitoring)
9. [Full Operational Loop Reference](#full-operational-loop-reference)

---

## Prerequisites

- A funded Stellar account with enough USDC to post the minimum bond
  (50 USDC — `MIN_BOND = 50 * 10_000_000` stroops).
- Access to a Stellar RPC endpoint (Testnet or Mainnet).
- The deployed `intent_settlement` contract ID.
- Sufficient liquidity on the source chains you intend to serve (Ethereum, Base,
  etc.) to actually deliver fills.

---

## Protocol Overview

The intent lifecycle on-chain looks like this:

```
submit_intent()  →  [Open]
accept_intent()  →  [Accepted]  (5-minute fill window starts)
fill_intent()    →  [Filled]

If fill window expires without fill_intent():
slash_solver()   →  [Slashed, re-opened as Open]
```

Key constants baked into the contract:

| Constant         | Value     | Description                                        |
|------------------|-----------|----------------------------------------------------|
| `INTENT_EXPIRY`  | 1800 s    | How long an `Open` intent lives before expiring    |
| `FILL_WINDOW`    | 300 s     | Solver's exclusive fill window after `accept`      |
| `MIN_BOND`       | 50 USDC   | Minimum USDC bond to participate                   |
| `PROTOCOL_FEE`   | 0.05%     | Fee taken from `fill_amount` on each successful fill|

---

## Registering as a Solver

Before your bot can accept intents, it must be registered with a bond ≥ 50 USDC.
The bond is held as collateral: 10% is slashed for every fill window you miss.

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <SOLVER_SECRET_KEY> \
  --network testnet -- \
  register_solver \
  --solver <SOLVER_ADDRESS> \
  --bond_amount 500000000   # 50 USDC in stroops (7 decimal places)
```

You can top up an existing bond at any time by calling `register_solver` again
with a positive `bond_amount`. The contract checks that the *cumulative* total
meets `MIN_BOND`, so top-ups smaller than 50 USDC are accepted once you are
already above the threshold.

### Declaring served routes (optional)

If your bot only bridges specific `src_chain`/`dst_token` combinations, you can
advertise that on-chain via `set_solver_routes` so discovery tooling can filter
to solvers that actually service a given route. This is purely advisory:
`accept_intent` never enforces it, so you may still accept any intent you're
otherwise eligible for regardless of what you've declared.

```bash
stellar contract invoke --id <CONTRACT_ID> --source <SOLVER_SECRET_KEY> --network testnet -- \
  set_solver_routes \
  --solver <SOLVER_ADDRESS> \
  --src_chains '["ethereum","base"]' \
  --dst_tokens '["<USDC_SAC_ADDRESS>"]'
```

Never calling `set_solver_routes` (the default) reads back as "no declared
preference" — i.e. you're assumed to serve every route.

---

## Startup Eligibility Check

Every time your bot starts (or recovers from a crash), call `is_solver_eligible`
before entering the main loop. This single view encodes all the checks that
`accept_intent` enforces server-side, saving you a failed transaction and its fee.

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_KEY> \
  --network testnet -- \
  is_solver_eligible \
  --solver <SOLVER_ADDRESS>
```

Returns `true` when all three conditions hold:

1. The solver address is registered (`get_solver` returns `Some`).
2. `is_active == true` (not deactivated by a slash that dropped the bond below
   `MIN_BOND`).
3. `bond_amount >= MIN_BOND` (50 USDC).

If it returns `false`, call `get_solver` to diagnose which condition failed:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_KEY> \
  --network testnet -- \
  get_solver \
  --solver <SOLVER_ADDRESS>
```

The returned `SolverRecord` exposes:

| Field              | Type    | What to check                                  |
|--------------------|---------|------------------------------------------------|
| `bond_amount`      | i128    | Must be ≥ `500000000` (50 USDC)                |
| `is_active`        | bool    | Must be `true`; top up bond to reactivate      |
| `fills_completed`  | u32     | Running success count                          |
| `fills_failed`     | u32     | Running slash count                            |
| `active_intents`   | u32     | Open fill obligations right now                |

Also confirm the contract itself isn't paused before entering the loop:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_KEY> \
  --network testnet -- \
  is_paused
```

If `true`, the bot should wait and retry rather than attempt any state-changing
calls — `submit_intent`, `accept_intent`, and `fill_intent` all revert with
`ContractPaused (18)` when the contract is paused.

---

## Subscribing to Intent Events

The contract emits Soroban events for every state transition. Your bot should
subscribe to the ledger event stream and filter on `CONTRACT_ID` plus the topic
symbols listed below.

> **On-chain alternative for discovery (#249):** if all you need is *what's
> currently fillable*, you don't have to run a full event-replay index just to
> answer that question. Call `list_open_intents(offset, limit)` to page
> directly through the contract's own list of `Open`/`PartiallyFilled` intent
> IDs, bounded per call. Event subscription is still the right tool for
> reacting to state transitions in real time (fills, cancellations, slashes);
> `list_open_intents` is a cheaper way to bootstrap or periodically
> reconcile your local view of open opportunities.

### Event Topics

| Event symbol        | Emitted by         | Second topic (address) | Data value                                  |
|---------------------|--------------------|------------------------|---------------------------------------------|
| `intent_submitted`  | `submit_intent`    | user address           | `(intent_id: BytesN<32>, min_dst_amount: i128, deadline: u64)` |
| `intent_accepted`   | `accept_intent`    | solver address         | `(intent_id: BytesN<32>, fill_deadline: u64)`                  |
| `intent_filled`     | `fill_intent`      | solver address         | `(intent_id: BytesN<32>, fill_amount: i128, fee: i128)`        |
| `intent_cancelled`  | `cancel_intent`    | user address           | `intent_id: BytesN<32>`                                        |
| `intent_expired`    | `expire_intent`    | *(no second topic)*    | `intent_id: BytesN<32>`                                        |
| `solver_slashed`    | `slash_solver`     | solver address         | `(intent_id: BytesN<32>, slash_amount: i128)`                  |
| `solver_registered` | `register_solver`  | solver address         | `bond_amount: i128`                                            |

### Polling via Stellar RPC

If you don't have a streaming event subscription, poll for new ledgers and
call `getEvents` filtered to your contract:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "getEvents",
  "params": {
    "startLedger": <LAST_SEEN_LEDGER>,
    "filters": [
      {
        "type": "contract",
        "contractIds": ["<CONTRACT_ID>"],
        "topics": [["*", "*"]]
      }
    ]
  }
}
```

Filter client-side on `topic[0] == "intent_submitted"` to detect new intents.

### What to do on `intent_submitted`

When you see an `intent_submitted` event, fetch the full record to evaluate
profitability:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_KEY> \
  --network testnet -- \
  get_intent \
  --intent_id <INTENT_ID_HEX>
```

The `IntentRecord` fields your bot needs for quoting:

| Field            | Meaning                                           |
|------------------|---------------------------------------------------|
| `src_chain`      | Source chain (`"ethereum"`, `"base"`, `"solana"`, …) |
| `src_token`      | Token address on the source chain (see per-chain formats below) |
| `src_amount`     | Amount to bridge, in the source token's smallest unit |
| `dst_token`      | SAC/SEP-41 address you must deliver on Stellar    |
| `min_dst_amount` | Minimum amount the user will accept               |
| `deadline`       | Unix timestamp; intent is worthless after this    |
| `state`          | Must be `Open` before you can accept              |

Reject the intent immediately if:
- `state != Open`
- `deadline - now < FILL_WINDOW` (not enough time to accept + fill)
- Your quoted cost exceeds `min_dst_amount + fee` (unprofitable)

#### Interpreting `src_token` / `src_amount` per source chain

`src_amount` is always `human_amount × 10^decimals` in the source token's
smallest unit — but `decimals` and the `src_token` string differ by chain:

| `src_chain` | `src_token` format | How to get `decimals` |
|---|---|---|
| EVM (`ethereum`, `base`, `polygon`, `arbitrum`, `optimism`, `avalanche`, `bsc`) | `0x` + 40 hex chars | `decimals()` view on the ERC-20; usually 18 (native) / 6 (stablecoins), **but 18 for USDT/USDC on BSC** |
| `solana` | base58 SPL **mint address**, 32–44 chars, no `0x` | `decimals` field of the mint account (`getMint` / `getTokenSupply`). **Not uniform:** USDC/USDT = 6, wrapped SOL and most LSTs = 9, BONK = 5 |

For a Solana-sourced intent your bot must:
1. Treat `src_token` as an SPL mint address — resolve it against your Solana
   RPC / token list, not an EVM registry.
2. Fetch that mint's `decimals` (do **not** assume 6) to convert `src_amount`
   back to a human amount for quoting.
3. Price and perform the source-chain leg on Solana (transfer the SPL token
   from the user's escrow), exactly as you would the EVM leg — the Stellar
   contract does not verify it; your bond is the guarantee (see Step 2).

See [docs/132-supported-chains.md](./132-supported-chains.md) §3.2 and §4.8 for
the base58 rules, sample mints, and decimals.

---

## The Accept → Fill Loop

> **Scoped authorization (2026 update):** `accept_intent` and `fill_intent`
> now call `require_auth_for_args` instead of `require_auth`, scoped to
> `(intent_id)` and `(solver, intent_id, fill_amount)` respectively (see
> `docs/auth-audit.md`). If you invoke directly via `stellar contract invoke`
> or the standard SDK contract client with your own solver key, this is
> transparent — the simulated auth entries are signed for you as before. It
> only matters if you construct and sign `SorobanAuthorizationEntry` values
> by hand (e.g. for a delegated/invoker-contract flow): the signed payload
> must now match the specific call's arguments, not just the function name.

### Step 1 — Accept

Call `accept_intent` to claim the exclusive 5-minute fill window:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <SOLVER_SECRET_KEY> \
  --network testnet -- \
  accept_intent \
  --solver <SOLVER_ADDRESS> \
  --intent_id <INTENT_ID_HEX>
```

On success:
- The intent transitions from `Open` → `Accepted`.
- `intent.deadline` is reset to `now + 300` (5 minutes).
- `solver_record.active_intents` is incremented — your bond is now backing this obligation.
- An `intent_accepted` event is emitted with the new fill deadline.

On failure, the contract returns one of:

| Error code | Name                  | Recovery action                                   |
|------------|-----------------------|---------------------------------------------------|
| 3          | `IntentNotFound`      | Intent ID is wrong; discard.                      |
| 4          | `IntentNotOpen`       | Another solver got there first; move on.          |
| 5          | `IntentExpired`       | Deadline passed; discard.                         |
| 7          | `SolverNotRegistered` | Bot is misconfigured; run eligibility check.      |
| 12         | `SolverInactive`      | Bond fell below MIN_BOND after a slash; top up.   |
| 18         | `ContractPaused`      | Protocol is paused; wait for `unpause` event.     |

### Step 2 — Execute the source-chain side

After `accept_intent` succeeds, initiate the source-chain transfer
(e.g., release the user's locked ETH). The on-chain Stellar contract does not
verify the source-chain tx — your bond is the economic guarantee.

You have 5 minutes (`FILL_WINDOW = 300 s`). Budget your cross-chain execution
time conservatively; chain congestion can eat into that window.

### Step 3 — Fill

Once you have confirmed delivery (or are about to deliver) on the source chain,
call `fill_intent` to transfer `dst_token` from your Stellar account to the user:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <SOLVER_SECRET_KEY> \
  --network testnet -- \
  fill_intent \
  --solver <SOLVER_ADDRESS> \
  --intent_id <INTENT_ID_HEX> \
  --fill_amount <AMOUNT_IN_STROOPS>
```

What the contract does internally:
1. Transfers `fill_amount` of `dst_token` from the solver's Stellar account to
   the user — your Stellar account must have approved the contract or hold
   sufficient balance.
2. Transfers `fill_amount * 5 / 10_000` (0.05%) of `dst_token` to the protocol
   fee recipient — price this into your quote.
3. Marks the intent `Filled`, decrements `active_intents`, increments
   `fills_completed`, and updates cumulative `total_volume`.

`fill_amount` must be ≥ `min_dst_amount`. Passing exactly `min_dst_amount` is
valid; passing more improves your reputation stats.

**Important**: Before calling `fill_intent`, ensure your Stellar keypair has
authorized the contract to transfer at least `fill_amount + fee` of `dst_token`
on your behalf (via SEP-41 `approve`, or by signing the fill transaction that
includes the token transfer as a sub-invocation).

On failure:

| Error code | Name                  | Recovery action                                                  |
|------------|-----------------------|------------------------------------------------------------------|
| 9          | `InsufficientOutput`  | Your `fill_amount` is below `min_dst_amount`; increase amount.  |
| 10         | `FillWindowExpired`   | You missed the 5-minute window; `slash_solver` can now be called.|
| 15         | `IntentAlreadyFilled` | Duplicate submission; safe to ignore.                           |
| 2          | `Unauthorized`        | Solver address mismatch; check your keypair config.             |

---

## Handling Edge Cases and Errors

### Missed fill window

If your bot crashes, loses connectivity, or the source-chain tx stalls and you
miss the `FILL_WINDOW`:

1. `slash_solver` becomes callable by anyone — you will lose 10% of your bond.
2. The intent reverts to `Open` with a fresh 30-minute deadline, and a new
   solver can accept it.
3. If the slash drops your bond below `MIN_BOND`, `is_active` is set to `false`
   and you must top up before accepting new intents.
4. A slash also starts a `SLASH_COOLDOWN` (1 hour) during which `accept_intent`
   rejects you even if your bond is healthy. Call `get_slash_cooldown_remaining`
   to find out exactly how many seconds are left, instead of guessing or
   reimplementing the cooldown arithmetic yourself.

To recover:

```bash
# Check your current bond status
stellar contract invoke --id <CONTRACT_ID> --source <ANY_KEY> --network testnet -- \
  get_solver --solver <SOLVER_ADDRESS>

# Check whether you're still inside the post-slash cooldown window
stellar contract invoke --id <CONTRACT_ID> --source <ANY_KEY> --network testnet -- \
  get_slash_cooldown_remaining --solver <SOLVER_ADDRESS>

# Top up to re-activate (must bring total back to ≥ MIN_BOND)
stellar contract invoke --id <CONTRACT_ID> --source <SOLVER_SECRET_KEY> --network testnet -- \
  register_solver --solver <SOLVER_ADDRESS> --bond_amount <TOP_UP_AMOUNT>

# Confirm you're eligible again (only true once the cooldown above is 0)
stellar contract invoke --id <CONTRACT_ID> --source <ANY_KEY> --network testnet -- \
  is_solver_eligible --solver <SOLVER_ADDRESS>
```

If your bot crashes mid-fill-window and comes back up not knowing what it was
working on, call `get_solver_intents` to rediscover every `intent_id` you
currently hold `Accepted`, instead of replaying events since registration:

```bash
stellar contract invoke --id <CONTRACT_ID> --source <ANY_KEY> --network testnet -- \
  get_solver_intents --solver <SOLVER_ADDRESS>
```

### Concurrent intent acceptance

Your bot may see multiple `intent_submitted` events in the same ledger window.
Use separate worker threads/coroutines per intent, but enforce a local cap on
`active_intents` to keep your total fill obligation bounded relative to your
bond. There is no on-chain cap — the contract tracks `active_intents` per solver
but does not prevent over-commitment.

### Re-opened intents (after slash)

When `solver_slashed` fires, the affected intent returns to `Open` with a fresh
deadline. Your event loop will see this as a new opportunity if you filter on
`intent_submitted`, but that event is not re-emitted — filter on `solver_slashed`
as well, then call `get_intent` to re-evaluate profitability.

### Contract paused

If `is_paused` returns `true`:
- Do not attempt `accept_intent` or `fill_intent` — both will revert.
- Watch for the `paused` event with data `false` (an `unpause` call), then
  resume normal operation.
- `slash_solver` remains callable during a pause, so monitor your open
  `Accepted` intents and factor in whether a pause makes your fill impossible.

---

## Bond Health Monitoring

Your bot should poll `get_solver` periodically (every few minutes) and alert if:

- `bond_amount` drops below `MIN_BOND * 2` — you're one slash away from deactivation.
- `is_active == false` — you've been deactivated and cannot accept new intents.
- `fills_failed / (fills_completed + fills_failed) > threshold` — fill failure
  rate is trending up; investigate source-chain execution.

Maintaining a healthy bond buffer (e.g., 2–5× `MIN_BOND`) also gives you room
to take on more simultaneous intents without risking deactivation from a single
missed fill.

To withdraw excess bond without fully deregistering:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <SOLVER_SECRET_KEY> \
  --network testnet -- \
  withdraw_bond \
  --solver <SOLVER_ADDRESS> \
  --amount <WITHDRAWAL_AMOUNT>
```

The remaining bond must stay ≥ `MIN_BOND`. To fully exit, call
`deregister_solver` — but only when `active_intents == 0`.

---

## Full Operational Loop Reference

```
BOT STARTUP
  └── is_solver_eligible?  →  No  →  register_solver / top-up bond
                           →  Yes
  └── is_paused?           →  Yes →  wait for unpause event
                           →  No
  └── enter main loop

MAIN LOOP (each new ledger)
  ├── poll getEvents for contract
  │     ├── intent_submitted  →  get_intent → evaluate → queue for accept
  │     ├── solver_slashed    →  get_intent → re-evaluate re-opened intent
  │     └── paused (true)     →  pause bot, watch for paused (false)
  │
  ├── for each queued intent
  │     ├── accept_intent
  │     │     ├── success  →  start fill timer, execute src-chain tx
  │     │     └── error    →  log + discard (IntentNotOpen/Expired/etc.)
  │     └── (async) fill_intent within FILL_WINDOW
  │           ├── success  →  log fill, decrement active count
  │           └── error FillWindowExpired  →  log, expect slash event
  │
  └── periodic health check (every N ledgers)
        ├── is_solver_eligible?  →  alert + remediate if false
        ├── bond_amount check    →  alert if < 2 × MIN_BOND
        └── is_paused?           →  alert if true unexpectedly
```
