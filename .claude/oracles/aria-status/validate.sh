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

# AOR capabilities (ARIA-17) is OPTIONAL (NO-DATA when the disk companion is absent),
# but when present it must carry a "verbs" key (never an empty-capabilities claim).
if echo "$OUTPUT" | grep -q '"capabilities"'; then
    if ! echo "$OUTPUT" | grep -q '"verbs"'; then
        echo "FAIL: capabilities present but missing 'verbs' (empty-capabilities claim forbidden)" >&2
        exit 1
    fi
fi

# Operability Ladder fields (OPER-05, DEC-133) are OPTIONAL and additive:
# when present, operability_tier must be a valid T0..T3 token, switchboard
# entries must be aor.*-prefixed, and both require capabilities to be present
# (they are projections of it — a tier with no capabilities is a fabrication).
if echo "$OUTPUT" | grep -q '"operability_tier"'; then
    TIER=$(echo "$OUTPUT" | grep -oE '"operability_tier"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
    case "$TIER" in
        T0|T1|T2|T3) ;;
        *) echo "FAIL: operability_tier='$TIER' not in T0..T3" >&2; exit 1 ;;
    esac
    if ! echo "$OUTPUT" | grep -q '"capabilities"'; then
        echo "FAIL: operability_tier present without capabilities (projection without source)" >&2
        exit 1
    fi
fi
if echo "$OUTPUT" | grep -q '"switchboard"'; then
    if ! echo "$OUTPUT" | grep -q '"capabilities"'; then
        echo "FAIL: switchboard present without capabilities (projection without source)" >&2
        exit 1
    fi
    SB_BAD=$(echo "$OUTPUT" | grep -oE '"switchboard"[[:space:]]*:[[:space:]]*\[[^]]*\]' | sed -E 's/^"switchboard"[[:space:]]*:[[:space:]]*//' | grep -oE '"[^"]+"' | grep -v '^"aor\.' || true)
    if [ -n "$SB_BAD" ]; then
        echo "FAIL: switchboard contains non-aor.* entries: $SB_BAD" >&2
        exit 1
    fi
fi

echo "PASS: aria-status oracle validates (surface_present=$SP, exposure_mechanism=$MECH, briefing_len=${#BRIEFING})"
exit 0
