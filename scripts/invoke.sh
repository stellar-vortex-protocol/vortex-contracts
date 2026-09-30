#!/bin/bash

# Vortex Contract CLI Helper
# Wraps stellar contract invoke commands from the README examples.
# Reduces boilerplate by reading common parameters from environment variables or a config file.
#
# Usage:
#   ./scripts/invoke.sh submit_intent --user <USER> --src_chain ethereum --src_token <TOKEN> --src_amount <AMOUNT> --dst_token <USDC_SAC> --min_dst_amount <MIN>
#   ./scripts/invoke.sh register_solver --solver <SOLVER> --bond_amount <AMOUNT>
#   ./scripts/invoke.sh accept_intent --solver <SOLVER> --intent_id <INTENT_ID>
#   ./scripts/invoke.sh fill_intent --solver <SOLVER> --intent_id <INTENT_ID> --fill_amount <AMOUNT>
#   ./scripts/invoke.sh slash_solver --intent_id <INTENT_ID>
#   ./scripts/invoke.sh get_stats

set -euo pipefail

# Configuration: read from environment or .env.local file
CONFIG_FILE="${VORTEX_CONFIG:-${PWD}/.env.local}"

# Load configuration from file if it exists
if [[ -f "$CONFIG_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$CONFIG_FILE"
fi

# Use environment variables with fallback to defaults
CONTRACT_ID="${VORTEX_CONTRACT_ID:-}"
SECRET_KEY="${VORTEX_SECRET_KEY:-}"
NETWORK="${VORTEX_NETWORK:-testnet}"

# Validate required parameters
if [[ -z "$CONTRACT_ID" ]]; then
  echo "Error: VORTEX_CONTRACT_ID not set. Set it via environment or in $CONFIG_FILE"
  exit 1
fi

if [[ -z "$SECRET_KEY" ]]; then
  echo "Error: VORTEX_SECRET_KEY not set. Set it via environment or in $CONFIG_FILE"
  exit 1
fi

# Extract the command (first positional argument)
COMMAND="${1:-}"
if [[ -z "$COMMAND" ]]; then
  cat << 'EOF'
Vortex Contract CLI Helper

Usage:
  ./scripts/invoke.sh <command> [options]

Available Commands:
  submit_intent    Create a swap intent
  register_solver  Register a solver with a bond
  accept_intent    Accept an intent as a solver
  fill_intent      Fill an accepted intent
  slash_solver     Slash a solver that missed the fill window
  get_stats        Read protocol statistics
  is_intent_fillable  Check if an intent can be filled

Examples:
  # Register a solver with 50 USDC bond
  export VORTEX_CONTRACT_ID=<CONTRACT_ID>
  export VORTEX_SECRET_KEY=<SECRET_KEY>
  ./scripts/invoke.sh register_solver --solver <SOLVER_ADDRESS> --bond_amount 500000000

  # Submit an intent
  ./scripts/invoke.sh submit_intent \
    --user <USER_ADDRESS> \
    --src_chain ethereum \
    --src_token '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2' \
    --src_amount 1000000000000000000 \
    --dst_token <USDC_SAC_ADDRESS> \
    --min_dst_amount 35000000000

  # Accept an intent
  ./scripts/invoke.sh accept_intent --solver <SOLVER_ADDRESS> --intent_id <INTENT_ID>

  # Fill an intent
  ./scripts/invoke.sh fill_intent \
    --solver <SOLVER_ADDRESS> \
    --intent_id <INTENT_ID> \
    --fill_amount 35000000000

  # Get protocol stats
  ./scripts/invoke.sh get_stats

Configuration:
  Set these via environment variables or in .env.local:
    VORTEX_CONTRACT_ID    - Contract ID (required)
    VORTEX_SECRET_KEY     - Secret key for signing (required)
    VORTEX_NETWORK        - Network (default: testnet)
    VORTEX_CONFIG         - Config file path (default: .env.local)
EOF
  exit 1
fi

# Remove the command from arguments
shift

# Build the base stellar CLI command
STELLAR_CMD=(
  stellar contract invoke
  --id "$CONTRACT_ID"
  --source "$SECRET_KEY"
  --network "$NETWORK"
  --
  "$COMMAND"
)

# Parse command-specific arguments and build the full command
case "$COMMAND" in
  submit_intent)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --user)
          STELLAR_CMD+=(--user "$2")
          shift 2
          ;;
        --src_chain)
          # Quote string arguments for Soroban
          STELLAR_CMD+=(--src_chain "\"$2\"")
          shift 2
          ;;
        --src_token)
          STELLAR_CMD+=(--src_token "\"$2\"")
          shift 2
          ;;
        --src_amount)
          STELLAR_CMD+=(--src_amount "$2")
          shift 2
          ;;
        --dst_token)
          STELLAR_CMD+=(--dst_token "$2")
          shift 2
          ;;
        --min_dst_amount)
          STELLAR_CMD+=(--min_dst_amount "$2")
          shift 2
          ;;
        *)
          echo "Unknown option: $1"
          exit 1
          ;;
      esac
    done
    ;;

  register_solver)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --solver)
          STELLAR_CMD+=(--solver "$2")
          shift 2
          ;;
        --bond_amount)
          STELLAR_CMD+=(--bond_amount "$2")
          shift 2
          ;;
        *)
          echo "Unknown option: $1"
          exit 1
          ;;
      esac
    done
    ;;

  accept_intent)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --solver)
          STELLAR_CMD+=(--solver "$2")
          shift 2
          ;;
        --intent_id)
          STELLAR_CMD+=(--intent_id "$2")
          shift 2
          ;;
        *)
          echo "Unknown option: $1"
          exit 1
          ;;
      esac
    done
    ;;

  fill_intent)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --solver)
          STELLAR_CMD+=(--solver "$2")
          shift 2
          ;;
        --intent_id)
          STELLAR_CMD+=(--intent_id "$2")
          shift 2
          ;;
        --fill_amount)
          STELLAR_CMD+=(--fill_amount "$2")
          shift 2
          ;;
        *)
          echo "Unknown option: $1"
          exit 1
          ;;
      esac
    done
    ;;

  is_intent_fillable)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --intent_id)
          STELLAR_CMD+=(--intent_id "$2")
          shift 2
          ;;
        --solver)
          STELLAR_CMD+=(--solver "$2")
          shift 2
          ;;
        *)
          echo "Unknown option: $1"
          exit 1
          ;;
      esac
    done
    ;;

  slash_solver)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --intent_id)
          STELLAR_CMD+=(--intent_id "$2")
          shift 2
          ;;
        *)
          echo "Unknown option: $1"
          exit 1
          ;;
      esac
    done
    ;;

  get_stats)
    # No additional arguments needed
    ;;

  *)
    echo "Unknown command: $COMMAND"
    exit 1
    ;;
esac

# Execute the stellar CLI command
echo "Executing: ${STELLAR_CMD[*]}" >&2
exec "${STELLAR_CMD[@]}"
