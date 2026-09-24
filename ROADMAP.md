# Vortex Protocol Public Roadmap

**Last updated:** 2026-09-24

This document consolidates forward-looking work, deferred features, and known limitations scattered across `SECURITY.md`, design docs, and the README into a single, community-visible roadmap. It is a **thematic, high-level view** of what the team considers important future work, organized by category and current status.

**Status legend:**
- 🔄 **In Progress:** Active development or design underway.
- 🎯 **Not Started:** Scoped and ready to implement; awaiting resources or dependencies.
- 🔍 **Under Research:** Still in design/spike phase; requirements not yet final.
- ✅ **Shipped:** Implemented and live on mainnet (or testnet).
- 🚫 **Out of Scope (v1):** Deferred to a future release; design exists but not a priority for v1.

---

## 1. Cross-Chain Proof & Verification

**Goal:** Cryptographic verification of source-chain deposits using Wormhole or similar, to replace the current economic-only trust model.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| Proof Registry contract & interface | 🎯 Not Started | Contract to store and serve verified Wormhole VAAs; on-chain reference for `fill_intent(require_proof=true)` | #190, #124 |
| Proof verification integration with `fill_intent` | 🔄 In Progress | Optional on-chain gate: `fill_intent(..., require_proof=true)` checks the `ProofRegistry` before allowing a fill | #190 |
| Proof mismatch fallback & recovery | 🔄 In Progress | Handling for edge cases: proof arrives late, is invalid, or conflicts with a fill | #129 |
| Cross-chain oracle pricing (cross-token value comparison) | 🔍 Under Research | Currently slash amounts assume same-token or admin-set minimums; a price oracle would enable true cross-token comparisons | SECURITY.md, #193 |
| Multi-chain proof aggregation research | 🚫 Out of Scope (v1) | Extending beyond Wormhole to other proof sources; deferred until v1 stability is proven | |

**Coordination:** Issue #124 is the main implementation tracker. See also `docs/124-proof-verification-interface.md` for design.

---

## 2. Solver Reputation & Governance Infrastructure

**Goal:** Establish on-chain solver reputation tracking and off-chain governance signals to enable community-driven solver oversight.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| Solver Registry contract & enumerable list | ✅ Shipped | On-chain registry with paginated `list_solvers(start, limit)` for real-time solver discovery | #13, #197 |
| Solver reputation score computation | ✅ Shipped | `compute_reputation_score` based on fills completed, fills failed, and total volume | #41, #197 |
| Reputation-tier badge (on-chain, score-derived) | 🎯 Not Started | NFT-style on-chain tier display (Unranked / Bronze / Silver / Gold / Platinum) based on reputation score | #57 |
| Leaderboard & ranking tool (off-chain) | 🎯 Not Started | Public tool for sorting solvers by reputation, volume, and other signals; feeds tier-badge data | #41 |
| Governance-weight formula | 🎯 Not Started | Define "reputation × bond" or similar weighting for community signaling; used by reputation-weighted governance tool | #41, #305 |
| Off-chain reputation-weighted signaling tool | 🎯 Not Started | Tool for signed off-chain messages expressing community preference on governance proposals, weighted by governance-weight formula | #305 |
| Verified Solver program (off-chain vetting) | 🎯 Not Started | Human-reviewed endorsement of solver identity and operational practices; distinct from on-chain eligibility | #303 |
| Staking & multi-bond support | 🚫 Out of Scope (v1) | Optional solver collateral tiers and tiered slash rates; deferred pending tokenomics review | #60 |
| Fee rebate for high-volume solvers | 🚫 Out of Scope (v1) | Tiered fee discounts; implementation deferred to tokenomics phase | docs/solver-registry-design.md §8 |

**Coordination:** Issues #41, #197, #303, and #305 form a cohesive work stream. See `docs/solver-registry-design.md` for design.

---

## 3. Community Governance & Treasury

**Goal:** Establish transparent, community-governed processes for protocol decisions and fee allocation.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| RFC governance process (issue discussions + consensus) | 🎯 Not Started | Lightweight, GitHub-based proposal discussion framework for all major protocol decisions | #112 |
| Admin multisig design (Stellar native) | ✅ Shipped | Recommendation to use Stellar multi-sig accounts for admin key security; no code changes required | #114 |
| Timelocked admin operations | ✅ Shipped | `propose_*` / `execute_*` pattern for fee recipient, admin transfer, and dst_token changes with 14-day timelock | #115, #116, #118 |
| Treasury contract & spending governance | 🎯 Not Started | On-chain treasury with timelock and multisig gating; off-chain governance process for spending proposals | #37, #302 |
| Arbiter election & dispute resolution (future) | 🚫 Out of Scope (v1) | Reputation-weighted jury for user-solver disputes; deferred pending solver reputation stabilization | docs/dispute-resolution-design.md |

**Coordination:** Issues #112, #302 establish the governance processes. Issue #37 builds the on-chain treasury contract. See `docs/114-multisig-admin-design.md`, `docs/treasury-spending-governance.md`.

---

