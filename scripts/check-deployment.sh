#!/usr/bin/env bash
# check-deployment.sh — health check for a deployed vortex-intent-settlement contract
#
# Usage:
#   ./scripts/check-deployment.sh <CONTRACT_ID>              # check on mainnet
#   ./scripts/check-deployment.sh <CONTRACT_ID> testnet      # check on testnet
#
#   # Check the five wave contracts as well (governor, backstop_vault,
#   # fee_router, intent_periphery, treasury_streams). Contract IDs may be
#   # supplied via environment variables or an env file produced by
#   # setup-local-network.sh (default: .local-network.env in the repo root).
#   ./scripts/check-deployment.sh <CONTRACT_ID> testnet --env-file .local-network.env
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
  die "Usage: check-deployment.sh <CONTRACT_ID> [NETWORK] [--env-file <PATH>]"
fi

CONTRACT_ID="$1"
NETWORK="${2:-mainnet}"

# Optional --env-file argument (may appear anywhere after the first two args).
ENV_FILE=""
shift $(( $# >= 2 ? 2 : 1 ))
while [[ $# -gt 0 ]]; do
  case "$1" in
    --env-file)
      [[ $# -ge 2 ]] || die "--env-file requires a path argument"
      ENV_FILE="$2"
      shift 2
      ;;
    *)
      die "Unknown argument: $1"
      ;;
  esac
done

# Load contract IDs for the wave contracts. Precedence: explicit environment
# variables win over values from the env file.
if [[ -z "${ENV_FILE}" && -f ".local-network.env" ]]; then
  ENV_FILE=".local-network.env"
fi
if [[ -n "${ENV_FILE}" ]]; then
  [[ -f "${ENV_FILE}" ]] || die "Env file not found: ${ENV_FILE}"
  info "Loading contract IDs from ${ENV_FILE}"
  # shellcheck disable=SC1090
  set -a
  source "${ENV_FILE}"
  set +a
fi

# Contract IDs for the wave contracts (empty means "not configured").
GOVERNOR_ID="${GOVERNOR_ID:-}"
BACKSTOP_VAULT_ID="${BACKSTOP_VAULT_ID:-}"
FEE_ROUTER_ID="${FEE_ROUTER_ID:-}"
INTENT_PERIPHERY_ID="${INTENT_PERIPHERY_ID:-}"
TREASURY_STREAMS_ID="${TREASURY_STREAMS_ID:-}"

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
    [[ -n "${result}" ]] || die "Empty result for ${view_name}: contract ${CONTRACT_ID} is not deployed on ${NETWORK}"
    echo "${result}"
  else
    die "Query failed for ${view_name} on contract ${CONTRACT_ID} (${NETWORK}): ${result}"
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
  [[ -n "${result}" ]] || die "Empty result for get_stats: contract ${CONTRACT_ID} is not deployed on ${NETWORK}"
  echo "${result}"
else
  die "Query failed for get_stats on contract ${CONTRACT_ID} (${NETWORK}): ${result}"
fi

# ---------------------------------------------------------------------------
# Wave contracts health checks
# ---------------------------------------------------------------------------
# Helper to query a view on an arbitrary contract ID. Fails hard on error.
query_contract_view() {
  local contract_id="$1"
  local view_name="$2"
  local label="$3"

  echo -n "${label}: "
  if result=$(stellar contract invoke \
      --id "${contract_id}" \
      --network "${NETWORK}" \
      ${SOURCE_KEY:+--source "${SOURCE_KEY}"} \
      -- "${view_name}" 2>&1); then
    [[ -n "${result}" ]] || die "Empty result for ${label} (${view_name}): contract ${contract_id} is not deployed on ${NETWORK}"
    echo "${result}"
  else
    die "Query failed for ${label} (${view_name} on ${contract_id}): ${result}"
  fi
}

# Helper to validate a contract ID supplied for a wave contract.
require_contract_id() {
  local var_name="$1"
  local value="$2"
  if [[ -z "${value}" ]]; then
    die "Missing ${var_name}. Set it in the environment or via --env-file (see setup-local-network.sh)."
  fi
  if [[ ! "${value}" =~ ^C[A-Z0-9]{55}$ ]]; then
    die "Invalid ${var_name} format: ${value}"
  fi
}

check_wave_contracts() {
  echo ""
  echo "=== Wave Contracts — Deployment Health Check ==="
  echo ""

  # --- governor -----------------------------------------------------------
  require_contract_id "GOVERNOR_ID" "${GOVERNOR_ID}"
  echo "--- Governor (${GOVERNOR_ID}) ---"
  query_contract_view "${GOVERNOR_ID}" "get_config" "Config"
  query_contract_view "${GOVERNOR_ID}" "get_admin" "Admin"
  query_contract_view "${GOVERNOR_ID}" "is_paused" "Paused"

  # --- backstop_vault -----------------------------------------------------
  require_contract_id "BACKSTOP_VAULT_ID" "${BACKSTOP_VAULT_ID}"
  echo ""
  echo "--- Backstop Vault (${BACKSTOP_VAULT_ID}) ---"
  query_contract_view "${BACKSTOP_VAULT_ID}" "get_admin" "Admin"
  query_contract_view "${BACKSTOP_VAULT_ID}" "get_settlement" "Settlement"

  # --- fee_router ---------------------------------------------------------
  require_contract_id "FEE_ROUTER_ID" "${FEE_ROUTER_ID}"
  echo ""
  echo "--- Fee Router (${FEE_ROUTER_ID}) ---"
  query_contract_view "${FEE_ROUTER_ID}" "get_admin" "Admin"
  query_contract_view "${FEE_ROUTER_ID}" "get_weights" "Weights"

  # --- intent_periphery ---------------------------------------------------
  require_contract_id "INTENT_PERIPHERY_ID" "${INTENT_PERIPHERY_ID}"
  echo ""
  echo "--- Intent Periphery (${INTENT_PERIPHERY_ID}) ---"
  query_contract_view "${INTENT_PERIPHERY_ID}" "is_initialized" "Initialized"
  query_contract_view "${INTENT_PERIPHERY_ID}" "get_admin" "Admin"

  # --- treasury_streams ---------------------------------------------------
  require_contract_id "TREASURY_STREAMS_ID" "${TREASURY_STREAMS_ID}"
  echo ""
  echo "--- Treasury Streams (${TREASURY_STREAMS_ID}) ---"
  query_contract_view "${TREASURY_STREAMS_ID}" "get_admin" "Admin"
  query_contract_view "${TREASURY_STREAMS_ID}" "get_stream_count" "Stream Count"
}

# Only run the wave checks when at least one wave contract ID is configured.
if [[ -n "${GOVERNOR_ID}${BACKSTOP_VAULT_ID}${FEE_ROUTER_ID}${INTENT_PERIPHERY_ID}${TREASURY_STREAMS_ID}" ]]; then
  check_wave_contracts
else
  info "No wave contract IDs configured; skipping governor/backstop_vault/fee_router/intent_periphery/treasury_streams checks."
  info "Provide them via environment variables or --env-file to enable these checks."
fi

echo ""
echo "=== Health check complete ==="
