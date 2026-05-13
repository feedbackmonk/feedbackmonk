#!/bin/bash
# project-type oracle self-test (Unix)
# Verifies the oracle runs successfully and produces valid JSON matching the schema.
# Exit 0 on pass, non-zero on fail.

set -e

ORACLE_DIR="$(dirname "$0")"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || {
    echo "FAIL: run.sh exited non-zero" >&2
    echo "$OUTPUT" >&2
    exit 1
}

# Must be valid JSON
if ! echo "$OUTPUT" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
    if ! echo "$OUTPUT" | python -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        echo "$OUTPUT" >&2
        exit 1
    fi
fi

# Must have all required schema fields
for field in languages frameworks build_systems test_command dev_command package_managers; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        echo "$OUTPUT" >&2
        exit 1
    fi
done

echo "PASS: project-type oracle validates"
exit 0
