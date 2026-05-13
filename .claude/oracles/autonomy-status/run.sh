#!/bin/bash
# autonomy-status oracle (Unix)
# Resolves the four-step autonomy cascade and emits {level, source, arc_id, expires_at, source_detail, briefing}.
#
# Cascade order (first non-skip-non-empty wins):
#   1. Session override (out-of-band; not readable from disk — caller may pass --session-override=<level>)
#   2. ltads/sessions/current-session.md **Autonomy Override** line (skip if Status: CONCLUDED)
#   3. .claude/session-state/task-arc-autonomy.json (skip if expired or grantor PID dead)
#   4. ltads/config.json autonomy.default (cap at collaborative)
#   5. Default: collaborative

set -e

SESSION_OVERRIDE=""
for arg in "$@"; do
    case "$arg" in
        --session-override=*) SESSION_OVERRIDE="${arg#--session-override=}" ;;
    esac
done

# -----------------------------------------------------------------------------
# Helpers
# -----------------------------------------------------------------------------

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

emit_json() {
    local level="$1"
    local source="$2"
    local arc_id="$3"
    local expires_at="$4"
    local detail="$5"

    local arc_json="null"
    [ -n "$arc_id" ] && arc_json="\"$(esc "$arc_id")\""
    local expires_json="null"
    [ -n "$expires_at" ] && expires_json="\"$(esc "$expires_at")\""

    local briefing=""
    if [ "$level" != "collaborative" ]; then
        if [ -n "$arc_id" ] && [ -n "$expires_at" ]; then
            briefing="{\"level\":\"$level\",\"source\":\"$source\",\"arc_id\":\"$(esc "$arc_id")\",\"expires_at\":\"$(esc "$expires_at")\"}"
        else
            briefing="{\"level\":\"$level\",\"source\":\"$source\"}"
        fi
    fi

    cat <<EOF
{"level":"$level","source":"$source","arc_id":$arc_json,"expires_at":$expires_json,"source_detail":"$(esc "$detail")","briefing":"$(esc "$briefing")"}
EOF
}

is_pid_alive() {
    local pid="$1"
    [ -z "$pid" ] || [ "$pid" = "0" ] && return 1
    kill -0 "$pid" 2>/dev/null
}

# -----------------------------------------------------------------------------
# Step 1: Session override (caller-supplied)
# -----------------------------------------------------------------------------

if [ -n "$SESSION_OVERRIDE" ]; then
    case "$SESSION_OVERRIDE" in
        autopilot|supervised|collaborative|controlled|manual)
            emit_json "$SESSION_OVERRIDE" "session-override" "" "" "Session override passed via --session-override flag"
            exit 0
            ;;
    esac
fi

# -----------------------------------------------------------------------------
# Step 2: LTADS current-session.md Autonomy Override (skip if Status: CONCLUDED)
# -----------------------------------------------------------------------------

LTADS_FILE="ltads/sessions/current-session.md"
if [ -f "$LTADS_FILE" ]; then
    STATUS=$(grep -oE '\*\*Status\*\*[[:space:]]*:[[:space:]]*[A-Z_]+' "$LTADS_FILE" 2>/dev/null | head -1 | grep -oE '[A-Z_]+$' || true)
    if [ "$STATUS" != "CONCLUDED" ] && [ "$STATUS" != "PAUSED" ]; then
        OVERRIDE_LINE=$(grep -E '^\*\*Autonomy Override\*\*[[:space:]]*:' "$LTADS_FILE" 2>/dev/null | head -1 || true)
        if [ -n "$OVERRIDE_LINE" ]; then
            LEVEL=$(printf '%s' "$OVERRIDE_LINE" | sed -E 's/^\*\*Autonomy Override\*\*[[:space:]]*:[[:space:]]*//' | tr -d ' ' | tr '[:upper:]' '[:lower:]')
            case "$LEVEL" in
                autopilot|supervised|collaborative|controlled|manual)
                    emit_json "$LEVEL" "ltads-session" "" "" "ltads/sessions/current-session.md Autonomy Override line"
                    exit 0
                    ;;
            esac
        fi
    fi
fi

# -----------------------------------------------------------------------------
# Step 3: .claude/session-state/task-arc-autonomy.json (skip if expired or grantor dead)
# -----------------------------------------------------------------------------

ARC_FILE=".claude/session-state/task-arc-autonomy.json"
if [ -f "$ARC_FILE" ]; then
    # Read fields with simple grep (avoid jq dependency)
    ARC_LEVEL=$(grep -oE '"level"[[:space:]]*:[[:space:]]*"[^"]+"' "$ARC_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    ARC_ID=$(grep -oE '"arc_id"[[:space:]]*:[[:space:]]*"[^"]+"' "$ARC_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    ARC_EXPIRES=$(grep -oE '"expires_at"[[:space:]]*:[[:space:]]*"[^"]+"' "$ARC_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    ARC_PID=$(grep -oE '"grantor_pid"[[:space:]]*:[[:space:]]*[0-9]+' "$ARC_FILE" 2>/dev/null | head -1 | grep -oE '[0-9]+$')

    case "$ARC_LEVEL" in
        autopilot|supervised|collaborative|controlled|manual)
            # TTL check
            ARC_EXPIRED=false
            if [ -n "$ARC_EXPIRES" ]; then
                # Compare ISO-8601 strings lexically (works for UTC Zulu timestamps)
                NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
                if [ "$NOW" \> "$ARC_EXPIRES" ]; then ARC_EXPIRED=true; fi
            fi
            # Liveness check
            ARC_DEAD=false
            if [ -n "$ARC_PID" ]; then
                if ! is_pid_alive "$ARC_PID"; then ARC_DEAD=true; fi
            fi
            if [ "$ARC_EXPIRED" != "true" ] && [ "$ARC_DEAD" != "true" ]; then
                emit_json "$ARC_LEVEL" "task-arc-autonomy" "$ARC_ID" "$ARC_EXPIRES" ".claude/session-state/task-arc-autonomy.json (TTL valid, grantor alive)"
                exit 0
            fi
            ;;
    esac
fi

# -----------------------------------------------------------------------------
# Step 4: ltads/config.json autonomy.default (cap at collaborative)
# -----------------------------------------------------------------------------

CONFIG_FILE="ltads/config.json"
if [ -f "$CONFIG_FILE" ]; then
    CONFIG_LEVEL=$(grep -oE '"default"[[:space:]]*:[[:space:]]*"[^"]+"' "$CONFIG_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    case "$CONFIG_LEVEL" in
        collaborative|controlled|manual)
            emit_json "$CONFIG_LEVEL" "config" "" "" "ltads/config.json autonomy.default"
            exit 0
            ;;
        autopilot|supervised)
            # CAP: refuse to honor; treat as collaborative
            emit_json "collaborative" "config" "" "" "ltads/config.json autonomy.default is '$CONFIG_LEVEL' (CAPPED to collaborative per cascade rule)"
            exit 0
            ;;
    esac
fi

# -----------------------------------------------------------------------------
# Step 5: Default
# -----------------------------------------------------------------------------

emit_json "collaborative" "default" "" "" "No override / LTADS / arc-autonomy / config — falling through to documented default"
