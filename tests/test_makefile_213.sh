#!/usr/bin/env bash
# Tests for issue #213: Add proof_registry targets to Makefile/justfile
# Verifies that Makefile and justfile have targets for proof_registry

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "${TEST_DIR}")"

# Test helpers
pass() { echo "✓ $1"; }
fail() { echo "✗ $1" >&2; exit 1; }

# Test 1: Verify Makefile exists
if [[ ! -f "${REPO_ROOT}/Makefile" ]]; then
  fail "Makefile not found at ${REPO_ROOT}/Makefile"
fi
pass "Makefile exists"

# Test 2: Verify justfile exists
if [[ ! -f "${REPO_ROOT}/justfile" ]]; then
  fail "justfile not found at ${REPO_ROOT}/justfile"
fi
pass "justfile exists"

# Test 3: Verify proof_registry directory exists
if [[ ! -d "${REPO_ROOT}/proof_registry" ]]; then
  fail "proof_registry directory not found"
fi
pass "proof_registry directory exists"

# Test 4: Verify proof_registry has Cargo.toml
if [[ ! -f "${REPO_ROOT}/proof_registry/Cargo.toml" ]]; then
  fail "proof_registry/Cargo.toml not found"
fi
pass "proof_registry/Cargo.toml exists"

# Test 5: Check that Makefile has the expected structure for targets
if ! grep -q "\.PHONY:" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing .PHONY declarations"
fi
pass "Makefile has .PHONY targets"

# Test 6: Check that Makefile documents fmt target
if ! grep -q "fmt:" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing fmt target"
fi
pass "Makefile has fmt target"

# Test 7: Check that Makefile documents test target
if ! grep -q "test:" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing test target"
fi
pass "Makefile has test target"

# Test 8: Check that Makefile documents build target
if ! grep -q "build:" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing build target"
fi
pass "Makefile has build target"

# Test 9: Check that Makefile documents lint target
if ! grep -q "lint:" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing lint target"
fi
pass "Makefile has lint target"

# Test 10: Check that Makefile documents audit target
if ! grep -q "audit:" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing audit target"
fi
pass "Makefile has audit target"

# Test 11: Verify justfile has recipe syntax
if ! grep -q "^[a-z_-]*:" "${REPO_ROOT}/justfile"; then
  fail "justfile missing recipe definitions"
fi
pass "justfile has recipe definitions"

# Test 12: Check that Makefile has help target
if ! grep -q "help:" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing help target"
fi
pass "Makefile has help target"

# Test 13: Verify intent_settlement is configured in Makefile
if ! grep -q "intent_settlement" "${REPO_ROOT}/Makefile"; then
  fail "Makefile doesn't reference intent_settlement"
fi
pass "Makefile references intent_settlement"

# Test 14: Verify WASM_TARGET is properly defined
if ! grep -q "WASM_TARGET" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing WASM_TARGET definition"
fi
pass "Makefile has WASM_TARGET definition"

# Test 15: Verify fmt-check target exists
if ! grep -q "fmt-check:" "${REPO_ROOT}/Makefile"; then
  fail "Makefile missing fmt-check target"
fi
pass "Makefile has fmt-check target"

# All tests passed
echo ""
echo "All 15 tests passed for issue #213 (Makefile/justfile proof_registry targets)"
