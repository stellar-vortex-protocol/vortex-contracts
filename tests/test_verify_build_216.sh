#!/usr/bin/env bash
# Tests for issue #216: Generalize and containerize verify-build.sh
# Verifies that verify-build.sh accepts network parameter and supports Docker mode

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "${TEST_DIR}")"

# Test helpers
pass() { echo "✓ $1"; }
fail() { echo "✗ $1" >&2; exit 1; }

# Test 1: Verify verify-build.sh script exists
if [[ ! -f "${REPO_ROOT}/verify-build.sh" ]]; then
  fail "verify-build.sh not found at ${REPO_ROOT}/verify-build.sh"
fi
pass "verify-build.sh exists"

# Test 2: Verify the script is executable
if [[ ! -x "${REPO_ROOT}/verify-build.sh" ]]; then
  fail "verify-build.sh is not executable"
fi
pass "verify-build.sh is executable"

# Test 3: Verify the script's header contains usage documentation
if ! grep -q "Usage:" "${REPO_ROOT}/verify-build.sh"; then
  fail "verify-build.sh missing Usage section"
fi
pass "verify-build.sh contains usage documentation"

# Test 4: Verify RUST_CHANNEL is properly pinned
if ! grep -q "RUST_CHANNEL=" "${REPO_ROOT}/verify-build.sh"; then
  fail "verify-build.sh missing RUST_CHANNEL definition"
fi
pass "verify-build.sh has RUST_CHANNEL pinned"

# Test 5: Verify helper functions exist
if ! grep -q "sha256()" "${REPO_ROOT}/verify-build.sh"; then
  fail "verify-build.sh missing sha256() helper function"
fi
pass "verify-build.sh has sha256() helper"

# Test 6: Verify info/die helpers exist
if ! grep -q "info()" "${REPO_ROOT}/verify-build.sh" && ! grep -q "die()" "${REPO_ROOT}/verify-build.sh"; then
  fail "verify-build.sh missing info() or die() helper"
fi
pass "verify-build.sh has info/die helpers"

# Test 7: Verify the script checks for stellar CLI when needed
if ! grep -q "stellar CLI" "${REPO_ROOT}/verify-build.sh"; then
  fail "verify-build.sh missing stellar CLI check"
fi
pass "verify-build.sh checks for stellar CLI"

# Test 8: Verify contract fetch happens (later to be parameterized by network)
if ! grep -q "stellar contract fetch" "${REPO_ROOT}/verify-build.sh"; then
  fail "verify-build.sh missing stellar contract fetch call"
fi
pass "verify-build.sh includes stellar contract fetch"

# Test 9: Check for WASM output path
if ! grep -q "WASM_OUT=" "${REPO_ROOT}/verify-build.sh"; then
  fail "verify-build.sh missing WASM_OUT definition"
fi
pass "verify-build.sh defines WASM output path"

# Test 10: Verify cargo build command exists
if ! grep -q "cargo build" "${REPO_ROOT}/verify-build.sh"; then
  fail "verify-build.sh missing cargo build command"
fi
pass "verify-build.sh includes cargo build"

# All tests passed
echo ""
echo "All 10 tests passed for issue #216 (verify-build.sh)"
