# Custody Transparency — Admin and Fee Recipient Keys

**Tracking issue:** [#299](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/299)

This document publicly discloses the custody setup for the `admin` and `fee_recipient` keys in the live Vortex Protocol deployment. The custody model is grounded in `SECURITY.md`'s "Admin Key Operational Security" section and is continuously updated whenever key rotation or custody changes occur.

---

## Current Custody Status

**Last verified:** This document is updated operationally within the same window as any key rotation event. The date below reflects the last time keys were rotated or custody model was re-verified.

**Last verified date:** Pre-mainnet (not yet deployed to mainnet)

---

### Admin Key

| Property | Value |
|----------|-------|
| **Custody model** | Single-key (hardware wallet) — pre-multisig state |
| **Key holder(s)** | Vortex Protocol core team lead (pseudonymous, hardware-wallet-backed) |
| **Threshold** | 1-of-1 (single signature required) |
| **Hardware** | Ledger Nano S/X |
| **Network** | Staging / Pre-mainnet only |

**Status note:** The single-key model is documented in `SECURITY.md` as a known limitation (see "Known Limitations" § "Single admin key"). Transition to a 2-of-3 or 3-of-5 multisig is tracked in issue #36 (`docs/114-multisig-admin-design.md`). A multisig wrapper will be deployed before mainnet launch.

**Expected transition to multisig:** Prior to mainnet deployment. Once multisig is live, this table will be updated to reflect the new signers and threshold.

---

### Fee Recipient

| Property | Value |
|----------|-------|
| **Custody model** | Single-key (hardware wallet) — pre-multisig state |
| **Key holder(s)** | Vortex Protocol treasurer / treasury multisig (pseudonymous, hardware-wallet-backed) |
| **Threshold** | 1-of-1 (single signature required) |
| **Hardware** | Ledger Nano S/X |
| **Network** | Staging / Pre-mainnet only |

**Status note:** Fee recipient custody follows the same pre-multisig path as the admin key. Once mainnet treasury infrastructure is established (per issue #37), the fee recipient will transition to a multisig or escrow arrangement. This document will be updated at that time.

**Expected transition:** Concurrent with mainnet launch. A treasury multisig or delegation contract will be designated, and this table will reflect its custody model.

---

## Key Rotation and Update Procedure

Whenever the admin key or fee recipient key is rotated:

1. **Rotation is performed** on-chain via `transfer_admin` (for admin) or `propose_fee_recipient` / `accept_fee_recipient` (for fee recipient), with dual authorization from old and new key holders (for `transfer_admin`).

2. **This document is updated** within **the same business day** with:
   - The new custody model (if changed, e.g., from 1-of-1 to 2-of-3).
   - The new key holder(s) or signer identities (if custody is transitioning to a named multisig, the signers are listed; if remaining pseudonymous, the update states "pseudonymous multisig").
   - The new threshold (M-of-N).
   - The new hardware setup (if applicable).
   - The updated **last verified date**.

3. **A cross-reference event** is posted to `#security` in the team's private Slack (or equivalent comms channel) with a link to the updated page, so stakeholders are aware of the change.

4. **Solvers and users are notified** via:
   - A GitHub discussion or announcement pinned in the main README.
   - An optional blog post or announcement if the custody change is a major milestone (e.g., transition to 3-of-5 multisig).

---

## Pre-mainnet State (Current)

During staging and pre-mainnet testing:

- The admin and fee recipient keys are developer-controlled, hardware-wallet-backed EOAs.
- They are **not** representative of the mainnet custody model, which will use a multisig or treasury structure.
- Key rotation on testnet does not trigger updates to this document (testnet-only changes are not production-relevant).

---

## Post-mainnet Launch

Once deployed to mainnet:

- **Every key rotation is a material event.** Any unexpected admin transfer or fee recipient change triggers immediate investigation and postmortem per issue #301 (`docs/incident-postmortem-template.md`).
- **This document is the authoritative record** of who currently controls the protocol. Community members and solvers should reference this page to confirm a custody change matches the public announcement.
- **Custody transitions are pre-announced** (where possible) with at least 48 hours' notice to solvers, so they can assess risk and adjust their bond allocation if needed.

---

## Verification

To verify the current custody model on-chain:

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_admin
# Returns the current admin address on-chain

stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_fee_recipient
# Returns the current fee recipient address on-chain
```

The addresses returned by these read-only functions **must match the custody model stated in this document**. If they do not, this document is stale and should be updated immediately.

---

## Related Documents

- `SECURITY.md` — "Admin Key Operational Security" section (the custody recommendations this document reports conformance against)
- `docs/114-multisig-admin-design.md` — design for the multisig wrapper expected before mainnet
- `docs/incident-postmortem-template.md` — postmortem process for unexpected key events
- `docs/mainnet-deployment-runbook.md` — deployment checklist confirming custody setup before launch

---

## FAQ

**Q: Why is this document public if the keys are not public?**

A: The custody model (single key vs. 2-of-3 multisig, hardware vs. software) is a **governance and transparency commitment** to the community. Publishing it ensures solvers and users know the level of centralization and can assess their trust accordingly. Individual key identities (if pseudonymous) remain private for operational security.

**Q: What if the admin key is compromised?**

A: Immediate steps per `SECURITY.md`'s "Post-incident key rotation" section:
1. Call `transfer_admin` to rotate to a new admin address (requires both current and new admin signatures).
2. Update this document within the same business day to reflect the new key.
3. Publish a security incident postmortem per issue #301.

**Q: When does the transition to multisig happen?**

A: On mainnet launch, at the earliest. The multisig contract (issue #36) must be deployed and tested before `initialize` is called with the multisig address. This document will be updated at that time, and the multisig signers will be disclosed (at a level of detail the threat model permits).

**Q: Can custody change without an on-chain event?**

A: No. `transfer_admin` and `propose_fee_recipient` / `accept_fee_recipient` both emit on-chain events (`admin_transferred`, `fee_recipient_proposed`, `fee_recipient_updated`). This document is updated as an on-chain event is executed, ensuring synchronization.

---

## Revision History

| Date | Change | Justification |
|------|--------|---------------|
| 2026-09-24 | Initial document created | Issue #299; pre-mainnet state documented |

---

*Last updated: 2026-09-24*
