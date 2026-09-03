# Pre-Testnet-Deployment Security Checklist

Tracking issue: [#44](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/44)

This checklist must be fully green before any testnet or mainnet deployment of
`intent_settlement`. Each item links to the issue that resolves it. Items are
checked off as their linked PR is merged into `main`.

---

## Security Items

### Deadline & ordering correctness

- [ ] **#42 — Audit deadline comparison operators across accept_intent / fill_intent / slash_solver / expire_intent for off-by-one consistency**
  Trace every `intent.deadline` read/write, write boundary tests at `now == deadline`
  for each lifecycle function, add rustdoc boundary-semantics comments.

### Authorization hardening

- [x] **#45 — Audit all `require_auth()` call sites for correctness vs. Soroban's `require_auth_for_args`**
  Review all 12 call sites; document per-function conclusion; upgrade any site
  where scoped authorization meaningfully reduces delegated-execution risk.
  See `docs/auth-audit.md`. `submit_intent`, `accept_intent`, and `fill_intent`
  upgraded to `require_auth_for_args`; all other sites kept as-is.

### Economic / bond sizing

- [ ] **Review `MIN_BOND` and slash percentage against realistic USDC liquidity**
  Confirm that 50 USDC minimum bond and 10 % slash are economically meaningful
  deterrents on testnet and, separately, on mainnet. Adjust constants or make
  them governance-settable before mainnet if needed.

### Allowlist & dst_token validation

- [ ] **Confirm dst_token allowlist is populated before enabling on mainnet**
  `set_dst_allowlist_enabled(true)` must only be called after a reviewed set of
  SAC/SEP-41 addresses has been added via `add_allowed_dst_token`. Ensure the
  deployment runbook enforces this ordering.

### Fee-recipient confirmation

- [ ] **Confirm fee_recipient address is correct before deployment**
  The fee recipient set at `initialize` time cannot be changed without admin
  authority. Validate the address in a dry-run against testnet before mainnet
  deployment.

### Pause / unpause key management

- [ ] **Confirm admin key is a multisig or hardware wallet before mainnet**
  `pause()` / `unpause()` and all admin functions are gated behind a single
  admin address. Ensure the key is stored securely and, on mainnet, is a
  threshold multisig.

### Dispute resolution (roadmap dependency)

- [ ] **#48 — Design a dispute-resolution flow for contested fills**
  A time-boxed dispute window and arbitration path must be designed before
  mainnet launch to handle fills that technically meet `min_dst_amount` but
  are disputed by the user.

---

## Process

1. File or link a PR that closes each item above.
2. Check the item off once the PR is merged into `main`.
3. Do not deploy to testnet until all items are checked.
4. Run a final review pass with a fresh set of eyes after all items close.

---

*Last updated: 2026-07-27*
