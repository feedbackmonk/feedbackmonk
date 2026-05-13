#!/bin/bash
# ltads-state oracle (Unix)
# Formalized from the state detection originally embedded in session-start.sh.
# Reports the LTADS state: none / permanent / temporary / legacy / incomplete_temp / broken

set -e

LTADS_PATH="ltads"

# No ltads/ directory = none
if [ ! -d "$LTADS_PATH" ]; then
    echo '{"state":"none","has_ltads_dir":false,"is_tracked":false,"config_exists":false,"is_temporary":false,"session_id":null,"session_status":null,"summary":"No LTADS on this project"}'
    exit 0
fi

# Read config.json
config_exists=false
is_temporary=false
if [ -f "$LTADS_PATH/config.json" ]; then
    config_exists=true
    if grep -qE '"temporary"[[:space:]]*:[[:space:]]*true' "$LTADS_PATH/config.json" 2>/dev/null; then
        is_temporary=true
    fi
fi

# Read current-session.md
session_status=""
session_id=""
current_session_path="$LTADS_PATH/sessions/current-session.md"
if [ -f "$current_session_path" ]; then
    session_status=$(grep -oE '^(##[[:space:]]+|-[[:space:]]+\*\*|\*\*)Status(\*\*)?[[:space:]]*:[[:space:]]*[A-Z_]+' "$current_session_path" 2>/dev/null | head -1 | grep -oE '[A-Z_]+$')
    session_id=$(grep -oE '^-?[[:space:]]*\*\*ID\*\*[[:space:]]*:[[:space:]]*[^[:space:]].*' "$current_session_path" 2>/dev/null | head -1 | sed -E 's/^-?[[:space:]]*\*\*ID\*\*[[:space:]]*:[[:space:]]*//')
fi

# Check git tracking
is_tracked=false
if command -v git >/dev/null 2>&1; then
    tracked_files=$(git ls-files "$LTADS_PATH/" 2>/dev/null || echo "")
    if [ -n "$tracked_files" ]; then
        is_tracked=true
    fi
fi

# Classify
state="broken"
summary=""
if [ "$config_exists" = "false" ]; then
    if [ "$is_tracked" = "true" ]; then
        state="legacy"
        summary="Legacy permanent LTADS (no config.json, tracked in git). Run /0-uldf-ltads-admin init to upgrade."
    else
        state="incomplete_temp"
        summary="Incomplete temporary state (no config.json, untracked). Safe to delete ltads/ manually."
    fi
elif [ "$is_temporary" = "true" ]; then
    state="temporary"
    summary="Temporary LTADS"
    if [ -n "$session_id" ]; then summary="$summary, session $session_id"; fi
    if [ -n "$session_status" ]; then summary="$summary ($session_status)"; fi
else
    state="permanent"
    summary="Permanent LTADS"
    if [ -n "$session_id" ]; then summary="$summary, session $session_id"; fi
    if [ -n "$session_status" ]; then summary="$summary ($session_status)"; fi
fi

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

session_id_json="null"
if [ -n "$session_id" ]; then session_id_json="\"$(esc "$session_id")\""; fi
session_status_json="null"
if [ -n "$session_status" ]; then session_status_json="\"$(esc "$session_status")\""; fi

cat <<EOF
{"state":"$state","has_ltads_dir":true,"is_tracked":$is_tracked,"config_exists":$config_exists,"is_temporary":$is_temporary,"session_id":$session_id_json,"session_status":$session_status_json,"summary":"$(esc "$summary")"}
EOF
