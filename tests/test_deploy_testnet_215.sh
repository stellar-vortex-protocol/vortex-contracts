#!/usr/bin/env bash
# Tests for issue #215: Harden deploy-testnet.sh with preflight validation
# Verifies that deploy-testnet.sh has proper validation and safety checks

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "${TEST_DIR}")"

# Test helpers
pass() { echo "✓ $1"; }
fail() { echo "✗ $1" >&2; exit 1; }

# Test 1: Verify deploy-testnet.sh exists
if [[ ! -f "${REPO_ROOT}/deploy-testnet.sh" ]]; then
  fail "deploy-testnet.sh not found at ${REPO_ROOT}/deploy-testnet.sh"
fi
pass "deploy-testnet.sh exists"

# Test 2: Verify the script is executable
if [[ ! -x "${REPO_ROOT}/deploy-testnet.sh" ]]; then
  fail "deploy-testnet.sh is not executable"
fi
pass "deploy-testnet.sh is executable"

# Test 3: Verify usage documentation exists
if ! grep -q "Usage:" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh missing Usage section"
fi
pass "deploy-testnet.sh has usage documentation"

# Test 4: Verify deploy-testnet.env.example exists
if [[ ! -f "${REPO_ROOT}/deploy-testnet.env.example" ]]; then
  fail "deploy-testnet.env.example not found"
fi
pass "deploy-testnet.env.example exists"

# Test 5: Verify require_var helper exists
if ! grep -q "require_var()" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh missing require_var() helper"
fi
pass "deploy-testnet.sh has require_var() helper"

# Test 6: Verify info/die helpers exist
if ! grep -q "info()" "${REPO_ROOT}/deploy-testnet.sh" || ! grep -q "die()" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh missing info() or die() helpers"
fi
pass "deploy-testnet.sh has info/die helpers"

# Test 7: Verify BOND_TOKEN_ADDRESS is required
if ! grep -q "require_var BOND_TOKEN_ADDRESS" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't require BOND_TOKEN_ADDRESS"
fi
pass "deploy-testnet.sh requires BOND_TOKEN_ADDRESS"

# Test 8: Verify ADMIN_ADDRESS is required
if ! grep -q "require_var ADMIN_ADDRESS" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't require ADMIN_ADDRESS"
fi
pass "deploy-testnet.sh requires ADMIN_ADDRESS"

# Test 9: Verify FEE_RECIPIENT_ADDRESS is required
if ! grep -q "require_var FEE_RECIPIENT_ADDRESS" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't require FEE_RECIPIENT_ADDRESS"
fi
pass "deploy-testnet.sh requires FEE_RECIPIENT_ADDRESS"

# Test 10: Verify NETWORK is required
if ! grep -q "require_var NETWORK" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't require NETWORK"
fi
pass "deploy-testnet.sh requires NETWORK"

# Test 11: Verify SOURCE_SECRET_KEY is required
if ! grep -q "require_var SOURCE_SECRET_KEY" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't require SOURCE_SECRET_KEY"
fi
pass "deploy-testnet.sh requires SOURCE_SECRET_KEY"

# Test 12: Verify env file loading exists
if ! grep -q "source.*ENV_FILE" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't load environment file"
fi
pass "deploy-testnet.sh loads environment file"

# Test 13: Verify stellar contract deploy is called
if ! grep -q "stellar contract deploy" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh missing stellar contract deploy call"
fi
pass "deploy-testnet.sh calls stellar contract deploy"

# Test 14: Verify stellar contract invoke for initialize exists
if ! grep -q "initialize" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh missing initialize invocation"
fi
pass "deploy-testnet.sh calls initialize"

# Test 15: Verify WASM_PATH is checked before deployment
if ! grep -q "WASM_PATH" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't check WASM_PATH"
fi
pass "deploy-testnet.sh checks WASM_PATH"

# Test 16: Verify .last-deploy-testnet file is persisted
if ! grep -q ".last-deploy-testnet" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't persist deployment info"
fi
pass "deploy-testnet.sh persists deployment info"

# Test 17: Verify script uses set -euo pipefail for safety
if ! grep -q "set -euo pipefail" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't use set -euo pipefail"
fi
pass "deploy-testnet.sh uses strict error handling"

# Test 18: Verify --skip-build option exists
if ! grep -q "\-\-skip-build" "${REPO_ROOT}/deploy-testnet.sh"; then
  fail "deploy-testnet.sh doesn't support --skip-build"
fi
pass "deploy-testnet.sh supports --skip-build"

# All tests passed
echo ""
echo "All 18 tests passed for issue #215 (deploy-testnet.sh hardening)"
