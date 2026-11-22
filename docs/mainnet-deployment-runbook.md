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
11. [Deploy the Fee Router](#deploy-the-fee-router)

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

### Alternative: Download from GitHub Release

For reproducible verification without rebuilding, download the verified `.wasm`
binary and `SHASUMS256.txt` from the [GitHub Release](https://github.com/stellar-vortex-protocol/vortex-contracts/releases)
corresponding to the version tag you're deploying:

```bash
# Download SHASUMS256.txt from the release
curl -L https://github.com/stellar-vortex-protocol/vortex-contracts/releases/download/v1.0.0/SHASUMS256.txt -o SHASUMS256.txt

# Download the wasm binary
curl -L https://github.com/stellar-vortex-protocol/vortex-contracts/releases/download/v1.0.0/vortex_intent_settlement.wasm -o vortex_intent_settlement.wasm

# Verify checksum
sha256sum -c SHASUMS256.txt
```

This binary is built deterministically using Rust 1.78.0 and can be independently
verified to match the source code at that tag — no local build required.

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

To run these checks in one pass, use `scripts/check-deployment.sh <CONTRACT_ID>`.
The script exits non-zero, naming the contract ID it checked, if any lookup
fails or returns an empty result (for example when the contract is not deployed
on the selected network). A zero exit means every lookup returned a value.

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

Initial solver partners c

/* … truncated 10883 chars — edit only what you need near the top … */

---

## Deploy the Fee Router

The `fee_router` contract is the protocol's fee recipient. Instead of sending
fees and slash proceeds to a single address, point `fee_recipient` at the
fee-router contract so revenue is split across governance-configured sinks
(treasury, backstop vault, badge-holder rebate pool, etc.).

### Build the artifact

```bash
cd fee_router
stellar contract build
ls -lh target/wasm32-unknown-unknown/release/vortex_fee_router.wasm
```

### Deploy and initialize

```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/vortex_fee_router.wasm \
  --source <DEPLOYER_SECRET_KEY> \
  --network mainnet

FEE_ROUTER_ID=<paste the output here>

stellar contract invoke \
  --id $FEE_ROUTER_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  initialize \
  --admin <ADMIN_ADDRESS> \
  --weight_delay <TIMELOCK_SECONDS>
```

### Configure sinks and weights

Weights are expressed in basis points and must sum to exactly `10,000`. Weight
changes are timelocked: propose first, then apply after `weight_delay` seconds.
The number of sinks is capped (see `MAX_SINKS` in the contract).

```bash
# Propose a new weight set (sinks + bps, summing to 10_000)
stellar contract invoke \
  --id $FEE_ROUTER_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  propose_weights \
  --sinks '[<TREASURY_ADDRESS>, <BACKSTOP_VAULT_ADDRESS>, <REBATE_POOL_ADDRESS>]' \
  --weights '[5000, 3000, 2000]'

# After weight_delay has elapsed, apply the pending weights
stellar contract invoke \
  --id $FEE_ROUTER_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  apply_weights

# Verify the active weights
stellar contract invoke \
  --id $FEE_ROUTER_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_weights
```

### Point the settlement contract at the router

Set the settlement contract's `fee_recipient` to `$FEE_ROUTER_ID` so all fees
and slash proceeds flow into the router.

### Distribute accumulated fees

`distribute(token)` is permissionless — anyone can call it to pay out the
router's balance for a token according to the active weights. Accounting is
pull-based (`claimable[sink][token]`): if a sink rejects a transfer, its share
is held and can be retried later rather than reverting the whole distribution.
Rounding dust is assigned to the first sink.

```bash
stellar contract invoke \
  --id $FEE_ROUTER_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  distribute \
  --token <USDC_SAC_ADDRESS>

# Inspect a sink's claimable balance for a token
stellar contract invoke \
      --id $FEE_ROUTER_ID \
      --source <ANY_KEY> \
      --network mainnet -- \
      claimable \
      --sink <TREASURY_ADDRESS> \
      --token <USDC_SAC_ADDRESS>
```

---

## Deploy the Solver Registry

The `solver_registry` contract tracks solver tiers and provides tier-based
perks (fill-window bonus, reduced slash rate). It must be deployed and
initialized before pointing the settlement contract at it.

### Build the artifact

```bash
cd solver_registry
stellar contract build
ls -lh target/wasm32-unknown-unknown/release/vortex_solver_registry.wasm
```

### Deploy and initialize

```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/vortex_solver_registry.wasm \
  --source <DEPLOYER_SECRET_KEY> \
  --network mainnet

SOLVER_REGISTRY_ID=<paste the output here>

stellar contract invoke \
  --id $SOLVER_REGISTRY_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  initialize \
  --admin <ADMIN_ADDRESS>
```

### Configure tier perks

```bash
# Set fill-window bonus and slash rate per tier (basis points)
# Tier 0 = Unranked, 1 = Bronze, 2 = Silver, 3 = Gold, 4 = Platinum
stellar contract invoke \
  --id $SOLVER_REGISTRY_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  set_tier_perks \
  --tier 1 \
  --fill_window_bonus_bps 500 \
  --slash_bps 500

# Verify
stellar contract invoke \
  --id $SOLVER_REGISTRY_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  get_fill_window_bonus_bps \
  --tier 1
```

### Point the settlement contract at the registry

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  set_solver_registry \
  --registry $SOLVER_REGISTRY_ID
```

---

## Configure Source Chain Allowlist

If you want to restrict which source chains users can submit intents from
(recommended for mainnet), configure and enable the allowlist before going live.

```bash
# Allow Ethereum mainnet
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  add_allowed_src_chain \
  --chain "ethereum"

# Add additional chains as needed
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  add_allowed_src_chain \
  --chain "base"

# Verify
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ANY_KEY> \
  --network mainnet -- \
  is_src_chain_allowed \
  --chain "ethereum"
# Expected: true

# Enable enforcement
stellar contract invoke \
  --id $CONTRACT_ID \
  --source <ADMIN_SECRET_KEY> \
  --network mainnet -- \
  set_src_allowlist_enabled \
  --enabled true
```

---

## Rollback Procedure

If any post-deploy verification fails or an incident is detected within the
first 24 hours, follow this procedure:

1. **Pause the contract immediately**:
   ```bash
   stellar contract invoke \
     --id $CONTRACT_ID \
     --source <ADMIN_SECRET_KEY> \
     --network mainnet -- \
     pause
   ```

2. **Verify paused state**:
   ```bash
   stellar contract invoke \
     --id $CONTRACT_ID \
     --source <ANY_KEY> \
     --network mainnet -- \
     is_paused
   # Expected: true
   ```

3. **Investigate the issue** using on-chain data and logs.

4. **If a fix is required**, deploy a new wasm and upgrade:
   ```bash
   stellar contract deploy \
     --wasm target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm \
     --source <DEPLOYER_SECRET_KEY> \
     --network mainnet

   stellar contract invoke \
     --id $CONTRACT_ID \
     --source <ADMIN_SECRET_KEY> \
     --network mainnet -- \
     upgrade \
     --new_wasm_hash <NEW_WASM_HASH>
   ```

5. **If rollback to previous version**, ensure you have the previous wasm hash
   and use the same upgrade process.

6. **Unpause after resolution**:
   ```bash
   stellar contract invoke \
     --id $CONTRACT_ID \
     --source <ADMIN_SECRET_KEY> \
     --network mainnet -- \
     unpause
   ```

---

## Incident Response

### Key contacts
| Role | Contact | Responsibility |
|------|---------|----------------|
| Protocol Admin | <ADMIN_ADDRESS> | Pause/unpause, upgrade, config changes |
| Fee Recipient | <FEE_RECIPIENT_ADDRESS> | Monitor fee distribution |
| On-call Engineer | <SLACK/PAGERDUTY> | First responder for alerts |

### Alerting
- Configure Stellar RPC alerts for `pause` events
- Monitor `get_stats` for anomalous volume spikes
- Watch for `slash_solver` events (indicates solver failures)

### Escalation
1. Pause contract (admin action)
2. Notify on-call engineer
3. Assess impact and coordinate fix
4. Communicate status to community via Discord/Twitter
5. Execute rollback or upgrade as needed

---

## Post-Deployment Checklist (24h)

- [ ] Contract is not paused (`is_paused` = false)
- [ ] Stats show expected activity (`get_stats`)
- [ ] Destination token allowlist enforced if configured
- [ ] Source chain allowlist enforced if configured
- [ ] Solver registry linked and tier perks verified
- [ ] Fee router distributing correctly (`distribute` called successfully)
- [ ] No unexpected `slash_solver` or `dispute_fill` events
- [ ] Monitoring dashboards green
- [ ] Team notified of successful deployment
