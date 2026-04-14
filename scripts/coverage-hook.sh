#!/usr/bin/env bash
set -euo pipefail

COVERAGE_OUT=$(mktemp "${TMPDIR:-/tmp}/sw-coverage-prepush-XXXXXX")
trap 'rm -f "$COVERAGE_OUT"' EXIT
BASELINE=coverage-baseline.json

echo "Running coverage check (this takes a moment)..."

# Measure coverage.
cargo llvm-cov --workspace --json --output-path "$COVERAGE_OUT"

# Check: fails if any crate is below baseline or below 80% floor (once >= 80%).
# Use 'if !' so set -e does not abort before we can print a helpful message.
if ! python3 scripts/check-coverage.py "$COVERAGE_OUT" "$BASELINE"; then
    echo ""
    echo "Push blocked: coverage check failed (see above)."
    echo "Write tests to raise coverage, then push again."
    exit 1
fi

# Update: raises baseline entries if coverage improved.
# Exit 1 means entries were raised (needs a commit); exit 2 means error.
update_exit=0
python3 scripts/update-coverage-baseline.py "$COVERAGE_OUT" "$BASELINE" || update_exit=$?

if [ "$update_exit" -eq 1 ]; then
    echo ""
    echo "Push blocked: coverage improved and coverage-baseline.json was updated."
    echo "Review with: git diff coverage-baseline.json"
    echo "Then commit the updated baseline and push again."
    exit 1
fi

if [ "$update_exit" -eq 2 ]; then
    echo ""
    echo "Push blocked: error updating coverage baseline (see above)."
    exit 1
fi

echo "Coverage OK — push proceeding."
