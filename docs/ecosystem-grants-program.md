# Vortex Protocol Ecosystem Grants Program

## Overview

The Vortex Protocol Ecosystem Grants Program is a treasury-funded initiative to support external contributors building complementary tooling on top of `intent_settlement` and `proof_registry`. This program is one concrete, launchable allocation category within the broader protocol treasury spending governance process (see [issue #117](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/117)).

The program prioritizes **tooling grants** — funding for indexers, monitoring dashboards, solver bots, integration libraries, and extensions to the reference implementations already in this repository. It does not fund solver bonds, core-team runway, or general business development.

---

## Scope & Eligible Projects

### What We Fund

The program funds external-contributor projects that:

1. **Extend reference tooling** already in the Vortex repository:
   - Forks or extensions of `indexer/reference-indexer.js` (e.g., a production-ready hosted indexer service, a time-series database backend, an API layer)
   - Extensions to `examples/risk_aware_solver_bot.py` (e.g., multi-chain liquidity management, real-time PnL tracking, integration with market-making frameworks)

2. **Build new complementary tools** in these categories:
   - **Monitoring & Alerting**: Dashboards tracking protocol health, solver bond totals, intent settlement velocity, or specific route profitability
   - **Integration Libraries**: SDKs or client libraries in underrepresented languages or frameworks (e.g., Go, Rust async libraries, browser-based integrations)
   - **Solver Infrastructure**: Tools for solver fleet management, bond management, or cross-solver coordination
   - **Data & Analytics**: Proof-of-concept indexers, analytics dashboards, or research tools exploring protocol usage patterns

3. **Solve documented protocol gaps** identified in the issue tracker:
   - If an open issue in the repository explicitly flags a tooling gap (e.g., "there is no production indexer," "monitoring queries are too expensive"), a grant application addressing that gap is particularly strong

### What We Don't Fund

- **Core protocol development**: Changes to `intent_settlement`, `proof_registry`, or other on-chain contracts should follow the standard RFC/governance process, not this grants program
- **Solver bonds**: The program does not reimburse solver bond deposits; solvers must post their own collateral
- **Core team funding**: Grants are for external contributors, not for funding the core team's own runway or employment
- **General business development or marketing**: Grants are limited to tooling with clear technical scope and deliverables
- **Duplicative work**: If an open grant or RFC proposal is already addressing the same scope, a new application should coordinate with the existing effort rather than duplicate funding

---

## Program Phases & Timeline

### Phase 1: Governance Adoption (Prerequisite)

This program **explicitly depends on issue #117** (treasury spending governance process). Before grants are awarded:

1. **Issue #117 must be implemented**: The protocol community must adopt a formal spending-governance process. This grants program is one specific instance of that general process.
2. **Treasury must exist**: The protocol's on-chain treasury (described in a separate issue) must be deployed and accumulating funds.
3. **Governance group must be seated**: The decision-making body (however #117 specifies it) must be operational.

**Until these prerequisites are met, the program is documented and applicants may submit early proposals, but no grants will be awarded.**

### Phase 2: Initial Grants Round (Post-Governance)

Once #117 is adopted and the treasury exists, the governance group will open the first grants round with:

- A **call for proposals** (posted in discussions, announced in project updates)
- An **application deadline** (e.g., 30 days from posting)
- A **review & decision period** (e.g., 21 days for governance group evaluation)
- **Grant amounts TBD** by the community via governance; this program does not pre-commit specific fund allocations

### Phase 3: Ongoing Grants

After the initial round, the program operates on:

- **Rolling application windows**: Applicants may submit proposals during open periods, with predictable triage timelines
- **Quarterly decision cycles**: The governance group reviews accumulated applications and makes funding decisions on a quarterly (or community-determined) cadence
- **Transparent tracking**: Approved grants, grant recipients, and completion status are published on a public tracker (see "Accountability & Reporting" below)

---

## Application & Approval Process

### Application Requirements

Applicants submit a proposal (via GitHub issue or a form TBD by issue #117's process) including:

1. **Project Description** (2–3 paragraphs)
   - What tooling are you building or extending?
   - How does it fit into the Vortex ecosystem? (Is it a reference-indexer extension, a solver dashboard, etc.?)
   - Why does the ecosystem need this?

2. **Scope & Deliverables** (bullet list)
   - Concrete, measurable deliverables (e.g., "production-ready indexer with <500ms query latency," "solver bot with automated rebalancing")
   - Success criteria: how will the community know the grant is complete and successful?
   - Timeline: expected completion date and major milestones

3. **Budget**
   - Requested grant amount (in USDC or the treasury's denomination)
   - Budget breakdown (e.g., developer time, infrastructure, external dependencies)
   - Justification for the amount

4. **Team & Experience**
   - Who is building this? (Names, GitHub profiles, or organizational affiliation)
   - Relevant experience: have you built similar tooling? Contributed to Stellar ecosystem projects?
   - Time commitment: is this full-time, part-time, or a one-time project?

5. **Relationship to Existing Work**
   - Does this extend an existing reference implementation (e.g., `indexer/reference-indexer.js`)? If so, how?
   - Is there an open issue in this repository describing this gap? Link it.
   - Are there any other active grants or proposals addressing the same scope? If so, how do you coordinate?

6. **Reporting & Deliverables License**
   - How will you report progress? (Monthly updates to an issue? Public GitHub repository?)
   - Will the final deliverable be open-source? Under what license? (Grants generally expect MIT/Apache 2.0 or equivalent)
   - Will you maintain the tool post-launch, or hand it off to the community?

### Review & Approval

The governance group (once seated per issue #117) reviews applications using these criteria:

1. **Alignment**: Does the project fit the program's scope (tooling grants for reference-implementation extensions or documented gaps)?
2. **Feasibility**: Is the scope realistic given the budget and timeline? Does the team have relevant experience?
3. **Community need**: Is there clear demand for this tool? Does it solve a gap documented in the issue tracker or flagged by multiple community members?
4. **Sustainability**: Is the tool maintainable beyond launch? Is there a clear post-grant support plan?
5. **Budget justification**: Is the requested amount reasonable for the scope?

### Decision & Notification

- The governance group makes a decision within 21 days of the application deadline (or per #117's process)
- Accepted grants are announced publicly; rejected proposals receive feedback explaining the decision
- Grant agreements (if needed) specify:
  - Grant amount and payment schedule (e.g., upfront, milestone-based, post-completion)
  - Reporting requirements and cadence
  - Intellectual property and open-source licensing
  - Conditions for reclaiming funds if deliverables are not met

---

## Accountability & Reporting

### Grant Recipients

Recipients commit to:

1. **Transparent progress updates** (monthly or per the grant agreement):
   - Posted to the original issue or grant-tracker repository
   - Accessible to the community
   - Including blockers, adjustments to scope, and completion estimates

2. **Final completion writeup** (within 7 days of launching the tool):
   - How was the tool built? What were key decisions or challenges?
   - How does it integrate with the broader Vortex ecosystem?
   - Maintenance & support: how long will you maintain it? How should users report issues?
   - Link to the public GitHub repository (or equivalent) and user documentation

3. **Open-source release** (default expectation):
   - Code is published under an open-source license (MIT, Apache 2.0, or equivalent)
   - Clear documentation for users and future contributors
   - Existing reference implementations (e.g., `indexer/reference-indexer.js`) should be listed as prior art / inspiration

### Protocol Maintainers

Maintainers commit to:

1. **Transparent grant tracking**: A public tracker (GitHub project, discussion board, or published table) listing:
   - Approved grants, recipients, grant amounts
   - Status (in-progress, completed, inactive)
   - Links to progress updates and final writeups

2. **Responsive communication**: Governance group responds to grant-related questions within 7 days

3. **Community feedback loop**: Lessons learned from each round (e.g., "most grants overran by 20%; budget planning needs adjustment") are documented and incorporated into future rounds

---

## Edge Cases & Scope Boundaries

### Overlap with Existing Issues

**Scenario**: An applicant proposes building the tooling that issue #32 (hypothetically, a "real indexer service") is already supposed to deliver.

**Resolution**: 
- If issue #32 is an open RFC being actively pursued by core team, suggest the applicant coordinate with that effort (co-fund, collaborate, or build a complementary layer)
- If issue #32 is stalled or not being actively pursued, the grant application is stronger because it fills a gap
- The governance group evaluates whether funding a grant duplicates or complements the existing work

### Grants Addressing Governance-Process Feedback

**Scenario**: A piece of [Solver Feedback](./solver-feedback-process.md) escalates into a potential grant (e.g., "solvers need better bond-monitoring tooling" → a grant for a dashboard).

**Resolution**:
- Link the grant application back to the original solver feedback issue
- The governance group can cite solver feedback in its approval decision, signaling that community input shaped funding priorities
- This makes the feedback loop credible: solvers see their concerns translate into action

### Long-Term Maintenance & Hand-Off

**Scenario**: A grant recipient builds a great tool but announces they're moving on 6 months later.

**Resolution**:
- The grant agreement should specify an expected maintenance window (e.g., "12 months of support, then community-maintained")
- If the tool is critical (e.g., the only indexer), the governance group can approve a follow-on grant for ongoing maintenance or seek a new maintainer
- This prevents "grant-and-abandon" where useful tools bitrot

---

## Examples of Grant-Eligible Projects

To help applicants understand scope, here are concrete examples:

### Example 1: Production Indexer Service (Extension Grant)

**What**: Build on `indexer/reference-indexer.js` to create a publicly-hosted indexer service with SLA guarantees.

**Scope**:
- Migrate reference indexer to production-ready stack (e.g., Node.js + PostgreSQL + GraphQL API)
- Expose common queries (list_intents, get_solver, protocol_stats) with <500ms latency
- Publish SLA: 99.5% uptime, rate-limited to 100 reqs/sec per API key
- Complete within 6 months

**Budget**: $50,000 USDC

**Reporting**: Monthly progress updates, weekly status in Slack, final writeup with usage metrics

### Example 2: Solver Real-Time Dashboard (New Tool)

**What**: Build a monitoring dashboard for solvers tracking their individual and aggregate performance.

**Scope**:
- Real-time display of open intents, accepted intents, fill success rate
- Historical PnL chart, bond health monitor, slashing alerts
- Supports 1–10 connected solvers via read-only API
- Complete within 3 months

**Budget**: $15,000 USDC

**Reporting**: Monthly progress, public GitHub repo, final writeup with video demo

### Example 3: Go Integration Library (New Tool)

**What**: Build an idiomatic Go client library for the Vortex Protocol.

**Scope**:
- Type-safe contract calls, event parsing, account management
- Example solver bot in Go
- Full integration tests against testnet
- Complete within 4 months

**Budget**: $20,000 USDC

**Reporting**: Monthly updates, public GitHub repo, Go documentation, final writeup with usage examples

### Example 4: Cross-Protocol Liquidity Manager (Solver Infrastructure)

**What**: Build a solver tool for optimally routing fills across multiple Vortex clusters (if multi-instance deployment exists).

**Scope**:
- Aggregate intent discovery across clusters
- Liquidity-pool rebalancing logic
- Performance benchmarks vs. single-cluster operation
- Complete within 5 months

**Budget**: $30,000 USDC

**Reporting**: Monthly progress, open-source code, final analysis report

---

## Dependency on Issue #117

**This program is explicitly a specific instance of issue #117's spending-governance process.**

What this means:

1. **Process consistency**: Any approval/rejection processes or governance-group roles defined in #117 also apply to ecosystem grants decisions
2. **Treasury dependency**: Grants are paid from the same treasury that #117 establishes
3. **Community voice**: The same community mechanism that #117 uses for spending decisions applies to grants
4. **Reporting & accountability**: Grant-tracking uses the same transparency/accountability standards #117 establishes

If issue #117 specifies different triage timelines, decision bodies, or reporting requirements, this program adopts those standards for grants.

---

## Roadmap

1. **Now**: Document this program (this file)
2. **Pending #117**: Once #117's spending-governance process is adopted, announce the first grants round
3. **Month 1–3 (pilot)**: Accept and review a small initial batch of proposals; make 2–3 grants to test the process
4. **Month 4+**: Transition to ongoing rolling applications with quarterly decision cycles
5. **Annual review**: Assess program effectiveness (Are tools being built? Is the community using them? Is the process transparent?) and adjust

---

## FAQ

**Q: How much budget is available for grants?**

A: That's determined by the community via issue #117's spending-governance process, not by this program. Once the treasury exists, the governance group allocates a portion to ecosystem grants; the remainder funds operations, core development, or other priorities.

**Q: Can I apply for a grant while employed by the core team?**

A: Ecosystem grants prioritize external contributors. If you're on the core team and building reference tooling, that's part of your regular responsibilities, not a grant. If you're interested in funding a side project, coordinate with project leadership first.

**Q: What if I want to build something not in the listed categories?**

A: You can still apply — the listed categories are not exhaustive. In your application, explain why your project fits the spirit of the program (complementary tooling that extends the protocol ecosystem). The governance group evaluates based on alignment with program goals.

**Q: What happens if I miss a milestone?**

A: That depends on the grant agreement. Typically, grants have milestone-based payments (e.g., 30% upfront, 40% at mid-point, 30% on completion). Missing a milestone triggers a review: did you hit a blocker? Do you need a scope adjustment or extension? Communication is key — if you're going to slip, flag it early.

**Q: Can I fork this program for another protocol?**

A: Absolutely — this program is open-source documentation. Feel free to adapt it for your own ecosystem and attribution where relevant.

**Q: Who decides if a project qualifies?**

A: The governance group (once seated per issue #117), using the Review Criteria outlined in the "Application & Approval Process" section.

