# Design Doc: Event Topic Naming Conventions for Future Contracts

**Issue:** [#113](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/113)
**Branch:** `feat/ops-monitoring-and-health-check`
**Status:** Convention adopted — applies to new contracts (`solver_registry`, etc.) going forward

---

## 1. Problem Statement

`solver_registry` and other planned contracts will share indexer tooling
with `intent_settlement`. If each contract picks event topic naming ad hoc
— verb tense, `Symbol` vs. `String`, topic ordering — the shared indexer
ends up special-casing every contract instead of applying one consistent
parsing rule. This doc audits how `intent_settlement` currently names its
event topics, calls out the one real inconsistency found, and defines the
convention future contracts should follow.

---

## 2. Audit of Current `intent_settlement` Events

All 21 event topics currently published, in source order:

| Topic | Extra topic segment | Data payload |
|---|---|---|
| `fee_recipient_proposed` | — | new fee recipient |
| `fee_recipient_updated` | — | new fee recipient |
| `admin_transferred` | — | new admin |
| `config_updated` | — | new `ProtocolConfig` |
| `dst_token_allowed` | — | token |
| `dst_token_disallowed` | — | token |
| `bond_multiplier_set` | — | (token, multiplier) |
| `src_chain_allowed` | — | chain |
| `src_chain_disallowed` | — | chain |
| `paused` (payload `true`) | — | `true` |
| `paused` (payload `false`) | — | `false` |
| `tokens_rescued` | `to` (Address) | (token, amount) |
| `solver_registered` | `solver` (Address) | bond info |
| `solver_deregistered` | `solver` (Address) | — |
| `bond_withdrawn` | `solver` (Address) | amount |
| `intent_submitted` | `user` (Address) | (intent_id, min_dst_amount, expiry) |
| `intent_accepted` | `solver` (Address) | (intent_id, deadline) |
| `intent_filled` | `solver` (Address) | fill details |
| `intent_cancelled` | `user` (Address) | intent_id |
| `solver_slashed` | `solver` (Address) | (intent_id, slash_amount) |
| `intent_expired` | — | intent_id |
| `extension_granted` | `solver` (Address) | new deadline |

### 2.1 Verb tense

Every topic in the contract is a **past-tense verb + noun**, e.g.
`solver_registered`, `intent_filled`, `admin_transferred` — describing a
state transition that already happened, not an imperative or a bare noun.
This is consistent across all 21 topics and is the convention to keep.

The one apparent outlier, `paused`, is still past tense (`pause` → `paused`),
so it fits the verb-tense rule. Its actual problem is topic reuse (§2.3).

### 2.2 Symbol vs. string

Every topic is constructed with `Symbol::new(&env, "...")` — none use
`String`. This is correct and should stay the rule: topic values are
short, fixed, ASCII identifiers known at compile time, which is exactly
what `Symbol` is for. `String` is reserved for *data* payload fields that
hold genuinely variable text (e.g. `src_chain` inside `IntentRecord`).

### 2.3 Topic ordering and the `paused` inconsistency

The convention used everywhere else in the contract is: **one event name
per distinct occurrence.** `dst_token_allowed` / `dst_token_disallowed` and
`src_chain_allowed` / `src_chain_disallowed` both follow this — two
distinct topics for the two distinct transitions, rather than one shared
topic disambiguated by payload.

`pause()` and `unpause()` break this pattern: both publish the **same**
topic, `Symbol::new(&env, "paused")`, distinguished only by a boolean data
payload (`true` for pause, `false` for unpause). An indexer that filters
purely on topic name — which is the fast path most indexers take — cannot
tell a pause from an unpause without also decoding and branching on the
payload. This is the one real inconsistency in the current contract.

**This doc does not change `intent_settlement`'s existing `paused` topic.**
Renaming it is a breaking change for any indexer already tracking it, and
should ship as its own deliberate, versioned change (tracked in
[#110](110-monitoring-alerting-spec.md) §5), not silently bundled into a
docs-only issue. It's called out here so future contracts don't repeat it.

### 2.4 Indexed segments (the second topic slot)

Functions that act on behalf of, or primarily concern, one address include
that address as a second topic segment — e.g. `(solver_registered, solver)`,
`(intent_submitted, user)`. This lets an indexer subscribe to
"all events concerning address X" via topic filtering alone, without
decoding every event body first. Global/admin-level events
(`config_updated`, `admin_transferred`, `dst_token_allowed`, `paused`, …)
correctly omit a second topic segment since there's no single subject
address to index by.

---

## 3. Convention for Future Contracts

1. **Topic name = past-tense verb phrase**, `snake_case`, describing the
   state transition that just committed (`solver_slashed`, not `slash` or
   `slashing`). One event name per distinct logical occurrence — do not
   reuse one topic name for two different outcomes disambiguated only by
   payload (the mistake in §2.3).
2. **Always `Symbol::new(&env, "...")`** for the topic value itself, never
   `String`. Keep topic strings short (Soroban `Symbol` has a length
   limit) and stable once published — treat a topic rename as a breaking
   change requiring a migration note.
3. **Topic tuple shape**: `(Symbol, [Address])` — the event name first,
   followed by the single most relevant indexed `Address` (the user,
   solver, or admin the event concerns) if one exists. Omit the second
   slot for protocol-global events with no single subject address.
4. **Payload = everything else the event needs to convey** (ids, amounts,
   before/after values), as a plain tuple in call order matching the
   function's own parameter order where practical, so indexers don't have
   to memorize a bespoke field order per event.
5. **Every new event gets a one-line rustdoc comment above the `publish`
   call** stating what triggers it and what its payload fields mean, in
   the same style already used for `DataKey` variants and structs in
   `intent_settlement`.

`solver_registry` and any other new Vortex contract should follow this
convention from their first commit rather than reconciling divergent event
shapes after the fact once shared indexer tooling depends on them.

---

*Closes #113*
