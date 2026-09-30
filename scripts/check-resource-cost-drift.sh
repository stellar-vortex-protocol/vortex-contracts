#!/bin/bash
#
# Check for resource cost drift in intent_settlement contract.
#
# This script regenerates resource cost measurements and compares them against
# the baseline published in docs/149-resource-cost-per-entrypoint.md. Any
# entrypoint whose cost has drifted beyond a documented tolerance (±10%)
# fails the check.
#
# Usage:
#   ./scripts/check-resource-cost-drift.sh
#
# Exit codes:
#   0 — All costs within tolerance
#   1 — Drift detected or measurement failed
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
INTENT_SETTLEMENT="$REPO_ROOT/intent_settlement"
DOC_FILE="$REPO_ROOT/docs/149-resource-cost-per-entrypoint.md"

# Tolerance: ±10% (as documented in issue #285)
TOLERANCE_PERCENT=10

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=== Resource Cost Drift Detection ==="
echo "Repository: $REPO_ROOT"
echo "Tolerance: ±${TOLERANCE_PERCENT}%"
echo ""

# Step 1: Parse the baseline from the doc
echo "Parsing baseline from: $DOC_FILE"

# Extract the per-entrypoint cost table (section 3)
# The table format is:
# | Entrypoint | CPU instructions | Memory bytes |
# |---|--:|--:|
# | `submit_intent` | 281,113 | 39,630 |
# ...

# Use awk to extract table rows (skip header and separator lines)
BASELINE=$(mktemp)
trap "rm -f '$BASELINE'" EXIT

