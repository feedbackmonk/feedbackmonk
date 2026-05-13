#!/bin/bash
# ui-surface-detector oracle self-test (Unix)
set -e
ORACLE_DIR="$(dirname "$0")"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

# Output must be valid JSON
if ! echo "$OUTPUT" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
    if ! echo "$OUTPUT" | python -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        echo "Output: $OUTPUT" >&2
        exit 1
    fi
fi

# Required schema fields
for field in surface_kind confidence evidence; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

# surface_kind must be one of the allowed values
KIND=$(echo "$OUTPUT" | grep -oE '"surface_kind"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
case "$KIND" in
    tauri-desktop|electron-desktop|web-spa|react-native|flutter|mobile-native|backend-service|cli-tool|none) ;;
    *) echo "FAIL: surface_kind='$KIND' is not a valid value" >&2; exit 1 ;;
esac

# confidence must be one of the allowed values
CONF=$(echo "$OUTPUT" | grep -oE '"confidence"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
case "$CONF" in
    high|medium|low) ;;
    *) echo "FAIL: confidence='$CONF' is not a valid value" >&2; exit 1 ;;
esac

echo "PASS: ui-surface-detector oracle validates (surface_kind=$KIND, confidence=$CONF)"
exit 0
