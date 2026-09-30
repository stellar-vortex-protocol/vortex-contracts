#!/usr/bin/env bash
# check-deployment.sh — health check for a deployed vortex-intent-settlement contract
#
# Usage:
#   ./scripts/check-deployment.sh <CONTRACT_ID>              # check on mainnet
#   ./scripts/check-deployment.sh <CONTRACT_ID> testnet      # check on testnet
#
# This script performs a quick deployment health overview by querying the
# contract's key state values. All queries are read-only (no fees, no side
# effects).
#
# Requires:
#   - stellar CLI installed and in $PATH
#   - A valid Stellar source key available (identity or STELLAR_SECRET_KEY env var)
#   - Network access to a Stellar RPC endpoint

set -euo pipefail

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info() { echo "[check-deployment] $*"; }
die()  { echo "[check-deployment] ERROR: $*" >&2; exit 1; }

# ---------------------------------------------------------------------------
# Argument validation
# ---------------------------------------------------------------------------
if [[ $# -lt 1 ]]; then
  die "Usage: check-deployment.sh <CONTRACT_ID> [NETWORK]"
fi

CONTRACT_ID="$1"
NETWORK="${2:-mainnet}"

# Validate CONTRACT_ID format (Stellar contract IDs start with 'C')
if [[ ! "${CONTRACT_ID}" =~ ^C[A-Z0-9]{55}$ ]]; then
  die "Invalid CONTRACT_ID format: ${CONTRACT_ID}"
fi

info "Contract: ${CONTRACT_ID}"
info "Network: ${NETWORK}"
echo ""

# ---------------------------------------------------------------------------
# Pre-flight check: stellar CLI and source key are available
# ---------------------------------------------------------------------------
if ! command -v stellar &> /dev/null; then
  die "stellar CLI not found. Install from https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli"
fi

# Determine the source key: use STELLAR_SECRET_KEY if set, otherwise use the
# default identity from stellar config.
SOURCE_KEY=""
if [[ -n "${STELLAR_SECRET_KEY:-}" ]]; then
  SOURCE_KEY="${STELLAR_SECRET_KEY}"
else
  # Try to use the default identity; stellar will error if none is configured.
  # We don't explicitly validate it here; let stellar's error message be the guide.
  if ! stellar keys list --global &> /dev/null; then
    die "No STELLAR_SECRET_KEY provided and no configured identity found. Set STELLAR_SECRET_KEY or configure a default stellar identity."
  fi
fi

# If we have a SOURCE_KEY, pass it; otherwise trust the default identity.
STELLAR_ARGS=("--id" "${CONTRACT_ID}" "--network" "${NETWORK}")
if [[ -n "${SOURCE_KEY}" ]]; then
  STELLAR_ARGS+=("--source" "${SOURCE_KEY}")
fi

# ---------------------------------------------------------------------------
# Query contract state
# ---------------------------------------------------------------------------
echo "=== Vortex Intent Settlement — Deployment Health Check ==="
echo ""

# Helper to query a contract view and handle errors gracefully
query_view() {
  local view_name="$1"
  local label="$2"

  echo -n "${label}: "
  if result=$(stellar contract invoke "${STELLAR_ARGS[@]}" -- "${view_name}" 2>&1); then
    echo "${result}"
  else
    die "Query failed for ${view_name}: ${result}"
  fi
}

# Run all six status checks
query_view "get_admin" "Admin"
query_view "get_fee_recipient" "Fee Recipient"
query_view "get_bond_token" "Bond Token"
query_view "is_paused" "Paused"
query_view "is_dst_allowlist_enabled" "Allowlist enabled"

# get_stats returns a 3-tuple: (total_intents, total_volume, open_intents)
echo -n "Stats:            "
if result=$(stellar contract invoke "${STELLAR_ARGS[@]}" -- get_stats 2>&1); then
  # The result is printed as a tuple; no post-processing needed
  echo "${result}"
else
  die "Query failed for get_stats: ${result}"
fi

echo ""
echo "=== Health check complete ==="