## 4. Monitoring, Alerting & Observability

**Goal:** Comprehensive visibility into protocol health, solver performance, and potential incidents.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| Monitoring & alerting spec | 🔍 Under Research | Define ops signals: fees, slashes, solver bonding, fill-window utilization | #110 |
| Per-entrypoint resource-cost tracking | 🔍 Under Research | Benchmark and publish gas/size costs for each contract entrypoint | #149 |
| Event coverage audit & logging | ✅ Shipped | Review of event emission to ensure indexers can derive full protocol state from events alone; fixes for gaps | #111 |
| Solver bond & liquidity dashboards | 🎯 Not Started | Public dashboards showing aggregate bonded USDC, solver participation, fill volume over time | #110 (follow-up) |
| Cross-chain proof arrival latency tracking | 🎯 Not Started | Once issue #190 ships, monitor Wormhole VAA latency and fallback frequency | |

**Coordination:** Issue #110 is the main tracking issue. See `docs/110-monitoring-alerting-spec.md` for details.

---

## 5. Contract Upgrades, Maintenance & Technical Debt

**Goal:** Maintain and enhance the core settlement and registry contracts with minimal disruption.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| In-place contract upgrade mechanism | ✅ Shipped | `propose_upgrade` / `execute_upgrade` with 14-day timelock; one-time storage migration hook | #194 |
| Storage layout & footprint reviews | ✅ Shipped | Audits of instance storage, `SolverRecord`, `IntentRecord` to ensure TTL and cost efficiency | #144, #147 |
| TTL bump frequency & cost analysis | ✅ Shipped | Review of soroban TTL bump costs and recommendations for bump scheduling | #145 |
| Batch operation support | ✅ Shipped | `batch_submit_intent`, `batch_accept_intent`, `batch_fill_intent`, `batch_cancel_intent` for multi-operation atomicity | #199 |
| Wasm size budget & optimization | 🎯 Not Started | As new features (proof verification, registry) are integrated, optimize contract size to stay within limits | #149 |
| Slash-cooldown and reputation exploit closure | ✅ Shipped | Prevent solvers from resetting reputation by deregistering/re-registering; preserve reputation snapshot across cycles | #272 |
| Bond slash formula refinement | 🔍 Under Research | Current slash is proportional to intent size and bond (10% cap); evaluate if formula balances incentives well across bond sizes | #193 |

**Coordination:** Issue #194 is the main upgrade tracker. See `docs/149-resource-cost-per-entrypoint.md` for resource benchmarks.

---

## 6. Protocol Limits & Operational Parameters

**Goal:** Document and optimize protocol-wide constraints.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| Fill window duration (currently 300 seconds) | 🔍 Under Research | Evaluate if 5 minutes is optimal; consider source-chain settlement times and solver latency | docs/bridge-protocol-comparison.md |
| Proof deadline per source chain | 🎯 Not Started | Currently proof-deadline is global; a per-chain parameter would better match settlement times | #190 (follow-up) |
| Max intent batch size | ✅ Shipped | `MAX_BATCH_SIZE` limits atomic batch operations to prevent gas exhaustion; tuned per testnet results | #199 |
| Max slash cycles before abandonment | ✅ Shipped | `ProtocolConfig.max_slash_cycles` caps how many times an intent can be slashed before transitioning to `Abandoned` terminal state | SECURITY.md |
| Protocol fee rate & discount tiers | ✅ Shipped | Base 5 bps (0.05%); admin-tunable via `set_config` with volume-tier rebate structure | README.md, #192 |

**Coordination:** Parameters are tracked through their respective design documents; see `docs/bridge-protocol-comparison.md` for fill-window rationale.

---

## 7. Security & Compliance

**Goal:** Maintain high security posture and support compliance requirements.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| Pre-launch security checklist | ✅ Shipped | Verification steps for admin keys, allowlists, and mainnet configuration | docs/pre-deploy-security-checklist.md |
| Dynamic slash (optional punishment severity) | 🚫 Out of Scope (v1) | Slash rate could vary by intent size or solver history; currently fixed at 10%; deferred pending governance maturity | SECURITY.md |
| Multisig admin (before mainnet) | ✅ Shipped | Recommendation to use Stellar native multisig; documented in design doc | #114 |
| Intent allowlist by default | 🔍 Under Research | Currently allowlist is opt-in; evaluate making it mandatory for mainnet | SECURITY.md |
| Session token storage compliance | 🎯 Not Started | Auth middleware review for regulatory compliance (impacts backend & infrastructure, not this contract) | |

**Coordination:** See `SECURITY.md` for threat model and known limitations. `docs/pre-deploy-security-checklist.md` is a pre-mainnet verification tool.

---

## 8. Integration & Developer Experience

