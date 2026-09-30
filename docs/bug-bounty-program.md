# Vortex Contracts — Bug Bounty Program

> **Status:** Draft / unfunded. No treasury payouts are authorized.
> `REAL_MONEY=false` until treasury infrastructure (see issue #37) can
> settle rewards. Platform listing (Immunefi, HackenProof, or similar) is
> a separate business decision and is out of scope for this document.

This program covers the on-chain settlement surface in this repository:
intent lifecycle, solver bonds, slashing, and protocol fees. Severity is
defined against the Assets at Risk table, Trust Assumptions, and the
admin-key blast-radius table in [`SECURITY.md`](../SECURITY.md).

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

## 2. Severity tiers (Assets at Risk)

Source assets (see `SECURITY.md`): solver bonds (≥ `MIN_BOND`, currently
50 USDC per solver), user swap output (unbounded), protocol fees, admin
privileges. Trust Assumptions §1–5 still apply.

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

## 5. Submission template

1. Affected contract, frozen tag/commit, and function.
2. Asset impacted (row in the Assets at Risk table) and Trust Assumption
   violated, if any.
3. PoC (test or trace) against the frozen tag.
4. Impact write-up and proposed severity from §2.
5. Duplicate check: Known Limitations and open Security issues searched
   (list issue numbers).

## 6. Activation

The program **activates** at the mainnet-deploy freeze described in
`docs/pre-deploy-security-checklist.md` and
`docs/mainnet-deployment-runbook.md`. Until that freeze, reports are
still welcome under `SECURITY.md` but are informational unless they are
re-validated on the frozen tag after activation.
