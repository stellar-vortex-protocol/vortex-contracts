# Security Policy — `vortex-contracts`

> For general vulnerability reporting instructions, see the org-level
> [SECURITY.md](https://github.com/vortex-protocol/.github/blob/main/SECURITY.md).

---

## Threat Model — `intent_settlement`

This document describes the trust assumptions, assets at risk, and known
limitations of the `intent_settlement` contract ahead of a mainnet deployment.
It is intended for auditors and integrators.

---

### Assets at Risk

| Asset | Where held | Value |
|-------|-----------|-------|
| Solver bonds | `intent_settlement` contract account (USDC) | ≥ 50 USDC per solver |
| User swap output | Solver's token balance (transferred in `fill_intent`) | Unbounded |
| Protocol fees | `FeeRecipient` address | 0.05 % of filled volume |
| Admin privileges | `Admin` key in instance storage | Full contract control |

---

### Trust Assumptions

#### 1. Solver self-reporting

By default the contract has **no on-chain proof** that a solver actually sent
tokens on the source chain. `fill_intent` trusts the solver to transfer
`dst_token` to the user on Stellar; the economic incentive not to default is the
bond slash (`slash_solver` takes 10 % of the bond permissionlessly once the fill
window expires).

**Implication:** A solver can still accept an intent, fail to fill, absorb the
slash, and profit if the source-chain spread outweighs it. The proportional
formula removes the old asymmetry where a large bond made a small-intent default
disproportionately expensive (or a small bond made a large-intent default
cheap); the bond size remains the primary economic deterrent.

**Optional cryptographic gate (issue #190).** An admin can call
`set_proof_registry` to point `intent_settlement` at a deployed `ProofRegistry`
(Wormhole-backed). A solver may then call
`fill_intent(solver, intent_id, fill_amount, require_proof = true)`, which
cross-checks a verified source-chain deposit record before any token moves:

- no proof for the intent → `ProofNotFound`;
- proof's Wormhole `src_chain_id` ≠ mapped `intent.src_chain` → `ProofChainMismatch`;
- `proof.src_amount < intent.src_amount` → `ProofAmountInsufficient`;
- registry not configured → `ProofRegistryNotSet`.

Every failure path reverts before touching state, so the intent stays
`Accepted` and `slash_solver` remains the backstop (see
`docs/129-proof-mismatch-fallback.md`). This is **opt-in and defaults off**:
`require_proof = false` — the value every existing caller passes — reads no
registry and preserves the self-reported-fill behaviour above byte-for-byte,
mirroring how `DstAllowlistEnabled` defaults off. When the gate is used, trust
for that fill becomes *cryptographic (Guardian quorum) + economic (bond)* rather
than economic alone; residual risk is Guardian collusion, shared with all
Wormhole users.

#### 2. Admin key custody

A single `Admin` address controls:

- `pause` / `unpause` — can halt all new swaps.
- `set_fee_recipient` — redirects fee and slash proceeds.
- `transfer_admin` — rotates the admin key (requires both old and new admin to
  sign).
- `add_allowed_dst_token` / `set_dst_allowlist_enabled` — controls which tokens
  users can target.

**Implication:** A compromised admin key can grief the protocol (pause, fee
redirection) but **cannot steal user funds or solver bonds directly**, because
no admin function transfers tokens out of user or solver custody.  Key rotation
requires dual authorization, reducing the single-key-failure surface.

#### 3. Destination token allowlist (default: off)

By default, `submit_intent` accepts any `dst_token` address.  If an admin
enables the allowlist (`set_dst_allowlist_enabled(true)`), only pre-approved
tokens may be used as destinations.

**Implication (allowlist disabled):** A user or solver could name a malicious
contract as `dst_token`.  The risk lands on the user who submitted the intent —
they would receive tokens from a contract they did not vet.  Enabling the
allowlist before mainnet is strongly recommended.

**Implication (allowlist enabled):** The allowlist is stored in instance
storage, so adding/removing tokens is cheap but centralized under the admin
key.

#### 4. Fill-window timing

The fill window (`FILL_WINDOW = 300 s`) starts from `accept_intent`.  The
deadline stored on-chain is `now + 300`.  A solver and a block producer
colluding to manipulate `ledger().timestamp()` could extend the fill window
slightly, but Stellar's timestamp drift is bounded by consensus rules and is not
a meaningful attack surface in practice.

#### 5. Permissionless slash and expire

`slash_solver` and `expire_intent` are callable by anyone.  This is intentional
— it prevents a solver from dodging accountability by waiting out a pause or
ensuring only friendly callers settle the intent.  A griefing scenario where
someone repeatedly calls `expire_intent` on an already-expired intent is
defended by the `IntentNotOpen` guard (idempotent after first call).

---

---

### Admin Key Operational Security (#122)

The `Admin` address is the single most sensitive key in the protocol. It
controls pause/unpause, fee routing, the destination-token allowlist, the
source-chain allowlist, and admin rotation. A compromised admin key can halt
the protocol and redirect all fee and slash proceeds. The recommendations below
apply before and after mainnet launch.

#### Recommended custody model

| Deployment stage | Recommended setup |
|-----------------|-------------------|
| Testnet / staging | Developer EOA on a hardware wallet (Ledger/Trezor) |
| Mainnet launch | 2-of-3 or 3-of-5 multisig (e.g. Gnosis Safe equivalent on Stellar, or a Stellar multi-sig account with threshold set appropriately) |
| Long-term mainnet | Hardware-backed multisig with geographically distributed key holders; a minimum M-of-N threshold of M ≥ 2 |

A design for a multisig admin wrapper is tracked in
[`docs/114-multisig-admin-design.md`](./docs/114-multisig-admin-design.md).
Until that wrapper is deployed, at minimum the admin key must reside on a
hardware wallet — never a hot wallet or a key stored in plaintext.

#### Key rotation

- Rotate the admin key with `transfer_admin`. Both the outgoing and incoming
  admin must sign the same transaction, so a typo'd address can never
  accidentally brick the contract.
- After rotation, verify `get_admin()` returns the expected new address on-chain
  before discarding the old key material.
- Rotation should be rehearsed on testnet before any live key handover.

#### Operational hygiene

1. **Hardware wallet required.** The admin signing key must never exist only in
   software (e.g. a `.env` file, a CI secret, or an in-memory key). Use a
   Ledger, Trezor, or equivalent hardware device for all admin transactions.

2. **Separate key per environment.** Testnet, staging, and mainnet must each
   have a distinct admin key. Reusing a key across environments means a
   compromise on a lower-trust environment immediately threatens mainnet.

3. **Minimal admin transactions.** Admin calls should originate from an
   air-gapped or dedicated signing machine. Never sign admin transactions from
   a machine used for general web browsing or software development.

4. **Pre-launch checklist.** Before enabling mainnet enforcement:
   - Populate `add_allowed_src_chain` with every supported source chain.
   - Call `set_src_chain_allowlist_enabled(true)`.
   - Populate `add_allowed_dst_token` for every supported output token.
   - Call `set_dst_allowlist_enabled(true)`.
   - Confirm `get_admin()` and `get_fee_recipient()` return the expected
     multisig addresses.
   - See [`docs/pre-deploy-security-checklist.md`](./docs/pre-deploy-security-checklist.md)
     for the full checklist.

5. **Monitor fee recipient.** `set_fee_recipient` (via the two-step
   `propose_fee_recipient` / `accept_fee_recipient` flow) redirects all
   protocol fee and slash proceeds. Monitor the `fee_recipient_proposed` and
   `fee_recipient_updated` events on-chain; any unexpected proposal should be
   treated as a potential compromise.

6. **Pause is not a long-term solution.** `pause()` is an incident-response
   tool, not a substitute for fixing a bug. A paused contract still holds
   solver bonds; resolve the incident and unpause as quickly as safely
   possible to minimise disruption to solvers and users.

7. **Post-incident key rotation.** Any time there is evidence that the admin
   key may have been exposed (phishing, malware, shoulder-surfing), rotate
   immediately via `transfer_admin` and notify all registered solvers.

#### What a compromised admin key can and cannot do

| Can do | Cannot do |
|--------|-----------|
| Pause the contract (halt new swaps) | Transfer user funds directly |
| Redirect future fees and slashes via `propose_fee_recipient` | Drain solver bonds |
| Add/remove tokens from dst and src allowlists | Cancel or fill intents on behalf of others |
| Rotate itself to a new admin address | Bypass the solver bond slash (still permissionless) |
| Disable allowlist enforcement | Recover already-transferred tokens |

The blast radius of a compromised key is **griefing and fee theft**, not direct
fund theft. Solver bonds and in-flight intent outputs are only moved by
`fill_intent`, `slash_solver`, and `rescue_tokens` — none of which are callable
unilaterally by the admin without other protocol preconditions being met first
(except `rescue_tokens`, which is limited to non-bond, non-active-intent tokens).

---

### Known Limitations

- **Cross-chain proof is opt-in.** By default the contract does not verify the
  source-chain transaction and trust is economic, not cryptographic. Issue #190
  adds an admin-enabled `ProofRegistry` gate (`fill_intent(..., require_proof =
  true)`) that makes a verified Wormhole deposit a precondition for the fill;
  until an operator configures it and solvers opt in, the economic-only model
  above still applies.
- **Single admin key.** There is no multi-sig or timelock on admin operations.
  Protocol operators should secure the admin key with a hardware wallet or
  multi-sig wrapper before mainnet.
- **Bond slash is proportional to intent size (issue #193).** `slash_solver`
  slashes `min(unfilled_output, bond) / 10` — an amount scaled to the intent the
  solver failed to fill — capped at 10 % of the bond (so a well-matched bond is
  never punished harder than the old flat rate) and floored at 1 stroop (issue
  #32). Cross-token value comparison between the bond token and the destination
  token is still out of scope and assumes same-token comparability or an
  admin-set per-token minimum; a price-oracle-based comparison remains on the
  roadmap.
- **Intent re-open after slash.** After `slash_solver` the intent is reset to
  `Open` (or `PartiallyFilled`) with a fresh deadline. This is now bounded:
  `ProtocolConfig.max_slash_cycles` caps how many times an intent can cycle
  through `Open → Accepted → Slashed` before it transitions to the terminal
  `Abandoned` state instead of re-opening.
- **No allowlist by default.** Until an admin calls `set_dst_allowlist_enabled(true)`,
  any token address — including malicious contracts — can be used as `dst_token`.

#### Closed gap: slash-cooldown and reputation reset via deregister/re-register (#272)

**Previously:** `deregister_solver` removed the `SolverRecord` entirely.  A
re-registration by the same address would create a brand-new record with
`last_slash_time = 0`, `fills_completed = 0`, and `fills_failed = 0`.  This
allowed a just-slashed solver to (a) bypass `SLASH_COOLDOWN` immediately by
deregistering and re-registering, and (b) wipe their entire fill history to
reset `compute_reputation_score` to a clean slate — defeating the reputation
system's value as a persistent, hard-to-game track record.

**Fix (option b — preserve across the cycle):** `deregister_solver` now writes
a `ReputationSnapshot` to `DataKey::SolverReputation(address)` before removing
the `SolverRecord`.  `register_solver` reads that snapshot (if present) and
carries `last_slash_time`, `fills_completed`, `fills_failed`, and `total_volume`
forward into the new record, then removes the snapshot.  A solver who was never
slashed and has no prior fills sees identical behaviour to before (no snapshot
→ zero-initialised fields).

**Why option b over option a (block deregistration during cooldown)?** Blocking
deregistration would add friction to legitimate solver exits that happen to
coincide with a recent slash — for example, a solver winding down operations
after a network disruption.  Preserving the fields instead closes both the
cooldown-bypass and the reputation-reset exploits without restricting the
deregistration path at all.

---

### Reporting a Vulnerability

Please do **not** open a public GitHub issue for security vulnerabilities.
Follow the responsible-disclosure process described in the org-level
[SECURITY.md](https://github.com/vortex-protocol/.github/blob/main/SECURITY.md).
