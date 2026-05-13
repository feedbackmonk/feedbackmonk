#!/bin/bash
# aria-status oracle self-test (Unix)
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
for field in surface_present exposure_mechanism endpoint_reachable foundation_layer briefing; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

# foundation_layer must contain errors/async/navigation
for fl_field in errors async navigation; do
    if ! echo "$OUTPUT" | grep -qE "\"$fl_field\"[[:space:]]*:"; then
        echo "FAIL: foundation_layer missing '$fl_field'" >&2
        exit 1
    fi
done

# exposure_mechanism enum check
MECH=$(echo "$OUTPUT" | grep -oE '"exposure_mechanism"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
case "$MECH" in
    tauri-ipc|http|websocket|file|none) ;;
    *) echo "FAIL: exposure_mechanism='$MECH' not in enum" >&2; exit 1 ;;
esac

# Briefing length cap (≤200 chars)
BRIEFING=$(echo "$OUTPUT" | grep -oE '"briefing"[[:space:]]*:[[:space:]]*"[^"]*"' | sed -E 's/^"briefing"[[:space:]]*:[[:space:]]*"(.*)"$/\1/')
if [ ${#BRIEFING} -gt 200 ]; then
    echo "FAIL: briefing length ${#BRIEFING} exceeds 200-char cap" >&2
    exit 1
fi

# When surface_present=false, briefing must be empty
SP=$(echo "$OUTPUT" | grep -oE '"surface_present"[[:space:]]*:[[:space:]]*(true|false)' | grep -oE '(true|false)$')
if [ "$SP" = "false" ] && [ -n "$BRIEFING" ]; then
    echo "FAIL: surface_present=false but briefing is non-empty: '$BRIEFING'" >&2
    exit 1
fi

echo "PASS: aria-status oracle validates (surface_present=$SP, exposure_mechanism=$MECH, briefing_len=${#BRIEFING})"
exit 0
