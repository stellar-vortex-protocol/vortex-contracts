#!/usr/bin/env bash
# setup-local-network.sh — spin up a local Soroban test network via docker-compose
# and configure the Stellar CLI to point to it.
#
# Usage:
#   ./setup-local-network.sh start     — start the network
#   ./setup-local-network.sh stop      — stop the network
#   ./setup-local-network.sh clean     — stop and remove volumes
#   ./setup-local-network.sh logs      — tail logs
#   ./setup-local-network.sh status    — check network status
#   ./setup-local-network.sh deploy-timelock <admin> [min_delay]
#                                      — build and deploy the standalone timelock

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
CONTAINER_NAME="vortex-soroban-testnet"
HORIZON_URL="http://localhost:8000"
SOROBAN_RPC_URL="http://localhost:8001"
NETWORK_NAME="local"
PASSPHRASE="Standalone Network ; February 2025"

# Deployment configuration
DEPLOY_ENV_FILE="${DEPLOY_ENV_FILE:-.local-network.env}"
DEPLOYER_IDENTITY="${DEPLOYER_IDENTITY:-local-deployer}"
WASM_DIR="${WASM_DIR:-target/wasm32-unknown-unknown/release}"
# Local-dev parameters
FEE_ROUTER_WEIGHT_DELAY="${FEE_ROUTER_WEIGHT_DELAY:-100}"
TREASURY_STREAM_DURATION="${TREASURY_STREAM_DURATION:-86400}"

info() { echo -e "${BLUE}[setup-local-network]${NC} $*"; }
success() { echo -e "${GREEN}✓${NC} $*"; }
error() { echo -e "${RED}✗${NC} $*" >&2; }
warn() { echo -e "${YELLOW}⚠${NC} $*"; }

# ---------------------------------------------------------------------------
# Start network
# ---------------------------------------------------------------------------
start_network() {
  info "Starting local Soroban test network..."

  if docker-compose ps "$CONTAINER_NAME" &>/dev/null; then
    if [ "$(docker-compose ps -q "$CONTAINER_NAME")" ]; then
      warn "Network is already running (container: $CONTAINER_NAME)"
      return 0
    fi
  fi

  docker-compose up -d
  success "Network started"

  info "Waiting for Horizon to be ready..."
  local max_attempts=30
  local attempt=0
  while [ $attempt -lt $max_attempts ]; do
    if curl -s "$HORIZON_URL" &>/dev/null; then
      success "Horizon is ready at $HORIZON_URL"
      break
    fi
    attempt=$((attempt + 1))
    echo -n "."
    sleep 1
  done

  if [ $attempt -eq $max_attempts ]; then
    error "Horizon failed to start after ${max_attempts}s"
    docker-compose logs "$CONTAINER_NAME"
    exit 1
  fi

  info "Waiting for Soroban RPC to be ready..."
  attempt=0
  while [ $attempt -lt $max_attempts ]; do
    if curl -s -X POST "$SOROBAN_RPC_URL" \
      -H "Content-Type: application/json" \
      -d '{"jsonrpc":"2.0","id":1,"method":"getNetwork","params":[]}' &>/dev/null; then
      success "Soroban RPC is ready at $SOROBAN_RPC_URL"
      break
    fi
    attempt=$((attempt + 1))
    echo -n "."
    sleep 1
  done

  if [ $attempt -eq $max_attempts ]; then
    error "Soroban RPC failed to start after ${max_attempts}s"
    docker-compose logs "$CONTAINER_NAME"
    exit 1
  fi

  # Configure Stellar CLI to use the local network
  info "Configuring Stellar CLI..."
  stellar network add \
    --rpc-url "$SOROBAN_RPC_URL" \
    --network-passphrase "$PASSPHRASE" \
    "$NETWORK_NAME" 2>/dev/null || \
  stellar network add-rpc-url \
    --rpc-url "$SOROBAN_RPC_URL" \
    "$NETWORK_NAME" 2>/dev/null || true

  success "Network is ready!"
  info ""
  info "To use the local network, set STELLAR_NETWORK environment variable:"
  info "  export STELLAR_NETWORK=$NETWORK_NAME"
  info ""
  info "Or pass it to stellar CLI:"
  info "  stellar contract invoke --network $NETWORK_NAME ..."
  info ""
  info "Horizon API: $HORIZON_URL"
  info "Soroban RPC: $SOROBAN_RPC_URL"
  info "Network Passphrase: $PASSPHRASE"
  info ""
}

# ---------------------------------------------------------------------------
# Deploy and initialize the five new contracts
# ---------------------------------------------------------------------------
# Helper: ensure the deployer identity exists and is funded.
ensure_deployer() {
  if ! stellar keys address "$DEPLOYER_IDENTITY" &>/dev/null; then
    info "Generating deployer identity: $DEPLOYER_IDENTITY"
    stellar keys generate "$DEPLOYER_IDENTITY" --network "$NETWORK_NAME" --fund
  fi
  DEPLOYER_ADDRESS="$(stellar keys address "$DEPLOYER_IDENTITY")"
  success "Deployer address: $DEPLOYER_ADDRESS"
}

