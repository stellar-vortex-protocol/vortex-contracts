# Mainnet Deployment Runbook

Step-by-step checklist for promoting `intent_settlement` from testnet to mainnet.
Work through every section in order; do not skip the verification steps.

---

## Table of Contents

1. [Pre-deployment Checklist](#pre-deployment-checklist)
2. [Build the Release Artifact](#build-the-release-artifact)
3. [Deploy the Contract](#deploy-the-contract)
4. [Initialize the Contract](#initialize-the-contract)
5. [Post-deploy Verification](#post-deploy-verification)
6. [Configure the Destination Token Allowlist](#configure-the-destination-token-allowlist)
7. [Register Initial Solvers](#register-initial-solvers)
8. [Smoke Test](#smoke-test)
9. [Rollback Procedure](#rollback-procedure)
10. [Incident Response](#incident-response)

---

## Pre-deployment Checklist

Complete every item before building the wasm artifact.

- [ ] All CI checks pass on the commit you intend to deploy
      (`cargo fmt`, `cargo clippy`, `cargo test`, `stellar contract build`,
      `cargo audit`).
- [ ] The CHANGELOG has an entry for this release under a dated version heading.
- [ ] The `fee_recipient` address is a multisig or hardware-wallet-backed account
      (not a hot key).
- [ ] The `admin` address is a multisig or hardware-wallet-backed account.
- [ ] You have confirmed the mainnet USDC asset's SAC address
      (canonical Stellar USDC contract address for the Stellar mainnet network).
- [ ] The deployer keypair has enough XLM to cover contract deployment and
      initialization transaction fees (recommend ≥ 10 XLM as a buffer).
- [ ] You have a monitored Stellar RPC endpoint for mainnet.
- [ ] Rollback plan reviewed (see [Rollback Procedure](#rollback-procedure)).

---

## Build the Release Artifact

```bash
cd intent_settlement

# Ensure the wasm32 target is present
rustup target add wasm32-unknown-unknown

# Clean and build optimized wasm
cargo clean
stellar contract build
```

Confirm the artifact was produced:

```bash
ls -lh target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm
```

Note the file hash — you'll compare it against the on-chain stored hash after
deployment:

```bash
sha256sum target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm
```

---

## Deploy the Contract

```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm \
  --source <DEPLOYER_SECRET_KEY> \
  --network mainnet
```

The CLI prints the newly assigned `CONTRACT_ID`. Record it immediately — this
is the canonical address for the entire deployment:

```
CONTRACT_ID=<paste the output here>
```

> **Security note**: The deployer key is only needed for this step. After
> `initialize` is called with a separate `admin` address, the deployer key
> has no special privileges over the contract.

---

## Initialize the Contract

`initialize` can only be called once. Calling it a second time panics with
`AlreadyInitialized (1)`. Get the parameters right on the first attempt.

### Parameters

| Parameter       | Expected value                                      |
|-----------------|-----------------------------------------------------|
| `admin`         | Multisig/hardware-wallet Stellar address            |
| `fee_recipient` | Address that receives protocol fees and slash proceeds |
| `bond_token`    | Mainnet USDC SAC address                            |

### Verify the bond_token address

Before invoking `initialize`, double-check the USDC contract address using a
read call you can verify independently:

```bash
# The address should resolve to "USDC" with issuer GAULP... on mainnet.
# Cross-reference with Stellar Expert or the Circle/Stellar documentation.
stellar contract invoke \
  --id <USDC_SAC_ADDRESS> \
  --source <ANY_KEY> \
  --network mainnet -- \
  symbol
```

### Call initialize

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  initialize \
  --admin <ADMIN_ADDRESS> \
  --fee_recipient <FEE_RECIPIENT_ADDRESS> \
  --bond_token <USDC_SAC_ADDRESS>
```

---

## Post-deploy Verification

Run every command in this section and confirm the output matches the expected
value before proceeding. These commands are all read-only (no fees, no side
effects).

### 1. Confirm admin address

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_admin
```

Expected: the `<ADMIN_ADDRESS>` passed to `initialize`.

### 2. Confirm fee recipient

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_fee_recipient
```

Expected: the `<FEE_RECIPIENT_ADDRESS>` passed to `initialize`.

### 3. Confirm bond token

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_bond_token
```

Expected: the USDC SAC address passed to `initialize`. Cross-check this output
character-by-character against the address you verified before calling
`initialize`.

### 4. Confirm contract is not paused

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  is_paused
```

Expected: `false`. If this returns `true`, something went wrong — investigate
before continuing.

### 5. Confirm protocol stats are zeroed

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_stats
```

Expected: `(0, 0)` — `(total_intents, total_volume)`. Any other value indicates
the contract was previously initialized (possibly by a replay attack or
misconfiguration).

### 6. Confirm allowlist is off by default

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  is_dst_allowlist_enabled
```

Expected: `false`. The allowlist is disabled by default and must be explicitly
opted into via `set_dst_allowlist_enabled`.

---

## Configure the Destination Token Allowlist

If you want to restrict which destination tokens users can request (recommended
for mainnet), configure and enable the allowlist before going live.

```bash
# Allow mainnet USDC as a destination token
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  add_allowed_dst_token \
  --token <USDC_SAC_ADDRESS>

# Add any additional allowed tokens (EURC, etc.)
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  add_allowed_dst_token \
  --token <OTHER_TOKEN_ADDRESS>

# Verify each token was added
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  is_dst_token_allowed \
  --token <USDC_SAC_ADDRESS>
# Expected: true

# Enable enforcement
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  set_dst_allowlist_enabled \
  --enabled true

# Confirm enforcement is on
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  is_dst_allowlist_enabled
# Expected: true
```

---

## Register Initial Solvers

Initial solver partners can now register their bonds. Each solver runs this
against the live contract:

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <SOLVER_SECRET_KEY> \
  --network mainnet -- \
  register_solver \
  --solver <SOLVER_ADDRESS> \
  --bond_amount <BOND_IN_STROOPS>  # minimum 500000000 (50 USDC)
```

Verify each solver registration:

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  is_solver_eligible \
  --solver <SOLVER_ADDRESS>
# Expected: true
```

---

## Smoke Test

Before opening the contract to public users, run a minimal end-to-end test with
controlled accounts.

```bash
# 1. Submit a test intent from a test user account
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <TEST_USER_SECRET_KEY> \
  --network mainnet -- \
  submit_intent \
  --user <TEST_USER_ADDRESS> \
  --src_chain '"ethereum"' \
  --src_token '"0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"' \
  --src_amount 1000000000000000000 \
  --dst_token <USDC_SAC_ADDRESS> \
  --min_dst_amount 100000000   # 10 USDC minimum

# 2. Verify intent exists and is Open
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_intent \
  --intent_id <RETURNED_INTENT_ID>
# Expected: state = Open

# 3. Accept with a registered solver
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <TEST_SOLVER_SECRET_KEY> \
  --network mainnet -- \
  accept_intent \
  --solver <TEST_SOLVER_ADDRESS> \
  --intent_id <INTENT_ID>

# 4. Fill within 5 minutes
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <TEST_SOLVER_SECRET_KEY> \
  --network mainnet -- \
  fill_intent \
  --solver <TEST_SOLVER_ADDRESS> \
  --intent_id <INTENT_ID> \
  --fill_amount 100000000

# 5. Confirm intent is now Filled
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_intent \
  --intent_id <INTENT_ID>
# Expected: state = Filled

# 6. Confirm stats updated
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_stats
# Expected: total_intents = 1, total_volume = 100000000
```

If any step fails, pause the contract (see [Incident Response](#incident-response))
before investigating.

---

## Contract Upgrade (#194)

The contract has an in-place upgrade path, so a bug fix or new feature does
**not** require redeploying to a new address and re-onboarding solvers. It is
admin-only and timelocked (`ADMIN_TIMELOCK_DELAY`, 48 h) exactly like the
other sensitive admin actions, and every step emits an event.

### 1. Build and upload the new Wasm

```bash
# From the repo root, build the optimized wasm (see "Build the Release Artifact").
stellar contract upload \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet \
  --wasm intent_settlement/target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm
# Prints the 32-byte wasm hash (hex). Save it as $NEW_WASM_HASH.
```

### 2. Propose the upgrade

```bash
stellar contract invoke --id $CONTRACT_ID --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  propose_upgrade --new_wasm_hash $NEW_WASM_HASH
```

Emits `upgrade_proposed(new_wasm_hash, eta)`. `eta` is the earliest ledger
timestamp `execute_upgrade` can run. Off-chain monitors should alert on this
event. To read it back later: `get_pending_upgrade` → `Some((hash, eta))`.
Re-running `propose_upgrade` with a different hash replaces the proposal and
resets the 48 h clock.

### 3. Execute after the timelock

```bash
# Only after the current ledger time >= eta.
stellar contract invoke --id $CONTRACT_ID --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  execute_upgrade --new_wasm_hash $NEW_WASM_HASH
```

Fails with `TimelockNotElapsed (29)` before `eta`, `Unauthorized (2)` if the
hash doesn't match the proposal, `NoPendingUpgrade (35)` if nothing is
pending. On success it swaps the code and emits `upgraded(new_wasm_hash)`.
The contract address, all storage, admin, config, solver bonds, and in-flight
intents are unchanged.

### 4. Run the migration hook (only if the release requires it)

If the new release's changelog says it changes a persisted storage shape,
run the one-time migration immediately after `execute_upgrade`:

```bash
stellar contract invoke --id $CONTRACT_ID --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  migrate
```

`migrate` is guarded by an on-chain version marker: it runs once per release
and returns `AlreadyMigrated (36)` on any subsequent call, so a repeated or
double-applied migration is impossible. Releases with no storage change need
no `migrate` call (a fresh `initialize` already stamps the current version).
Emits `migrated(from_version, to_version)`.

### Upgrade safety notes

- An upgrade landing while intents are `Open` / `Accepted` does not touch
  their storage; the new code reads the same entries.
- Test the exact upgrade on testnet first: deploy current, create state,
  `propose_upgrade` + `execute_upgrade` to the new hash, verify `get_intent`
  / `get_solver` / `get_stats` still return the expected values, then run any
  `migrate`.

---

## Rollback Procedure

Rolling *back* a bad upgrade uses the same upgrade path in reverse: `stellar
contract upload` the previous known-good wasm and `propose_upgrade` /
`execute_upgrade` to its hash (still subject to the 48 h timelock — `pause`
first if the regression is actively harmful). If the contract cannot be
recovered by re-upgrading, the fallback is a fresh deployment:

1. **Immediately pause the contract** to halt new activity (see below).
2. **Deploy a patched contract** to a new address.
3. **Communicate the new address** to all integrated solvers and frontends.
4. **Drain active intents** from the old contract:
   - Intents in `Accepted` state: wait for the fill window (max 5 minutes) and
     call `slash_solver` if they weren't filled. The intent reverts to `Open`.
   - Intents in `Open` state: the user can call `cancel_intent` to abandon them.
   - Intents in terminal states (`Filled`, `Cancelled`, `Expired`, `Slashed`)
     require no action.
5. **Return solver bonds**: after all `active_intents` reach zero, solvers can
   call `deregister_solver` to recover their bonds from the old contract.

There is no automated migration path for in-flight state — plan deployments
for low-activity windows.

---

## Incident Response

### Pause the contract (admin only)

Use this immediately if you suspect an exploit, unexpected behavior, or need
maintenance time:

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  pause
```

Effect: `submit_intent`, `accept_intent`, and `fill_intent` revert with
`ContractPaused (18)`. `slash_solver`, `cancel_intent`, and all read-only
views remain available.

### Resume normal operation

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  unpause
```

### Pause the proof registry (admin only)

`proof_registry` has its own independent pause flag (issue #264), separate
from `intent_settlement`'s. Use this if you suspect a forged-proof attack or
other proof-ingestion incident:

```bash
stellar contract invoke \
  --id $PROOF_REGISTRY_CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  pause
```

Effect: `receive_message` reverts with `ContractPaused (8)`. `get_proof` and
`has_proof` remain available. Resume with the same `unpause` invocation used
for `intent_settlement`, targeted at `$PROOF_REGISTRY_CONTRACT_ID`.

### Rotate admin key

If the admin key is compromised, use `transfer_admin`. This requires
authorization from *both* the current and the new admin keypair:

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <CURRENT_ADMIN_SECRET_KEY> \
  --network mainnet -- \
  transfer_admin \
  --new_admin <NEW_ADMIN_ADDRESS>
# The new admin must also sign this transaction
```

### Rotate fee recipient

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  set_fee_recipient \
  --new_fee_recipient <NEW_FEE_RECIPIENT_ADDRESS>
```

### Quick status check script

Save this as `scripts/check-deployment.sh` and run it any time you need a
fast health overview:

```bash
#!/usr/bin/env bash
set -euo pipefail
CONTRACT_ID="${1:?Usage: check-deployment.sh <CONTRACT_ID>}"
NETWORK="${2:-mainnet}"

echo "=== Vortex Intent Settlement — Deployment Health Check ==="
echo "Contract: $CONTRACT_ID  Network: $NETWORK"
echo ""

echo -n "Admin:          "; stellar contract invoke --id "$CONTRACT_ID" --source "$STELLAR_SECRET_KEY" --network "$NETWORK" -- get_admin
echo -n "Fee Recipient:  "; stellar contract invoke --id "$CONTRACT_ID" --source "$STELLAR_SECRET_KEY" --network "$NETWORK" -- get_fee_recipient
echo -n "Bond Token:     "; stellar contract invoke --id "$CONTRACT_ID" --source "$STELLAR_SECRET_KEY" --network "$NETWORK" -- get_bond_token
echo -n "Paused:         "; stellar contract invoke --id "$CONTRACT_ID" --source "$STELLAR_SECRET_KEY" --network "$NETWORK" -- is_paused
echo -n "Allowlist on:   "; stellar contract invoke --id "$CONTRACT_ID" --source "$STELLAR_SECRET_KEY" --network "$NETWORK" -- is_dst_allowlist_enabled
echo -n "Stats:          "; stellar contract invoke --id "$CONTRACT_ID" --source "$STELLAR_SECRET_KEY" --network "$NETWORK" -- get_stats
echo ""
echo "=== Done ==="
```
