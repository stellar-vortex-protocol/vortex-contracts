#!/bin/bash
#
# Issue #290 / #495: Check that examples/view-calls.postman_collection.json is in
# sync with the actual read-only view functions across all six contracts.
#
# This script:
# 1. Extracts all pub fn get_*/is_*/list_* functions from each contract's lib.rs
# 2. Verifies each one has a corresponding request in the Postman collection
# 3. Fails (exit 1) if drift is detected, succeeds (exit 0) if in sync
#
# Heuristic: a function is considered a "view" if it:
#   - Starts with get_, is_, or list_ (read-only naming convention)
#   - Is declared as "pub fn"
# This is conservative and may need refinement as the contract evolves.
#
# Collection request names are "<contract>_<function>" (e.g.
# "governor_get_config") so entries from different contracts stay distinct.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

COLLECTION_FILE="$REPO_ROOT/examples/view-calls.postman_collection.json"

# Contracts to check, in the same order as the generator script.
CONTRACTS=(
  "intent_settlement"
  "governor"
  "backstop_vault"
  "fee_router"
  "treasury_streams"
  "intent_periphery"
)

if [ ! -f "$COLLECTION_FILE" ]; then
  echo "Error: $COLLECTION_FILE not found"
  exit 1
fi

# Extract all view function names from a contract (get_*, is_*, list_*),
# prefixed as "<contract>_<function>" to match collection request names.
extract_views() {
  local contract="$1"
  local contract_file="$REPO_ROOT/$contract/src/lib.rs"

  if [ ! -f "$contract_file" ]; then
    echo "Error: $contract_file not found" >&2
    return 1
  fi

  grep "pub fn" "$contract_file" | \
    sed 's/.*pub fn //' | \
    sed 's/(.*$//' | \
    grep -E "^(get_|is_|list_)" | \
    sort -u | \
    sed "s/^/${contract}_/"
}

# Collect expected view names across all contracts.
EXPECTED_VIEWS=""
for contract in "${CONTRACTS[@]}"; do
  echo "Extracting view functions from $contract/src/lib.rs..."
  views=$(extract_views "$contract")
  if [ -n "$views" ]; then
    if [ -n "$EXPECTED_VIEWS" ]; then
      EXPECTED_VIEWS="$EXPECTED_VIEWS
$views"
    else
      EXPECTED_VIEWS="$views"
    fi
  fi
done
EXPECTED_VIEWS=$(echo "$EXPECTED_VIEWS" | sort -u)

# Extract all request names from the Postman collection
echo "Extracting requests from $COLLECTION_FILE..."
COLLECTION_VIEWS=$(jq -r '.item[].name' "$COLLECTION_FILE" | sort -u)

# Compare
echo ""
echo "=== View Functions in Contracts ==="
echo "$EXPECTED_VIEWS"
echo ""
echo "=== Requests in Postman Collection ==="
echo "$COLLECTION_VIEWS"
echo ""

# Find functions missing from collection
MISSING=$(comm -23 <(echo "$EXPECTED_VIEWS") <(echo "$COLLECTION_VIEWS"))

if [ -n "$MISSING" ]; then
  echo "❌ DRIFT DETECTED: The following view functions are missing from $COLLECTION_FILE:"
  for contract in "${CONTRACTS[@]}"; do
    contract_missing=$(echo "$MISSING" | grep "^${contract}_" || true)
    if [ -n "$contract_missing" ]; then
      echo "  $contract ($contract/src/lib.rs):"
      echo "$contract_missing" | sed "s/^${contract}_/    - /"
    fi
  done
  echo ""
  echo "Fix: add a request named <contract>_<function> for each entry above to"
  echo "     $COLLECTION_FILE, or run: python3 scripts/generate-view-calls-collection.py"
  echo "     Then re-run this script."
  exit 1
else
  echo "✓ Collection is in sync with contract view functions"
  exit 0
fi
