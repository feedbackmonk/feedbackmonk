#!/usr/bin/env bash
# dispatch-reconciliation -- "did those unconfirmed dispatches ever land?"
#
# DISPATCH-12 (DEC-230). Verification Oracle.
#
# The gap it closes (GitCellar, 2026-08-02): dispatch-log.jsonl durably records
# every attempt, but nothing ever answered whether an UNCONFIRMED one eventually
# landed -- so the result stayed permanent NO-DATA and the only way to settle it
# was to hand-read .claude/session-state/turn-state/*.json, which is exactly
# what a live session had to do mid-incident.
#
# ---------------------------------------------------------------------------
# THE VERDICT VOCABULARY IS DELIBERATELY WEAKER THAN "CONFIRMED".
# ---------------------------------------------------------------------------
# The originating brief proposed upgrading these to CONFIRMED-LATE. That
# overclaims and was not adopted. TSTATE holds only the target's LATEST
# transition, so a stamp newer than the dispatch proves the target took *a* turn
# afterwards -- never that the turn was OURS. A user typing something unrelated
# is indistinguishable. ACTIVITY-AFTER-DISPATCH says precisely what was measured.
#
# FAIL is a four-way conjunction, and the `still live` leg is what keeps this
# lane self-clearing: a prompt cannot strand a session that has exited, and a
# permanently-red gate is one nobody reads.
#
# Exit codes:  0 pass | 1 stranded (actionable) | 2 no-data

set -uo pipefail

WORK_DIR="${1:-$(pwd)}"
[[ "$WORK_DIR" == --* ]] && WORK_DIR="$(pwd)"

LOOKBACK_HOURS="${ULDF_DISPATCH_RECONCILE_HOURS:-24}"
case "$LOOKBACK_HOURS" in ''|*[!0-9]*) LOOKBACK_HOURS=24 ;; esac

STATE_DIR="$WORK_DIR/.claude/session-state"
LOG="$STATE_DIR/dispatch-log.jsonl"
TURN_DIR="$STATE_DIR/turn-state"
REGISTRY="$WORK_DIR/.claude/collaboration/active-sessions.json"

CONTRACT="ACTIVITY-AFTER-DISPATCH means the target took SOME turn after the dispatch -- it is consistent with consumption, never proof of it. turn-state keeps only the latest transition."

json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

emit() { # $1=status $2=exit
    printf '{"status":"%s","details":{"log_present":%s,"entries_scanned":%d,"lookback_hours":%d,"activity_after_dispatch":%d,"no_activity_since":%d,"no_data":%d,"stranded_actionable":%d,"contract":"%s"},"entries":[%s]}\n' \
        "$1" "$LOG_PRESENT" "$SCANNED" "$LOOKBACK_HOURS" "$N_ACTIVITY" "$N_NOACT" "$N_NODATA" "$N_STRANDED" \
        "$(json_escape "$CONTRACT")" "$ENTRIES_JSON"
    exit "$2"
}

LOG_PRESENT="false"
SCANNED=0
N_ACTIVITY=0
N_NOACT=0
N_NODATA=0
N_STRANDED=0
ENTRIES_JSON=""

if [[ ! -f "$LOG" ]]; then
    emit "no-data" 2
fi
LOG_PRESENT="true"

NOW_EPOCH="$(date -u +%s)"
CUTOFF=$(( NOW_EPOCH - LOOKBACK_HOURS * 3600 ))

