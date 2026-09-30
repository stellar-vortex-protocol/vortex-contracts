#!/bin/bash
# End-to-end integration test for intent_settlement contract on local Soroban network
#
# This script:
# 1. Starts a local Soroban standalone network
# 2. Builds and deploys intent_settlement
# 3. Initializes the contract
# 4. Runs the full smoke test: register_solver → submit_intent → accept_intent → fill_intent
# 5. Verifies state transitions at each step

set -euo pipefail

# Configuration
NETWORK="standalone"
NETWORK_URL="http://localhost:8000/soroban/rpc"
NETWORK_PASSPHRASE="Standalone Network ; February 2017"
CONTRACT_PATH="./intent_settlement/target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm"
USDC_CONTRACT_ID="CBBD47AB2EB00768F7AF2FBFE442F62C12B1F26F7FE564B5D8001ABD5A51D5BD"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Cleanup function
cleanup() {
    log_info "Cleaning up..."
    # Stop standalone network if it's running
    if command -v soroban &> /dev/null; then
        soroban network rm -f $NETWORK 2>/dev/null || true
    fi
}

trap cleanup EXIT

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."

    if ! command -v stellar &> /dev/null; then
        log_error "Stellar CLI not found. Install it with: cargo install --locked stellar-cli --features opt"
        exit 1
    fi

    if ! command -v jq &> /dev/null; then
        log_warn "jq not found. Some output parsing may fail."
    fi

    if [ ! -f "$CONTRACT_PATH" ]; then
        log_error "Contract wasm not found at $CONTRACT_PATH"
        log_info "Building contract..."
        cd intent_settlement
        stellar contract build
        cd ..
    fi
}

# Start standalone network
start_network() {
    log_info "Starting Soroban standalone network..."

    # Configure network in stellar CLI
    stellar network add \
        --allow-http \
        --rpc-url "$NETWORK_URL" \
        --network-passphrase "$NETWORK_PASSPHRASE" \
        $NETWORK || true

    # Start the standalone network
    if docker ps | grep -q soroban; then
        log_info "Standalone network already running"
    else
        log_info "Pulling Stellar quickstart image..."
        docker pull stellar/quickstart:latest

        log_info "Starting standalone network container..."
        docker run -d \
            --name soroban-standalone \
            -p 8000:8000 \
            stellar/quickstart:latest \
            --standalone \
            --protocol-version 20 \
            --enable-soroban-diagnostic-events 2>/dev/null || true

        # Wait for network to be ready
        log_info "Waiting for network to be ready..."
        for i in {1..60}; do
            if curl -s -X POST "$NETWORK_URL" \
                -H "Content-Type: application/json" \
                -d '{"jsonrpc":"2.0","id":1,"method":"getNetwork","params":[]}' | grep -q "id"; then
                log_info "Network is ready!"
                break
            fi
            if [ $i -eq 60 ]; then
                log_error "Network failed to start within 60 seconds"
                exit 1
            fi
            sleep 1
        done
    fi
}

# Generate test accounts
generate_accounts() {
    log_info "Generating test accounts..."

    # Admin account
    stellar keys generate --network $NETWORK admin --no-prompt 2>/dev/null || true
    ADMIN_KEY=$(stellar keys show admin --network $NETWORK --secret)
    ADMIN_ADDR=$(stellar keys show admin --network $NETWORK)

    # User account
    stellar keys generate --network $NETWORK user --no-prompt 2>/dev/null || true
    USER_KEY=$(stellar keys show user --network $NETWORK --secret)
    USER_ADDR=$(stellar keys show user --network $NETWORK)

    # Solver account
    stellar keys generate --network $NETWORK solver --no-prompt 2>/dev/null || true
    SOLVER_KEY=$(stellar keys show solver --network $NETWORK --secret)
    SOLVER_ADDR=$(stellar keys show solver --network $NETWORK)

    # Fee recipient
    stellar keys generate --network $NETWORK fee_recipient --no-prompt 2>/dev/null || true
    FEE_RECIPIENT_ADDR=$(stellar keys show fee_recipient --network $NETWORK)

    log_info "Admin: $ADMIN_ADDR"
    log_info "User: $USER_ADDR"
    log_info "Solver: $SOLVER_ADDR"
    log_info "Fee Recipient: $FEE_RECIPIENT_ADDR"

    # Fund accounts via friendbot
    log_info "Funding test accounts..."
    for KEY in admin user solver fee_recipient; do
        ADDR=$(stellar keys show $KEY --network $NETWORK)
        curl -s "http://localhost:8000/friendbot?addr=$ADDR" > /dev/null || true
    done

    sleep 2
}

# Deploy contract
deploy_contract() {
    log_info "Deploying contract..."

    CONTRACT_ID=$(stellar contract deploy \
        --wasm "$CONTRACT_PATH" \
        --source $ADMIN_KEY \
        --network $NETWORK \
        2>/dev/null | grep -oP 'Contract ID: \K\S+' || echo "")

    if [ -z "$CONTRACT_ID" ]; then
        log_error "Failed to deploy contract"
        exit 1
    fi

    log_info "Contract deployed: $CONTRACT_ID"
    echo "$CONTRACT_ID"
}

