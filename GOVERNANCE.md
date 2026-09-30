# Protocol Governance — Vortex Intent Settlement

This document establishes the off-chain process for governance decisions — changes to
protocol parameters, admin actions, and upgrades — that precedes the on-chain timelocked
confirmation mechanism.

---

## Table of Contents

1. [Overview](#overview)
2. [What Requires a Proposal](#what-requires-a-proposal)
3. [Proposal Process](#proposal-process)
4. [Discussion and Deliberation](#discussion-and-deliberation)
5. [The 48-Hour Timelock](#the-48-hour-timelock)
6. [Emergency Exception](#emergency-exception)
7. [Proposal Template](#proposal-template)

---

## Overview

The contract's **on-chain timelock** (`ADMIN_TIMELOCK_DELAY = 48 hours`) provides affected
parties a window to react before an admin action goes live. This governance document
establishes the **off-chain half** — the discussion and deliberation period *before* an
admin even calls `propose_*`.

Why both?

- **On-chain timelock** ensures technical feasibility for someone to pause/rollback if
  the proposal turns out to be harmful.
- **Off-chain RFC process** ensures the community understands why a proposal is being
  made and has had a chance to raise concerns *before* the timelock starts.

Together they transform the timelock from "react fast" to "react with full information."

---

## What Requires a Proposal

A formal RFC proposal is required for any of the following admin actions:

- `propose_fee_recipient(new_fee_recipient)` — changes where protocol fees and slash
  proceeds are sent.
- `propose_admin_transfer(new_admin)` — transfers admin control.
- `propose_upgrade(new_wasm_hash)` — upgrades the contract logic.
- `propose_add_dst_token(token)` — adds a destination token to the allowlist.
- `propose_remove_dst_token(token)` — removes a destination token from the allowlist.
- `set_config(...)` — any change to protocol constants (bond multipliers, TTL settings,
  timelock duration, etc.). Note: `set_config` does not have an on-chain timelock yet
  but will once [#35](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/35)
  and [#43](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/43) land.

A proposal is **not** required for:

- **Emergency pause.** `pause()` is for active incident response and cannot wait for
  deliberation. Use it for imminent threats (e.g., a discovered exploit) and inform
  stakeholders immediately after.
- **Routine operations** (registering a solver, submitting an intent, accepting an intent).
  These are permissionless or self-service actions, not governance decisions.

---

## Proposal Process

### Phase 1: Proposal Writeup (Required)

Before calling any `propose_*` or `set_config`, open a GitHub Issue using the
[Governance Proposal template](../.github/ISSUE_TEMPLATE/governance-proposal.md)
with:

1. **Proposal title**: Short, descriptive name (e.g., "Add Tether (USDT) to destination
   token allowlist").
2. **Rationale**: Why this change is needed. What problem does it solve? Who benefits?
3. **Scope**: Exactly what is being changed. Parameter names, old value, new value.
4. **Trade-offs**: What is given up? Cost, risk, or flexibility loss?
5. **Affected parties**: Users, solvers, fee recipients, liquidity providers — who has
   a stake?
6. **Rollback plan**: How would we undo this if it goes wrong? Can it be reverted
   immediately or does it require another proposal?

### Phase 2: Community Discussion (Minimum 3 Business Days)

The proposal issue stays open for **at least 3 business days** before an admin may call
`propose_*`. This window allows:

- Solvers to flag operational concerns.
- Users to ask questions about impacts on their workflows.
- Maintainers to refine scope based on feedback.
- Stakeholders to voice concerns or request data (e.g., "show me impact analysis on fees").

If no concerns emerge and the proposal gains informal approval from maintainers and key
stakeholders (e.g., active solver partners), the proposal can move to Phase 3.

If concerns are raised:
- The issue stays open until concerns are addressed (refined proposal, more data,
  or a decision to proceed despite the risk).
- If the concern is significant enough that the proposal should not proceed, maintainers
  close the issue with rationale.

### Phase 3: Call Propose (On-chain Proposal)

Once the discussion window closes and consensus is reached:

1. An admin calls the on-chain `propose_*` function.
2. The contract records the proposal and starts the `ADMIN_TIMELOCK_DELAY` (48 hours).
3. Link the on-chain transaction hash in the GitHub issue as a comment.

### Phase 4: The 48-Hour Reaction Window

During the timelock, anyone with an interest can:

- Review the on-chain transaction.
- Prepare a rollback or mitigation strategy.
- Post concerns in the GitHub issue (new replies are still welcome).
- If necessary, pause the contract via `pause()` to halt the proposal.

At the end of the 48-hour window, the admin calls the corresponding `confirm_*` function
to execute the change.

### Phase 5: Execution and Post-mortems

Once executed:

1. Document the change in `CHANGELOG.md`.
2. If the change has user-facing impact (new allowlist tokens, fee change), update
   relevant docs and notify integrators.
3. Monitor metrics and incident reports for the next 24–48 hours.
4. If problems emerge, be prepared to execute an immediate emergency pause.

---

## Discussion and Discussion Management

**Where discussions happen:**

- GitHub Issues (primary, linked from above).
- GitHub Discussions (if the org enables them).
- For time-sensitive concerns, escalation to the Slack/Discord channel
  (if one exists).

**Who participates:**

- Maintainers (required to initiate and shepherd proposals).
- Active solver partners (encouraged to attend and voice concerns).
- Protocol users (welcome to comment, though many may not actively follow).
- Security auditors or consultants (if hired for a major upgrade).

**Moderation:**

- Keep discussions focused on the proposal's merits, risks, and trade-offs.
- Ad-hominem or off-topic comments may be removed or the discussion moved to
  a private channel if consensus cannot be reached.

---

## The 48-Hour Timelock

The on-chain `ADMIN_TIMELOCK_DELAY` (48 hours) is enforced by every `propose_*` function
in the contract. An admin cannot skip it or shorten it.

**Why 48 hours?**

- Long enough for concerned parties to coordinate a response (pause call, legal action,
  multi-sig rejection).
- Short enough that urgent operational changes can still execute within a business day
  or two.
- Matches the typical incident response window in a live system.

**Who can block a proposal during the timelock?**

1. **The admin itself** — a multi-sig admin can reject the proposal by refusing to sign
   the `confirm_*` call.
2. **The pause mechanism** — any holder of an emergency pause key can call `pause()`,
   which will halt the protocol but does not unwind a pending proposal. A pause buys time
   to coordinate rollback or legal action.

---

## Emergency Exception

The `pause()` entrypoint **does not require an RFC or timelock**. It is for immediate
incident response (e.g., a discovered exploit) and may be called by designated emergency
admins without prior discussion.

**When to use emergency pause:**

- A security vulnerability has been discovered and exploitation is imminent.
- An attacker is actively exploiting a bug and tokens are at risk.
- A critical dependency (e.g., Stellar network) is compromised.

**When not to use:**

- A solver has a high default rate (use `deregister_solver` or bond slashing).
- A token was added to the allowlist in error (wait for the timelock; it's only 48 hours).
- A fee amount seems too high (discuss in an issue and propose an adjustment normally).

**After using emergency pause:**

1. Inform all stakeholders immediately (GitHub issue, email, Discord, etc.).
2. Within 24 hours, open an issue documenting what triggered the pause and the recovery plan.
3. Do not leave the protocol paused indefinitely — either resolve the issue and unpause,
   or go through a normal governance cycle to make a permanent change.

---

## Proposal Template

See [`.github/ISSUE_TEMPLATE/governance-proposal.md`](../.github/ISSUE_TEMPLATE/governance-proposal.md)
for the GitHub issue template. When opening a proposal, use the template to ensure all
required information is present.

### Example Proposal Structure

**Title:** Add Tether (USDT) to Destination Token Allowlist

**Rationale:** 
Many users want to swap Ethereum ETH for Stellar USDT. Currently, USDT is not on the
allowlist and cannot be a destination. This blocks an entire category of intent.

**Scope:**
- Call `add_allowed_dst_token` with the Stellar SAC address for USDT (CBT...).
- This is an append-only operation; other tokens remain unchanged.

**Trade-offs:**
- Pro: Opens a new trading corridor, increases protocol volume.
- Con: Introduces a new token to monitor; if the SAC has issues, users can target it
  and be at risk.

**Affected Parties:**
- Users: Now able to request USDT destination.
- Solvers: New profit opportunity if the spread is favorable.
- Protocol: Minimal risk — the allowlist is opt-in and users control their own selection.

**Rollback Plan:**
- Call `remove_allowed_dst_token(USDT)` immediately if issues emerge.
- Existing USDT intents in `Accepted` state are not affected; only *new* intents
  cannot target USDT.

---

## Reference

- [**`.github/workflows/ci.yml`**](../.github/workflows/ci.yml) — CI/CD pipeline (no
  enforcement of governance rules; governance is off-chain).
- [**`SECURITY.md`**](../SECURITY.md) — Trust assumptions and threat model, including
  admin key custody.
- [**`docs/114-multisig-admin-design.md`**](../docs/114-multisig-admin-design.md) —
  How admin privileges are structured and how multi-sig can be used.
- [**`CONTRIBUTING.md`**](../CONTRIBUTING.md) — On-chain code review and PR process.
- [**Issue #35**](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/35),
  [**Issue #43**](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/43) —
  Forthcoming timelocked `set_config` and bond-multiplier changes.