grep -A 100 "^## 3\. Per-entrypoint cost" "$DOC_FILE" | \
  grep "^| \`" | \
  awk -F'|' '{
    # Extract: | `entrypoint` | cpu_instructions | memory_bytes |
    gsub(/^[[:space:]]*/, "", $2); gsub(/[[:space:]]*$/, "", $2);  # trim
    gsub(/^[[:space:]]*/, "", $3); gsub(/[[:space:]]*$/, "", $3);
    gsub(/^[[:space:]]*/, "", $4); gsub(/[[:space:]]*$/, "", $4);

    # Remove backticks from entrypoint name
    gsub(/`/, "", $2);

    # Remove commas from numbers (parse as integers)
    gsub(/,/, "", $3); gsub(/,/, "", $4);

    if ($2 != "" && $3 != "" && $4 != "") {
      printf "%s\t%s\t%s\n", $2, $3, $4
    }
  }' > "$BASELINE"

if [ ! -s "$BASELINE" ]; then
  echo -e "${RED}ERROR: Could not parse baseline table from $DOC_FILE${NC}"
  echo "Expected format: | \`entrypoint_name\` | cpu_instructions | memory_bytes |"
  exit 1
fi

echo -e "${GREEN}✓ Parsed baseline for $(wc -l < "$BASELINE") entrypoints${NC}"
echo ""

# Step 2: Regenerate measurements from the benchmark harness
echo "Regenerating measurements using: cargo test --features testutils bench::resource_cost_report"
echo ""

cd "$INTENT_SETTLEMENT"

# Capture benchmark output
BENCH_OUTPUT=$(mktemp)
trap "rm -f '$BASELINE' '$BENCH_OUTPUT'" EXIT

if ! cargo test --features testutils bench::resource_cost_report -- --nocapture > "$BENCH_OUTPUT" 2>&1; then
  echo -e "${RED}ERROR: Benchmark harness failed${NC}"
  cat "$BENCH_OUTPUT"
  exit 1
fi

echo -e "${GREEN}✓ Benchmark completed successfully${NC}"
echo ""

# Step 3: Extract measurements from benchmark output
# Expected format from bench::resource_cost_report:
# Resource cost for submit_intent: 281113 CPU instructions, 39630 memory bytes
MEASUREMENTS=$(mktemp)
trap "rm -f '$BASELINE' '$MEASUREMENTS' '$BENCH_OUTPUT'" EXIT

# Parse the output for lines like:
# "Resource cost for {name}: {cpu} CPU instructions, {mem} memory bytes"
grep "Resource cost for" "$BENCH_OUTPUT" | \
  sed 's/.*Resource cost for \([^:]*\): \([0-9]*\) CPU instructions, \([0-9]*\) memory bytes.*/\1\t\2\t\3/' > "$MEASUREMENTS"

if [ ! -s "$MEASUREMENTS" ]; then
  echo -e "${RED}ERROR: Could not extract measurements from benchmark output${NC}"
  echo "Benchmark output:"
  cat "$BENCH_OUTPUT"
  exit 1
fi

echo -e "${GREEN}✓ Extracted measurements for $(wc -l < "$MEASUREMENTS") entrypoints${NC}"
echo ""

# Step 4: Compare baseline vs. measurements using pure awk (no bc dependency)
echo "Comparing measurements against baseline..."
echo ""

# Create a comparison report with awk
COMPARISON=$(mktemp)
trap "rm -f '$BASELINE' '$MEASUREMENTS' '$BENCH_OUTPUT' '$COMPARISON'" EXIT

join -t $'\t' <(sort "$BASELINE") <(sort "$MEASUREMENTS") | \
awk -v tol="$TOLERANCE_PERCENT" '
  {
    name=$1; baseline_cpu=$2; baseline_mem=$3; measured_cpu=$4; measured_mem=$5;

    # Calculate drift percentage for CPU
    if (baseline_cpu > 0) {
      cpu_drift = (measured_cpu - baseline_cpu) * 100 / baseline_cpu;
      cpu_drift_abs = (cpu_drift < 0) ? -cpu_drift : cpu_drift;
    } else {
      cpu_drift = 0; cpu_drift_abs = 0;
    }

    # Calculate drift percentage for Memory
    if (baseline_mem > 0) {
      mem_drift = (measured_mem - baseline_mem) * 100 / baseline_mem;
      mem_drift_abs = (mem_drift < 0) ? -mem_drift : mem_drift;
    } else {
      mem_drift = 0; mem_drift_abs = 0;
    }

    # Store results for later processing
    printf "%s\t%s\t%s\t%s\t%s\t%.1f\t%.1f\t%s\t%s\n",
      name, baseline_cpu, baseline_mem, measured_cpu, measured_mem,
      cpu_drift, cpu_drift_abs, mem_drift, mem_drift_abs > "'$COMPARISON'"
  }
'

# Process comparison results
DRIFT_DETECTED=0

while IFS=$'\t' read -r name baseline_cpu baseline_mem measured_cpu measured_mem cpu_drift cpu_drift_abs mem_drift mem_drift_abs; do
  cpu_exceeds=0
  mem_exceeds=0

  # Compare using awk since bash can't do floating point
  cpu_exceeds=$(awk -v drift="$cpu_drift_abs" -v tol="$TOLERANCE_PERCENT" 'BEGIN { if (drift > tol) print 1; else print 0 }')
  mem_exceeds=$(awk -v drift="$mem_drift_abs" -v tol="$TOLERANCE_PERCENT" 'BEGIN { if (drift > tol) print 1; else print 0 }')

  if [ "$cpu_exceeds" -eq 1 ] || [ "$mem_exceeds" -eq 1 ]; then
    DRIFT_DETECTED=1
    echo -e "${YELLOW}⚠ Drift detected: ${NC}${name}"
    if [ "$cpu_exceeds" -eq 1 ]; then
      echo -e "  CPU: ${baseline_cpu} → ${measured_cpu} (${cpu_drift}%, tolerance: ±${TOLERANCE_PERCENT}%)"
    fi
    if [ "$mem_exceeds" -eq 1 ]; then
      echo -e "  Memory: ${baseline_mem} → ${measured_mem} (${mem_drift}%, tolerance: ±${TOLERANCE_PERCENT}%)"
    fi
  else
    # Drift is within tolerance (only log if there's any change)
    if [ "$baseline_cpu" != "$measured_cpu" ] || [ "$baseline_mem" != "$measured_mem" ]; then
      echo -e "${GREEN}✓ ${name}${NC} (within tolerance)"
    fi
  fi
done < "$COMPARISON"

# Count diffs for summary
DIFFS=$(awk -v tol="$TOLERANCE_PERCENT" '
  {
    cpu_drift_abs=$7;
    mem_drift_abs=$9;
    if (cpu_drift_abs > tol || mem_drift_abs > tol) count++;
  }
  END { print count }
' "$COMPARISON")

TOTAL=$(wc -l < "$BASELINE")

echo ""
echo "=== Summary ==="
echo "Entrypoints measured: $TOTAL"
echo "Entrypoints within tolerance: $((TOTAL - DIFFS))"
echo "Entrypoints exceeding tolerance: $DIFFS"
echo ""

if [ "$DIFFS" -gt 0 ]; then
  echo -e "${RED}❌ DRIFT DETECTED: $DIFFS entrypoint(s) exceed ±${TOLERANCE_PERCENT}% tolerance${NC}"
  echo ""
  echo "To update the baseline after confirming these changes are expected:"
  echo "  1. Review the changes to lib.rs (run 'git diff')"
  echo "  2. Re-run the benchmark: cargo test --features testutils bench::resource_cost_report -- --nocapture"
  echo "  3. Update docs/149-resource-cost-per-entrypoint.md with the new measurements"
  echo "  4. Commit and include in your PR"
  echo ""
  exit 1
else
  echo -e "${GREEN}✓ All entrypoints within tolerance${NC}"
  exit 0
fi