# ISO8601 "2026-08-02T21:31:09Z" -> epoch, without GNU-vs-BSD `date -d` divergence.
iso_to_epoch() {
    local iso="$1" y mo d h mi s days
    [[ "$iso" =~ ^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})Z?$ ]] || { echo 0; return; }
    y=$((10#${BASH_REMATCH[1]})); mo=$((10#${BASH_REMATCH[2]})); d=$((10#${BASH_REMATCH[3]}))
    h=$((10#${BASH_REMATCH[4]})); mi=$((10#${BASH_REMATCH[5]})); s=$((10#${BASH_REMATCH[6]}))
    # Days from civil (Howard Hinnant's algorithm) -- pure arithmetic, no
    # external date(1), so bash and PowerShell twins agree exactly.
    local yy=$y
    (( mo <= 2 )) && yy=$(( yy - 1 ))
    local era=$(( (yy >= 0 ? yy : yy - 399) / 400 ))
    local yoe=$(( yy - era * 400 ))
    local mp=$(( (mo + 9) % 12 ))
    local doy=$(( (153 * mp + 2) / 5 + d - 1 ))
    local doe=$(( yoe * 365 + yoe/4 - yoe/100 + doy ))
    days=$(( era * 146097 + doe - 719468 ))
    echo $(( days * 86400 + h * 3600 + mi * 60 + s ))
}

# Latest turn-state stamp for a session id (epoch, 0 if absent).
turn_stamp_epoch() {
    local sid="$1"
    local safe="${sid//[^A-Za-z0-9._-]/_}"
    local f="$TURN_DIR/$safe.json"
    [[ -f "$f" ]] || { echo "0|"; return; }
    local line at ep
    line="$(head -n 1 "$f" 2>/dev/null)"
    [[ -n "$line" ]] || { echo "0|"; return; }
    ep="$(printf '%s' "$line" | sed -n 's/.*"atEpoch"[[:space:]]*:[[:space:]]*\([0-9]*\).*/\1/p')"
    at="$(printf '%s' "$line" | sed -n 's/.*"at"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
    [[ -n "$ep" ]] || ep=0
    echo "$ep|$at"
}

# Is the target still live? Registry entry active+dispatchable with a live PID.
# Uses the pid-liveness lib, never a raw `kill -0`: Git-Bash cannot see native
# Windows PIDs and returns a confident false "dead" (TWIN-02 / DISC-PRO-23).
PID_LIB="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." 2>/dev/null && pwd)/scripts/lib/pid-liveness.sh"
[[ -f "$PID_LIB" ]] || PID_LIB="$HOME/.claude/scripts/lib/pid-liveness.sh"
# shellcheck disable=SC1090
[[ -f "$PID_LIB" ]] && . "$PID_LIB" 2>/dev/null

target_is_live() {
    local sid="$1"
    [[ -f "$REGISTRY" ]] || { echo "false"; return; }
    local entry
    entry="$(tr '{' '\n' < "$REGISTRY" | grep -F "\"id\":\"$sid\"" | head -n 1)"
    [[ -n "$entry" ]] || { echo "false"; return; }
    [[ "$entry" == *'"status":"active"'* ]] || { echo "false"; return; }
    [[ "$entry" == *'"dispatchable":true'* ]] || { echo "false"; return; }
    local p
    p="$(printf '%s' "$entry" | sed -n 's/.*"claudeShellPid"[[:space:]]*:[[:space:]]*\([0-9]*\).*/\1/p')"
    [[ -n "$p" && "$p" != "0" ]] || { echo "false"; return; }
    if type pid_is_alive >/dev/null 2>&1; then
        if pid_is_alive "$p"; then echo "true"; else echo "false"; fi
    else
        # No lib: report NOT live rather than guess. Under-reporting liveness can
        # only SUPPRESS a fail, never manufacture one -- the safe direction.
        echo "false"
    fi
}

while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -n "$line" ]] || continue
    outcome="$(printf '%s' "$line" | sed -n 's/.*"outcome"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
    case "$outcome" in
        delivered-unconfirmed|delivered-queued) : ;;
        *) continue ;;
    esac

    ts="$(printf '%s' "$line" | sed -n 's/.*"ts"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
    d_epoch="$(iso_to_epoch "$ts")"
    (( d_epoch >= CUTOFF )) || continue

    target="$(printf '%s' "$line" | sed -n 's/.*"target"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
    rid="$(printf '%s' "$line" | sed -n 's/.*"resolvedId"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
    reason="$(printf '%s' "$line" | sed -n 's/.*"reason"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
    [[ -n "$rid" ]] || rid="$target"

    SCANNED=$(( SCANNED + 1 ))

    stamp="$(turn_stamp_epoch "$rid")"
    s_epoch="${stamp%%|*}"
    s_at="${stamp#*|}"

    if [[ "$s_epoch" == "0" ]]; then
        verdict="NO-DATA"; N_NODATA=$(( N_NODATA + 1 ))
    elif (( s_epoch > d_epoch )); then
        verdict="ACTIVITY-AFTER-DISPATCH"; N_ACTIVITY=$(( N_ACTIVITY + 1 ))
    else
        verdict="NO-ACTIVITY-SINCE"; N_NOACT=$(( N_NOACT + 1 ))
    fi

    live="$(target_is_live "$rid")"

    # The four-way conjunction. `delivered-queued` is excluded by construction:
    # a non-receipt from a mid-turn target is the EXPECTED result (DISPATCH-11),
    # not a symptom, and grading it would re-import the very conflation this
    # whole change removed.
    actionable="false"
    if [[ "$reason" == "receipt-timeout" && "$verdict" == "NO-ACTIVITY-SINCE" && "$live" == "true" ]]; then
        actionable="true"
        N_STRANDED=$(( N_STRANDED + 1 ))
    fi

    [[ -n "$ENTRIES_JSON" ]] && ENTRIES_JSON="$ENTRIES_JSON,"
    ENTRIES_JSON="$ENTRIES_JSON{\"ts\":\"$(json_escape "$ts")\",\"target\":\"$(json_escape "$target")\",\"resolvedId\":\"$(json_escape "$rid")\",\"outcome\":\"$(json_escape "$outcome")\",\"reason\":\"$(json_escape "$reason")\",\"verdict\":\"$verdict\",\"targetLive\":$live,\"actionable\":$actionable,\"stampAt\":\"$(json_escape "$s_at")\"}"
done < "$LOG"

if (( SCANNED == 0 )); then
    emit "pass" 0
fi
if (( N_STRANDED > 0 )); then
    emit "fail" 1
fi
emit "pass" 0
