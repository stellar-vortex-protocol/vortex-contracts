# Storage TTL Constants — Rationale

This document explains the four TTL constants defined at the top of
`intent_settlement/src/lib.rs` (lines 27–35), why their specific values were
chosen, and the trade-offs involved for anyone tuning them.

---

## Background: Soroban State Archival

Soroban (Stellar's smart-contract platform) distinguishes between two storage
tiers that matter here:

- **Persistent storage** — used for `Intent` and `Solver` records. Entries
  survive indefinitely if their TTL is extended before it expires; otherwise
  they are archived (removed from the active ledger state) and can only be
  accessed again after an explicit restore operation.
- **Instance storage** — a single ledger entry that holds the contract's
  global state (`Admin`, `FeeRecipient`, `BondToken`, protocol stats) *and*
  the contract's own executable code. If this entry archives, the entire
  contract becomes unreachable until restored.

Both storage tiers measure TTL in **ledgers**, not seconds.

---

## The `DAY_IN_LEDGERS` Assumption

```rust
const DAY_IN_LEDGERS: u32 = 17280; // ~5s per ledger
```

Stellar mainnet targets a ledger close time of approximately 5 seconds.
One day therefore corresponds to:

```
86400 s/day ÷ 5 s/ledger = 17280 ledgers/day
```

This is the baseline from which all other TTL constants are derived. It is
a *target*, not a guarantee — actual close times vary with network load —
but it is the standard assumption used across the Soroban ecosystem.

---

## Persistent Storage Constants (Intent and Solver Records)

```rust
const PERSISTENT_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 14; // ~14 days
const PERSISTENT_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 30; // ~30 days
```

### How they work together

On every write to a `Intent` or `Solver` entry, the contract calls
`extend_ttl(PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO)`.

Soroban's `extend_ttl` only extends if the current remaining TTL is *below*
the threshold. This avoids redundant ledger writes on entries that were
recently extended. Concretely:

- If an entry has fewer than 14 days of TTL remaining, extend it to 30 days.
- If it already has more than 14 days remaining, do nothing.

### Why 14 days as the threshold?

An intent's maximum active lifespan is capped by `INTENT_EXPIRY` (30 minutes)
plus one `FILL_WINDOW` (5 minutes). Even accounting for re-opens after a slash
(each granting another 30-minute window), an intent cannot remain in an active
state for more than a few hours under normal circumstances.

14 days is therefore a very conservative floor: any entry that is still being
accessed (written to) within a 14-day window is by definition still relevant to
active protocol activity, and the cost of extending it is justified. An entry
that has not been written to for 14 days is either:

1. In a terminal state (`Filled`, `Cancelled`, `Expired`, `Slashed`) and
   unlikely to need further on-chain reads; or
2. Genuinely abandoned and can safely archive.

Solver records have a longer natural activity cycle (a solver might be dormant
between market opportunities for days), so 14 days also comfortably covers
typical inactivity windows without requiring constant top-up transactions.

### Why 30 days as the extend-to target?

Extending to 30 days on each write means that even if a record is never touched
again after the last write, it remains accessible for up to 30 days. This
provides:

- **Archive-risk buffer**: front-ends and indexers querying historical intent
  data have a full month to read records before they archive.
- **Cost proportionality**: extending to 30 days from a 14-day threshold means
  at most ~16 days of "paid-for but potentially unneeded" TTL per write — a
  small overhead relative to the per-byte ledger rent costs.
- **Operational headroom**: in incident scenarios (contract paused, indexer
  outage), 30 days provides enough time for operators to react without records
  disappearing.

---

## Instance Storage Constants (Contract Instance Entry)

```rust
const INSTANCE_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 30; // ~30 days
const INSTANCE_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 60; // ~60 days
```

### Why are these higher than the persistent constants?

The contract instance entry is special: if it archives, the *entire contract*
becomes unreachable. Every public function calls `bump_instance_ttl`, so the
instance TTL is refreshed on every state-changing transaction. However, the
consequences of it ever archiving are far more severe than a single Intent or
Solver record archiving, so the safety margins are larger.

### Why 30 days as the threshold?

If the contract goes completely dormant (no transactions for any reason), the
instance entry must still survive long enough for operators to notice and either
submit a transaction or restore it. 30 days gives a full calendar month of
dormancy tolerance before the extension is triggered.

On an active deployment the threshold will never actually be reached (transactions
happen many times per day), so this is purely a safety floor for the worst case.

### Why 60 days as the extend-to target?

Extending to 60 days means even zero activity for an entire month will not
threaten the instance entry for another full month after that. This 2× ratio
(threshold = ½ × extend-to) mirrors the pattern of the persistent constants
and gives the same cost-proportionality property: on every write, the overhead
is at most ~30 days of "extra" TTL.

---

## Cost vs. Archival-Risk Trade-off Summary

| Constant                    | Value      | Rationale                                                        |
|-----------------------------|------------|------------------------------------------------------------------|
| `PERSISTENT_TTL_THRESHOLD`  | 14 days    | Conservative inactivity floor; covers dormant solvers            |
| `PERSISTENT_TTL_EXTEND_TO`  | 30 days    | ~1 month buffer; reasonable archive-risk/rent-cost balance       |
| `INSTANCE_TTL_THRESHOLD`    | 30 days    | Full calendar month of dormancy tolerance for the contract itself |
| `INSTANCE_TTL_EXTEND_TO`    | 60 days    | 2-month buffer; high safety margin justified by catastrophic consequence of archiving |

If you are deploying in an environment with significantly different ledger close
times or different rent cost structures, recalculate `DAY_IN_LEDGERS` first and
then re-evaluate the day multipliers using the same logic above.

Raising `PERSISTENT_TTL_EXTEND_TO` increases ledger rent costs linearly.
Lowering `PERSISTENT_TTL_THRESHOLD` increases the frequency of TTL-extension
writes (each write bumps TTL, so more writes happen when the threshold is lower
relative to the extend-to target). The values chosen aim for a sensible middle
ground on Stellar mainnet pricing as of the contract's initial deployment.

---

## Previously Unmanaged Persistent Keys (fixed in issue #271)

The four keys below were written via `env.storage().persistent()` but never
had their TTL bumped — a gap that would have caused silent correctness failures
as entries gradually aged toward Soroban's state-archival threshold.  The same
`PERSISTENT_TTL_THRESHOLD` / `PERSISTENT_TTL_EXTEND_TO` constants apply to all
four, for the same reasons documented above.

### `DataKey::CancelCooldown(Address)`

**Written by:** `cancel_intent`  
**Read by:** `cancel_intent` (spam guard at the top of the function)  
**Failure mode without TTL management:** Once archived, the entry reads as
absent. `cancel_intent` treats absence as "user has never cancelled", silently
resetting the `CANCEL_COOLDOWN` delay and allowing the user to cancel at full
rate again after any sufficiently long period of inactivity. The spam-deterrence
mechanism is defeated without any error surfacing to the protocol.

**Fix:** `bump_cancel_cooldown_ttl` is called immediately after every
`CancelCooldown` write in `cancel_intent`.

**Why the same constants?** The cooldown window (`CANCEL_COOLDOWN = 60 s`) is
far shorter than the archival window, so the archival risk is not about the
cooldown expiring — it is about the *record of the cooldown* expiring.  The
14-day threshold is comfortably above any realistic gap between a user's
`cancel_intent` calls during normal protocol activity.

---

### `DataKey::MinBondMultiplier(Address)`

**Written by:** `set_min_bond_multiplier` (admin-only)  
**Read by:** `get_adjusted_min_bond` (called from `accept_intent`)  
**Failure mode without TTL management:** Once archived, the entry reads as
absent. `get_adjusted_min_bond` treats absence as the 1.0× default, silently
reverting the token's bond requirement to the minimum floor even if the admin
had deliberately set a higher multiplier for a higher-risk token. Solvers can
then accept intents against that token with an under-sized bond, reducing the
protocol's collateral guarantee precisely where the admin intended to increase
it.

**Fix:** `bump_min_bond_multiplier_ttl` is called immediately after every
`MinBondMultiplier` write in `set_min_bond_multiplier`.

**Why the same constants?** Multipliers are admin-configured and not
continuously refreshed by routine protocol activity.  Without a TTL bump they
would archive after approximately the minimum Soroban persistent-TTL (~17 days
at the time of writing).  The 30-day extend-to target comfortably exceeds this
floor, giving admins a full month of headroom between re-configuring the same
token.

---

### `DataKey::ExtensionGranted(BytesN<32>)`

**Written by:** `request_extension`  
**Read by:** `request_extension` (via `has`, one-shot guard)  
**Failure mode without TTL management:** Once archived, `has` returns `false`.
A solver whose extension flag archived can call `request_extension` again on the
same intent and receive a second fill-window extension, bypassing the one-per-
intent constraint.  This could be exploited to repeatedly extend a fill window
without the corresponding bond risk the protocol intends.

**Fix:** `bump_extension_granted_ttl` is called immediately after every
`ExtensionGranted` write in `request_extension`.

**Why the same constants?** The flag only needs to survive as long as the intent
it protects.  Intents themselves are bumped with the same constants by
`bump_intent_ttl`, so aligning `ExtensionGranted` to the same window ensures the
flag and the intent archive together (if at all).

---

### `DataKey::UserIntents(Address)`

**Written by:** `submit_intent`  
**Read by:** `list_intents_by_user` (public view)  
**Failure mode without TTL management:** Once archived, `list_intents_by_user`
returns `unwrap_or_else(|| Vec::new(&env))` — an empty list — silently truncating
the user's intent history at the point the entry archived. Front-ends and
indexers calling this view would present incomplete history without any error,
making the gap invisible to end users.

**Fix:** `bump_user_intents_ttl` is called immediately after every `UserIntents`
write in `submit_intent`.

**Why the same constants?** The list can grow without bound as a user submits
more intents over time; it should survive at least as long as any individual
`IntentRecord` it references.  Using the same 14-day threshold / 30-day extend-to
ensures both the list and its constituent records remain accessible for the same
window, so a `list_intents_by_user` call that returns an ID will always be
followed by a successful `get_intent` call on that ID.

---

## Cost Analysis for the Four New Keys

All four keys use the same `extend_ttl` call pattern as `bump_intent_ttl` and
`bump_solver_ttl`.  The per-call cost is one Soroban ledger read + conditional
write (Soroban only writes if the remaining TTL is below the threshold).  On a
hot path (`submit_intent`, `accept_intent`, `cancel_intent`) this adds at most
one conditional persistent-storage write — comparable to what the existing
`bump_intent_ttl` / `bump_solver_ttl` calls already incur, and negligible
relative to the `IntentRecord` / `SolverRecord` reads.

`MinBondMultiplier` is written only by the admin and is read once per
`accept_intent`; the bump cost is confined to the infrequent admin call.
`CancelCooldown` and `UserIntents` are written on user-initiated paths
(`cancel_intent`, `submit_intent`) that already pay for `IntentRecord` I/O.
`ExtensionGranted` is written once per intent for the rare extension path.

None of the four keys are expected to materially affect the wasm-size or
resource-fee budget beyond the already-established TTL-management overhead.
