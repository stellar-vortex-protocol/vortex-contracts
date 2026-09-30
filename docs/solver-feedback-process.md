# Solver Feedback Process

## Overview

The Vortex Protocol depends on solvers as a first-class operational constituency. This document establishes a dedicated, lightweight feedback channel for solvers to raise concerns about the protocol's operational constraints, incentive structures, or market design—distinct from bug reports and distinct from the formal [Slash Appeal Process](./dispute-resolution-design.md).

This channel is a **listening mechanism**, not a voting mechanism. Solver feedback informs protocol decisions and potential governance proposals (see the RFC process in [CONTRIBUTING.md](../CONTRIBUTING.md)), but does not automatically trigger changes. The goal is to make the channel a credible, responsive commitment: solvers know their operational concerns will be heard and considered, even if not all concerns result in protocol changes.

---

## Scope

### In Scope: Solver Feedback

- **Operational constraints**: "The fill window is systematically too short for route X," "bond requirements disproportionately affect smaller solvers"
- **Market design concerns**: "Fee tiers don't account for Y external cost," "the incentive structure disadvantages solver type Z"
- **Observability gaps**: "I can't reliably monitor bond health across all my positions," "there's no way to detect collateral sufficiency trends"
- **Route or market coverage**: "There's a profitable route no solver currently serves; protocol design might explain why"
- **Real-world operational friction**: Any concern stemming from actually running a solver in production or testnet

### Out of Scope: Slash Appeals & Governance

- **Slash event appeals**: Use the [Slash Appeal Process](./dispute-resolution-design.md) for contesting a specific slashing incident
- **Bug reports**: Use the standard Bug Report template; solver feedback is not for bug triage
- **Formal governance proposals**: Use the RFC process (see [CONTRIBUTING.md](../CONTRIBUTING.md)) if your feedback has matured into a concrete proposal affecting protocol rules, contract behavior, or resource allocation

---

## Process

### 1. Filing Solver Feedback