# Helper: deploy a contract and echo its contract ID.
deploy_contract() {
  local name="$1"
  local wasm="$WASM_DIR/${name}.wasm"
  if [ ! -f "$wasm" ]; then
    error "WASM not found for $name at $wasm (build with: cargo build --target wasm32-unknown-unknown --release)"
    exit 1
  fi
  info "Deploying $name..."
  local contract_id
  contract_id="$(stellar contract deploy \
    --wasm "$wasm" \
    --source "$DEPLOYER_IDENTITY" \
    --network "$NETWORK_NAME")"
  success "$name deployed: $contract_id"
  echo "$contract_id"
}

# Helper: invoke an initializer on a deployed contract.
invoke_init() {
  local contract_id="$1"
  shift
  stellar contract invoke \
    --id "$contract_id" \
    --source "$DEPLOYER_IDENTITY" \
    --network "$NETWORK_NAME" \
    -- "$@"
}

# Helper: persist a contract ID to the env file.
record_contract() {
  local key="$1"
  local value="$2"
  # Remove any existing entry for this key, then append the new one.
  if [ -f "$DEPLOY_ENV_FILE" ]; then
    grep -v "^${key}=" "$DEPLOY_ENV_FILE" > "${DEPLOY_ENV_FILE}.tmp" || true
    mv "${DEPLOY_ENV_FILE}.tmp" "$DEPLOY_ENV_FILE"
  fi
  echo "${key}=${value}" >> "$DEPLOY_ENV_FILE"
}

deploy_contracts() {
  info "Deploying and initializing new-wave contracts..."
  ensure_deployer

  # Reset the env file for a clean deployment record.
  : > "$DEPLOY_ENV_FILE"
  echo "# Auto-generated by setup-local-network.sh deploy-contracts" >> "$DEPLOY_ENV_FILE"
  echo "NETWORK_NAME=${NETWORK_NAME}" >> "$DEPLOY_ENV_FILE"
  echo "SOROBAN_RPC_URL=${SOROBAN_RPC_URL}" >> "$DEPLOY_ENV_FILE"
  echo "DEPLOYER_ADDRESS=${DEPLOYER_ADDRESS}" >> "$DEPLOY_ENV_FILE"

  # --- treasury_streams ---------------------------------------------------
  # Deployed first: its address is used as the settlement target for the
  # backstop_vault and as a dependency for intent_periphery.
  local treasury_streams_id
  treasury_streams_id="$(deploy_contract treasury_streams)"
  record_contract TREASURY_STREAMS_ID "$treasury_streams_id"
  info "Initializing treasury_streams (admin=$DEPLOYER_ADDRESS)..."
  invoke_init "$treasury_streams_id" initialize \
    --admin "$DEPLOYER_ADDRESS" \
    --default_duration "$TREASURY_STREAM_DURATION"
  success "treasury_streams initialized"

  # --- backstop_vault -----------------------------------------------------
  # settlement wired to treasury_streams.
  local backstop_vault_id
  backstop_vault_id="$(deploy_contract backstop_vault)"
  record_contract BACKSTOP_VAULT_ID "$backstop_vault_id"
  info "Initializing backstop_vault (settlement=$treasury_streams_id)..."
  invoke_init "$backstop_vault_id" initialize \
    --admin "$DEPLOYER_ADDRESS" \
    --settlement "$treasury_streams_id"
  success "backstop_vault initialized"

  # --- fee_router ---------------------------------------------------------
  # admin = deployer, weight_delay = local-dev value.
  local fee_router_id
  fee_router_id="$(deploy_contract fee_router)"
  record_contract FEE_ROUTER_ID "$fee_router_id"
  info "Initializing fee_router (admin=$DEPLOYER_ADDRESS, weight_delay=$FEE_ROUTER_WEIGHT_DELAY)..."
  invoke_init "$fee_router_id" initialize \
    --admin "$DEPLOYER_ADDRESS" \
    --weight_delay "$FEE_ROUTER_WEIGHT_DELAY"
  success "fee_router initialized"

  # --- governor -----------------------------------------------------------
  # registry and timelock wired to the deployer-controlled addresses.
  local governor_id
  governor_id="$(deploy_contract governor)"
  record_contract GOVERNOR_ID "$governor_id"
  info "Initializing governor (registry=$DEPLOYER_ADDRESS, timelock=$DEPLOYER_ADDRESS)..."
  invoke_init "$governor_id" initialize \
    --admin "$DEPLOYER_ADDRESS" \
    --registry "$DEPLOYER_ADDRESS" \
    --timelock "$DEPLOYER_ADDRESS"
  success "governor initialized"

  # --- intent_periphery ---------------------------------------------------
  # Dependencies: fee_router and treasury_streams.
  local intent_periphery_id
  intent_periphery_id="$(deploy_contract intent_periphery)"
  record_contract INTENT_PERIPHERY_ID "$intent_periphery_id"
  info "Initializing intent_periphery (fee_router=$fee_router_id, treasury=$treasury_streams_id)..."
  invoke_init "$intent_periphery_id" initialize \
    --admin "$DEPLOYER_ADDRESS" \
    --fee_router "$fee_router_id" \
    --treasury "$treasury_streams_id"
  success "intent_periphery initialized"

  success "All new-wave contracts deployed and initialized"
  info "Contract IDs recorded in: $DEPLOY_ENV_FILE"
  info "Source it to load them:  set -a; . ./$DEPLOY_ENV_FILE; set +a"
}

