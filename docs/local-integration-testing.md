# Local Integration Testing with Docker

This guide walks through running a local Soroban test network and deploying Vortex contracts against it for close-to-production integration testing without a testnet account or network latency.

## Prerequisites

- **Docker** (and docker-compose): Install from [docker.com](https://www.docker.com/products/docker-desktop)
- **Stellar CLI**: Install from [developers.stellar.org](https://developers.stellar.org/docs/tools/developer-tools/cli/install)
- **Rust toolchain** (1.75+): Install from [rustup.rs](https://rustup.rs/)

## Quick Start

### 1. Start the Local Network

```bash
make local-network-start
# or directly: ./setup-local-network.sh start
```

This:
- Spins up a Stellar quickstart container with Horizon and Soroban RPC enabled
- Waits for both to be healthy and responsive
- Registers a `local` network with the Stellar CLI

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

This builds the release wasm for every wasm crate in the workspace. Output goes to the workspace-level `target/wasm32-unknown-unknown/release/`. To build only `intent_settlement`:

```bash
make intent_settlement-build
```

### 3. Deploy and Test

Set the network:

```bash
export STELLAR_NETWORK=local
```

Create a test account (or use a pre-funded account from the quickstart container):

```bash
stellar keys generate --network $STELLAR_NETWORK --fund test-account
export STELLAR_ACCOUNT=$(stellar keys address test-account)
```

Deploy the intent settlement contract:

```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm \
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
    --fee_recipient test-account \
    --bond_token <bond_token_contract_id>
```

`bond_token` must be a deployed SEP-41 token contract (`initialize` probes it and fails otherwise).

### 4. Run Integration Tests

The workspace integration suite builds fresh wasms and runs against them in-process:

```bash
make integration-test
```

To run the end-to-end smoke test against a local standalone network (register_solver, submit_intent, accept_intent, fill_intent), use:

```bash
./scripts/e2e-test.sh
```

To run the `intent_settlement` unit tests:

```bash
make intent_settlement-test
```

## Network Management

### Check Network Status

```bash
make local-network-status
```

### View Logs

```bash
make local-network-logs
```

### Stop Network (Preserves Data)

```bash
make local-network-stop
```

### Clean Network (Destructive)

```bash
make local-network-clean
```

This removes all ledger data and volumes. Use before starting fresh.

### Deploy the Other Contracts

`setup-local-network.sh` also provides `deploy-contracts` (deploys and initializes the newer contracts from `target/wasm32-unknown-unknown/release`, writing IDs to `.local-network.env`) and `deploy-timelock <admin> [min_delay]`. Run `./setup-local-network.sh` with an unknown command to print the full usage.

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
  --wasm target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm \
  --source test-account \
  --network $STELLAR_NETWORK)

# Initialize
stellar contract invoke \
  --id $INTENT_ID \
  --source test-account \
  --network $STELLAR_NETWORK \
  -- initialize --admin test-account --fee_recipient test-account \
  --bond_token <bond_token_contract_id>

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
    --deadline 9999999999
```

## Architecture

- **Horizon**: REST API for balance queries, transaction submission (port 8000)
- **Soroban RPC**: JSON-RPC 2.0 endpoint for smart contract invocation (port 8001)
- **Docker service**: `stellar` (container `vortex-soroban-testnet`, image `stellar/quickstart`), defined in `docker-compose.yml`
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
make local-network-logs
```

### Contract deploy fails with "InvalidContractData"

Ensure the WASM binary was built in release mode:

```bash
make intent_settlement-build
```

## Next Steps

- See [Solver Integration Guide](./solver-integration-guide.md) for building a solver bot
- See [Dispute Resolution Design](./dispute-resolution-design.md) for understanding fill workflows
- See main [README](../README.md) for full API reference

## References

- [Stellar Quickstart Image](https://github.com/stellar/quickstart)
- [Soroban Documentation](https://developers.stellar.org/docs/smart-contracts)
- [Stellar CLI Reference](https://developers.stellar.org/docs/tools/developer-tools/cli/reference)