1. Open a new issue in [stellar-vortex-protocol/vortex-contracts](https://github.com/stellar-vortex-protocol/vortex-contracts).
2. Choose the **Solver Feedback** issue template.
3. Fill in:
   - **Concern Category**: Select the type of operational issue (fee structure, timing, bond, route coverage, observability, other)
   - **Operational Context**: Describe what you observed in production/testnet and why it matters
   - **Scale and Impact**: Indicate whether this is a niche issue or systemic
   - **Suggested Direction** (optional): Share any thoughts on how the protocol could adapt, but this is input, not a binding proposal

4. Label the issue `solver-feedback`.

### 2. Triage & Response

**Triage window**: Solver feedback issues are triaged within **7 calendar days** of filing.

**Triage response** includes:

- **Acknowledgment**: Issue author receives a comment confirming receipt.
- **Initial categorization**:
  - Is this a known operational constraint already documented (e.g., fill-window timing is specified in [solver-integration-guide.md](./solver-integration-guide.md))?
  - Is this actionable feedback that might inform a governance proposal?
  - Is this better handled as a bug report or formal governance request?
  - Is this out-of-scope solver feedback (e.g., a slash appeal misrouted here)?
- **Next steps**:
  - If actionable and in-scope: the issue remains open and tagged `solver-feedback` while the maintainers consider escalation to the RFC process or future protocol versions.
  - If already documented/resolved: explain the existing design rationale and close with a reference to relevant docs.
  - If this should be a formal governance proposal: suggest the RFC process and note how the feedback could feed into it.
  - If out-of-scope: redirect to the appropriate process and close.

### 3. Escalation to Governance

If solver feedback identifies a concern that merits formal protocol change:

1. Raise an RFC issue (see [CONTRIBUTING.md](../CONTRIBUTING.md)) that explicitly references the original solver feedback.
2. Link back from the original feedback issue to the RFC, so both form an audit trail.
3. The RFC process then follows the standard governance flow.

### 4. Documenting Rationale

If a piece of solver feedback is **not** escalated to governance (e.g., a concern about the fill window that is actually a designed constraint), the triage response documents why. This keeps the feedback channel honest: solvers see that their concerns are considered, even when the answer is "this is by design for reason X."

---

## Review Cadence

| Cadence | Responsibility |
|---------|---|
| **Triage**: Within 7 calendar days | Protocol maintainers |
| **Consideration & escalation decision**: Within 14 calendar days | Core team and governance group |
| **Monthly summary**: Optional | Maintainers share aggregated solver feedback themes in dev notes or governance updates |

---

## Responsibilities

### Solvers (Issue Filers)

- Use this channel for operational concerns, not bug reports or governance proposals
- Be specific and concrete about the constraint you're encountering
- Indicate scale: is this affecting you alone, or a broader solver population?
- Be respectful; this is a listening channel, not a negotiation

### Protocol Maintainers

- Triage all solver-feedback issues within 7 days
- Explain design rationale when feedback reflects existing constraints
- Escalate to governance when feedback merits formal consideration
- Keep the channel responsive; a multi-week silence erodes the channel's credibility

### Governance Group (Issue #117's process, once adopted)

- Review solver-feedback issues during formal RFC discussions if escalated
- Factor solver operational insights into governance decisions
- Document how solver feedback influenced (or didn't) a final decision

---

## Edge Cases

### Case 1: Solver Feedback That Should Be a Slash Appeal

**Scenario**: A solver files feedback that is actually about contesting a specific slash event.

**Resolution**: Maintainer redirects to the [Slash Appeal Process](./dispute-resolution-design.md) and closes the feedback issue, explaining the distinction.

### Case 2: Feedback About an Already-Open RFC or Governance Proposal

**Scenario**: Solver feedback surfaces an operational concern that a current RFC is already addressing.

**Resolution**: Link both issues together; the solver feedback becomes input to the ongoing RFC discussion rather than spawning a new parallel process.

### Case 3: Systemic Feedback That Reveals a Design Gap

**Scenario**: Multiple solvers independently report the same operational constraint (e.g., "fill window is too short for route X").

**Resolution**: Maintainers or governance participants can aggregate these issues, recognize a pattern, and escalate a single well-scoped RFC rather than leaving multiple disconnected feedback issues open. The aggregation is transparent (linked issues).

### Case 4: Feedback With No Clear Resolution Path

**Scenario**: A solver raises a legitimate concern (e.g., "smaller solvers feel disadvantaged") that is more of a long-term design question than a discrete protocol change.

**Resolution**: Triage acknowledges the concern, documents that it's recognized but beyond the scope of near-term governance, and leaves it open as a standing concern for the community to revisit. Closing without resolution is worse than leaving it open to signal "we heard you, and this is a hard problem we're still thinking about."

---

## Relationship to Other Processes

### vs. Bug Reports

- **Bug reports** are code defects, contract logic errors, or observability tools breaking unexpectedly.
- **Solver feedback** is operational concerns or constraints that may be working as designed.

Use the Bug Report template if you've found a defect; use Solver Feedback if you're raising a concern about how the protocol's operational constraints affect your business.

### vs. Slash Appeals

- **Slash appeals** (see [dispute-resolution-design.md](./dispute-resolution-design.md)) contest a specific slash event: "I was slashed unfairly for reason X."
- **Solver feedback** is broader: "the protocol's design makes it hard to avoid slashing for scenario Y."

If you were slashed and want to appeal, use the Slash Appeal process. If you want to raise a concern that affects multiple scenarios, use Solver Feedback.

### vs. RFC / Governance Proposals

- **Solver feedback** is input and listening: "here's what I'm observing operationally."
- **RFC proposals** (see [CONTRIBUTING.md](../CONTRIBUTING.md)) are concrete: "the protocol should change X because Y."

Solver feedback can **feed into** an RFC, but filing feedback isn't the same as proposing a change. Once you have a concrete proposal, file an RFC.

---

## Reference

- [Solver Integration Guide](./solver-integration-guide.md) — operational constraints and integration details
- [Risk-Aware Solver Bot](./risk-aware-solver-bot.md) — example of solver implementation navigating protocol constraints
- [Dispute Resolution Design](./dispute-resolution-design.md) — slash appeals and arbitration (not this channel)
- [CONTRIBUTING.md](../CONTRIBUTING.md) — RFC process for formal governance proposals
- [Slash Appeal Process](./dispute-resolution-design.md#slash-appeal-process) — process for contesting specific slash events

---

## Frequently Asked Questions

**Q: Is this feedback going to actually change the protocol?**

A: Maybe. Solver feedback is input that informs decisions. If enough solvers surface the same operational constraint, or if the concern is particularly acute, it may become an RFC proposal and eventually a protocol change. But feedback alone doesn't commit the protocol to change anything. The channel's value is that your concerns are heard and considered, not that they're guaranteed to result in changes.

**Q: How is this different from just opening a GitHub issue?**

A: A labeled solver-feedback issue signals to maintainers that this is an operational concern from an active solver, not a random feature request. It also triggers a triage commitment (response within 7 days) and a documented escalation path (RFC, documentation update, or closure with rationale). A generic issue might be overlooked; this channel ensures your concern is triaged.

**Q: What if I disagree with the triage decision?**

A: The triage response should explain the maintainer's reasoning. If you believe the reasoning is wrong, you can comment on the issue and ask for reconsideration. If it's a persistent disagreement, you can raise the issue in governance discussions or escalate to the RFC process with your own proposal. The feedback channel is input; RFC is where disagreements become proposals.

**Q: Can I file multiple feedback issues?**

A: Yes, if they address distinct operational concerns. Don't file ten issues about the same fill-window problem; instead, file one and let others comment/react. But if you have concerns about bond requirements, fill windows, and fee tiers, three separate issues is appropriate and helps the community see the breadth of solver concerns.

