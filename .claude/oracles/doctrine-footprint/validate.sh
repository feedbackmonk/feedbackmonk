#!/bin/bash
# doctrine-footprint oracle -- self-test (Unix). DAUD-01.
#
# Runs the oracle inside each test-fixtures/<name>/ and exact-matches stdout
# against that fixture's expected-output.json (golden file). Exit 0 iff all match.
# Also runs each fixture twice to assert determinism.

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
        echo "validate.sh: fixture '$name' missing expected-output.json -- skipping" >&2
        continue
    fi
    # Pin the global-claude fallback inside the fixture so the JIG-05 wire check
    # never reads the real ~/.claude (hermetic golden files).
    actual=$(cd "$fixture" && DOCTRINE_FOOTPRINT_GLOBAL_CLAUDE="$fixture/.claude" bash "$ORACLE_RUN" 2>&1)
    actual2=$(cd "$fixture" && DOCTRINE_FOOTPRINT_GLOBAL_CLAUDE="$fixture/.claude" bash "$ORACLE_RUN" 2>&1)
    expected_content=$(cat "$expected")
    actual_trim=$(printf '%s' "$actual" | tr -d '\r\n')
    actual2_trim=$(printf '%s' "$actual2" | tr -d '\r\n')
    expected_trim=$(printf '%s' "$expected_content" | tr -d '\r\n')

    if [ "$actual_trim" != "$actual2_trim" ]; then
        FAIL=$((FAIL + 1)); FAILED_NAMES+=("$name(non-deterministic)")
        echo "FAIL: $name -- non-deterministic across two runs"
        continue
    fi
    if [ "$actual_trim" = "$expected_trim" ]; then
        PASS=$((PASS + 1)); echo "PASS: $name"
    else
        FAIL=$((FAIL + 1)); FAILED_NAMES+=("$name")
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
