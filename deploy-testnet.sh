#!/usr/bin/env bash
# deploy-testnet.sh — config-driven deploy + initialize for vortex-contracts
#
# Usage:
#   cp deploy-testnet.env.example deploy-testnet.env   # fill in your values
#   ./deploy-testnet.sh                                 # deploy & initialize
#   ./deploy-testnet.sh --skip-build                    # initialize only (wasm already built)
#
# The script reads all parameters from deploy-testnet.env (or the file
# pointed to by DEPLOY_ENV_FILE) so there's no need to edit this script
# between deployments.
#
# Closes #105

set -euo pipefail

# ---------------------------------------------------------------------------
# Locate and load the env file
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${DEPLOY_ENV_FILE:-${SCRIPT_DIR}/deploy-testnet.env}"

if [[ ! -f "${ENV_FILE}" ]]; then
  cat >&2 <<EOF
[deploy-testnet] ERROR: config file not found: ${ENV_FILE}

Copy the example file and fill in your values:
  cp deploy-testnet.env.example deploy-testnet.env

Then re-run this script.
EOF
  exit 1
fi

# shellcheck source=/dev/null
source "${ENV_FILE}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info() { echo "[deploy-testnet] $*"; }
die()  { echo "[deploy-testnet] ERROR: $*" >&2; exit 1; }

require_var() {
  local var="$1"
  [[ -n "${!var:-}" ]] || die "Required variable \$${var} is not set in ${ENV_FILE}"
}

# ---------------------------------------------------------------------------
# Validate required config
# ---------------------------------------------------------------------------
require_var ADMIN_ADDRESS
require_var FEE_RECIPIENT_ADDRESS
require_var BOND_TOKEN_ADDRESS
require_var SOURCE_SECRET_KEY
require_var NETWORK

WASM_PATH="${WASM_PATH:-${SCRIPT_DIR}/intent_settlement/target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm}"

info "Network:          ${NETWORK}"
info "Admin:            ${ADMIN_ADDRESS}"
info "Fee recipient:    ${FEE_RECIPIENT_ADDRESS}"
info "Bond token:       ${BOND_TOKEN_ADDRESS}"
info "WASM:             ${WASM_PATH}"

# ---------------------------------------------------------------------------
# 1. Build (unless --skip-build is passed)
# ---------------------------------------------------------------------------
SKIP_BUILD=false
for arg in "$@"; do
  [[ "${arg}" == "--skip-build" ]] && SKIP_BUILD=true
done

if [[ "${SKIP_BUILD}" == false ]]; then
  info "Building WASM ..."
  (cd "${SCRIPT_DIR}/intent_settlement" && cargo build --target wasm32-unknown-unknown --release --locked)
  info "Build complete."
fi

if [[ ! -f "${WASM_PATH}" ]]; then
  die "WASM file not found at ${WASM_PATH}. Run without --skip-build or set WASM_PATH."
fi

# ---------------------------------------------------------------------------
# 2. Deploy the contract
# ---------------------------------------------------------------------------
info "Deploying contract to ${NETWORK} ..."
CONTRACT_ID=$(stellar contract deploy \
  --wasm "${WASM_PATH}" \
  --source "${SOURCE_SECRET_KEY}" \
  --network "${NETWORK}")

info "Deployed contract ID: ${CONTRACT_ID}"

# ---------------------------------------------------------------------------
# 3. Initialize the contract
#
#    initialize(admin, fee_recipient, bond_token) sets the three privileged
#    addresses and must be called exactly once after deployment.
# ---------------------------------------------------------------------------
info "Initializing contract ..."
stellar contract invoke \
  --id "${CONTRACT_ID}" \
  --source "${SOURCE_SECRET_KEY}" \
  --network "${NETWORK}" \
  -- \
  initialize \
  --admin "${ADMIN_ADDRESS}" \
  --fee_recipient "${FEE_RECIPIENT_ADDRESS}" \
  --bond_token "${BOND_TOKEN_ADDRESS}"

info "Contract initialized successfully."

# ---------------------------------------------------------------------------
# 4. Persist the contract ID for future invocations
# ---------------------------------------------------------------------------
LAST_DEPLOY_FILE="${SCRIPT_DIR}/.last-deploy-testnet"
echo "CONTRACT_ID=${CONTRACT_ID}" > "${LAST_DEPLOY_FILE}"
info "Contract ID saved to ${LAST_DEPLOY_FILE}"

# ---------------------------------------------------------------------------
# 5. Summary
# ---------------------------------------------------------------------------
cat <<EOF

──────────────────────────────────────────────
  Deployment complete
  Network:      ${NETWORK}
  Contract ID:  ${CONTRACT_ID}
──────────────────────────────────────────────

Next steps:
  # Register a solver
  stellar contract invoke --id ${CONTRACT_ID} --source <SOLVER_KEY> --network ${NETWORK} -- \\
    register_solver --solver <SOLVER_ADDRESS> --bond_amount <AMOUNT>

  # Verify the deployment
  stellar contract invoke --id ${CONTRACT_ID} --source <ANY_KEY> --network ${NETWORK} -- \\
    get_stats

  # Verify the wasm hash
  ./verify-build.sh ${CONTRACT_ID}
EOF
