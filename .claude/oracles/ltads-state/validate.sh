#!/bin/bash
# ltads-state oracle self-test (Unix)
set -e
ORACLE_DIR="$(dirname "$0")"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if ! echo "$OUTPUT" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
    if ! echo "$OUTPUT" | python -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        exit 1
    fi
fi

for field in state has_ltads_dir is_tracked config_exists is_temporary session_id session_status summary; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

echo "PASS: ltads-state oracle validates"
exit 0
