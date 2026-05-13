#!/bin/bash
# stale-ltads-state oracle (Unix)
#
# CSI-14 (Phase 1.6): emit a [stale-ltads-state] briefing line when
# ltads/sessions/current-session.md Status is ACTIVE/PAUSED/IN_PROGRESS but
# the matching active-sessions.json entry is closed/expired/missing/PID-dead.
#
# Output: single-line JSON (always-fresh; ~60ms budget).
# Gracefully absent: when state is consistent, briefing field is empty so the
# session-start hook emits no line (parallel to dispatchable-sessions's
# empty-result silence).

set +e

CS_MD="ltads/sessions/current-session.md"
REGISTRY=".claude/collaboration/active-sessions.json"

# ---- Default empty/consistent output ---------------------------------------
emit_consistent() {
    local status_json="$1"   # JSON string for current_session_status (e.g. '"ACTIVE"' or 'null')
    local sid_json="$2"      # JSON string for current_session_id
    cat <<EOF
{"stale":false,"details":{"current_session_status":$status_json,"current_session_id":$sid_json,"registry_status":"active","registry_pid_alive":null,"inconsistency_kind":"none"},"briefing":""}
EOF
    exit 0
}

# ---- Graceful absence: no LTADS file -> nothing to compare -----------------
if [ ! -f "$CS_MD" ]; then
    emit_consistent "null" "null"
fi

# ---- Read current-session.md -----------------------------------------------
status_value=$(grep -m 1 -E '^Status:[[:space:]]*' "$CS_MD" 2>/dev/null | sed -E 's/^Status:[[:space:]]*([^[:space:]]+).*/\1/')
session_id=$(grep -m 1 -E '^Session:[[:space:]]*' "$CS_MD" 2>/dev/null | sed -E 's/^Session:[[:space:]]*([^[:space:]]+).*/\1/')

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

status_json="null"
if [ -n "$status_value" ]; then status_json="\"$(esc "$status_value")\""; fi
sid_json="null"
if [ -n "$session_id" ]; then sid_json="\"$(esc "$session_id")\""; fi

# Only ACTIVE/PAUSED/IN_PROGRESS warrant the inconsistency check.
case "$status_value" in
    ACTIVE|PAUSED|IN_PROGRESS) ;;
    *) emit_consistent "$status_json" "$sid_json" ;;
esac

# Status is ACTIVE/PAUSED/IN_PROGRESS but no Session: id -> can't lookup; treat
# as consistent to avoid false positives (session id will appear once the next
# /0-uldf-ltads-start writes the file).
if [ -z "$session_id" ]; then
    emit_consistent "$status_json" "$sid_json"
fi

# ---- No registry -> session never registered; emit missing ------------------
if [ ! -f "$REGISTRY" ]; then
    cat <<EOF
{"stale":true,"details":{"current_session_status":$status_json,"current_session_id":$sid_json,"registry_status":"missing","registry_pid_alive":null,"inconsistency_kind":"registry-missing-state-active"},"briefing":"current-session.md Status: $status_value (session $session_id) but active-sessions.json missing"}
EOF
    exit 0
fi

# ---- Find matching registry entry (active, closed, or absent) --------------
# Pick parser: jq preferred, python fallback. Probe-verify python (Windows
# Microsoft Store stub silently exits non-zero).
parser=""
if command -v jq >/dev/null 2>&1; then
    parser="jq"
elif command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
    parser="python3"
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
    parser="python"
fi

if [ -z "$parser" ]; then
    # No parser -> graceful absence (don't emit a stale signal we can't verify)
    emit_consistent "$status_json" "$sid_json"
fi

reg_status=""
reg_pid=""
if [ "$parser" = "jq" ]; then
    reg_status="$(jq -r --arg sid "$session_id" '
        ((.sessions // []) | map(select(.id == $sid)) | .[0]) as $a
        | ((.closed   // []) | map(select(.id == $sid)) | .[0]) as $c
        | if   $a then "active"
          elif $c then ($c.status // "closed")
          else "missing"
          end
    ' "$REGISTRY" 2>/dev/null)"
    if [ "$reg_status" = "active" ]; then
        reg_pid="$(jq -r --arg sid "$session_id" '
            (.sessions // []) | map(select(.id == $sid)) | .[0].claudeShellPid // ""
        ' "$REGISTRY" 2>/dev/null)"
    fi
else
    out="$(SID="$session_id" "$parser" - "$REGISTRY" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    print("missing\t")
    sys.exit(0)
if not isinstance(d, dict):
    print("missing\t")
    sys.exit(0)
sid = os.environ["SID"]
for s in (d.get("sessions") or []):
    if isinstance(s, dict) and s.get("id") == sid:
        pid = s.get("claudeShellPid", "")
        print(f"active\t{pid}")
        sys.exit(0)
for s in (d.get("closed") or []):
    if isinstance(s, dict) and s.get("id") == sid:
        st = s.get("status", "closed")
        print(f"{st}\t")
        sys.exit(0)
print("missing\t")
PY
)"
    reg_status="${out%%	*}"
    reg_pid="${out#*	}"
fi

[ -z "$reg_status" ] && reg_status="missing"

# ---- Determine inconsistency kind ------------------------------------------
inconsistency_kind="none"
pid_alive_json="null"

case "$reg_status" in
    active)
        # Check if registered PID is alive. If not, that's a stale-state
        # signal (session died but no GC ran yet AND state is still ACTIVE).
        if [ -n "$reg_pid" ] && [ "$reg_pid" != "0" ]; then
            if kill -0 "$reg_pid" 2>/dev/null; then
                pid_alive_json="true"
            else
                pid_alive_json="false"
                inconsistency_kind="registry-pid-dead-state-active"
            fi
        fi
        ;;
    closed)
        inconsistency_kind="registry-closed-state-active"
        ;;
    expired)
        inconsistency_kind="registry-expired-state-active"
        ;;
    missing)
        inconsistency_kind="registry-missing-state-active"
        ;;
esac

if [ "$inconsistency_kind" = "none" ]; then
    emit_consistent "$status_json" "$sid_json"
fi

# ---- Compose briefing -------------------------------------------------------
case "$inconsistency_kind" in
    registry-closed-state-active)
        briefing="current-session.md Status: $status_value (session $session_id) but registry shows entry as CLOSED -- run /0-uldf-finalize or /0-uldf-ltads-stop to reconcile"
        ;;
    registry-expired-state-active)
        briefing="current-session.md Status: $status_value (session $session_id) but registry shows entry as EXPIRED (CSI-05 GC swept it) -- state should have been auto-flipped by CSI-13"
        ;;
    registry-pid-dead-state-active)
        briefing="current-session.md Status: $status_value (session $session_id) but registered PID is dead -- next GC sweep will reconcile, or run /0-uldf-finalize manually"
        ;;
    registry-missing-state-active)
        briefing="current-session.md Status: $status_value (session $session_id) but no matching registry entry -- session never registered or registry was reset"
        ;;
    *)
        briefing="stale-ltads-state inconsistency"
        ;;
esac

cat <<EOF
{"stale":true,"details":{"current_session_status":$status_json,"current_session_id":$sid_json,"registry_status":"$reg_status","registry_pid_alive":$pid_alive_json,"inconsistency_kind":"$inconsistency_kind"},"briefing":"$(esc "$briefing")"}
EOF
exit 0
