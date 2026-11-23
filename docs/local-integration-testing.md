# Local Integration Testing with Docker

This guide walks through running a local Soroban test network and deploying Vortex contracts against it for close-to-production integration testing without a testnet account or network latency.

## Prerequisites

- **Docker** (and docker-compose): Install from [docker.com](https://www.docker.com/products/docker-desktop)
- **Stellar CLI**: Install from [developers.stellar.org](https://developers.stellar.org/docs/tools/developer-tools/cli/install)
- **Rust toolchain** (1.75+): Install from [rustup.rs](https://rustup.rs/)

## Quick Start

### 1. Start the Local Network

```bash
./setup-local-network.sh start
```

This:
- Spins up a Stellar quickstart container with Horizon and Soroban RPC enabled
- Waits for both to be healthy and responsive
- Configures the Stellar CLI to connect to the local network
- Provides test accounts with pre-funded balances

**Output:**
```
✓ Network started
✓ Horizon is ready at http://localhost:8000
✓ Soroban RPC is ready at http://localhost:8001
✓ Network is ready!

To use the local network, set STELLAR_NETWORK environment variable:
  export STELLAR_NETWORK=local

Or pass it to stellar CLI:
  stellar contract invoke --network local ...

Horizon API: http://localhost:8000
Soroban RPC: http://localhost:8001
Network Passphrase: Standalone Network ; February 2025
```

### 2. Build the Contracts

```bash
make build
```

Or build all contracts at once:

```bash
cd intent_settlement && cargo build --target wasm32-unknown-unknown --release
cd ../solver_registry && cargo build --target wasm32-unknown-unknown --release
cd ../proof_registry && cargo build --target wasm32-unknown-unknown --release
cd ../reputation_badge && cargo build --target wasm32-unknown-unknown --release
cd ../governor && cargo build --target wasm32-unknown-unknown --release
cd ../backstop_vault && cargo build --target wasm32-unknown-unknown --release
cd ../fee_router && cargo build --target wasm32-unknown-unknown --release
cd ../intent_periphery && cargo build --target wasm32-unknown-unknown --release
cd ../timelock && cargo build --target wasm32-unknown-unknown --release
cd ../treasury_streams && cargo build --target wasm32-unknown-unknown --release
```

### 3. Deploy and Test

Set the network:

```bash
export STELLAR_NETWORK=local
```

Create a test account (or use a pre-funded account from the quickstart container):

```bash
stellar keys generate --network $STELLAR_NETWORK test-account
export STELLAR_ACCOUNT=$(stellar account info test-account --network $STELLAR_NETWORK | jq -r '.id')
```

Deploy the intent settlement contract:

```bash
stellar contract deploy \
  --wasm intent_settlement/target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm \
  --source test-account \
  --network $STELLAR_NETWORK
```

This returns a contract ID (e.g., `CBQG2N...`). Store it:

```bash
export CONTRACT_ID="<contract_id_from_deploy>"
```

Initialize the contract (requires admin account):

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize \
    --admin test-account \
    --fee_recipient test-account
```

### 4. Run Integration Tests

With the network running and contract deployed:

```bash
export STELLAR_NETWORK=local
export CONTRACT_ID="<your_contract_id>"
cargo test --package vortex-intent-settlement --lib --features testutils -- --test-threads=1
```

Or run a specific test:

```bash
cargo test --package vortex-intent-settlement --lib test_submit_and_accept -- --nocapture
```

## Network Management

### Check Network Status

```bash
./setup-local-network.sh status
```

### View Logs

```bash
./setup-local-network.sh logs
```

### Stop Network (Preserves Data)

```bash
./setup-local-network.sh stop
```

### Clean Network (Destructive)

```bash
./setup-local-network.sh clean
```

This removes all ledger data and volumes. Use before starting fresh.

## Common Workflows

### Deploy All Contracts

```bash
#!/bin/bash
set -e

export STELLAR_NETWORK=local

# Build all
make build

# Deploy intent_settlement
INTENT_ID=$(stellar contract deploy \
  --wasm intent_settlement/target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

# Initialize
stellar contract invoke \
  --id $INTENT_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --admin test-account --fee_recipient test-account

echo "Intent Settlement Contract ID: $INTENT_ID"

# Deploy solver_registry
SOLVER_REG_ID=$(stellar contract deploy \
  --wasm solver_registry/target/wasm32-unknown-unknown/release/vortex_solver_registry.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

stellar contract invoke \
  --id $SOLVER_REG_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --admin test-account --bond_token <BOND_TOKEN_ADDRESS> --fee_recipient test-account

echo "Solver Registry Contract ID: $SOLVER_REG_ID"

# Deploy proof_registry
PROOF_REG_ID=$(stellar contract deploy \
  --wasm proof_registry/target/wasm32-unknown-unknown/release/vortex_proof_registry.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

stellar contract invoke \
  --id $PROOF_REG_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --admin test-account --configurator test-account \
     --wormhole_core <WORMHOLE_CORE_ADDRESS> --axelar_gateway <AXELAR_GATEWAY_ADDRESS>

echo "Proof Registry Contract ID: $PROOF_REG_ID"

# Deploy reputation_badge
BADGE_ID=$(stellar contract deploy \
  --wasm reputation_badge/target/wasm32-unknown-unknown/release/vortex_reputation_badge.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

stellar contract invoke \
  --id $BADGE_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --admin test-account --solver_registry $SOLVER_REG_ID

echo "Reputation Badge Contract ID: $BADGE_ID"

# Deploy governor
GOVERNOR_ID=$(stellar contract deploy \
  --wasm governor/target/wasm32-unknown-unknown/release/vortex_governor.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

stellar contract invoke \
  --id $GOVERNOR_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --admin test-account --solver_registry $SOLVER_REG_ID \
     --config '{"quorum_bps":1000,"voting_period_ledgers":17280,"timelock_delay":172800}'

echo "Governor Contract ID: $GOVERNOR_ID"

# Deploy backstop_vault
BACKSTOP_ID=$(stellar contract deploy \
  --wasm backstop_vault/target/wasm32-unknown-unknown/release/vortex_backstop_vault.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

stellar contract invoke \
  --id $BACKSTOP_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --admin test-account --bond_token <BOND_TOKEN_ADDRESS> \
     --settlement $INTENT_ID --min_deposit 500000000 --withdrawal_cooldown_secs 86400

echo "Backstop Vault Contract ID: $BACKSTOP_ID"

# Deploy fee_router
FEE_ROUTER_ID=$(stellar contract deploy \
  --wasm fee_router/target/wasm32-unknown-unknown/release/vortex_fee_router.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

stellar contract invoke \
  --id $FEE_ROUTER_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --governance $GOVERNOR_ID \
     --sinks '[{"address":"test-account","weight_bps":10000}]' \
     --weight_delay 172800

echo "Fee Router Contract ID: $FEE_ROUTER_ID"

# Deploy intent_periphery
PERIPHERY_ID=$(stellar contract deploy \
  --wasm intent_periphery/target/wasm32-unknown-unknown/release/vortex_intent_periphery.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

stellar contract invoke \
  --id $PERIPHERY_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --admin test-account --settlement $INTENT_ID

echo "Intent Periphery Contract ID: $PERIPHERY_ID"

# Deploy timelock
TIMELOCK_ID=$(stellar contract deploy \
  --wasm timelock/target/wasm32-unknown-unknown/release/vortex_timelock.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

# timelock uses __constructor; pass args at deploy time via --args
echo "Timelock Contract ID: $TIMELOCK_ID"

# Deploy treasury_streams
TREASURY_ID=$(stellar contract deploy \
  --wasm treasury_streams/target/wasm32-unknown-unknown/release/vortex_treasury_streams.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

stellar contract invoke \
  --id $TREASURY_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --governor $GOVERNOR_ID

echo "Treasury Streams Contract ID: $TREASURY_ID"
```

### Submit an Intent

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- submit_intent \
    --user test-account \
    --src_chain "ethereum" \
    --src_token "0x..." \
    --src_amount "1000000" \
    --dst_token "CA5MFQQ2Z7EJGG37CGIQY2RJ2XGPKPVDQGTIDWX6VVMGUMG5JQYWF7U" \
    --min_dst_amount "3000000" \
    --deadline "9999999999"
```

## Architecture

- **Horizon**: REST API for balance queries, transaction submission (port 8000)
- **Soroban RPC**: JSON-RPC 2.0 endpoint for smart contract invocation (port 8001)
- **Ledger Data**: Persisted in Docker volume `stellar-ledger` (auto-created)
- **Network Passphrase**: `Standalone Network ; February 2025` (hardcoded in quickstart)

## Troubleshooting

### "docker-compose: command not found"

Modern Docker includes `docker compose` (without hyphen). Use:

```bash
docker compose up -d
```

Or install the standalone `docker-compose` tool.

### "Connection refused" on Soroban RPC

Wait a few more seconds for the RPC to fully boot. Check logs:

```bash
./setup-local-network.sh logs
```

### Contract deploy fails with "InvalidContractData"

Ensure the WASM binary was built in release mode:

```bash
cd intent_settlement && cargo build --target wasm32-unknown-unknown --release
```

## Testing the Newer Contracts

### `governor` + `timelock`

The governance flow requires a live `solver_registry` (for voting power) and a deployed
`timelock`. A minimal end-to-end test on the local network:

```bash
# 1. Register a solver so the account has voting power
stellar contract invoke --id $SOLVER_REG_ID --source test-account --network $STELLAR_NETWORK \
  -- register_solver --solver test-account --bond_amount 500000000

# 2. Sync voting power checkpoint
stellar contract invoke --id $GOVERNOR_ID --source test-account --network $STELLAR_NETWORK \
  -- sync_power --account test-account

# 3. Create a proposal (description + target calls encoded as JSON)
stellar contract invoke --id $GOVERNOR_ID --source test-account --network $STELLAR_NETWORK \
  -- propose --caller test-account --description '"test proposal"' --calls '[]'

# 4. Cast a vote (For = 0)
stellar contract invoke --id $GOVERNOR_ID --source test-account --network $STELLAR_NETWORK \
  -- cast_vote --voter test-account --proposal_id 0 --vote_type 0 --reason '"lgtm"'
```

### `backstop_vault`

```bash
# Deposit bond token as an LP
stellar contract invoke --id $BACKSTOP_ID --source test-account --network $STELLAR_NETWORK \
  -- deposit --lp test-account --amount 500000000

# Check LP position (shares, queued_amount, queued_at)
stellar contract invoke --id $BACKSTOP_ID --source test-account --network $STELLAR_NETWORK \
  -- get_lp_position --lp test-account

# Queue a withdrawal
stellar contract invoke --id $BACKSTOP_ID --source test-account --network $STELLAR_NETWORK \
  -- withdraw --lp test-account --shares 500000000
```

### `fee_router`

```bash
# Trigger a fee distribution for the bond token
stellar contract invoke --id $FEE_ROUTER_ID --source test-account --network $STELLAR_NETWORK \
  -- distribute --token <BOND_TOKEN_ADDRESS>

# Check claimable balance for a sink
stellar contract invoke --id $FEE_ROUTER_ID --source test-account --network $STELLAR_NETWORK \
  -- claimable --sink test-account --token <BOND_TOKEN_ADDRESS>
```

### `intent_periphery`

All calls are read-only proxies; no auth required. Wire it to the deployed settlement first:

```bash
# Verify the periphery points at the right settlement contract
stellar contract invoke --id $PERIPHERY_ID --source test-account --network $STELLAR_NETWORK \
  -- get_settlement

# Fetch a dashboard snapshot (stats + health in one round-trip)
stellar contract invoke --id $PERIPHERY_ID --source test-account --network $STELLAR_NETWORK \
  -- get_dashboard_snapshot
```

### `treasury_streams`

```bash
# Create a 30-day linear stream for a contributor (no cliff)
stellar contract invoke --id $TREASURY_ID --source test-account --network $STELLAR_NETWORK \
  -- create_stream \
     --recipient <RECIPIENT_ADDRESS> \
     --token <BOND_TOKEN_ADDRESS> \
     --total_amount 10000000000 \
     --start $(date +%s) \
     --end $(($(date +%s) + 2592000)) \
     --cliffed false

# Recipient withdraws vested tokens
stellar contract invoke --id $TREASURY_ID --source test-account --network $STELLAR_NETWORK \
  -- withdraw --stream_id 0
```

## Next Steps

- See [Solver Integration Guide](./solver-integration-guide.md) for building a solver bot
- See [Dispute Resolution Design](./dispute-resolution-design.md) for understanding fill workflows
- See main [README](../README.md) for full API reference

## References

- [Stellar Quickstart Image](https://github.com/stellar/quickstart)
- [Soroban Documentation](https://developers.stellar.org/docs/smart-contracts)
- [Stellar CLI Reference](https://developers.stellar.org/docs/tools/developer-tools/cli/reference)
