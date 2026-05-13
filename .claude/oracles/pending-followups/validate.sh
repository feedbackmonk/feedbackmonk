#!/bin/bash
# pending-followups oracle self-test (Unix)
set -e
ORACLE_DIR="$(dirname "$0")"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if ! echo "$OUTPUT" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
    if ! echo "$OUTPUT" | python -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        exit 1
    fi
fi

for field in has_followups_section total overdue items; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

echo "PASS: pending-followups oracle validates"
exit 0
