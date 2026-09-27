# Design Doc: `expire_intent`'s Event Coverage vs. the Passive Read Path

**Issue:** [#111](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/111)
**Branch:** `feat/ops-monitoring-and-health-check`
**Status:** Confirmed — gap documented, no contract change required; dead writes cleaned up

---

## 1. The Question

`expire_intent` (`lib.rs:1449-1479`) emits `intent_expired` only when it is
actually called and successfully transitions an intent's stored `state`
from `Open`/`PartiallyFilled` to `Expired`. The question: **can an indexer
that relies purely on events (never calling `get_intent`) distinguish a
materialized `Expired` state from an intent that is simply `Open` and past
its deadline but not yet materialized?**

**Confirmed: no, not from topic matching alone.** A pure event-stream
indexer sees no event at all for the "past deadline, not yet materialized"
case — because no transaction has occurred yet to emit one. It can only
reconstruct this state by also tracking numeric payload fields already
present in earlier events and comparing them to wall-clock time itself.
Details and the exact mechanism follow.

---

## 2. Why the Gap Exists

`IntentRecord.state` is mutated only inside functions that run as
transactions. Nothing mutates it, and no event fires, purely as a result of
time passing:

- `submit_intent` sets `state: Open` and stores `deadline: expiry`
  (`lib.rs:1027-1039`), emitting `intent_submitted` with payload
  `(intent_id, min_dst_amount, expiry)` (`lib.rs:1069-1072`).
- `accept_intent` checks `now >= intent.deadline` and, if the deadline has
  passed, **panics with `Error::IntentExpired`** (`lib.rs:1118-1124`).
  Because Soroban discards all state writes from a panicking invocation,
  the `state` field is *not* actually updated to `Expired` here, and no
  event is emitted — the transaction simply fails. (The preceding
  `.set()`/`bump_intent_ttl` calls just before the `panic_with_error!` were
  dead writes for the same reason; they have since been removed and replaced
  with an explanatory comment — see the cleanup in `lib.rs`'s
  `accept_intent` expiry branch.)
- `expire_intent` is the **only** function that durably writes
  `state: Expired` and emits `intent_expired` — and only when someone
  calls it (it's permissionless, but still requires a submitted
  transaction).

So between the moment `now >= deadline` becomes true and the moment someone
calls `expire_intent`, the intent is in a state this doc calls
**"logically expired, not yet materialized"**: `get_intent` still returns
`state: Open` (or `PartiallyFilled`), and no event has fired to say
otherwise, because none has occurred.

---

## 3. What a Pure-Event Indexer *Can* Reconstruct

Although no dedicated event marks the logically-expired-but-unmaterialized
transition, the indexer isn't fully blind to it — the deadline itself is
already present in event payloads it would already be tracking:

- `intent_submitted` payload includes `expiry` (`lib.rs:1071`) — the
  original deadline.
- `intent_accepted` payload includes the **updated** `deadline`
  (`lib.rs:1148`) — `accept_intent` extends it to `now + fill_window`
  (`lib.rs:1134`), so the live deadline changes over an intent's lifetime
  and the indexer must track the latest one, not just the first.

So a pure-event indexer *can* derive "logically expired, not yet
materialized" for a given `intent_id` as:

```
last_known_deadline = deadline from the most recent of
  {intent_submitted, intent_accepted} seen for this intent_id
no terminal event has been seen for this intent_id yet
  (i.e. none of intent_filled / intent_cancelled / intent_expired
  / solver_slashed has occurred)
current_time >= last_known_deadline
```

This is *derivable*, but it is not the same as *distinguishable from
events alone* in the sense the issue asks about — it requires the indexer
to hold per-intent state and do its own time comparison, not just switch
on topic name. An indexer that only counts/reacts to event topics (no
payload decoding, no clock) cannot tell the two cases apart, because the
unmaterialized case produces literally zero events.

---

## 4. Practical Implication

Any indexer or dashboard that reports "open intents" needs to treat these
as two distinct states even though the contract's own `IntentState` enum
has only one (`Open`) for both:

| On-chain `state` (via `get_intent`) | Materialized via event? | What it actually means |
|---|---|---|
| `Open`, `now < deadline` | n/a | Genuinely open, fillable |
| `Open`, `now >= deadline` | No — no event exists for this | Logically expired, awaiting someone to call `expire_intent` |
| `Expired` | Yes — `intent_expired` | Materialized, permanently terminal |

Consumers that need an accurate "is this intent still actionable" signal
without polling `get_intent` per-intent must compute row 2 themselves from
tracked deadlines, as in §3 — they cannot wait for an event that will never
come unless someone calls `expire_intent`.

This also means `expire_intent` is not just a bookkeeping nicety: until it
is called, the intent continues to occupy the "open" bucket in any
purely-materialized-state view (including a future aggregate view — see
[#112](../intent_settlement/src/lib.rs)'s `get_protocol_health`, which
reports storage-level counts, not deadline-adjusted ones). Ops tooling
that watches expiry-rate as a health signal (see
[#110](110-monitoring-alerting-spec.md) §3.3) should keep this distinction
in mind: a low `intent_expired` event rate does not by itself mean few
intents are going unfilled — it may mean `expire_intent` simply isn't being
called promptly.

---

## 5. Recommendation

No contract change is required — `expire_intent`'s existing single-event
model (`intent_expired` on materialization) is sufficient as long as
consumers are aware of the distinction in §4. Recommendations:

1. **Document this behavior for indexer authors** (this doc) rather than
   changing the contract, since the deadline data needed to derive the
   logically-expired case is already present in `intent_submitted` /
   `intent_accepted` payloads — no new event is needed to make the
   information available, just correct handling on the consumer side.
2. Any off-chain "intent watcher" service that surfaces logically-expired
   intents to users/ops (e.g. to prompt someone to call `expire_intent`
   and free up solver bond accounting) should compute row 2 from §3, not
   wait on a contract event.
3. If a future need arises for the contract itself to report
   deadline-adjusted counts (e.g. "open and still live" vs. "open but
   logically expired") as a view rather than requiring indexer-side
   computation, that would be a separate, explicitly-scoped follow-up —
   not bundled into this confirmation.

---

*Closes #111*

---

## 6. Code Cleanup (dead write removal)

The `.set()`/`bump_intent_ttl` calls immediately before `panic_with_error!`
in `accept_intent`'s expiry branch — identified as dead writes in §2 above —
have been removed.  They were replaced with an inline comment explaining that
Soroban discards all storage mutations from a panicking invocation, so no write
is possible at that call site.

A regression test (`accept_expired_intent_state_unchanged` in `test.rs`)
was added to make the expected observable behavior explicit: after a failed
`accept_intent` call on a past-deadline intent, `get_intent` must still return
`state: Open` (not `Expired`) and `solver: None`, confirming no partial write
committed.  The test also guards against the dead-write pattern being
reintroduced accidentally in the future.
