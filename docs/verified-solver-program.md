# Verified Solver Program

**Issue:** [#303](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/303)  
**Status:** Proposed  
**Last updated:** 2026-09-24

---

## 1. Overview

The Verified Solver Program is an **off-chain, human-vetted endorsement** of solver operators. It is **explicitly not** a permission or ranking mechanism, and it imposes no on-chain restrictions; instead, it provides users, integrators, and other solvers with an additional trust signal beyond the mechanical, on-chain reputation scores defined in `compute_reputation_score()` and `is_solver_eligible()` (`intent_settlement/src/lib.rs`).

**What this is not:**
- **Not a filter on `accept_intent()`:** Any solver that passes `is_solver_eligible()` can accept intents; vetting does not gate on-chain participation.
- **Not a reputation tier badge (issue #57):** That issue defines on-chain, score-derived tier indicators; this is a separate, off-chain human-judgment layer.
- **Not a ranking or leaderboard (issue #41):** The leaderboard is a measurement tool; this program is a vetting/endorsement tool.

**What this is:**
- An attestation that a solver operator has disclosed their identity/organization, provided references, and committed to operational-security practices.
- A public list distinguishing "on-chain eligible" solvers from "community-vetted" solvers.
- A process for revoking vetting status if a solver's conduct changes (e.g., after a slash event or discovered misrepresentation).

---

## 2. Core Principles

1. **Falsifiable vetting criteria:** Approval is based on concrete, verifiable claims (e.g., "operates under a named legal entity," "has run a solver on protocol X for N months"), not subjective trust.

2. **Clarity of boundaries:** The public list explicitly separates "on-chain eligible" from "community-vetted," so users never confuse these signals or mistake vetting for permission.

3. **Revocation is explicit:** If a solver's status should be withdrawn (due to a slash, misrepresentation, or changed operational practices), there is a documented process and public explanation. A program without revocation loses credibility.

4. **Independence:** Vetting review is conducted by one or more reviewers (not the Vortex Labs core team alone) to avoid the appearance of gatekeeping. Reviewers are named and their role is public.

5. **Transparency:** All applications (approved, rejected, and pending) are tracked publicly so the vetting process is auditable.

---

## 3. Application Requirements

A solver operator applying for community vetting must submit (via a GitHub issue or other public venue designated by issue #112) the following:

### 3.1 Identity & Organizational Information

- **Full name or organization name** under which the solver operates.
- **Registered legal entity** (if applicable; e.g., LLC, GmbH, etc.):
  - Country/jurisdiction.
  - Registration number or public URL (e.g., corporate registry link).
  - Beneficial owner(s) (if applicable).
- **Point of contact:** Email and/or Discord handle for communication.
- **Public website or social media** (if any) where solvers can learn more about the operator.

**Why:** Establishes that the operator is identifiable and accountable, not anonymous. Does not require DAO or enterprise structure—a solo operator with a verifiable identity is acceptable.

### 3.2 Operational History

- **Track record predating Vortex Protocol:**
  - Other protocols/platforms where the operator has run bots, solvers, or market-making services (e.g., "ran a solver on protocol X from Month/Year to Month/Year").
  - Links to public evidence (e.g., a GitHub account, published research, community references).
  - **Rationale:** Solvers with a history of responsible operation elsewhere are lower-risk than brand-new operators, even if their Vortex-specific reputation is still building.

- **Activity on Vortex Protocol:**
  - How long the solver's address has been registered on-chain.
  - Approximate volume filled to date (or range, if exact data is private).
  - Any slash events or on-chain disputes (disclosed transparently).

### 3.3 Operational-Security Self-Attestation

A written statement (500–1500 words) addressing:

- **Key management:** How the solver stores and rotates signing keys. (E.g., "hardware wallet for production keys," "multi-sig approval for fund transfers," "key rotation quarterly.") No need to disclose the actual key structure, but enough detail that a reviewer can assess whether the operator takes this seriously.

- **Incident response:** If a key were compromised, what is the procedure? (E.g., "pause all activity, coordinate with Vortex admins via designated emergency contact, rotate keys.")

- **Fund custody:** Where are solver-controlled funds held? (E.g., "in a multi-sig account," "in a custodian like Fireblocks," "in a cold storage account accessed only for withdrawals.") Reassurance, not perfection—even a single signer account is acceptable if the operator is transparent about it.

- **Monitoring and alerting:** How does the solver detect a failed fill or other anomaly? (E.g., "automated monitoring of fill window expiry, Slack alerts on slash event.")

- **Public commitment:** A statement affirming that the operator agrees to the values listed in §2 (falsifiable criteria, transparency, revocation clause).

**Why:** This is not a security audit. It is a signal that the operator has thought about these risks and is willing to be held accountable for their practices. A one-paragraph answer is acceptable; opacity is the red flag, not perfection.

### 3.4 References

- **Two or more references** from the operator's past work:
  - A protocol lead, auditor, or other solver from a previous engagement.
  - A public link or contact information for the reference.
  - Short description of what the reference can attest to (e.g., "worked with operator on protocol X; can confirm consistent, responsible operation from 2023–2025").

**Why:** Peer references are lightweight and add credibility without requiring formal credentials.

---

## 4. Vetting Review Process

### 4.1 Application Intake & Triage

1. Applicant submits via a designated GitHub issue template (see §9 for template).
2. A **vetting coordinator** (a community member or Vortex Labs designate) acknowledges receipt within 3 business days and confirms the application is complete.
3. If incomplete, the coordinator requests missing information. If complete, the application enters review.

### 4.2 Review & Decision (Target: 10 business days)

1. **Two independent reviewers** (not the same person) are assigned. Reviewers should be:
   - Vortex Protocol contributors (core team, auditors, integrators) or trusted community members.
   - Named publicly so conflicts of interest can be disclosed.
   - Not the solver's employer (to avoid appearance of favoritism).

2. Each reviewer assesses:
   - **Identity verification:** Is the legal entity or individual real and traceable?
   - **References:** Did the references respond? Do they endorse the applicant?
   - **Operational-security statement:** Is it credible? Does it suggest genuine care?
   - **On-chain history:** If the solver has been active on Vortex, is the activity normal and consistent with their stated practices?
   - **Red flags:** Any slash events, governance proposals to revoke vetting, or public disputes?

3. **Approval decision:**
   - If both reviewers approve → solver is vetted.
   - If one approves, one rejects → escalation to a third reviewer or vetting committee (see §4.3).
   - If both reject → application is denied; applicant may reapply in 6 months or after addressing specific feedback.

4. **Feedback:** Accepted or rejected, the applicant receives written feedback (2–3 sentences) explaining the decision.

### 4.3 Escalation & Appeals

If a decision is split or contentious:

1. A **vetting committee** (3–5 trusted reviewers named publicly) makes a final decision.
2. The committee's reasoning is published, allowing the community to audit the decision.
3. A denied applicant can appeal once after 6 months if they believe material new information changes the decision.

---

## 5. Vetting Revocation

A vetted solver's status can be revoked if:

1. **Slash event:** A solver incurs a slash (10% bond penalty for missing a fill window). The on-chain event is public; the vetting coordinator flags it and a reviewer decides whether revocation is warranted.
   - **Threshold:** A single slash does not trigger automatic revocation, but it triggers a review conversation. Multiple slashes (e.g., 3+ in 30 days) or a pattern of slashing may warrant revocation.

2. **Misrepresentation:** If a solver's disclosed identity, history, or operational practices are found to be false or misleading after vetting, revocation is recommended.

3. **Changed operational practices:** If a vetted solver materially changes their key management or fund custody (e.g., moves to a less secure approach) and refuses to update their attestation, revocation can be proposed.

4. **Governance proposal:** Any community member can propose revoking a solver's vetting status (following the RFC process in issue #112, if that process applies to vetting changes). A proposal is treated like a vetting appeal: evidence is gathered, and a decision is made by a reviewer or committee.

### 5.1 Revocation Process

1. A reviewer or community member flags the solver for potential revocation.
2. The vetting coordinator notifies the solver and gives them 7 calendar days to respond.
3. If the solver provides a satisfactory explanation (e.g., "the slash was due to a network issue and I've updated my monitoring since"), the review is closed.
4. If no satisfactory response, a reviewer makes a revocation decision, with public reasoning.
5. The solver is removed from the Verified Solvers list and notified. They may reapply after 6 months if circumstances change.

**Transparency:** Every revocation is logged (with a brief reason) in the public list, so the community understands that the vetting program has teeth and is not a rubber stamp.

---

## 6. Public Verified Solvers List

A public file (`docs/SOLVERS.md` or a page consuming issue #13's on-chain enumerable solver list) maintains:

### 6.1 Format

```markdown
# Verified Solvers

Last updated: 2026-10-15

## On-Chain Eligible (Not Community-Vetted)

All registered solvers (`is_solver_eligible() == true`) that are **not** listed below are on-chain eligible but not yet community-vetted. They can accept intents and participate fully in the protocol.

**Data source:** issue #13 enumerable solver registry (updated from on-chain `list_solvers()` every 24 hours).

---

## Community-Vetted Solvers

| Solver Address | Operator Name | Verified On | Vetting Expires | Notes |
|---|---|---|---|---|
| `GXXXXXX...` | Example Solver LLC | 2026-09-15 | Annual review required | Operates across 3 protocols; multi-sig fund custody |
| `GYYYY...` | Solo Operator Alice | 2026-08-20 | Annual review required | Track record on protocol X; self-custodied; slash-free record |

---

## Recently Revoked

| Solver Address | Operator Name | Revoked On | Reason |
|---|---|---|---|
| `GZZZZ...` | Former Solver Inc | 2026-09-10 | Multiple slash events (3 in 7 days) and unresponsive to review |

---

## Vetting Criteria

See [`docs/verified-solver-program.md`](./verified-solver-program.md) for full details.

## How to Apply

1. Open a GitHub issue with title `[Verified Solver Application] Your Operator Name`.
2. Follow the template in [`docs/SOLVERS.md#Application-Template`](./SOLVERS.md#Application-Template).
3. A reviewer will contact you within 3 business days.

---

## Reviewers & Governance

**Current Vetting Committee:**
- Alice (Vortex Labs, lead coordinator)
- Bob (Independent auditor)
- Carol (Community contributor)

This committee meets quarterly to review new applications, revocation proposals, and process improvements.

**Process updates:** Changes to this vetting program follow issue #112 (RFC governance process).
```

### 6.2 Update Cadence

- **New verifications:** Listed within 48 hours of approval.
- **Revocations:** Logged within 48 hours with reason.
- **Annual review:** Every 12 months, vetted solvers' vetting status is reviewed (no new application needed, but continued compliance with the attestation is expected).

### 6.3 Distinction from On-Chain Eligibility

The list **explicitly states:**
> "All registered solvers (`is_solver_eligible() == true`) that are not listed below are on-chain eligible but not yet community-vetted."

This phrasing ensures users never mistake eligibility for vetting, and it celebrates the on-chain data as the source of truth for who can participate, while the vetting list is purely informational.

---

## 7. Vetting Does Not Gate On-Chain Participation

**Critical:** This program is informational and reputational only. It does **not** modify `accept_intent()` or any other on-chain logic.

A solver who is **not** vetted:
- ✅ Can still call `accept_intent()` and fill intents.
- ✅ Has the same fill-window bonuses as a vetted solver (based on issue #57 tier, not vetting status).
- ✅ Can still earn reputation and accumulate volume.

A solver who is **rejected** or **revoked:**
- ❌ Cannot be re-approved for 6 months (for rejected applicants) or after a review period (for revoked solvers).
- ✅ Can still call `accept_intent()` and fill intents on-chain (vetting revocation does not disable on-chain access).

**Rationale:** The on-chain protocol is open and permissionless. Vetting is an optional trust signal, not a gate.

---

## 8. Coordination with Other Issues

- **Issue #13 (Enumerable solver registry):** When issue #13 ships, the Verified Solvers list will cross-reference on-chain addresses from issue #13's enumerable list for easy lookup.
- **Issue #41 (Leaderboard tool):** The leaderboard can display a "verified" badge or filter alongside reputation scores, giving users a multi-signal view.
- **Issue #57 (Reputation tier badge):** Tier badges are on-chain and score-derived; vetting is off-chain and human-judged. Both are presented to users, but they answer different questions.
- **Issue #112 (RFC governance process):** Applications and revocations follow issue #112's discussion and proposal norms if they become contentious.

---

## 9. Application Template

```markdown
# [Verified Solver Application] Your Operator Name

## Identity & Organization

- **Operator Name:** (individual or legal entity name)
- **Jurisdiction:** (country/registration info if applicable)
- **Point of Contact:** (email/Discord)
- **Website/Social:** (optional)

## Track Record

### Off-Vortex History
- Protocol/platform 1: Dates, activities, references
- Protocol/platform 2: Dates, activities, references

### On-Vortex History
- Solver address: `GXXXXXX...`
- Registration date: YYYY-MM-DD
- Approximate volume: (range acceptable)
- Slash events: (none / describe any)

## Operational-Security Attestation

[500–1500 words addressing key management, incident response, fund custody, monitoring, and public commitment]

## References

1. **Name, Role, Contact Info**
   - Description of past engagement

2. **Name, Role, Contact Info**
   - Description of past engagement

---

**Applicant signature/confirmation:** I affirm that the above information is accurate and I agree to the terms in `docs/verified-solver-program.md`, including revocation for misrepresentation.
```

---

## 10. Initial State & Pilot

**Phase 1 (Months 1–3 after adoption):**
- The vetting program is announced and open to applications.
- Reviewers are named and trained on the criteria.
- The first 3–5 applications are expected (conservative, early adoption).
- The program operates as described in §1–§9.

**Phase 2 (Months 4–12):**
- Vetting is routine; the list grows to 10–20 verified solvers.
- Revocation triggers are tested (e.g., first revocation due to slash event).
- Community feedback is gathered on the criteria and process.

**Annual review (Year 2):**
- Lessons learned from Phase 1–2 are documented.
- The program is refined (if needed) via issue #112's RFC process.
- Vetting expiration is introduced for long-standing verified solvers (e.g., annual recertification).

---

## 11. References

- `docs/solver-integration-guide.md` — Integration guide (cross-reference vetting in the "finding a solver" section).
- `docs/solver-registry-design.md` — Solver registry design (cross-reference vetting as a complementary trust signal).
- Issue #13 — Enumerable solver registry (on-chain source of truth for solver addresses).
- Issue #41 — Leaderboard tool (can display vetting status alongside reputation scores).
- Issue #57 — Reputation tier badge (on-chain tier, distinct from off-chain vetting).
- Issue #112 — RFC governance process (appeals and revocation proposals follow this process).
