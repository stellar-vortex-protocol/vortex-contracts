# Design Doc: Monitoring & Alerting Spec for Ops

**Issue:** [#110](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/110)
**Branch:** `feat/ops-monitoring-and-health-check`
**Status:** Design complete — spec ready for ops tooling implementation

---

## 1. Problem Statement

`intent_settlement` exposes `pause()` specifically so an operator can halt
`submit_intent` / `accept_intent` / `fill_intent` during an incident, but
there is no documented set of signals that would tell an operator an
incident is happening in the first place. Today, detecting a problem
depends on someone noticing manually. This doc defines the signals an ops
team should watch, where each one comes from (event vs. view), and the
conditions that should page someone.

---

## 2. Signal Sources

Two complementary sources feed monitoring:

- **Events** (`env.events().publish(...)`) — real-time, but only fire when
  a transaction actually executes. A condition that never triggers a
  transaction (e.g. an intent quietly sitting past its deadline with no one
  calling `expire_intent`) produces **no event**. See
  [#111](111-expire-intent-event-coverage.md) for the specific gap around
  intent expiry.
- **Views** (`get_stats`, `is_paused`, `get_solver_count`,
  `get_protocol_health` — see [#112](../intent_settlement/src/lib.rs)) —
  point-in-time state, good for reconciliation and dashboards, but require
  polling and can't alone tell you *when* something changed.

Ops tooling should run both: an event-stream listener for real-time paging,
and a periodic poll of `get_protocol_health` for a reconciling "ground
truth" snapshot (catches anything a missed/dropped event would hide).

---

## 3. Signal Catalog

### 3.1 Availability / incident-response signals

| Signal | Source | Condition | Severity |
|---|---|---|---|
| Unexpected pause | `paused` event, payload `true` | Any occurrence outside a pre-announced maintenance window | **P1 — page immediately.** `submit_intent`/`accept_intent`/`fill_intent` are now halted protocol-wide. |
| Unexpected unpause | `paused` event, payload `false` | Any occurrence outside a pre-announced maintenance window | **P1 — page immediately.** Confirm it was an authorized admin action, not a compromised key restoring cover for further abuse. |
| Paused longer than expected | `is_paused()` (or `get_protocol_health().paused`) polled | `true` for longer than the announced maintenance window | **P2** — escalate if pause persists past its stated end time. |

The `paused` topic is reused for both pause and unpause, distinguished only
by the boolean payload — an indexer must decode the payload, not just match
the topic name, to tell the two apart. This ambiguity is called out as an
anti-pattern to avoid in future contracts; see
[#113](113-event-topic-naming-conventions.md).

### 3.2 Solver risk signals

| Signal | Source | Condition | Severity |
|---|---|---|---|
| Unusual slash rate | `solver_slashed` event count vs. `intent_accepted` count in the same rolling window | Slash rate over a trailing 1h window exceeds a baseline (suggested starting point: >20% of accepted intents slashed, or >5 slashes in 10 minutes, whichever trips first) | **P2** — may indicate a bug in fill validation, a coordinated solver failure, or an economic condition (bond too small relative to opportunity cost) making default attractive. |
| Bond utilization dropping | `bond_withdrawn` + `solver_deregistered` event volume, reconciled against `get_solver(address).bond_amount` for tracked solvers | Aggregate bonded collateral across actively-monitored solvers drops materially (suggested: >25%) within a short window (e.g. 1h) | **P2** — solvers de-risking ahead of an anticipated problem is a leading indicator, not just a lagging one. |
| Mass solver exit | `solver_deregistered` event count | More than N deregistrations (suggested N=3) within 1h | **P2** — correlate with recent `slash_solver` or `config_updated` events for a likely cause. |
| Repeated slash-cooldown lockouts | `accept_intent` reverting with `Error::SolverInactive` while `last_slash_time` cooldown is active (visible via `get_solver`) | A given solver hitting this repeatedly | **P3** — informational; expected behavior, but a spike across many solvers simultaneously correlates with the "unusual slash rate" signal above. |

There is currently no view that sums bonded collateral across *all*
registered solvers in one call — computing the aggregate requires
enumerating known solver addresses and calling `get_solver` per address.
If "aggregate bonded USDC" becomes a first-class ops signal, a follow-up
issue should evaluate adding a `TotalBonded` running counter (same pattern
as `TotalVolume`) so it's available as a single instance-storage read
instead of requiring off-chain enumeration.

### 3.3 Intent-flow health signals

| Signal | Source | Condition | Severity |
|---|---|---|---|
| Fill-rate stagnation | `get_stats().1` (`total_volume`) polled periodically, or absence of `intent_filled` events | No increase over an expected window (baseline depends on observed traffic) | **P3** — informational unless sustained; may indicate solvers are offline or the destination-token allowlist is misconfigured. |
| Expiry-rate spike | `intent_expired` event count | Sustained increase relative to `intent_submitted` volume in the same window | **P2** — indicates either solvers are not accepting intents (see 3.2) or `submit_intent` deadlines are being set too tight upstream. |
| Extension-granting frequency | `extension_granted` event, repeated for the same `intent_id` | Same intent granted more than one extension | **P3** — a solver repeatedly requesting extensions on the same intent is a stalling/griefing pattern worth a manual look. |
| Config/allowlist churn | `dst_token_allowed`/`disallowed`, `src_chain_allowed`/`disallowed`, `bond_multiplier_set`, `config_updated` | Any occurrence outside a pre-announced change window | **P3**, escalate to **P2** if unexpected — these directly change what `submit_intent` will accept. |

### 3.4 Admin / key-compromise signals

| Signal | Source | Condition | Severity |
|---|---|---|---|
| Admin transfer | `admin_transferred` event | Any occurrence not matching a pre-announced rotation ceremony | **P1 — page immediately.** This event reassigns control of every privileged function in the contract. |
| Fee recipient change | `fee_recipient_proposed` / `fee_recipient_updated` events | Any occurrence not matching a pre-announced change | **P1 — page immediately.** A silent change here redirects protocol fee and slashed-bond flow. |
| `rescue_tokens` invocation | `tokens_rescued` event | Any occurrence | **P1 — page immediately** and confirm it matches a known incident-response action; this function moves arbitrary token balances out of the contract. |

---

## 4. Suggested Dashboard Layout

A single top-of-dashboard status panel backed by `get_protocol_health`
(#112) covers the "is the protocol currently healthy" question in one
call: paused state, total intents, total volume, and solver count. Below
it, time-series panels driven by the event stream cover the per-category
signals in §3.

---

## 5. Open Follow-Ups

- Aggregate bonded-collateral view (§3.2) — separate issue if needed.
- Consider emitting a distinguishable topic for pause vs. unpause instead
  of a shared `paused` topic with a boolean payload (tracked alongside the
  broader naming-convention cleanup in [#113](113-event-topic-naming-conventions.md);
  changing an existing topic is an indexer-breaking change and should ship
  as its own versioned migration, not bundled here).

---

*Closes #110*
