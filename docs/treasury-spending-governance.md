# Treasury Spending Governance Process

**Issue:** [#302](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/302)  
**Status:** Proposed  
**Last updated:** 2026-09-24

---

## 1. Overview

This document defines the community process by which accumulated protocol-fee revenue in the treasury is governed and allocated. It is **not** a technical specification of the treasury contract itself (see issue #37 for that), but rather the governance layer—the human process and decision criteria—that the technical contract must enable.

The treasury accumulates protocol fees (`0.05%` of filled intent volume) and slash proceeds (10% of slashed solver bonds). Both streams represent value extracted from the protocol ecosystem. How that value gets deployed back into the ecosystem—funding security audits, ecosystem development, incentive programs, etc.—is the decision space this process governs.

---

## 2. Core Principles

1. **Legitimacy through process, not mechanism alone:** The technical security of the treasury (a timelock, a multisig) is necessary but insufficient. A perfectly secure contract with no community input is technically safer but not more trustworthy.

2. **Conservative default:** Until this process is tested and proven, funds accumulate in the treasury with **no spending authorized**. This is the safe initial stance and an explicit statement that process maturity, not just fund size, gates spending.

3. **Transparency and accountability:** Every proposal, approval, and execution is documented publicly. The community can audit every decision and trace how fees flowed back into the protocol.

4. **Community input before admin action:** Proposals follow an RFC-style discussion period (see section §4 and issue #112) before an admin even initiates a `propose_spending` transaction. This is a cultural commitment, not a technical gate—the contract can move faster than this process dictates, but the community process is the expected and endorsed path.

---

## 3. Valid Spending Proposal Types

A spending proposal is any use of treasury funds. Valid categories include:

| Category | Examples | Constraints |
|----------|----------|-------------|
| **Security & Audits** | Smart contract audits, bug bounties, incident response | Must name the target (e.g. "Halborn audit of intent_settlement v2.0.1") and expected cost |
| **Ecosystem Development** | Solver integration incentives, user onboarding grants, integration-partner subsidies | Must define success metrics and duration (e.g., "6-month solver-adoption rebate program: $X per fill, up to $Y total") |
| **Research & Protocol Innovation** | Cross-chain proof optimization, new fee mechanisms, DAO governance tooling | Must cite a specific design doc or research goal, and a delivery target (e.g., "issue #190: Wormhole proof registry design implementation by Q1 2027") |
| **Community Infrastructure** | Monitoring dashboards, public RPC nodes, indexer deployment | Must specify maintenance owner and SLA (e.g., "public metrics dashboard maintained by Vortex Labs, 99% uptime SLA") |
| **Grants & Bounties** | Retroactive funding for past work, hackathon prizes, ongoing contributor stipends | Must include selection criteria and, for ongoing work, a review cadence (e.g., "quarterly contributor stipend review") |
| **Operations & Admin** | Legal entity fees, compliance costs, insurance | Must be itemized and limited to actual costs, not profit margins |

**Out of scope:** Treasury funds **cannot** be:
- Diverted to private parties with no public accountability.
- Used for short-term price support or market manipulation.
- Allocated to issues not grounded in the Vortex Protocol's technical roadmap or community governance process.

---

## 4. Proposal Process

### 4.1 Discussion Phase (RFC-style, min. 14 days)

Before any `propose_spending` transaction is initiated on-chain, the proposal must follow the RFC process defined in issue #112 (`docs/GOVERNANCE.md`):

1. **Proponent opens a GitHub discussion** (or equivalent, per issue #112) with:
   - Title: `[Treasury Proposal] {proposal name}`
   - Category from §3 (or explain why a novel category applies)
   - Requested amount (in USD or the native treasury token)
   - Itemized justification with link(s) to supporting design docs or research
   - Timeline: when the work starts, expected completion, and reporting intervals
   - Success criteria: how the community will measure whether funds were well-spent

2. **Community discussion window:** 14 days minimum (calendar days, not business days). Solvers, users, and other stakeholders can comment; the proponent refines the proposal in response.

3. **Signal-check (optional, recommended):** Once discussion has stabilized, the proponent may request a reputation-weighted signal via the tool defined in issue #305 (off-chain signaling mechanism). This is **not a gate**—discussion happens first, signaling is a refinement—but it surfaces community preference before the admin commits on-chain.

4. **Approval by multisig signers:** The admin (a Stellar multisig account per issue #114 / `docs/114-multisig-admin-design.md`) reviews the proposal once the discussion period ends and community sentiment is known. The admin makes a **final approval/rejection decision**, with reasoning (e.g., in a GitHub comment tying the approval to the discussion).

### 4.2 On-Chain Timelock Phase (14 days)

Once the admin approves off-chain, they call `propose_spending()` on the treasury contract with:
- Recipient address (the account receiving funds)
- Amount
- A URI pointing to the approved proposal (e.g., the GitHub discussion URL)

The contract enforces a **14-day timelock delay** before the funds can be moved (matching the discussion period, signaling process maturity rather than urgency).

During this window:
- The community has a final opportunity to review the on-chain proposal.
- Any errors or mismatches between the approved proposal and the on-chain execution are public and can be contested (via off-chain discussion; the admin can cancel if convinced a mistake was made).

### 4.3 Execution Phase

After the 14-day timelock expires, the multisig signers call `execute_spending()`, which transfers the approved amount to the recipient address on-chain.

### 4.4 Time-Sensitive Exception (Expedited Path)

Some spending needs are genuinely time-sensitive (e.g., funding an emergency security audit discovered during an incident). For these cases:

1. **Expedited discussion:** A proponent can request an expedited RFC discussion (issue #112 defines approval criteria for expedited status—typically "credible external threat").

2. **Expedited multisig approval:** The admin reviews and approves on an accelerated timeline (no fixed minimum, but logged with reasoning).

3. **Shortened timelock:** The contract supports a `propose_spending_expedited()` variant with a **3-day timelock** (vs. the standard 14 days) for emergency spending up to a **maximum of $50,000 USD equivalent per proposal**. This limit can be raised by governance (it's a parameter in issue #37's contract).

4. **Post-execution report:** The admin **must** publish a post-execution report within 7 days explaining the emergency and outcome.

**Rationale:** True emergencies (e.g., a discovered exploit) need faster response than a 14-day discussion period allows. The 3-day shortened timelock with a cap preserves the multisig-review gate while reducing delay. The post-execution accountability is mandatory to prevent this path from becoming routine.

---

## 5. Approval Thresholds

The treasury multisig account (issue #114 / `docs/114-multisig-admin-design.md`) is the sole authority for initiating spending. The signer threshold is determined by the multisig configuration at deployment time.

**Recommended configuration for mainnet:**
- **Signers:** 5 representatives (team, auditors, community delegates, etc., per governance process in issue #112)
- **Threshold:** 3-of-5 (a supermajority, preventing any single signer from acting alone; not so high that a lost key blocks all spending)

Each signer is expected to:
1. Review the proposal for alignment with the treasury-spending categories (§3).
2. Validate that the community discussion period was observed.
3. Confirm that the proponent's justification is grounded and realistic.
4. Sign the `propose_spending` transaction only if convinced the spend is legitimate.

The multisig account itself is Stellar-native (ed25519 signers), so each signer uses standard Stellar tooling (a keypair or hardware wallet). No custom on-chain voting logic is required.

---

## 6. Reporting and Transparency

### 6.1 Treasury Balance & Inflow Report (Quarterly, Public)

Every quarter (January, April, July, October), a treasury report is published that includes:

- **Opening balance** (in USD equivalent, using end-of-quarter spot rates)
- **Inflows** (protocol fees, slash proceeds, any other revenue)
- **Outflows** (each approved spending, with link to the approved proposal)
- **Closing balance**
- **Agenda for next quarter** (any proposals in discussion, expected timeline)

This report is pinned in the GitHub `docs/` folder (e.g., `docs/treasury-reports/Q4-2026.md`) and cross-referenced from the main `README.md`.

### 6.2 Proposal Tracking

A public checklist (e.g., `docs/treasury-proposals.md` or a pinned GitHub discussion) lists:
- All active and past proposals (title, amount, status: approved/rejected/executed)
- Links to the discussion and on-chain proposal URI
- Current date and expected execution date (if approved but not yet executed)

### 6.3 Post-Execution Report (Per Proposal)

After funds are disbursed, the recipient **must** publish:
- A brief report (within 30 days of execution) confirming receipt of funds
- Progress toward the stated success criteria at 30, 60, and 90 days post-execution
- A final report at project completion

For ongoing work (e.g., a quarterly contributor stipend), reports happen on the cadence defined in the proposal (typically quarterly).

---

## 7. Treasury Technical Primitives (Issue #37 Dependency)

For this governance process to be enforceable on-chain, issue #37's treasury contract must expose:

| Primitive | Used for | Details |
|-----------|----------|---------|
| `propose_spending(recipient, amount, proposal_uri)` | Initiate a spending proposal | Requires admin multisig authorization; stores proposal and timelock deadline |
| `propose_spending_expedited(recipient, amount, proposal_uri, emergency_justification)` | Emergency spending | Requires admin multisig authorization; uses 3-day timelock instead of 14 days; capped at $50k per proposal |
| `execute_spending(proposal_id)` | Finalize a proposal after timelock expires | Transfers funds to recipient; succeeds only if timelock deadline has passed and recipient address is valid |
| `cancel_spending(proposal_id)` | Abort a proposed spending before execution | Requires admin multisig authorization; called if a mistake is discovered during the timelock window |
| `get_spending_proposals(start, limit)` | Enumerate all proposals (active and historical) | Paginated read; allows external indexing and reporting |
| `get_spending_proposal_details(proposal_id)` | Read a specific proposal's state and timeline | Returns recipient, amount, proposal_uri, timelock_deadline, status (proposed/executed/cancelled) |
| `get_treasury_balance()` | Read total treasury balance | Sum of all fees and slashes less all executed spending; used in quarterly reports |

The contract **does not** enforce this governance process (discussion periods, approval thresholds, vetting criteria). It only provides the timelock and multi-sig gating primitives. The governance process lives off-chain in community practice and documented norms.

---

## 8. Governance Evolution & Amendments

This process can be amended by the community via the RFC process in issue #112. A proposal to change the treasury spending rules follows the same discussion and approval process as any other proposal, but:

1. The change is documented in a new version of this document (with a date stamp).
2. The change applies to future proposals, not retroactively to approved or executing proposals.
3. A summary of the change is added to `CHANGELOG.md`.

---

## 9. Transition & Initial State

**Until this process is adopted and tested:**

- No spending is authorized.
- Protocol fees and slash proceeds accumulate in the treasury contract.
- The admin (multisig) can receive fee_recipient updates (issue #115) to route new fees to a designated address, but **no draws from accumulated funds** occur.

**Once this process is ratified (via a governance proposal under issue #112):**

1. The treasury contract (issue #37) is deployed with the first 6 months designated as a **pilot phase**.
2. Proposals in the pilot phase are expected to be small, conservative, and well-researched (e.g., funding a specific, well-scoped audit).
3. After 6 months, the community reviews the pilot: Did the process work? Are amendments needed? Based on feedback, the process either stabilizes or is refined.

This conservative ramp-up ensures the community has time to build confidence in both the process and the technical contract before large sums are deployed.

---

## 10. Related Issues & Coordination

- **Issue #37 (Treasury contract):** This governance process depends on issue #37's timelock and multisig primitives. Coordinate to ensure the contract design supports the human process defined here.
- **Issue #112 (RFC governance process):** All treasury spending proposals follow issue #112's discussion-phase norms.
- **Issue #114 (Multisig admin design):** The treasury is governed by a Stellar multisig account per this design.
- **Issue #305 (Off-chain reputation-weighted signaling):** Provides an optional signal mechanism to measure community preference before multisig approval.

---

## 11. References

- `SECURITY.md` — "Protocol fees" as an Asset at Risk (this process is the answer to that trust problem)
- `docs/114-multisig-admin-design.md` — Multisig admin key architecture
- `GOVERNANCE.md` (issue #112) — Community RFC process for all proposals
- Issue #37 — Treasury contract design & implementation
- Issue #305 — Off-chain reputation-weighted signaling tool