# ---------------------------------------------------------------------------
# Stop network
# ---------------------------------------------------------------------------
stop_network() {
  info "Stopping local Soroban test network..."
  docker-compose stop
  success "Network stopped"
}

# ---------------------------------------------------------------------------
# Clean network
# ---------------------------------------------------------------------------
clean_network() {
  info "Removing local Soroban test network (including volumes)..."
  docker-compose down -v
  success "Network cleaned"
  warn "Network passphrase and all data have been removed"
}

# ---------------------------------------------------------------------------
# Show logs
# ---------------------------------------------------------------------------
show_logs() {
  info "Showing network logs (Ctrl+C to exit)..."
  docker-compose logs -f "$CONTAINER_NAME"
}

# ---------------------------------------------------------------------------
# Check status
# ---------------------------------------------------------------------------
check_status() {
  if docker-compose ps "$CONTAINER_NAME" 2>/dev/null | grep -q "Up"; then
    success "Network is running"
    info "Horizon: $HORIZON_URL"
    info "Soroban RPC: $SOROBAN_RPC_URL"

    # Test Horizon
    if curl -s "$HORIZON_URL" &>/dev/null; then
      success "Horizon is responding"
    else
      error "Horizon is not responding"
    fi

    # Test Soroban RPC
    if curl -s -X POST "$SOROBAN_RPC_URL" \
      -H "Content-Type: application/json" \
      -d '{"jsonrpc":"2.0","id":1,"method":"getNetwork","params":[]}' \
      | grep -q "jsonrpc"; then
      success "Soroban RPC is responding"
    else
      error "Soroban RPC is not responding"
    fi
  else
    warn "Network is not running"
    info "Start it with: $0 start"
  fi
}

# ---------------------------------------------------------------------------
# Deploy the standalone timelock contract to the local network
# ---------------------------------------------------------------------------
deploy_timelock() {
  local admin="${1:?usage: $0 deploy-timelock <admin_address> [min_delay_secs]}"
  local min_delay="${2:-60}"
  local source="${STELLAR_SOURCE:-default}"

  info "Building timelock..."
  (cd "$(dirname "$0")/timelock" && stellar contract build)

  local wasm
  wasm="$(find "$(dirname "$0")" -path '*/wasm32-unknown-unknown/release/*timelock*.wasm' ! -name '*.optimized.wasm' | head -n1)"
  [ -n "$wasm" ] || { error "timelock wasm not found after build"; exit 1; }

  info "Deploying timelock (admin=$admin, min_delay=${min_delay}s)..."
  local id
  id="$(stellar contract deploy --wasm "$wasm" --source "$source" --network "$NETWORK_NAME" \
    -- --admin "$admin" --proposers "[\"$admin\"]" --executors "[\"$admin\"]" \
    --cancellers "[\"$admin\"]" --min_delay "$min_delay")"
  success "timelock deployed: $id"
}

# ---------------------------------------------------------------------------
# Prerequisites
# ---------------------------------------------------------------------------
check_prereqs() {
  local bin missing=""
  for bin in stellar docker-compose; do
    command -v "$bin" &>/dev/null || missing="$missing $bin"
  done
  if [ -n "$missing" ]; then
    error "Missing required tool(s):$missing. Install them per CONTRIBUTING.md (Toolchain Setup) and docs/local-integration-testing.md (Prerequisites)."
    exit 1
  fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
  local command="${1:-start}"

  check_prereqs

  case "$command" in
    start)
      start_network
      ;;
    stop)
      stop_network
      ;;
    clean)
      clean_network
      ;;
    logs)
      show_logs
      ;;
    status)
      check_status
      ;;
    deploy-contracts)
      deploy_contracts
      ;;
    deploy-timelock)
      shift
      deploy_timelock "$@"
      ;;
    *)
      echo "Usage: $0 {start|stop|clean|logs|status|deploy-contracts|deploy-timelock}"
      echo ""
      echo "Commands:"
      echo "  start             — start the local network and configure Stellar CLI"
      echo "  stop              — stop the network (preserves data)"
      echo "  clean             — stop and remove volumes (destructive)"
      echo "  logs              — tail network logs"
      echo "  status            — check if network is running"
      echo "  deploy-contracts  — deploy and initialize governor, backstop_vault,"
      echo "                      fee_router, intent_periphery, treasury_streams"
      echo "  deploy-timelock <admin> [min_delay] — deploy the standalone timelock"
      exit 1
      ;;
  esac
}

main "$@"