**Goal:** Lower the barrier for solvers, users, and integrators to participate and build on Vortex.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| Solver integration guide | ✅ Shipped | Step-by-step walkthrough for solver bot integration, including event subscription and fill workflow | docs/solver-integration-guide.md |
| Solver registry design documentation | ✅ Shipped | Architecture of on-chain registry and reputation model | docs/solver-registry-design.md |
| Solver registry interface (ABI) | ✅ Shipped | Public Soroban interface for `solver_registry` consumption | docs/solver-registry-interface.md |
| Event topic naming conventions | ✅ Shipped | Best practices for future contract event naming | docs/113-event-topic-naming-conventions.md |
| API examples & usage snippets | 🎯 Not Started | Expand examples/ with more solver and user flow scenarios | |
| SDK/library for signature verification (reputation-weighted signaling) | 🎯 Not Started | Convenience library for off-chain tools to verify Stellar signatures and compute governance weights | #305 |

**Coordination:** See `docs/solver-integration-guide.md` as the starting point. Issue #305's tool will need SDK support.

---

## 9. Future Research & Innovation

**Goal:** Explore emerging technologies and protocol improvements for future versions.

| Item | Status | Details | Issue(s) |
|------|--------|---------|----------|
| Dynamic pricing & variable fees | 🔍 Under Research | Could the protocol dynamically adjust fees based on volume, latency, or other signals? Currently fixed base rate with volume tiers | #192 |
| Batch intent settlement (optimistic rollup-style) | 🔍 Under Research | Instead of per-intent settlement, batch multiple intents and reduce cross-chain proof overhead | |
| Solver-to-solver delegation (intent routing) | 🔍 Under Research | Could a solver accept an intent but farm it to a different solver (on-chain)? Raises reputation attribution questions | |
| DAO governance & on-chain voting | 🚫 Out of Scope (v1) | Currently admin is single-sig or multisig; full DAO governance is deferred pending stabilization | #114 |
| Cross-protocol composability (e.g., Vortex → Vortex → external DEX) | 🚫 Out of Scope (v1) | Chaining intents across protocols; deferred pending architecture stabilization | |
| Zk-proof-based solver reputation (privacy-preserving) | 🚫 Out of Scope (v1) | Could a solver prove reputation without revealing identity; deferred pending demand signal | |

**Coordination:** These are longer-term explorations; no active issues yet. Feedback on priorities is welcome via governance discussions (issue #112).

---

## 10. Known Limitations (v1)

The following are documented limitations of the current release and known areas for future improvement:

| Limitation | Mitigation / Timeline | Tracking |
|-----------|----------------------|----------|
| **Cross-chain proof is opt-in** | Admin enables `ProofRegistry` link; solvers opt into `require_proof=true` per fill; economic trust model remains default | #190 |
| **Bond slash assumes same-token or admin-set minimums** | Cross-token value comparison via oracle is on roadmap; currently admin manually sets minimum bonds per token | #193, SECURITY.md |
| **Single point of failure in admin key** | v1: hardware wallet recommended. v1 shipped: multisig via Stellar native signers (issue #114). Future: DAO voting. | #114 |
| **No allowlist enforcement by default** | Allowlist is opt-in via `set_dst_allowlist_enabled(true)`. Recommendation: enable before mainnet launch. | SECURITY.md |
| **Intent can cycle through slash multiple times** | Bounded by `max_slash_cycles` parameter; capped at (default) 5 cycles before transitioning to `Abandoned` state | SECURITY.md |
| **Fill window is global (currently 300 seconds)** | Optimal for Ethereum; sub-optimal for some L2s or slower chains. Per-chain parameterization is future work. | docs/bridge-protocol-comparison.md |
| **No on-chain dispute resolution** | Dispute resolution (user vs. solver claims) is deferred; currently requires off-chain mediation or community arbitration | docs/dispute-resolution-design.md |
| **Solver reputation resets across deregister/re-register cycles** | **Fixed in v1.2 (#272):** Reputation snapshot is preserved across the cycle | #272 |

---

## 11. How This Roadmap Is Maintained

This roadmap is updated **whenever a major item ships or its status changes significantly**. A change triggers:

1. An update to this file with the new status and date.
2. A corresponding `CHANGELOG.md` entry.
3. A notification to the community (via governance discussion if formal consensus is needed).

**Contributors:** This roadmap is read-only here; feature requests and priorities are discussed via issue #112's RFC process.

---

## 12. Cross-References & Related Documents

- **SECURITY.md** — Threat model, trust assumptions, known limitations, and recommendations.
- **docs/114-multisig-admin-design.md** — Admin key architecture and rationale.
- **docs/110-monitoring-alerting-spec.md** — Observability and alerting framework.
- **docs/124-proof-verification-interface.md** — Cross-chain proof integration design.
- **docs/solver-registry-design.md** — Solver reputation model and registry.
- **docs/dispute-resolution-design.md** — Future dispute resolution architecture.
- **docs/treasury-spending-governance.md** — Treasury governance process.
- **docs/verified-solver-program.md** — Off-chain solver vetting program.
- **README.md** — Feature summary and contract overview.
- **issues.md** — Granular, per-task issue list (distinct from this high-level roadmap).

---

## 13. Feedback & Contributing

Have thoughts on priorities? Spot a missing roadmap item? Open a discussion via issue #112 (RFC governance process) or comment on the relevant issue linked in this roadmap.
