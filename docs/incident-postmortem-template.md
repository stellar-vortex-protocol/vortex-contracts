# Incident Postmortem Template

**Tracking issue:** [#301](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/301)

This document is a template for incident postmortems. Every P1 incident (see `docs/110-monitoring-alerting-spec.md` for severity definitions) that affects mainnet will be resolved via a postmortem following this structure. Postmortems are published to the community within **N business days** of incident resolution (see Publication Commitment below).

---

## Publication Commitment

**Vortex Protocol commits to publishing a postmortem for every P1 incident within 5 business days of resolution.** This commitment applies to:

- Unexpected pause events
- Unexpected unpause events
- Admin key transfer
- Fee recipient change
- `rescue_tokens` invocation
- Any other incident requiring mainnet downtime or user/solver communication

**Exceptions:** If full public disclosure of the root cause would create a new exploitable window (e.g., a not-yet-fully-patched vulnerability class), the protocol may publish a **redacted initial postmortem** within 5 business days, with a commitment to publish the full technical details within a defined safe-harbor period (e.g., 30 days after all affected deployments are patched).

---

# [INCIDENT POSTMORTEM TEMPLATE]

---

## Executive Summary

**Incident ID:** `INCIDENT-YYYY-MM-DD-001` (auto-generated identifier)
**Date/time of incident:** ISO 8601 start and end times (UTC)
**Duration:** Total time protocol was degraded (e.g., 2 hours 15 minutes)
**Severity:** P1 / P2 / P3 (per `docs/110-monitoring-alerting-spec.md`)
**Status:** Resolved / Ongoing
**Impact:** [One-sentence summary of what users/solvers experienced]

**Example:**

```
A smart contract bug in the slash-validator logic caused an unintended pause event
on 2026-09-15 at 14:32 UTC, lasting 2 hours 10 minutes. 47 in-flight intents were
stalled during the pause window. No user funds were at risk; the pause was
automatically resolved via contract upgrade at 16:42 UTC.
```

---

## Detection Timeline

Document **when and how** the incident was detected. Correlate against signals from `docs/110-monitoring-alerting-spec.md`:

| Time (UTC) | Signal | Source | Status |
|---|---|---|---|
| HH:MM | `paused` event fired | Event stream listener | ✅ Detected |
| HH:MM | Ops dashboard alert triggered: "Unexpected pause" | Monitoring system | ✅ Alert delivered |
| HH:MM | On-call engineer acknowledged alert | PagerDuty | ✅ Acknowledged |
| HH:MM | Initial investigation began | Slack #incident | ✅ Investigation started |
| HH:MM | Root cause identified | Logs + code review | ✅ Cause found |
| HH:MM | Fix deployed to testnet | CI/CD | ✅ Fix validated |
| HH:MM | Mainnet upgrade executed | Admin key | ✅ Patch deployed |
| HH:MM | `unpause` event fired | Event stream listener | ✅ Service recovered |

**Key metrics:**
- **Detection latency:** Time from incident start to first alert (goal: < 5 minutes).
- **Triage latency:** Time from alert to on-call response (goal: < 10 minutes).
- **Resolution latency:** Time from root cause identification to patch deployed (goal: < 30 minutes for critical fixes).

---

## Root Cause Analysis

Explain **what went wrong** and **why**. Structure as:

### 1. What happened (observed behavior)

Describe the symptoms:
- Which contract function(s) were affected?
- What state changes (or lack thereof) were observed?
- Did any events fail to emit?
- Were there off-chain effects (e.g., solver notifications not sent)?

**Example:**

```
The pause event fired at block 1,234,567 (ledger timestamp 2026-09-15T14:32:00Z).
All calls to submit_intent, accept_intent, and fill_intent immediately reverted with
error code 18 (ContractPaused). Read-only functions like get_intent and get_stats
remained available. The contract was not paused via an admin call; the pause was
unexpected.
```

### 2. Root cause (code + logic)

Identify the specific code path or logic error:
- Which file and function?
- What was the incorrect assumption or bug?
- Under what conditions does the bug trigger?
- Was this a new bug or a latent one exposed by recent changes?

**Example:**

```
The bug was in intent_settlement/src/slash_validator.rs, line 156.
The slash_validator called pause() unconditionally when an edge-case condition was met:
a solver's bond amount overflowed u64 after a withdrawn-bond transaction.

The intended logic was:
  if bond_amount < MIN_BOND { pause() }

The actual code was:
  if bond_amount.checked_sub(withdrawal_amount).is_none() { pause() } 
  // i.e., panic on underflow, but the panic was caught and pause was invoked as a fallback

This is a latent bug introduced in PR #192 (bond withdrawal feature). The condition
should have been validated via an explicit bounds check before the subtraction, not
by catching an arithmetic panic.

The bug was exposed on 2026-09-15 when solver #7 withdrew an amount that caused their
bond to exactly zero (hitting the exact boundary case the logic didn't anticipate).
```

### 3. Why was it not caught earlier?

Identify gaps in testing or code review:
- Was there a test for this edge case? If not, why not?
- Did code review miss this?
- Could static analysis or fuzzing have caught it?

**Example:**

```
The bond withdrawal logic was tested with typical values (e.g., withdrawing 10 USDC
from a 50 USDC bond). No test case covered:
  - Withdrawing to exactly MIN_BOND (boundary case).
  - Withdrawing a value that would cause an underflow if not guarded (i.e., testing
    the arithmetic path that wasn't actually reachable due to the bug).

Code review (PR #192) did not flag the unconditional panic-to-pause logic as risky.
Fuzzing (cargo fuzz) was not run on slash_validator before merge.
```

---

## Impact Assessment

Quantify the damage:

### User impact

- How many users were affected?
- Were any user funds at risk or lost?
- How many intents were stalled or failed?
- Did any intents fail to settle in time?

### Solver impact

- How many solvers were affected?
- Were any bonds at risk or lost?
- Did solvers lose fill opportunities during the pause?
- Estimated financial impact (if quantifiable).

### Protocol impact

- Was the contract paused? For how long?
- Was there a fee leakage or flow misdirection?
- Was there any data corruption or state divergence?

**Example:**

```
**User impact:**
- 47 open intents were stalled during the 2-hour pause window.
- No user funds were locked or lost. The pause prevented new submissions but did
  not freeze or redistribute existing intent balances.
- 12 of the 47 intents had reached their deadline during the pause and expired
  after unpause (as expected per normal lifecycle).
- Estimated user friction: 47 users experienced a 2-hour trading delay; estimated
  opportunity cost (based on average intent volume/latency): ~$5,000 across all
  affected users (rough estimate only).

**Solver impact:**
- 8 registered solvers were active during the pause window.
- All 8 lost potential fill opportunities (fill_intent reverted with ContractPaused).
- No solver bonds were slashed or lost (the pause does not trigger slash logic).
- Estimated solver impact: 8 solvers each missed ~30 minutes of trading activity.

**Protocol impact:**
- Protocol was paused from 2026-09-15T14:32:00Z to 2026-09-15T16:42:00Z (2 hours 10 minutes).
- No fee leakage occurred; all fees earned during paused period were $0 (no fills).
- No state corruption or divergence. Contract state at unpause matched the last
  on-chain snapshot before pause.
```

---

## Remediation Actions

List the steps taken to resolve the incident:

### Immediate actions (during incident)

- [ ] Pause called (or incident occurred with pause already active)
- [ ] Communication sent to solvers/users (date, time, channel)
- [ ] Incident war room opened / on-call team assembled
- [ ] Root cause identified (date, time)
- [ ] Fix prepared and tested on testnet (date, time)
- [ ] Fix deployed to mainnet (date, time)
- [ ] Unpause called (or auto-unpause triggered)
- [ ] Service verification performed (date, time)

### Short-term fixes (day 1–2)

- [ ] PR #XXX merged: [Fix description]
- [ ] Contract upgrade executed (date, time, executor)
- [ ] State reconciliation completed (if needed)
- [ ] Affected solvers/users notified of resolution

### Long-term preventive actions (tracked as issues)

Create GitHub issues for each of the following:

- [ ] Issue #XXX: Add test case for bond withdrawal boundary conditions
- [ ] Issue #XXX: Run cargo fuzz on slash_validator before next release
- [ ] Issue #XXX: Add static-analysis linter rule to flag unconditional panic paths
- [ ] Issue #XXX: Code review checklist update: require explicit bounds checks on arithmetic

**Example:**

```
**Immediate actions taken:**
- 2026-09-15T14:35:00Z: Pause detected; on-call engineer paged.
- 2026-09-15T14:42:00Z: Root cause identified in intent_settlement/src/slash_validator.rs.
- 2026-09-15T15:00:00Z: Postmortem begun; fix drafted and tested on testnet.
- 2026-09-15T15:15:00Z: Fix code review completed (2 approvals).
- 2026-09-15T16:30:00Z: Upgrade contract proposed (timelocked 12h delay per issue #194).
- 2026-09-15T16:42:00Z: Upgrade executed via admin key; unpause called.
- 2026-09-15T16:45:00Z: Service verified: submit_intent and fill_intent callable again.
- 2026-09-15T17:00:00Z: Solvers notified via Discord announcement.

**Short-term actions:**
- 2026-09-15T18:00:00Z: PR #999 merged: Add bond withdrawal boundary test case.
- 2026-09-16T10:00:00Z: Full regression test suite run; all pass.

**Preventive actions tracked (issues filed):**
- Issue #500: Add cargo fuzz to CI/CD for all validator modules.
- Issue #501: Add static-analysis rule to catch unconditional panic paths.
- Issue #502: Update code review checklist for arithmetic safety checks.
```

---

## Timeline: Detailed Incident Log

Provide a second-by-second timeline (or at the level of granularity that makes sense for your incident):

```
2026-09-15T14:32:00Z  Solver #7 calls withdraw_bond(bond_amount=0 USDC)
2026-09-15T14:32:01Z  slash_validator checks bond_amount (now 0); triggers panic handler
2026-09-15T14:32:02Z  Admin key called pause() (via panic fallback logic)
2026-09-15T14:32:03Z  paused event emitted; pause flag set to true
2026-09-15T14:32:05Z  Ops dashboard alert fired: "Unexpected pause"
2026-09-15T14:33:00Z  On-call engineer acknowledged alert in PagerDuty
2026-09-15T14:35:00Z  Engineer connected to war room Slack channel
2026-09-15T14:42:00Z  Root cause identified via code review; issue in slash_validator.rs
2026-09-15T15:00:00Z  Fix code drafted and tested on testnet (all tests pass)
2026-09-15T15:15:00Z  Code review completed (2 approvals)
2026-09-15T16:30:00Z  propose_upgrade() called with new wasm hash
2026-09-15T16:42:00Z  execute_upgrade() called; unpause() executed
2026-09-15T16:45:00Z  Smoke test passed: submit_intent works
2026-09-15T17:00:00Z  Solver notification posted to Discord; community alert published
```

---

## Monitoring and Alerting Effectiveness

Evaluate the effectiveness of the monitoring signals that detected this incident:

| Signal | Alert fired? | Latency to alert | Was it accurate? | Recommendations |
|---|---|---|---|---|
| `paused` event | ✅ Yes | 3 sec | ✅ Yes; correctly identified unexpected pause | Keep as-is |
| `is_paused` polling | ✅ Yes | 5 sec (next poll cycle) | ✅ Yes | Increase polling frequency to 2 sec |
| Dashboard manual check | ✅ Yes | 10 min (after alert) | ✅ Yes | N/A (reactive) |

**What monitoring did not catch:**
- The root cause (unconditional panic handler) was not directly observable from signals; it required code review and off-chain logs to diagnose.
- A custom metric counting total solver count or bond-amount updates might have caught the edge case earlier.

**Recommendations:**
- Add a metric to track `bond_withdrawal` calls, bucketed by withdrawal amount (to spot edge-case boundary tests in the future).
- Add an alerting rule: "Any call to pause() outside a pre-announced maintenance window."

---

## Lessons Learned

Summarize key takeaways:

### What went well

- Detection was fast (< 5 minutes).
- Triage was efficient; root cause found in < 15 minutes.
- Testnet reproduction and fix validation were swift.
- Communication to stakeholders was clear and timely.

### What could be improved

- The unconditional panic-to-pause fallback should not have been written that way; explicit bounds checks are safer.
- Testing did not cover the boundary case (bond withdrawal to exactly MIN_BOND).
- Fuzzing was not in the CI/CD pipeline; it would have caught this.
- Code review should have flagged arithmetic handling as risky in a DAO contract.

### Systemic changes

- Arithmetic operations on bonded collateral now require explicit bounds checks (code review checklist, issue #502).
- Fuzzing is now mandatory before merge for any contract changes (CI/CD update, issue #500).
- New monitoring rule: alert if any admin function (`pause`, `set_fee_recipient`) is called outside a pre-announced window (issue #XXX).

---

## Communication to Community

Summarize what was communicated and to whom:

| Audience | Message | Channel | Time |
|---|---|---|---|
| Registered solvers | "Pause event; expected to resolve within 4 hours" | Discord #announcements, email | 2026-09-15T14:45:00Z |
| Users | "Trading paused due to maintenance; expected resumption 16:45 UTC" | Frontend banner, Discord | 2026-09-15T14:50:00Z |
| Auditors & integrators | "Smart contract bug in slash_validator; root cause analysis and fix attached" | GitHub Discussions, email | 2026-09-16T09:00:00Z |
| External security community | Postmortem published (this document) | GitHub, blog | 2026-09-16T09:00:00Z |

---

## Post-Incident Verification

Checklist for confirming the incident is fully resolved:

- [ ] Contract is unpaused and all core functions are callable.
- [ ] Smoke test passed: submit_intent, accept_intent, fill_intent all work.
- [ ] All in-flight intents from before the pause have reached a terminal state (Filled, Expired, Cancelled, etc.) or remain Open.
- [ ] No funds are stuck in escrow or in unexpected accounts.
- [ ] Fee recipient and admin address are unchanged and correct.
- [ ] Solver count and bond totals are consistent with pre-incident state.
- [ ] All events emitted during and after resolution are correct and in order.
- [ ] Mainnet contract code hash matches the expected release commit.

**Verification performed by:** [Team member name], [Date]
**Result:** ✅ All checks passed / ❌ Issues found (detail below)

---

## Preventive Actions (Follow-up Issues)

Link to all GitHub issues created to prevent recurrence:

- Issue #XXX: Add test case for bond withdrawal boundary conditions
- Issue #XXX: Integrate cargo fuzz into CI/CD
- Issue #XXX: Add arithmetic safety linter rule
- Issue #XXX: Update code review checklist for collateral handling

**Completion deadline:** 30 days from incident resolution
**Owner:** [Team member]
**Status:** In progress / Backlogged

---

## Approval and Sign-off

- **Postmortem drafted by:** [Name], [Date]
- **Technical review by:** [Auditor or lead engineer], [Date]
- **Approved for publication by:** [Protocol lead], [Date]

---

# END TEMPLATE

---

## How to Use This Template

1. **Copy this file** to `docs/incident-postmortem-<YYYY-MM-DD>-001.md` for each incident.
2. **Fill in all sections** before publication.
3. **Remove this "How to Use" section** and the "END TEMPLATE" marker before publishing.
4. **Publish within 5 business days** of incident resolution (see Publication Commitment above).
5. **Link to the postmortem** from this repository's main README and from SECURITY.md under a "Published Postmortems" section.
6. **Archive postmortems indefinitely** — they are permanent records of the protocol's safety history.

---

## Published Postmortems

This section is updated as incidents occur and postmortems are published:

| Date | Incident | Root Cause | Duration | Link |
|---|---|---|---|---|
| TBD | — | — | — | — |

---

*Template last updated: 2026-09-24*
