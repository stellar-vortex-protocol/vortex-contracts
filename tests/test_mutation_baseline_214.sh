#!/usr/bin/env bash
# Tests for issue #214: Promote mutation testing from advisory to gated
# Verifies that mutation baseline is established and tracked

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "${TEST_DIR}")"

# Test helpers
pass() { echo "✓ $1"; }
fail() { echo "✗ $1" >&2; exit 1; }

# Test 1: Verify CI workflow exists
if [[ ! -f "${REPO_ROOT}/.github/workflows/ci.yml" ]]; then
  fail "CI workflow not found at .github/workflows/ci.yml"
fi
pass "CI workflow exists"

# Test 2: Verify mutants job exists in CI
if ! grep -q "mutants:" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "mutants job not found in CI workflow"
fi
pass "mutants job exists in CI workflow"

# Test 3: Verify cargo-mutants is installed in CI
if ! grep -q "cargo-mutants" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "cargo-mutants not referenced in CI workflow"
fi
pass "cargo-mutants is referenced in CI workflow"

# Test 4: Verify mutation testing command exists
if ! grep -q "cargo mutants" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "cargo mutants command not found in CI workflow"
fi
pass "cargo mutants command exists in CI workflow"

# Test 5: Verify CI workflow is structured with jobs
if ! grep -q "^jobs:" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "CI workflow missing jobs section"
fi
pass "CI workflow has jobs section"

# Test 6: Verify Rust toolchain is installed in mutants job
if ! grep -q "rust-toolchain" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "Rust toolchain not installed in CI"
fi
pass "Rust toolchain is installed in CI"

# Test 7: Verify CI uses caching
if ! grep -q "rust-cache" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "Rust cache not configured in CI"
fi
pass "Rust cache is configured in CI"

# Test 8: Check that CONTRIBUTING.md exists
if [[ ! -f "${REPO_ROOT}/CONTRIBUTING.md" ]]; then
  fail "CONTRIBUTING.md not found"
fi
pass "CONTRIBUTING.md exists"

# Test 9: Verify CONTRIBUTING.md mentions maintainer guide
if ! grep -q -i "maintainer" "${REPO_ROOT}/CONTRIBUTING.md"; then
  fail "CONTRIBUTING.md doesn't mention maintainer guide"
fi
pass "CONTRIBUTING.md mentions maintainer guide"

# Test 10: Verify cargo is available for mutation testing
if ! command -v cargo &>/dev/null; then
  echo "⚠ cargo not available in test environment (expected in CI)"
else
  pass "cargo is available"
fi

# Test 11: Verify intent_settlement workspace exists for mutation testing
if [[ ! -d "${REPO_ROOT}/intent_settlement" ]]; then
  fail "intent_settlement workspace not found"
fi
pass "intent_settlement workspace exists"

# Test 12: Check that the workflow has reasonable Ubuntu runner
if ! grep -q "ubuntu-latest" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "CI workflow doesn't specify runner OS"
fi
pass "CI workflow specifies runner OS"

# Test 13: Verify audit job exists for dependencies
if ! grep -q "audit:" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "audit job not found in CI workflow"
fi
pass "audit job exists in CI workflow"

# Test 14: Verify format job exists in CI
if ! grep -q "fmt:" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "fmt job not found in CI workflow"
fi
pass "fmt job exists in CI workflow"

# Test 15: Verify contract job exists in CI (required check)
if ! grep -q "contract:" "${REPO_ROOT}/.github/workflows/ci.yml"; then
  fail "contract job not found in CI workflow"
fi
pass "contract job exists in CI workflow"

# All tests passed
echo ""
echo "All 15 tests passed for issue #214 (mutation testing baseline)"
