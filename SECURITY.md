# Bug bounty

A draft program — in-scope assets, severity tiers, and (unfunded) reward
bands — is published in
[`docs/bug-bounty-program.md`](docs/bug-bounty-program.md). It is scoped
to the Assets at Risk table, Trust Assumptions, and the admin-key
blast-radius table in this file.

---

## Code of Conduct — financial-stakes supplement

This repository has a repo-specific [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md)
that supplements the org-wide `CONTRIBUTING.md` code-of-conduct reference. It
does not replace or supersede the org-wide policy; it adds conduct expectations
specific to a live, funds-bearing protocol. The sections below are the
security-relevant parts of that supplement, grounded in this file's threat
model.

### Responsible-disclosure conduct

The reporting instructions above ("do not open a public GitHub issue for
security vulnerabilities") are a conduct norm, not merely a suggestion. A
contributor who discloses an unpatched vulnerability in a public issue, PR,
chat, or social channel — including a partial hint sufficient to reproduce it —
has violated this code of conduct. Consequences are described in
[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md#responsible-disclosure-conduct) and
range from a warning to removal from the contributor pool, and, where funds are
at risk, referral to the maintainers' incident process. Legitimate security
research is welcome and protected; exploitation of a live protocol is not.

### Privileged information and conflicts of interest

Anyone with admin, arbiter, or maintainer authority — and any reviewer who is
also a registered, actively-bonded solver — holds privileged knowledge that can
move real money. In particular, knowledge of a pending `propose_*` change
during its timelock window, of an unpatched vulnerability, or of an upcoming
admin action (pause, fee-recipient change, allowlist change) must not be used
for personal advantage, front-running, or tipping off a solver. The
conflict-of-interest and privileged-information standard, including the
arbiter-specific policy tracked in issue `#115`
([`docs/arbiter-code-of-conduct.md`](docs/arbiter-code-of-conduct.md)), lives in
[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md#conflicts-of-interest-and-privileged-information).

### Enforcement and reporting

Conduct concerns — including suspected misuse of privileged information or a
public disclosure of an unpatched vulnerability — are reported through the
private channel described in [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md#enforcement-and-reporting),
not in a public issue thread. Security vulnerabilities themselves continue to
be reported per the instructions above.

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
| LP deposits | `backstop_vault` contract account (bond token / USDC) | Sum of all LP deposits |
| Escrowed stream deposits | `treasury_streams` contract account (stream token) | Sum of unvested stream balances |

#### `backstop_vault` custody

`backstop_vault` custodies real LP deposits: LPs transfer tokens in via
`deposit()`, and the contract holds them in its own account. Funds can only
leave through:

- `cover()` / `receive_income()` — settlement-only callbacks invoked by the
  settlement flow; they move funds as part of covering a shortfall or booking
  income, never on behalf of an arbitrary caller.
- `withdraw()` — LP-initiated, and only after the 48-hour cooldown has elapsed.

The admin's only authority over custodied funds is `set_emergency_mode`, which
queues withdrawals (it does not transfer tokens out). A compromised admin key
can therefore delay LP exits but cannot directly drain the vault.

#### `treasury_streams` custody

`treasury_streams` escrows real token deposits in its contract account for the
lifetime of each stream. Funds move only through:

- `create_stream` / `cancel` — governor-gated; they set up or tear down a
  stream and its escrow.
- `withdraw` — recipient-initiated, limited to the amount vested so far.

No other new contract holds third-party funds the way `backstop_vault` and
`treasury_streams` do; the remaining contracts hold no user or LP deposits.

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

### Proof of Reserves

The protocol publishes a **public proof-of-reserves dashboard** (see [`dashboard/`](./dashboard/) and [`docs/proof-of-reserves-dashboard.md`](./docs/proof-of-reserves-dashboard.md)) that reconciles on-chain solver bond totals against this Assets-at-Risk table. This is a **transparency artifact, not an insurance guarantee** — it verifies that collateral is on-chain but does not guarantee solver performance or user fund recovery.

The dashboard is continuously updated and independently reproducible (data sources and queries are published). Users should read this threat model and the FAQ in the dashboard for caveats.

---

### Admin Key Operational Security (#122)

The `Admin` address is the single most sensitive key in the protocol. It
controls pause/unpause, fee routing, the destination-token allowlist, the
source-chain allowlist, and admin rotation. A compromised admin key can halt
the protocol and redirect all fee and slash proceeds. The recommendations below
apply before and after mainnet launch.

**Custody transparency:** See [`docs/custody-transparency.md`](./docs/custody-transparency.md)
for the current custody model (who holds the keys, single-key vs. multisig threshold, and last-verified date).
This document is updated operationally whenever keys are rotated.

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

3. **Minimal admin transactio
ns per window.** Admin transactions should be batched and pre-announced where possible. An off-chain audit log of every admin call, indexed by ledger sequence, should be maintained and published.

---

## Threat Model — `solver_registry`

`solver_registry` is the canonical bond custodian for all registered solvers. It holds the USDC bond corpus and enforces tiered staking.

### Assets at Risk

| Asset | Where held | Value |
|-------|-----------|-------|
| Solver bonds | `solver_registry` contract account (USDC) | Sum of all registered bonds; Platinum tier requires ≥ 50,000 USDC per solver |
| Slashed bond proceeds | Transferred to `FeeRecipient` on slash | Up to 10 % of bond per slash event |

### Trust Assumptions

#### 1. Writer (settlement contract) blast radius

The `writer` address — set once via `set_writer` and rotatable only through the 48-hour `propose_writer` / `execute_writer` timelock — is the most powerful role in `solver_registry`. It can call `record_fill`, `record_failure`, `slash`, `lock_obligation`, and `release_obligation`. A compromised writer can slash any solver's bond repeatedly (guarded per intent by `AlreadyRecorded` idempotency) and redirect proceeds to any `FeeRecipient`. The timelock provides a 48-hour monitoring window for off-chain detection before a malicious writer rotation completes.

**Implication:** Monitor `writer_proposed` events. Any unexpected proposal must be treated as a compromise signal and escalated immediately.

#### 2. Admin key blast radius

The admin can tune tier thresholds within enforced bounds, initiate the writer rotation timelock, and initiate a two-step admin handover (also 48-hour timelocked). The admin **cannot** directly transfer bond tokens out — all token movements require writer or solver auth. However, a compromised admin can install a malicious writer, which can then drain bonds over time. The combined blast radius of admin + writer compromise is full bond corpus loss.

#### 3. Obligation-lock griefing

`lock_obligation` and `release_obligation` are writer-only (not admin-callable). If the settlement contract has a bug that locks obligations without ever releasing them, solvers cannot unstake or deregister until the lock is cleared. Solvers with perpetually locked obligations are effectively stuck — their bond is inaccessible.

#### 4. Bond floor on slash

The `slash` function reduces `bond_amount` with no floor. A solver slashed to zero bond retains their `SolverRecord` and can deregister to receive zero tokens back. There is no automatic deactivation at zero bond — the settlement contract is responsible for checking `is_active` / bond sufficiency before accepting new intents from the solver.

---

## Threat Model — `proof_registry`

`proof_registry` is a trust oracle. It converts Wormhole VAAs (and, in a future production path, Axelar GMP messages) into on-chain `ProofRecord` entries that `intent_settlement` uses to gate fills when `require_proof = true`.

### Assets at Risk

`proof_registry` holds no tokens. Its failure mode is **fraudulent proof injection**: a false `ProofRecord` would allow a solver to claim a fill was backed by a verified source-chain deposit when none occurred. This bypasses the economic-deterrent-only model and allows a solver to collect filled intent proceeds without having sent funds on the source chain.

### Trust Assumptions

#### 1. Wormhole Guardian quorum

`receive_message` delegates cryptographic verification entirely to the Wormhole Core contract. A Guardian set compromise (≥ 2/3 of 19 Guardians colluding) would allow fraudulent VAAs to be injected. This residual risk is shared with all Wormhole-integrated protocols and is outside this contract's control.

#### 2. Axelar path is not cryptographically verified (production gap)

`receive_message_axelar` checks that the `source_address` string matches the configured authorized source, but does **not** call the Axelar Gateway contract for message signature verification. Any caller who knows the expected `source_address` string can inject an arbitrary `ProofRecord` via this path. **The Axelar path must not be enabled for production use until full Gateway signature verification is implemented.**

#### 3. Emitter allowlist has no timelock

`set_authorized_emitter` and `set_authorized_axelar_source` are admin-only with no timelock. A compromised admin can add a malicious emitter for any chain ID immediately, allowing fraudulent proofs to be inserted. Monitor `emitter_authorized` and `axelar_source_authorized` events.

#### 4. Freshness window

`get_fresh_proof` rejects proofs older than `PROOF_VALIDITY_WINDOW` (3600 seconds). Callers that use the raw `get_proof` / `has_proof` instead of `get_fresh_proof` bypass this check and may accept stale proofs. `intent_settlement` is intended to call `get_fresh_proof` exclusively on the proof-gated path.

#### 5. testutils back-door

`mock_set_proof` / `mock_remove_proof` are compiled only under the `testutils` Cargo feature. A release build without this feature does not expose them. **Never deploy a `testutils`-enabled build to a live network.**

---

## Threat Model — `reputation_badge`

`reputation_badge` holds no tokens. It records soulbound tier badges (`Bronze` / `Silver` / `Gold` / `Platinum`) for registered solvers.

### Trust Assumptions

#### 1. Dual admin / registry authority

Both `mint_badge` and `burn_badge` accept calls from either the admin or the registered `solver_registry` contract. A compromised admin can mint or burn any badge without going through `solver_registry`'s tier logic. This is an intentional emergency override during the rollout period.

#### 2. `set_solver_registry` has no timelock

The admin can redirect the badge contract to any `solver_registry` address instantly. A compromised admin can point it at a contract that mints arbitrary badges for arbitrary addresses. Monitor `solver_registry_set` events.

#### 3. Badge TTL expiry causes silent downgrade

Badges are stored in persistent storage with a 30-day TTL. A badge that is not refreshed (because no tier-change event occurred) will eventually expire and `get_badge` will return `None` — equivalent to `Unranked` — without an explicit `burn_badge` call. Any consumer that gates privileges on badge presence must account for this silent expiry.

---

## Threat Model — `backstop_vault`

`backstop_vault` custodies LP deposits (USDC). LPs deposit in exchange for pro-rata shares; the vault absorbs settlement shortfalls via `cover()` and earns income via `receive_income()`.

### Assets at Risk

| Asset | Where held | Value |
|-------|-----------|-------|
| LP deposits | `backstop_vault` contract account (USDC) | Sum of all LP share redemption values |

(Already included in the `intent_settlement` Assets at Risk table above; this section adds the vault-specific trust model.)

### Trust Assumptions

#### 1. Settlement contract as sole cover/income caller

Only the `Settlement` address set at `initialize` can call `cover` and `receive_income`. This address is immutable after deployment — there is no `set_settlement` entrypoint. A compromised settlement contract can call `cover` to drain the vault to any recipient address. The immutability of the settlement address is a deliberate strong protection: the admin has no direct path to drain LP funds.

#### 2. Emergency mode as LP-exit delay (not drain)

`set_emergency_mode(true)` — admin-only — queues withdrawals instead of executing them. A compromised admin can delay LP exits indefinitely but cannot move tokens out of the vault directly. LPs can process their queued withdrawals once emergency mode is disabled or funds become available.

#### 3. Vault insolvency under repeated cover

`cover()` reduces `TotalAssets` and does not enforce a floor. Repeated cover calls can reduce `TotalAssets` to zero. Normal `withdraw` checks `assets_out > total_assets` and reverts with `VaultInsolvent`; `cover` silently caps its payout at available assets. LPs in a fully drained vault receive zero on withdrawal — this is socialized loss by design.

#### 4. Share-price inflation attack (mitigated)

The vault is seeded with `VIRTUAL_SHARES = 1,000,000,000` and `VIRTUAL_ASSETS = 1,000,000,000` at initialization. A dust first-deposit cannot manipulate the exchange rate to an extreme value.

---

## Threat Model — `arbiter_panel`

`arbiter_panel` custodies arbiter bonds (USDC) and drives the m-of-n dispute resolution process for `intent_settlement`.

### Assets at Risk

| Asset | Where held | Value |
|-------|-----------|-------|
| Arbiter bonds | `arbiter_panel` contract account | `bond_amount` × registered arbiter count |

### Trust Assumptions

#### 1. PRNG panel selection is deterministic

Panel selection uses `env.prng()` seeded with the intent ID. Because the intent ID is derived from deterministic parameters, an attacker who can predict or influence the intent ID before registering as an arbiter may predict their panel placement. This is a documented limitation (see design doc). Panel selection is "random-ish, not cryptographically unpredictable."

#### 2. Abstention-based quorum griefing

If fewer than `threshold` (m) arbiters vote within `VOTE_WINDOW` (24 hours), `tally` closes the round without slashing anyone (`outcome = None`). A small coalition of selected arbiters can strategically abstain to prevent resolution while avoiding slash penalties, leaving the dispute open until `intent_settlement`'s `ARBITER_WINDOW` timeout applies and the user receives the full escrow.

#### 3. Core parameters are immutable post-deploy

`settlement`, `bond_token`, `bond_amount`, `panel_size`, and `threshold` are set only at `initialize` and cannot be changed post-deployment. This is a deliberate restriction: it prevents admin manipulation of dispute mechanics but also means parameter errors require a full redeployment.

#### 4. Settlement cross-contract trust

`open_vote` calls `settlement.dispute_parties(intent_id)` to determine which addresses to exclude from the panel. A malicious or misconfigured settlement contract could return arbitrary `(user, solver)` addresses, manipulating arbiter eligibility.
