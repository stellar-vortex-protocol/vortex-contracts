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
# Main
# ---------------------------------------------------------------------------
main() {
  local command="${1:-start}"

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
    *)
      echo "Usage: $0 {start|stop|clean|logs|status}"
      echo ""
      echo "Commands:"
      echo "  start   — start the local network and configure Stellar CLI"
      echo "  stop    — stop the network (preserves data)"
      echo "  clean   — stop and remove volumes (destructive)"
      echo "  logs    — tail network logs"
      echo "  status  — check if network is running"
      exit 1
      ;;
  esac
}

main "$@"
