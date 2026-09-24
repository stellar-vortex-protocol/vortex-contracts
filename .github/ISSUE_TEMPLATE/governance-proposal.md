---
name: Governance Proposal
about: Propose a protocol parameter change or admin action requiring deliberation
title: "[PROPOSAL] "
labels: governance-proposal
assignees: ""
---

## Proposal Summary

Brief one-sentence summary of the change (e.g., "Add USDT to destination token allowlist").

---

## Rationale

**Why is this change needed?** What problem does it solve? Who benefits?

Provide context: Is this a response to user feedback, a security improvement, an operational
optimization? Link any related issues, discussions, or audit findings.

---

## Scope

**What exactly is being changed?** Be specific.

Examples:
- "Call `propose_add_dst_token(USDT_SAC_ADDRESS)` where USDT_SAC_ADDRESS = `CBT...`"
- "Increase `MIN_BOND` from 50 USDC to 100 USDC via `set_config`"
- "Transfer admin control to a new 2-of-3 multisig account"

Include parameter names, old values, and new values. If multiple settings are being changed,
list each one explicitly.

---

## Trade-offs

**What do we gain? What do we give up?**

| Aspect | Benefit | Cost/Risk |
|--------|---------|-----------|
| User experience | ... | ... |
| Solver economics | ... | ... |
| Protocol security | ... | ... |
| Operational complexity | ... | ... |

Be honest about downsides. If there are none, say so explicitly.

---

## Affected Parties

**Who has a stake in this change?**

- [ ] Users (affected how?)
- [ ] Solvers (affected how?)
- [ ] Fee recipient / protocol treasury
- [ ] Integrators / dApp partners
- [ ] Other: ___

---

## Rollback Plan

**How would we undo this if it goes wrong?**

- Can it be reverted immediately (e.g., `remove_allowed_dst_token`)?
- Does reverting require another proposal cycle, or is it instant?
- Would reverting affect in-flight intents?

If there is no safe rollback path, state that explicitly and explain why the risk is acceptable.

---

## Compliance with Governance

By opening this proposal, I confirm that:

- [ ] This action requires a governance proposal (it is one of `propose_fee_recipient`,
      `propose_admin_transfer`, `propose_upgrade`, `propose_add_dst_token`,
      `propose_remove_dst_token`, or `set_config` — see `GOVERNANCE.md`).
- [ ] I have read `GOVERNANCE.md` and understand the 3-business-day discussion window
      and 48-hour on-chain timelock.
- [ ] I am not requesting this as an emergency pause (which bypasses governance;
      use `pause()` only for active incident response).

---

## Discussion

Leave this section empty. Stakeholders will comment below with questions, concerns, or support.

Once the 3-business-day discussion window closes and consensus is reached, an admin will:
1. Call the on-chain `propose_*` or `set_config` function.
2. Link the transaction hash in this issue as a comment.
3. The 48-hour timelock begins.
