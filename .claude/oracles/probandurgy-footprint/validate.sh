#!/bin/bash
# probandurgy-footprint oracle — self-test (Unix)
#
# Iterates over test-fixtures/<name>/, runs the oracle inside each fixture, and
# compares stdout against the fixture's expected-output.json. Exit 0 iff all
# fixtures match. Emits per-fixture diffs on failure.
#
# Used standalone and by W5's IC-5 dogfood smoke harness.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ORACLE_RUN="$SCRIPT_DIR/run.sh"
FIXTURES_DIR="$SCRIPT_DIR/test-fixtures"

if [ ! -d "$FIXTURES_DIR" ]; then
    echo "validate.sh: no test-fixtures directory at $FIXTURES_DIR" >&2
    exit 2
fi

PASS=0
FAIL=0
FAILED_NAMES=()

for fixture in "$FIXTURES_DIR"/*/; do
    name=$(basename "$fixture")
    expected="$fixture/expected-output.json"
    if [ ! -f "$expected" ]; then
        echo "validate.sh: fixture '$name' missing expected-output.json — skipping" >&2
        continue
    fi
    actual=$(cd "$fixture" && bash "$ORACLE_RUN" 2>&1)
    expected_content=$(cat "$expected")
    # Normalize trailing newlines/whitespace for the comparison.
    actual_trim=$(printf '%s' "$actual" | tr -d '\r\n')
    expected_trim=$(printf '%s' "$expected_content" | tr -d '\r\n')
    if [ "$actual_trim" = "$expected_trim" ]; then
        PASS=$((PASS + 1))
        echo "PASS: $name"
    else
        FAIL=$((FAIL + 1))
        FAILED_NAMES+=("$name")
        echo "FAIL: $name"
        echo "  expected: $expected_trim"
        echo "  actual:   $actual_trim"
    fi
done

echo "---"
echo "validate.sh: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    echo "Failed fixtures: ${FAILED_NAMES[*]}"
    exit 1
fi
exit 0
