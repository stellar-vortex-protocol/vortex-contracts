# Community-Nomination Process for Arbiter Committee Selection

## Overview

This document describes an off-chain community-nomination and -endorsement process that informs (without technically binding) the admin's on-chain arbiter appointment decisions. It is the next step toward decentralization of the arbiter role beyond issue #42's initial admin-appointed committee, while explicitly stopping short of full trustless on-chain election infrastructure (which is deferred as a separate, larger effort).

**Scope & Governance Context:**

- **Issue #42** established the on-chain arbiter registry and admin-appointment mechanism (initially admin-rotatable; upgradeable to multisig or separate contract)
- **This issue (#309)** designs the off-chain process that informs appointment decisions
- **Issue #115** defines eligibility/conflict-of-interest criteria for arbiters (referenced and reused here)
- **Issue #120** defines a community-signaling tool that this process reuses for the community-endorsement phase

---

## Process Stages

### Stage 1: Nomination Period (28 days)

**Timeline:** Begins on a fixed schedule (e.g., first Monday of Q1, Q2, Q3, Q4) or on-demand when a committee seat becomes vacant.

**Who can nominate:** Any community member (no minimum reputation or stake required; low barrier to entry)

**How to nominate:**

1. Open a GitHub issue in this repository with title: `[Arbiter Nomination] <Candidate Name>`
2. Use the **Arbiter Nomination** issue template (see below)
3. Provide:
   - Candidate name (or pseudonym if preferred)
   - Candidate contact info (Discord, GitHub, email)
   - Why they should be an arbiter (background, relevant experience, why they're trusted)
   - Candidate's self-attestation (ideally the candidate responds to confirm interest)

**Screening:**

- Nominators and candidates must affirm they meet issue #115's eligibility criteria:
  - No direct financial stake in the protocol (no solver bonds, treasury allocations, or compensation beyond arbiter stipend)
  - No active disputes as a party (users with open slash appeals or fill disputes are not eligible during ongoing disputes)
  - No conflicts of interest (see issue #115 for full eligibility criteria)
- Repository maintainers screen nominations within 3 days; ineligible nominations are marked and closed with explanation

**Eligible nominations remain open for community discussion** throughout the period.

### Stage 2: Community-Signaling Period (14 days)

**Timeline:** Immediately follows the nomination period

**Who can signal:** Any community member

**Signaling mechanism:** Uses issue #120's community-signaling tool, scoped specifically to arbiter candidates:

- A signaling poll is created with all eligible nominees as options
- Community members endorse their preferred candidates (one-vote-per-person or weighted-stake, per #120's design)
- Results are publicly displayed and continuously updated

**Outcome:** A ranked list of candidates by community endorsement, published at the close of the signaling period

### Stage 3: Admin Appointment (within 7 days after signaling closes)

**Who decides:** The protocol admin (or multisig, per issue #114)

**Commitment:** The admin commits, as a **documented governance norm** (not code-enforced), to appoint the community-endorsed candidate(s) absent a **specific, disclosed, documented reason not to**.

**Examples of valid reasons to diverge from community endorsement:**

- Candidate became unavailable (withdrew, took another role, became ineligible)
- Candidate received new disqualifying information (post-nomination conflict of interest discovered, community concern raised that changes the risk profile)
- Admin believes the candidate lacks necessary operational experience (must be documented)

**Invalid reasons** (commits to *not* use as divergence rationale):

- "I prefer a different candidate with equal community support" (if community endorsed someone, appointment should follow)
- "I want to move faster" (process deliberation pace is the tradeoff for legitimacy)

**Publication:** The admin's appointment decision (and, if declining a community-endorsed candidate, the documented reason) is posted as a comment on the signaling poll and in the governance group's regular update.

### Stage 4: Term & Re-opening

**Term length:** 12 months (subject to change by governance; see RFC process below)

**Transition:** 30 days before a seat expires, the nomination period for that seat opens again.

**Mid-term removal:** If an arbiter becomes ineligible (new conflict of interest, extended absence), the admin or governance group may trigger a special election or temporary rotation per the arbiter registry contract (issue #42).

---

## Eligibility Criteria (Reference to Issue #115)

Candidates must meet issue #115's arbiter code-of-conduct and eligibility requirements:

| Criterion | Reason | Enforcement |
|-----------|--------|---|
| No direct protocol financial stake | Avoid incentive misalignment | Nominee self-attestation + community vetting |
| No active disputes as a party | Avoid judging own cases | Maintainer screening during nomination period |
| Conflict-of-interest disclosure | Full transparency | Nomination form + public discussion |
| Reasonable availability | Ensure timely dispute resolution | Nominee's commitment in nomination issue |
| No criminal convictions (flagrant financial crimes) | Risk mitigation | Nominee self-attestation; may be challenged |

**See [`docs/arbiter-code-of-conduct.md`](./arbiter-code-of-conduct.md) (issue #115) for full details.**

---

## Community-Signaling Mechanism (Reference to Issue #120)

This process reuses issue #120's community-signaling tool:

- **Scope:** Arbiter candidate endorsement
- **Voting:** One-vote-per-person (or stake-weighted, per #120's final design)
- **Duration:** 14 days
- **Transparency:** Real-time results, public leaderboard
- **Tie-breaking:** If two candidates receive equal endorsement, the signaling period may be extended by 7 days to seek consensus, or the admin may break the tie with documented reasoning

**If issue #120 has not shipped by the time a nomination cycle occurs:**

- Interim mechanism: pinned GitHub Discussion poll with thumbs-up/reactions as votes
- Document this interim choice in the cycle's announcement
- Plan transition to #120's tool once available

---

## Example: Nomination Cycle Timeline

```
Q1 Arbiter Nomination Cycle (3 seats up for reconfirmation or new candidates)

Week 1 (Jan 2)
  └─ Nomination period opens
     Announcement: "Arbiter Committee seats expiring March 31; nominations open until Jan 30"
     GitHub Discussion: https://github.com/stellar-vortex-protocol/vortex-contracts/discussions/...
     Issue template: Arbiter Nomination [link]

Week 2–4 (Jan 9–30)
  ├─ Community nominates candidates
  │  Example nominees: Alice (reconfirmation), Bob (new), Carol (new)
  │  Screening: Maintainers verify eligibility
  │  Ineligible: Dave (withdrew), Eve (has open dispute)
  │
  └─ Eligible nominees: Alice, Bob, Carol

Week 5 (Feb 2)
  └─ Community-signaling period opens
     Signaling poll (via issue #120):
       □ Alice (reconfirmation)
       □ Bob (new candidate)
       □ Carol (new candidate)
     Poll open until Feb 16

Week 5–6 (Feb 2–16)
  └─ Community votes
     Results (Feb 16 at poll close):
       Alice: 320 votes (51%)
       Bob:   210 votes (33%)
       Carol: 100 votes (16%)

Week 7 (Feb 23)
  └─ Admin appointment window
     Admin decision:
       1. Appoint Alice (reconfirmation) — community-endorsed, eligible ✓
       2. Appoint Bob — second-place community endorsement, admin agrees ✓
       3. For third seat: Carol (third place, 16%) OR admin's choice if conflict of interest
          - If admin diverges from Carol: publish documented reason
          - Examples: "Carol disclosed late conflict of interest" or "Concerns raised in discussion suggest elevated risk"
          
     Decision published in governance update (March 1)
     On-chain appointments via issue #42's registry (March 1–7)

March 31
  └─ New committee takes office
     Previous arbiters rotate off (or continue if reappointed)
```

---

## Edge Cases

### Case 1: No Eligible Candidates Nominated

**Scenario:** Nomination period ends with zero eligible candidates.

**Resolution:**

1. Extend nomination period by 14 days (ad-hoc, announced to community)
2. If still zero candidates: admin appoints a temporary arbiter from outside the process, with documented reasoning
3. Next cycle (same year, if mid-term), re-open nominations with lower-friction pathways (e.g., admin pre-nominates trusted community members)

**Root-cause mitigation:** Track why nominations are low (barrier to entry? lack of awareness?); adjust process in next cycle.

### Case 2: Admin Declines All Community-Endorsed Candidates

**Scenario:** Community signals strong support for candidate A, but admin appoints candidate B instead.

**Resolution:**

1. Admin **must** publish documented reasoning (e.g., "A withdrawn; B has operational experience A lacked")
2. Reasoning is posted to signaling poll + governance update
3. Community may escalate concern to RFC process (issue #112) if they believe the divergence was unjustified
4. RFC process then determines whether admin authority should be constrained (e.g., require governance vote if admin declines top-3 candidates)

### Case 3: Candidate Becomes Ineligible Post-Nomination (Conflict of Interest Discovered)

**Scenario:** During signaling period, previously-unknown conflict of interest surfaces (candidate's brother starts a solver operation).

**Resolution:**

1. Maintainer or governance group flags the candidate as ineligible
2. Candidate is removed from signaling poll (if already open) or disqualified before appointment
3. If removal occurs mid-signaling, extend signaling by 7 days to allow community to endorse alternatives
4. Update eligibility criteria in issue #115 to prevent recurrence (if new conflict type discovered)

### Case 4: Committee Seat Becomes Vacant Mid-Term

**Scenario:** An arbiter resigns or becomes ineligible (discovers conflict of interest, extended travel) before term end.

**Resolution:**

- **Short-term (<30 days to term end):** Admin appoints a temporary replacement (documented) until next regular cycle
- **Long-term (>30 days to term end):** Trigger a special nomination/signaling cycle for that seat only
- Document the mid-term change in governance updates

### Case 5: Signaling Tool (Issue #120) Not Yet Shipped

**Scenario:** A nomination cycle must occur before #120's signaling tool is available.

**Resolution:**

1. Use interim mechanism (pinned GitHub Discussion poll, thumbs-up reactions, or Snapshot poll)
2. Document the interim choice in cycle announcement: "Using [X] for signaling; will migrate to issue #120's tool once available"
3. Commit to plan for transition (e.g., "all future cycles will use #120's tool")
4. Audit interim mechanism for fairness (log vote counts, check for manipulation)

---

## Dependencies & Sequencing

This process depends on the following being completed or planned:

| Dependency | Status | Role |
|-----------|--------|---|
| Issue #42: Arbiter registry contract | Assumed shipped | Provides on-chain registry for appointments |
| Issue #115: Arbiter code-of-conduct | Assumed shipped | Defines eligibility criteria reused here |
| Issue #120: Community-signaling tool | Assumed shipped, interim OK | Enables community endorsement voting |
| Governance group (Issue #117) | Assumed seated | Makes final appointment decisions |

**If dependencies are not met:**

- **#42 not shipped:** This process cannot operate (no registry to appoint to). Document this as a blocker.
- **#115 not finalized:** Use preliminary eligibility criteria in first cycle; rebase on #115 once finalized.
- **#120 not shipped:** Use interim signaling mechanism (GitHub Discussion) per Edge Case #5.
- **Governance group not seated:** Admin unilaterally appoints; next cycle awaits governance (issue #117) setup.

---

## Responsibilities

### Community

- **Nominate qualified candidates:** Take the process seriously; nominate candidates you genuinely believe would serve as fair arbiters
- **Signal authentically:** Vote based on candidates' merit and trustworthiness, not tribalism or grudges
- **Escalate concerns:** If you believe admin divergence from community consensus was unfair, raise an RFC (issue #112)

### Nominators & Candidates

- **Disclose conflicts:** If you nominate someone (or are nominated), affirm they meet eligibility criteria
- **Respond to questions:** Candidates should actively engage in nomination-period discussion to build trust

### Admin & Governance Group

- **Triage nominations:** Review eligibility within 3 days of submission
- **Honor community signal:** Appoint community-endorsed candidates unless documented reason otherwise
- **Publish decisions:** All appointment decisions and reasons are public

### Maintainers

- **Manage process:** Issue GitHub template, track nominations, administer signaling, archive results
- **Monitor conflicts:** Flag eligibility concerns promptly (don't silently remove a nomination)
- **Maintain audit trail:** Keep all nomination issues and signaling results public and searchable

---

## Reporting & Audit Trail

### Public Record

All arbiter nomination cycles are archived:

- **Nomination issues:** Linked from a pinned GitHub Discussion or a "nomination archive" page
- **Signaling results:** Published in a governance update or results spreadsheet (GitHub + README)
- **Appointment decisions:** Documented in governance group update + cross-linked from nomination issues
- **Reasons for divergence:** If admin deviates from community consensus, reasoning is published at the same time

**Example archive:** A yearly governance update includes a table:

| Cycle | Seats | Nominees | Community Top Pick | Admin Appointed | Notes |
|-------|-------|----------|---|---|---|
| Q1 2026 | 3 | Alice, Bob, Carol | Alice (320 votes) | Alice | Reconfirmed |
| Q1 2026 | | | Bob (210 votes) | Bob | New, community-endorsed |
| Q1 2026 | | | Carol (100 votes) | Resigned; David | Carol unavailable; #42 mid-term appt |

### Frequency of Cycles

- **Regular:** Every 12 months (or per community governance decision)
- **Special:** If a seat becomes vacant mid-term or a community RFC requests early re-election

---

## Relationship to Other Processes

### vs. Formal On-Chain Election (Deferred)

This process is intentionally **off-chain and advisory**. A full on-chain election (where community votes are code-enforced) is deferred as a separate effort (issue #42 notes this as future work). That effort would require:

- Governance token or stake-weighted voting
- Trustless vote-counting on-chain
- Upgrade path for arbiter registry

**This process is a stepping stone**, not a permanent solution.

### vs. RFC Process (Issue #112)

- **This process:** Elects arbiters for a set term
- **RFC process:** Proposes changes to arbiter rules, eligibility, term length, or the nomination process itself

If community feedback suggests the nomination process is unfair, use the RFC process to propose changes.

### vs. Slash Appeals (Issue #39)

- **Arbiter election:** Determines who serves on the committee
- **Slash appeals:** Specific disputes contesting an individual slash event

These are separate channels; a solver contesting a slash uses the formal slash appeal process, not arbiters-election feedback.

---

## FAQ

**Q: What if I want to nominate someone but they're not a community member yet?**

A: You can still nominate them. Provide their contact info and a way for them to confirm interest. They must still meet issue #115's eligibility criteria.

**Q: Can an arbiter be re-elected indefinitely?**

A: Yes, unless the community or governance votes to change term limits via RFC. Arbiters who are reconfirmed in multiple cycles can serve indefinitely (or until they become ineligible). This allows experienced arbiters to remain while enabling turnover.

**Q: What if the signaling tool (issue #120) is down during the signaling period?**

A: Extend the signaling period by the number of days the tool was down (e.g., tool down for 3 days → extend period by 3 days). Document the extension in the nomination archive.

**Q: Can the admin appoint an arbiter who received zero votes in signaling?**

A: Technically yes, but it violates the documented commitment to honor community consensus. If this happens, admin must provide a detailed reason (e.g., "community-endorsed candidate withdrew; appointment of backup was necessary"). Community can challenge via RFC.

**Q: What happens if the admin key is compromised?**

A: This is a broader protocol risk. If the admin is compromised, all admin functions (including arbiter appointments) are at risk. Mitigation is addressed in issue #114 (multisig admin) and issue #122 (admin key security). This process assumes the admin key is secure.

**Q: How is the arbiters' performance reviewed?**

A: That is out of scope for this election process. See issue #115 (arbiter code-of-conduct, which may specify performance expectations) or a separate arbiter-performance-review RFC.

---

## Roadmap

1. **Now:** Document this process (this file)
2. **Pending issue #42:** Once arbiter registry ships, conduct first nomination cycle (pilot)
3. **Pilot cycle:** Run nomination, signaling, and appointment for 1–2 committee seats to validate process
4. **Learnings:** Document any bottlenecks or community feedback; adjust process for next cycle if needed
5. **Ongoing:** Execute regular nomination cycles on fixed schedule (annual or per governance decision)
6. **Future:** As issue #42 upgrades to multisig or on-chain voting, evolve this process or replace it with full trustless election

---

## References

- **Issue #42**: Arbiter registry contract (on-chain mechanism)
- **Issue #115**: Arbiter code-of-conduct and eligibility criteria (referenced for screening)
- **Issue #120**: Community-signaling tool (mechanism for endorsement voting)
- **Issue #112**: RFC process (escalation path for process changes)
- **Issue #117**: Governance spending process (context for governance group seat)
- **Issue #122**: Admin key security (assumes secure admin custody)
- `docs/dispute-resolution-design.md`: Dispute process that arbiters execute

