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

## Next Steps

- See [Solver Integration Guide](./solver-integration-guide.md) for building a solver bot
- See [Dispute Resolution Design](./dispute-resolution-design.md) for understanding fill workflows
- See main [README](../README.md) for full API reference

## References

- [Stellar Quickstart Image](https://github.com/stellar/quickstart)
- [Soroban Documentation](https://developers.stellar.org/docs/smart-contracts)
- [Stellar CLI Reference](https://developers.stellar.org/docs/tools/developer-tools/cli/reference)
