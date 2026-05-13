#!/bin/bash
# workflow-position oracle self-test (Unix)
set -e
ORACLE_DIR="$(dirname "$0")"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if ! echo "$OUTPUT" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
    if ! echo "$OUTPUT" | python -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        exit 1
    fi
fi

for field in position latest_intake latest_plan spec_exists ltads_active ltads_session_status suggested_next_command proceed_hint; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

# position must be one of the declared enums
pos=$(echo "$OUTPUT" | grep -oE '"position"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
case "$pos" in
    NONE|POST-IDEATE|POST-INTAKE|POST-SPEC|POST-PLAN|IN-EXECUTION|POST-IMPLEMENTATION|UNKNOWN) ;;
    *) echo "FAIL: position '$pos' is not in the declared enum" >&2; exit 1 ;;
esac

echo "PASS: workflow-position oracle validates (position=$pos)"
exit 0
