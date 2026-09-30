# Vortex Contracts — Bug Bounty Program

> **Status:** Draft / unfunded. No treasury payouts are authorized.
> `REAL_MONEY=false` until treasury infrastructure (see issue #37) can
> settle rewards. Platform listing (Immunefi, HackenProof, or similar) is
> a separate business decision and is out of scope for this document.

**Tracking issue:** [#298](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/298)

This document defines the Vortex Protocol's security bug bounty program, scope, and reward structure. It is grounded in the threat model and assets documented in `SECURITY.md`.

---

## Overview

The Vortex Protocol handles real economic value: solver bonds (≥ 50 USDC per solver), unbounded user swap output, protocol fees (0.05% of filled volume), and admin privileges with griefing-and-fee-theft capability. This bug bounty program incentivizes external security researchers to identify and responsibly disclose vulnerabilities before they can affect mainnet users and solvers.

**Eligibility:** Anyone may participate, except core team members and auditors of record (see Conflict of Interest, below).

---

## 1. Scope

### 1.1 In scope

- Soroban contracts under `contracts/` at the **pinned audit commit or
  release tag** named by `docs/pre-deploy-security-checklist.md`.
- Only findings against that frozen tag qualify for a reward. Findings
  against `main` after the freeze are informational unless they still
  reproduce on the frozen tag.
- Impact must land on an asset in the Assets at Risk table (solver bonds,
  user swap output, protocol fees, or admin privileges beyond the
  documented blast radius).
- This program covers the `intent_settlement` contract and the `proof_registry` contract (when deployed per issue #190).

### 1.2 Out of scope

- Issues already recorded in `SECURITY.md` **Known Limitations**, or any
  open Security & Auditing issue. Duplicates are not eligible for a
  reward (contribution credit only, at maintainer discretion).
- `vortex-backend`, `vortex-frontend`, DNS, CI secrets, dependency CVE
  intake with no contract exploit path, and social engineering.
- Scenarios that assume a compromised admin key **within** the blast
  radius documented in `SECURITY.md` (“What a compromised admin key can
  and cannot do”). Escalation *beyond* that table may qualify.
- Theoretical style / best-practice notes with no concrete exploit path
  and no effect on an in-scope asset.
- Testnet-only behaviour that does not reproduce on the frozen tag.

---

## 2. Severity tiers (Assets at Risk)

Source assets (see `SECURITY.md`): solver bonds (≥ `MIN_BOND`, currently
50 USDC per solver), user swap output (unbounded), protocol fees, admin
privileges. Trust Assumptions §1–5 still apply.

Severity is determined by the maximum plausible economic impact and blast radius, mapped directly to `SECURITY.md`'s Assets at Risk table.

| Severity | Definition | Example |
|---|---|---|
| Critical | Direct theft or unauthorized drain of solver bonds or user swap output; diversion of protocol fees to an attacker. | Steal bonded USDC via unauthorized `slash_solver` / withdraw; `compute_intent_id` collision (issue #82) if shown to redirect `dst_token` output to the attacker. |
| High | Permanent freeze or lockup of in-scope funds; bypass of bond slashing; admin escalation beyond the documented can/cannot table. | Overflow (issue #84) if shown to brick settlement or lock bonds; privilege change that grants an unlisted mint or withdraw. |
| Medium | Bounded griefing or DoS inside the documented admin blast radius; bounded fee manipulation; oracle / slippage loss with a cap. | `compute_intent_id` collision that only reverts (no theft); griefing `accept_intent` so the fill window (`FILL_WINDOW`, currently 300s) expires and the intent is retried. |
| Low / Informational | Non-exploitable deviation, missing events, gas griefing, hardening notes. | Unchecked return with no fund impact; lint-only findings. |

Rubric: **impact × exploitability**. A report needs a PoC (unit/integration
test or a transaction trace against the frozen tag). Maintainers will
sanity-check open items such as #82 and #84 against this table; those
issues are **not** paid as-is.

### Critical — Impact on core protocol assets

**Definition:** A finding that could directly result in:
- Draining solver bonds from the contract account
- Theft of user swap output (tokens transferred via `fill_intent`)
- Diversion of protocol fees or slashed bonds to an unauthorized party
- Arbitrary admin action without proper authorization

**Examples:**
- Integer overflow in bond accounting allowing unbounded slash amounts (#84, had this been undetected)
- Reentrancy enabling unauthorized `transfer_admin` or fee recipient rotation
- Access control bypass allowing non-admin callers to invoke `pause`, `transfer_admin`, or `set_fee_recipient`
- Cryptographic collision in `compute_intent_id` enabling intent substitution or replay (#82, had this been exploitable)

**How to verify:** A working exploit demonstrating the impact on a testnet instance, with documentation of the attack's preconditions.

### High — Direct impact to a single user/solver or temporary protocol halt

**Definition:** A finding that affects:
- A single user's or solver's economic security (e.g., unintended bond slash, fill denial, intent cancellation without user consent)
- The ability to pause the contract and prevent new intents/fills for extended periods (e.g., `pause` trapped by state corruption)
- Griefing or denial-of-service lasting longer than a few hours

**Examples:**
- Logic bug allowing a specific solver to bypass bond requirements
- TTL/deadline off-by-one enabling unintended expiry or fill window extension
- Event emission omission breaking off-chain monitoring that ops relies on for incident response
- DoS in `slash_solver` or `expire_intent` allowing a specific intent to block all subsequent calls

**How to verify:** A testnet reproduction showing the specific impact (e.g., a solver unable to fill, an intent stuck past expiry, an admin action failing).

### Medium — Griefing, information disclosure, or design-logic inconsistency

**Definition:** A finding that:
- Enables temporary griefing (e.g., repeated intent cancellations, temporary stalling of a single solver)
- Discloses unnecessary information (e.g., leaking solver identity through event ordering, exposing internal state in error messages)
- Violates the documented trust assumptions (e.g., a finding that allows a theoretically non-existent attack path per `SECURITY.md` to actually happen)
- Inconsistency between on-chain behavior and documentation (e.g., a comment claiming immutability that isn't actually enforced)

**Examples:**
- A condition allowing an admin to exceed the documented "griefing and fee theft, not direct fund theft" blast radius (e.g., a new path to user fund theft not listed in the "What a compromised admin key can do" table)
- Leaking solver bond amounts via event payloads when bonds should be opaque
- Inconsistent validation between `accept_intent` and `fill_intent` on deadline semantics, violating the documented off-by-one consistency
- A Proof Registry integration (issue #190) that claims to verify source-chain deposits but actually accepts fabricated proofs

**How to verify:** Documentation of the specific inconsistency or griefing path, with a testnet demonstration if applicable.

### Low — Minor inefficiencies, typos, or best-practice deviations

**Definition:** A finding that does not materially affect security but improves code quality or operational clarity.

**Examples:**
- Unused code paths or dead variables
- Typos or grammatical errors in documentation
- Deviation from Soroban or Stellar best practices that doesn't enable an attack but could in a future contract version
- Missing rustdoc on public functions

---

## 3. Rewards

Pending treasury funding (issue #37), rewards are **tier-ordered
commitments**, not dollar promises:

| Tier | Band (proposed, pending funding) |
|---|---|
| Critical | Tier 1 (highest) |
| High | Tier 2 |
| Medium | Tier 3 |
| Low / Informational | Tier 4 (recognition / thanks) |

- Final amounts will be written into this document when
  `REAL_MONEY` is set to `true`. Nothing in this draft authorizes a
  payout.
- Severity is assigned by maintainers using §2. Disputes get a second
  reviewer who must cite the Assets at Risk table and Trust Assumptions
  in `SECURITY.md`.
- One reward per root cause. The first valid report wins; later
  duplicates are closed.

---

## 4. Rules

- **Novelty.** The finding must not already be a Known Limitation or an
  open issue. A PR that restates a tracked bug does not earn a bounty.
- **No mainnet harm.** Do not extract funds, hold intents or bonds
  hostage, or publicly disclose before a fix plus a 30-day embargo
  (or earlier maintainer approval).
- **Disclosure path.** Follow the reporting process in
  [`SECURITY.md`](../SECURITY.md). Do not file a public issue for an
  unfixed in-scope vulnerability.
- **Safe harbor.** Good-faith research that stays in scope, does not
  steal or extort, and does not violate privacy will not be referred
  for legal action by the maintainers. This is not legal advice and is
  pending legal review.
- **SLA (target, not a contract).** Triage acknowledgement within 5
  business days; severity assignment within 15 business days.

---

## 5. Submission template

1. Affected contract, frozen tag/commit, and function.
2. Asset impacted (row in the Assets at Risk table) and Trust Assumption
   violated, if any.
3. PoC (test or trace) against the frozen tag.
4. Impact write-up and proposed severity from §2.
5. Duplicate check: Known Limitations and open Security issues searched
   (list issue numbers).

---

## 6. Activation

The program **activates** at the mainnet-deploy freeze described in
`docs/pre-deploy-security-checklist.md` and
`docs/mainnet-deployment-runbook.md`. Until that freeze, reports are
still welcome under `SECURITY.md` but are informational unless they are
re-validated on the frozen tag after activation.

---

## Out of Scope

The following are **not** eligible for bounty rewards:

1. **Known Limitations.** Issues already documented in `SECURITY.md`'s "Known Limitations" section are known trade-offs and not vulnerabilities. Examples:
   - Cross-chain proof is opt-in; the contract's self-reporting model is intentional (until issue #190 ships `ProofRegistry`).
   - Single admin key (expected until issue #36 ships multisig wrapper).
   - No allowlist by default (a documented risk; enabling the allowlist is the required mitigation).
   - Bond slash is proportional (per issue #193 — this is a design choice with documented tradeoffs).

2. **Already-tracked issues.** Issues in this repository's public issue list, whether open or closed, cannot qualify for a new bounty. If a finding duplicates or significantly overlaps an already-reported issue, the researcher should engage with that issue's existing discussion rather than filing a separate bounty claim. This prevents double-counting and ensures credit goes to the original reporter.

3. **Mainnet-specific configuration errors.** Misconfiguration of the contract during deployment (e.g., wrong fee recipient address set at `initialize`, or allowlist populated with the wrong token addresses) is an operational issue, not a contract vulnerability. Raise these via the responsible-disclosure process in `SECURITY.md`.

4. **Pre-mainnet versions.** Vulnerabilities in testnet-only contracts are not in scope. Focus on the mainnet-deployed contracts once live.

5. **Social engineering or off-chain attacks.** Phishing, key compromise via social engineering, or attacks on infrastructure (RPC endpoints, CI/CD, GitHub) are outside the scope of this contract-code program.

6. **Subjective design critiques.** Disagreement with architectural decisions (e.g., "the contract should use a different curve for economic modeling") is not a security finding unless the architecture provably violates the documented threat model.

---

## Conflict of Interest

The following groups **cannot** claim bounties:

- **Core team members** and maintainers of the Vortex Protocol (defined as active contributors with merge access to this repository).
- **Auditors** with a current engagement to audit `vortex-contracts` or any related Vortex component (as of the finding date).
- **Initial solver partners** with a financial arrangement that includes protocol revenue-sharing, during the term of their arrangement.

If you are unsure whether you qualify, ask before investing time in a submission.

---

## Submission and Payout Process

### 1. Responsible Disclosure

**Do not** open a public GitHub issue for security vulnerabilities. Follow the process in `SECURITY.md`'s "Reporting a Vulnerability" section: email `security@vortex-protocol.dev` with:

- A clear title and description of the vulnerability.
- Proof-of-concept code or a detailed reproduction on testnet.
- Your preferred payout method (USDC, ETH, or equivalent).
- Your GitHub username (or pseudonym) for CHANGELOG credit.

### 2. Triage and Investigation

The security team will:

1. Confirm receipt within 2 business days.
2. Verify the finding on testnet or mainnet (as applicable).
3. Assign it to a severity tier (Critical, High, Medium, Low).
4. Provide a timeline for patch development and disclosure.

If the finding is confirmed as in-scope, you will be notified of the tier and reward range.

### 3. Patch and Disclosure

For **Critical** and **High** findings:
- A patch will be developed and deployed to mainnet within the timeline communicated (typically 1–2 weeks).
- A postmortem will be published per issue #301 once the incident is resolved.
- You will be credited in the postmortem and CHANGELOG.

For **Medium** and **Low** findings:
- Fixes will be batched into a regular release cycle (typically 2–4 weeks).
- You will be credited in the CHANGELOG and in this bug bounty program's hall of fame (optional).

### 4. Reward Payment

Once the patch is merged and released, the reward is transferred to your nominated address:

- **USDC** — preferred, transferred on Stellar mainnet or Ethereum (your choice).
- **ETH or other ERC-20** — available on request.
- **Fiat (USD)** — subject to exchange-rate lock at payout time and applicable tax forms.

Payment is made within 5 business days of patch release.

---

## Examples: Mapping Real Findings to Severity Tiers

To ground this program in concrete examples, here are real issues from the vortex-contracts tracker and how they would be rated:

### Hypothetical: #82 (compute_intent_id collision audit)

If the audit had uncovered a collision vulnerability instead of confirming safety:

- **Severity:** Critical (enables intent substitution and replay, violating the immutability assumption).
- **Reward:** $40,000 – $50,000 (minimal additional conditions needed; high impact).
- **Verification:** Testnet demonstration of creating two distinct intents with the same ID, triggering unintended fills.

### Hypothetical: #84 (overflow audit)

If overflow was discovered in bond accounting instead of being confirmed safe:

- **Severity:** Critical (allows unbounded slashing and bond draining).
- **Reward:** $25,000 – $30,000 (requires specific bond and intent size setup, but impact is severe).
- **Verification:** Testnet setup with specific bond/intent amounts triggering the overflow and draining solver collateral.

### Hypothetical: Deadline off-by-one

A finding that `accept_intent` and `fill_intent` use inconsistent `<=` vs. `<` checks, allowing a fill at the exact deadline in one but not the other:

- **Severity:** High (affects individual solvers/users through unintended expiry or fill denial).
- **Reward:** $7,500 – $12,000 (affects specific intents, requires multi-step reproduction).
- **Verification:** Testnet demonstration with a tightly-timed fill call at the deadline boundary.

### Hypothetical: Event omission

Admin rotation (`transfer_admin`) fails to emit an `admin_transferred` event, breaking ops monitoring:

- **Severity:** Medium (violates documented threat model; ops cannot detect key compromise).
- **Reward:** $1,200 – $2,000 (violation of trust assumption, but no direct economic loss).
- **Verification:** Testnet call to `transfer_admin` with verification that no event is emitted.

---

## Hall of Fame

Researchers who report confirmed vulnerabilities will be credited here (with permission):

*To be updated as findings are resolved.*

---

## FAQ

**Q: Can I claim a bounty for an issue I co-authored with a core team member?**

A: No. Findings that involve core team input should be disclosed through internal channels first. If you discover a bug independently and the team has not yet published it, you can submit for bounty consideration, but reaching out first prevents duplicate reporting.

**Q: What if my finding affects the `proof_registry` contract after issue #190 ships?**

A: Proof Registry vulnerabilities fall within this program once the contract is deployed to mainnet. Follow the same submission and severity-tier process.

**Q: Does this program cover Wormhole or other dependencies?**

A: No. Vulnerabilities in Wormhole, Soroban, the Stellar protocol itself, or other external dependencies should be reported to their respective security teams. If a vulnerability in a dependency creates a new attack vector specific to Vortex, we may consider it in scope — ask first via `security@vortex-protocol.dev`.

**Q: Can I publish my finding before the patch is released?**

A: No. The responsible-disclosure timeline (typically 1–2 weeks for Critical findings) ensures the team has time to patch. Publishing before the patch would expose users and solvers to immediate risk. Violations of responsible disclosure may forfeit the bounty and result in legal action.

**Q: Do I need to be a professional auditor to claim a bounty?**

A: No. Anyone (individual researchers, teams, independent security engineers) may participate.

---

## References

- `SECURITY.md` — threat model, assets at risk, and trust assumptions
- `docs/mainnet-deployment-runbook.md` — incident-response procedures
- `docs/110-monitoring-alerting-spec.md` — ops signals that trigger incident response
- `docs/incident-postmortem-template.md` — postmortem process for disclosed incidents