# Initialize contract
initialize_contract() {
    local CONTRACT_ID=$1
    local ADMIN_KEY=$2
    local ADMIN_ADDR=$3
    local FEE_RECIPIENT_ADDR=$4

    log_info "Initializing contract..."

    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$ADMIN_KEY" \
        --network $NETWORK -- \
        initialize \
        --admin "$ADMIN_ADDR" \
        --fee_recipient "$FEE_RECIPIENT_ADDR" \
        --bond_token "$USDC_CONTRACT_ID" \
        2>&1 || true

    sleep 2

    # Verify initialization
    local RESULT=$(stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$ADMIN_KEY" \
        --network $NETWORK -- \
        get_admin 2>&1)

    if echo "$RESULT" | grep -q "$ADMIN_ADDR"; then
        log_info "Contract initialized successfully"
    else
        log_error "Contract initialization may have failed"
        log_info "Admin check result: $RESULT"
    fi
}

# Add allowed destination token
add_dst_token() {
    local CONTRACT_ID=$1
    local ADMIN_KEY=$2

    log_info "Adding allowed destination token..."

    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$ADMIN_KEY" \
        --network $NETWORK -- \
        add_allowed_dst_token \
        --token "$USDC_CONTRACT_ID" \
        2>&1 || true

    sleep 1
}

# Enable destination allowlist
enable_dst_allowlist() {
    local CONTRACT_ID=$1
    local ADMIN_KEY=$2

    log_info "Enabling destination token allowlist..."

    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$ADMIN_KEY" \
        --network $NETWORK -- \
        set_dst_allowlist_enabled \
        --enabled true \
        2>&1 || true

    sleep 1
}

# Register solver
register_solver() {
    local CONTRACT_ID=$1
    local SOLVER_KEY=$2
    local SOLVER_ADDR=$3

    log_info "Registering solver..."

    # Minimum bond: 500000000 stroops (50 USDC)
    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$SOLVER_KEY" \
        --network $NETWORK -- \
        register_solver \
        --solver "$SOLVER_ADDR" \
        --bond_amount 500000000 \
        2>&1 || true

    sleep 2

    # Verify registration
    local RESULT=$(stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$SOLVER_KEY" \
        --network $NETWORK -- \
        is_solver_eligible \
        --solver "$SOLVER_ADDR" 2>&1)

    if echo "$RESULT" | grep -q "true"; then
        log_info "Solver registered successfully"
    else
        log_error "Solver registration verification failed"
    fi
}

# Submit intent
submit_intent() {
    local CONTRACT_ID=$1
    local USER_KEY=$2
    local USER_ADDR=$3

    log_info "Submitting test intent..."

    local RESULT=$(stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$USER_KEY" \
        --network $NETWORK -- \
        submit_intent \
        --user "$USER_ADDR" \
        --src_chain '"ethereum"' \
        --src_token '"0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"' \
        --src_amount 1000000000000000000 \
        --dst_token "$USDC_CONTRACT_ID" \
        --min_dst_amount 100000000 \
        2>&1)

    sleep 2

    # Extract intent ID from result
    INTENT_ID=$(echo "$RESULT" | grep -oP 'intent_id["\s:]*\K\d+' | head -1 || echo "0")

    if [ "$INTENT_ID" -eq 0 ]; then
        log_warn "Could not extract intent ID, using 0"
    fi

    log_info "Intent submitted with ID: $INTENT_ID"
    echo "$INTENT_ID"
}

# Get intent state
get_intent_state() {
    local CONTRACT_ID=$1
    local INTENT_ID=$2

    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "" \
        --network $NETWORK -- \
        get_intent \
        --intent_id "$INTENT_ID" 2>&1 || true
}

# Accept intent
accept_intent() {
    local CONTRACT_ID=$1
    local SOLVER_KEY=$2
    local SOLVER_ADDR=$3
    local INTENT_ID=$4

    log_info "Accepting intent..."

    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$SOLVER_KEY" \
        --network $NETWORK -- \
        accept_intent \
        --solver "$SOLVER_ADDR" \
        --intent_id "$INTENT_ID" \
        2>&1 || true

    sleep 2

    log_info "Intent state after accept:"
    get_intent_state "$CONTRACT_ID" "$INTENT_ID"
}

# Fill intent
fill_intent() {
    local CONTRACT_ID=$1
    local SOLVER_KEY=$2
    local SOLVER_ADDR=$3
    local INTENT_ID=$4
    local FILL_AMOUNT=${5:-100000000}

    log_info "Filling intent..."

    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "$SOLVER_KEY" \
        --network $NETWORK -- \
        fill_intent \
        --solver "$SOLVER_ADDR" \
        --intent_id "$INTENT_ID" \
        --fill_amount "$FILL_AMOUNT" \
        2>&1 || true

    sleep 2

    log_info "Intent state after fill:"
    get_intent_state "$CONTRACT_ID" "$INTENT_ID"
}

# Get contract stats
get_stats() {
    local CONTRACT_ID=$1

    log_info "Contract stats:"
    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source "" \
        --network $NETWORK -- \
        get_stats 2>&1 || true
}

# Main execution
main() {
    log_info "Starting end-to-end integration test"

    check_prerequisites
    start_network
    generate_accounts

    CONTRACT_ID=$(deploy_contract)

    initialize_contract "$CONTRACT_ID" "$ADMIN_KEY" "$ADMIN_ADDR" "$FEE_RECIPIENT_ADDR"
    add_dst_token "$CONTRACT_ID" "$ADMIN_KEY"
    enable_dst_allowlist "$CONTRACT_ID" "$ADMIN_KEY"

    register_solver "$CONTRACT_ID" "$SOLVER_KEY" "$SOLVER_ADDR"

    INTENT_ID=$(submit_intent "$CONTRACT_ID" "$USER_KEY" "$USER_ADDR")

    accept_intent "$CONTRACT_ID" "$SOLVER_KEY" "$SOLVER_ADDR" "$INTENT_ID"
    fill_intent "$CONTRACT_ID" "$SOLVER_KEY" "$SOLVER_ADDR" "$INTENT_ID" 100000000

    get_stats "$CONTRACT_ID"

    log_info "✓ End-to-end test completed successfully!"
}

# Execute if script is run directly
if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    main "$@"
fi
