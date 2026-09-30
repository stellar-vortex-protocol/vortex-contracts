#!/bin/bash
#
# Issue #290: Check that examples/view-calls.postman_collection.json is in sync
# with the actual read-only view functions in intent_settlement/src/lib.rs.
#
# This script:
# 1. Extracts all pub fn get_*/is_*/list_* functions from intent_settlement/src/lib.rs
# 2. Verifies each one has a corresponding request in the Postman collection
# 3. Fails (exit 1) if drift is detected, succeeds (exit 0) if in sync
#
# Heuristic: a function is considered a "view" if it:
#   - Starts with get_, is_, or list_ (read-only naming convention)
#   - Is declared as "pub fn"
# This is conservative and may need refinement as the contract evolves.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

CONTRACT_FILE="$REPO_ROOT/intent_settlement/src/lib.rs"
COLLECTION_FILE="$REPO_ROOT/examples/view-calls.postman_collection.json"

if [ ! -f "$CONTRACT_FILE" ]; then
  echo "Error: $CONTRACT_FILE not found"
  exit 1
fi

if [ ! -f "$COLLECTION_FILE" ]; then
  echo "Error: $COLLECTION_FILE not found"
  exit 1
fi

# Extract all view function names from the contract (get_*, is_*, list_*)
echo "Extracting view functions from $CONTRACT_FILE..."
EXPECTED_VIEWS=$(grep "pub fn" "$CONTRACT_FILE" | \
  sed 's/.*pub fn //' | \
  sed 's/(.*$//' | \
  grep -E "^(get_|is_|list_)" | \
  sort -u)

# Extract all request names from the Postman collection
echo "Extracting requests from $COLLECTION_FILE..."
COLLECTION_VIEWS=$(jq -r '.item[].name' "$COLLECTION_FILE" | sort -u)

# Compare
echo ""
echo "=== View Functions in Contract ==="
echo "$EXPECTED_VIEWS"
echo ""
echo "=== Requests in Postman Collection ==="
echo "$COLLECTION_VIEWS"
echo ""

# Find functions missing from collection
MISSING=$(comm -23 <(echo "$EXPECTED_VIEWS") <(echo "$COLLECTION_VIEWS"))

if [ -n "$MISSING" ]; then
  echo "❌ DRIFT DETECTED: The following view functions are missing from the collection:"
  echo "$MISSING" | sed 's/^/  - /'
  echo ""
  exit 1
else
  echo "✓ Collection is in sync with contract view functions"
  exit 0
fi
