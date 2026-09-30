# Arbiter Code of Conduct and Selection Criteria

**Tracking issue:** [#300](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/300)

This document defines the governance and ethical standards for arbiters adjudicating disputes and appeals within the Vortex Protocol. It covers eligibility, conflict-of-interest disclosure, recusal procedures, decision rationale requirements, and escalation paths.

---

## Overview

Arbiters make binding decisions in two contexts:

1. **Dispute resolution** (issue #3, #48) — adjudicating a user's claim that a fill was wrong-recipient or otherwise defective.
2. **Slash appeals** (issue #39) — reviewing a slashed solver's appeal of a slash decision and determining whether the slash should be reversed.

In both cases, real economic stakes are on the line: bond restitution, fee recovery, and user fund allocation. Arbiters must meet high standards of impartiality, conflict-of-interest management, and transparent reasoning.

---

## Eligibility Criteria for Arbiters

An arbiter must meet **all** of the following:

### 1. Knowledge and expertise

- Demonstrated understanding of the Vortex Protocol's architecture (intent lifecycle, solver bonds, fill guarantees, and slashing economics).
- Familiarity with this SECURITY.md and the threat model it documents.
- Familiarity with the dispute-resolution and slashing-appeal state machines (issues #3, #39, #48).
- For v2+ multi-arbiter setups: experience with multi-sig governance or committee decision-making in DeFi protocols.

**Suggested verification:** Arbiters review `docs/dispute-resolution-design.md`, issue #39 (slashing appeals), and this document before accepting appointment. The appointing admin or committee may require a written acknowledgment of understanding.

### 2. Operational independence

- No financial interest in any registered solver's performance or bond status.
- No financial relationship with any end user of Vortex Protocol (e.g., not a strategic partner deriving revenue from the protocol's adoption).
- No role on Vortex's core development team that would create dual loyalties.

**Suggested verification:** Arbiters disclose any close relationships with solvers, users, or team members in writing (see Mandatory Disclosure below).

### 3. Institutional stability

- For v1 (admin arbiter): the admin's key must be hardware-wallet-backed and operated with dual-authorization practices per SECURITY.md.
- For v2+ (committee arbiters): each arbiter must maintain a stable, monitored operational presence (e.g., a known pseudonym with a track record, or a named representative of an established entity).

---

## Mandatory Conflict-of-Interest Disclosure

Every arbiter must disclose **before taking office**:

### 1. Direct economic interests

- **Registered solver?** Is the arbiter a registered, bonded solver in the same Vortex instance? If yes, name the solver address and bond amount.
- **Beneficiary of protocol fees or slashes?** Is the arbiter the fee recipient, a co-signer on the fee recipient's multisig, or a member of a treasury that receives protocol revenue?
- **Financial relationship with solvers?** Does the arbiter have an off-chain agreement (grant, revenue-share, service contract) with any active solver?

### 2. Governance roles

- **Core team membership?** Is the arbiter an active contributor to the vortex-contracts repository or broader Vortex protocol development?
- **Competing protocol involvement?** Does the arbiter have a governance role in a competing intent-settlement or cross-chain-swap protocol?

### 3. Prior decisions

- Has the arbiter previously adjudicated disputes or appeals involving any of the parties to a case they are now asked to arbitrate? If yes, disclose the prior case ID and outcome.

**Disclosure format:** Arbiters complete a written Conflict of Interest Attestation (see template below) and file it in the repository as `docs/arbiter-coi-<pseudonym>.md` with a revision history.

---

## Recusal Procedure

An arbiter **must recuse** (withdraw) from a specific dispute or appeal if any of the following is true:

### 1. Direct conflict of interest

- The dispute or appeal involves a solver registered by the arbiter or controlled by a close relative.
- The dispute or appeal involves a user or solver with whom the arbiter has a direct off-chain financial relationship.
- The arbiter is the subject of the dispute or appeal (e.g., an appeal contesting the arbiter's own prior decision).

### 2. Appearance of bias

- The arbiter has a prior dispute history with either party that a reasonable observer might perceive as favoritism or grudge-bearing.
- The arbiter has publicly stated a position on a similar case that would appear to prejudge this one.

### 3. Recent involvement

- The arbiter was involved in writing, auditing, or patching the code that is the subject of the dispute or appeal (e.g., if an appeal contests a slash triggered by a bug in `slash_solver`, the arbiter who patched that bug should recuse).

### Recusal process

1. **Self-recusal.** The arbiter, upon recognizing a conflict, immediately notifies the appointing admin or committee chair.
2. **Notification.** The parties to the dispute or appeal are notified that the arbiter is recusing and that a replacement will be assigned.
3. **Replacement assignment.** Another arbiter (or the full committee, in v2+) takes over the case. There is no delay in assigning a replacement; see Stalled Dispute Fallback below.
4. **Documentation.** The recusal and replacement are noted in the case record and in the postmortem (per issue #301).

---

## Decision-Rationale Disclosure Requirement

Arbiters **must publish** a written rationale for every dispute and appeal decision, regardless of outcome. This rationale:

- **Is public and permanent.** Archived in this repository under `docs/arbiter-decisions/case-<intent_id>.md` or linked from an Airtable/GitHub Discussions board.
- **Explains the reasoning,** not just the outcome. A decision must state:
  - Which facts are in dispute and how the arbiter resolved them.
  - Which trust assumptions or documented threat model elements informed the decision.
  - What precedent (if any) from prior cases was applied.
  - If the decision deviates from prior practice, why.
- **Acknowledges trade-offs.** If the arbiter sided with the solver over the user (or vice versa), the rationale must address the economic implications for both parties.
- **Is written within 24 hours of the decision.** A delayed rationale erodes confidence and makes it harder to identify patterns later.

**Example decision rationale structure:**

```
# Case <intent_id>: Dispute Resolution

## Parties
- User: <user_address>
- Solver: <solver_address>
- Arbiter: <arbiter_pseudonym>
- Decision date: <ISO 8601>

## Dispute summary
User claims the fill was sent to the wrong recipient. Dispute opened on <date>; arbiter investigation commenced on <date>.

## On-chain facts verified
- Intent was Open, then Accepted by solver on <timestamp>.
- begin_fill called on <timestamp> with fill_amount = <amount> USDC.
- Output token transfer to user address confirmed on-chain.
- User claims transfer went to address X, not the intent's user address.

## Findings
[Describe investigation: did we verify the output went to the intent.user address, or to a different address? Include token event hash and balance diff.]

## Arbiter decision: Upheld
The fill was indeed sent to <misrouted_address>, not the intent's user address <correct_user>. This violates the core invariant: `fill_intent` must transfer exactly `fill_amount` to `intent.user`. 

While the user could theoretically recover the tokens from the wrong address directly (out of scope for this arbitration), this represents a breach of the solver's fill obligation and justifies a 10% bond slash per the documented slash policy.

## Precedent
This decision aligns with prior case #2104 (wrong-recipient fill by Solver-B). The principle is consistent: output must reach the intent's specified recipient.

## Solver impact
Solver is slashed 10% of bond (~5 USDC). The solver may appeal per issue #39 if they believe the on-chain investigation was incomplete.
```

---

## Escalation Path and Stalled Dispute Fallback

### Normal escalation

If a party (user or solver) believes an arbiter's decision is biased or made in violation of this code of conduct, they may:

1. **File a formal appeal** within 7 days of the decision, stating the specific allegation (conflict of interest, procedural violation, factual error).
2. **Appeal is reviewed** by:
   - **v1:** The admin (separate from the arbiter who made the original decision). If the arbiter and admin are the same person, the appeal escalates to the core team or a pre-designated emergency contact.
   - **v2+:** A committee vote (2-of-3 or higher threshold) of other arbiters. The original arbiter does not vote on their own case.
3. **Appeal decision** is rendered within 7 days. The appeal can result in:
   - Uphold the original decision.
   - Overturn and re-arbitrate with a different arbiter.
   - Uphold with a censure (arbiter remains in office but is required to recuse from similar cases).
   - Remove the arbiter from office.

### Stalled dispute fallback

If a dispute or appeal enters a state where no non-conflicted arbiter is available:

- **v1:** Escalates to the core team or a pre-designated fallback arbiters. A minimum of 2-of-N of them must agree on the decision.
- **v2+:** The full committee votes on a resolution. If M-of-N quorum cannot be met due to recusals, the decision defaults to a conservative outcome:
  - In a user dispute: default to "user wins" (slash is reversed if applicable).
  - In a solver appeal: default to "slash upheld" (no restitution).

The fallback outcome is documented in the postmortem (issue #301) so the protocol can assess whether a different arbiter selection process is needed.

---

## Conflict of Interest Attestation Template

Arbiters complete this before taking office and repeat annually:

```markdown
# Conflict of Interest Attestation

**Arbiter pseudonym:** [Your identifier]
**Date:** [ISO 8601]
**Statement:**

I, the arbiter identified above, attest that I have read:
- SECURITY.md (Vortex Protocol threat model)
- docs/dispute-resolution-design.md (dispute state machine)
- docs/arbiter-code-of-conduct.md (this document)

I confirm that I meet all eligibility criteria and disclose the following conflicts of interest:

### Direct economic interests
- [ ] I am a registered solver. If yes: address = <>, bond = <>.
- [ ] I am the fee recipient or a co-signer. If yes: details = <>.
- [ ] I have an off-chain agreement with a solver. If yes: which solvers = <>.

### Governance roles
- [ ] I am a core team member of Vortex. If yes: which role = <>.
- [ ] I have a governance role in a competing protocol. If yes: details = <>.

### Prior cases
- [ ] I have previously adjudicated disputes or appeals. If yes: which cases = <>.

### Certification
I certify that the above disclosures are true and complete. I understand that false disclosure may result in removal from the arbiter role and reputational damage to my pseudonym. I agree to recuse myself from any case involving a conflict of interest per the procedure defined in docs/arbiter-code-of-conduct.md.

**Signature (or signed message):** [GitHub handle or signed message hash]
```

---

## Initial Arbiter Setup (v1)

For v1, the protocol has a single arbiter: the `admin` key.

- **Arbiter:** The admin address (multisig or hardware-wallet-backed per SECURITY.md).
- **Eligibility:** The admin is assumed to meet the eligibility criteria above as part of the deployment process.
- **Disclosure:** The admin files a Conflict of Interest Attestation in this repository (see template above) prior to mainnet launch.
- **Decision rationale:** The admin publishes a written rationale for every dispute and appeal decision within 24 hours.

---

## Transition to v2+ Multi-Arbiter Setup (issue #42)

Once issue #42 (arbiter registry) ships a separate `Pauser`-like role, the protocol can transition to a multisig arbitration committee:

- **Selection:** Arbiters are nominated by the admin and approved by a DAO vote or community signaling process (specific governance mechanism TBD).
- **Committee size:** Recommended 3-of-5 initially (any 3 of 5 arbiters can resolve a dispute).
- **Recusal handling:** If more arbiters recuse than a quorum can be formed, falls back to the core team decision-making process (see Stalled Dispute Fallback above).
- **Term limits:** Suggested 6-month terms, renewable via community vote, to ensure regular review of arbiter fitness.

---

## Consistency with Other Governance Policies

This code of conduct is grounded in:

- **SECURITY.md §2 (Admin key custody):** Arbiters are appointed by or act under delegation from the admin; they inherit the admin's trust assumptions.
- **docs/incident-postmortem-template.md (issue #301):** Every arbitration decision (especially appeals) is reflected in postmortems to identify systemic patterns.
- **docs/dispute-resolution-design.md (issue #3, #48):** Arbiters implement the state machine and fund flows defined there.
- **docs/114-multisig-admin-design.md (issue #36):** Admin key custody directly influences arbiter reliability; a compromised admin key compromises the arbiter role.

---

## FAQ

**Q: Can a solver who was slashed appeal to an arbiter, and if so, how is the arbiter chosen?**

A: Yes, per issue #39 (slashing appeals). The arbiter is initially the admin (v1); in v2+, a separate arbiter or committee reviews the appeal. The arbiter assigned to hear the appeal is chosen to minimize conflict of interest — ideally an arbiter with no prior relationship with the slashed solver. If no such arbiter is available, the appeal escalates per the Stalled Dispute Fallback procedure above.

**Q: What happens if an arbiter becomes unavailable mid-case (e.g., key loss, pseudonym retirement)?**

A: The case is reassigned to a replacement arbiter immediately. The replacement reviews the full case history and may either affirm the pending decision or re-open investigation if material facts were missed. This is documented in the case record.

**Q: Can a solver dispute an arbiter's decision by claiming the arbiter was conflicted?**

A: Yes, per the Escalation Path section above. An appeal alleging conflict of interest is heard by a separate authority (core team in v1, committee in v2+). If the allegation is substantiated, the original decision may be overturned and the case re-arbitrated by a different arbiter.

**Q: Does this code of conduct apply retroactively to disputes already resolved before this document existed?**

A: No. Retroactive application would cast doubt on settled cases and is unfair to arbiters who acted in good faith before the policy was formalized. Going forward, all arbitrations are subject to this code. Prior cases are grandfathered unless the specific arbiter involved is later removed for cause, in which case re-arbitration of their prior decisions is considered.

---

## References

- `docs/dispute-resolution-design.md` — dispute state machine and fund flows
- Issue #3 — dispute-resolution state machine implementation
- Issue #39 — slashing-appeal governance
- Issue #42 — rotating/elected arbiter registry
- `docs/incident-postmortem-template.md` (issue #301) — postmortem process capturing arbiter decisions

---

## Revision History

| Date | Change | Justification |
|------|--------|---------------|
| 2026-09-24 | Initial document created | Issue #300; establishes v1 admin-arbiter policy and v2+ migration path |

---

*Last updated: 2026-09-24*
