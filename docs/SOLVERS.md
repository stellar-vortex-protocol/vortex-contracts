# Verified Solvers

Last updated: 2026-09-24

---

## Overview

This page distinguishes **on-chain eligible** solvers from **community-vetted** solvers in the Vortex Protocol.

- **On-chain eligible:** Any solver that passes `is_solver_eligible()` in the `intent_settlement` contract (meets minimum bond, passes reputation checks). These solvers can accept intents and participate fully, regardless of vetting status.
- **Community-vetted:** A subset of on-chain eligible solvers who have completed the Verified Solver Program (see `docs/verified-solver-program.md`). Vetting is an optional, informational trust signal, not a permission gate.

See [`docs/verified-solver-program.md`](./verified-solver-program.md) for the full vetting criteria and process.

---

## On-Chain Eligible (Not Community-Vetted)

All registered solvers with `is_solver_eligible() == true` that are **not** listed in the Community-Vetted section below can accept intents and participate fully in the protocol.

**Data source:** Issue #13 enumerable solver registry. The authoritative, real-time list of eligible solvers is available on-chain via `list_solvers(start, limit)`.

To check if a specific solver address is eligible:
```bash
# Example: Query a deployed contract for solver eligibility
stellar contract invoke --id <CONTRACT_ID> --source <YOUR_KEY> -- \
  is_solver_eligible --solver <SOLVER_ADDRESS>
```

---

## Community-Vetted Solvers

| Solver Address | Operator Name | Verified On | Status | Notes |
|---|---|---|---|---|
| *(none yet)* | *(accepting applications)* | — | — | First vetting cohort to be added as applications are approved. |

---

## Recently Revoked

| Solver Address | Operator Name | Revoked On | Reason |
|---|---|---|---|
| *(none yet)* | — | — | — |

---

## Vetting Committee & Reviewers

**Current vetting committee:**
- *(To be appointed during governance phase; see issue #112)*

This committee oversees new applications, revocations, and process improvements. Committee membership is public and updated here when changes occur.

---

## How to Apply for Vetting

### Step 1: Prepare Your Application

Gather the following information (see [`docs/verified-solver-program.md`](./verified-solver-program.md#9-application-template) for detailed requirements):

1. **Identity & Organization:**
   - Legal name or organization name
   - Jurisdiction (if registered)
   - Point of contact (email/Discord)

2. **Track Record:**
   - Off-Vortex history: other protocols/platforms where you've operated
   - On-Vortex history: your solver address, registration date, approximate volume, any slash events

3. **Operational-Security Attestation:**
   - A written statement (500–1500 words) on key management, incident response, fund custody, and monitoring

4. **References:**
   - Two or more references from past engagements (with contact info)

### Step 2: Submit via GitHub

1. Go to [GitHub Issues](https://github.com/stellar-vortex-protocol/vortex-contracts/issues).
2. Click **New Issue**.
3. Use this template:

```markdown
# [Verified Solver Application] Your Operator Name

## Identity & Organization

- **Operator Name:** 
- **Jurisdiction:** 
- **Point of Contact:** 
- **Website/Social:** 

## Track Record

### Off-Vortex History
- Protocol 1: [Dates, activities, references]
- Protocol 2: [Dates, activities, references]

### On-Vortex History
- Solver address: `G...`
- Registration date: YYYY-MM-DD
- Approximate volume: 
- Slash events: 

## Operational-Security Attestation

[Your 500–1500 word statement on key management, incident response, fund custody, monitoring, and commitment to the vetting program]

## References

1. **Name, Role, Contact**
   - Description of engagement

2. **Name, Role, Contact**
   - Description of engagement

---

**Applicant confirmation:** I affirm that the above is accurate and agree to the terms in `docs/verified-solver-program.md`.
```

### Step 3: Vetting Review

1. A **vetting coordinator** will acknowledge your application within 3 business days.
2. Two independent **reviewers** will assess your application (target: 10 business days).
3. You'll receive written feedback and a decision (approved, rejected, or pending further review).

For details on the review criteria and appeals process, see `docs/verified-solver-program.md` §4.

---

## Vetting Criteria

At a glance:

- **Identity:** Must be verifiable (individual with known identity or legal entity).
- **Track record:** Evidence of responsible operation on other platforms (solvers with history are lower risk; new operators are accepted if other factors are strong).
- **Operational security:** A credible commitment to key management, incident response, and fund custody (transparency is valued over perfection).
- **References:** Peer endorsement from past engagements.

Vetting is **not**:
- A technical security audit.
- A performance guarantee.
- A permission for on-chain participation (that's determined by `is_solver_eligible()` alone).

---

## Revocation & Appeals

A solver's vetting status can be revoked if:

- **Slash events:** Multiple slashes (3+ in 30 days) may trigger revocation after review.
- **Misrepresentation:** False identity, history, or operational practices discovered after vetting.
- **Changed practices:** Material reduction in operational security without updating the attestation.

**Revocation process:**
1. A reviewer flags the solver and gives them 7 days to respond.
2. If no satisfactory response, the solver is removed and notified.
3. Reapplication is possible after 6 months or after circumstances change.

**Appeals:** A rejected or revoked solver can appeal once after 6 months.

For full details, see `docs/verified-solver-program.md` §5.

---

## Vetting Status vs. On-Chain Eligibility

| | On-Chain Eligible | Community-Vetted |
|---|---|---|
| **Can accept intents?** | ✅ Yes (via `accept_intent`) | ✅ Yes (via `accept_intent`) |
| **Can fill intents?** | ✅ Yes (via `fill_intent`) | ✅ Yes (via `fill_intent`) |
| **Can earn reputation?** | ✅ Yes (via volume/slashes) | ✅ Yes (via volume/slashes) |
| **Who determines eligibility?** | On-chain contract logic (`is_solver_eligible`) | Community reviewers (off-chain) |
| **Impacts on-chain behavior?** | ❌ No (vetting is informational) | ❌ No (vetting is informational) |

**Key point:** Vetting does not gate on-chain participation. It is a trust signal for users and integrators, not a permission mechanism.

---

## Integration & Reference

- **In `docs/solver-integration-guide.md`:** Recommend that new solvers consider the vetting program as a way to build trust with users.
- **In `docs/solver-registry-design.md`:** Note that vetting is a complementary trust signal to on-chain reputation scores.
- **In leaderboard tools (issue #41):** Optionally display a "verified" badge or filter option.

---

## History & Process Updates

- **2026-09-24:** Vetting program announced and accepting applications.
- *(Future entries will document changes, revocations, and process refinements.)*

For governance of the vetting program itself, see issue #112 (RFC governance process).

---

## Questions?

For questions about:
- **Vetting criteria or process:** See `docs/verified-solver-program.md`.
- **On-chain solver eligibility:** See `intent_settlement/src/lib.rs` (`is_solver_eligible`).
- **Governance and appeals:** See issue #112.
