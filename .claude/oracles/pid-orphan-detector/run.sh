#!/bin/bash
# pid-orphan-detector oracle (Unix + Git Bash on Windows)
# Answers: are there worker-shell-*.pid files referencing PIDs that are no longer alive?
#
# Operates on ltads/execution/worker-shell-*.pid (and legacy worker-shell.pid).
# Liveness-based -- NO TTL per DEC-54. Three-leg defense per DEC-55.
#
# Modes (mirrors archive-retention / dispatchable-sessions):
#   (default)    : briefing path -- emit {swept[], alive[], malformed[], briefing}
#                  reflecting CURRENT liveness without action (read-only).
#   --gc-cheap   : session-start hygiene sweep, ~100ms budget, defers if exceeded.
#                  Deletes orphans; writes _pid-summary.jsonl BEFORE delete.
#   --gc         : on-demand full sweep, no time budget. Same delete + audit shape.
#
# Sweep criteria: file under ltads/execution/, basename matches worker-shell-*.pid
# (or legacy worker-shell.pid), content is positive integer, PID not alive.
# Malformed (non-integer) content is NEVER deleted (failure-open). Alive PIDs are
# NEVER deleted regardless of age.
#
# SWEEP-08 invariant: summary write to ltads/execution/_pid-summary.jsonl must
# succeed BEFORE the delete. On summary write failure, the file is preserved.

set -e

EXEC_DIR="ltads/execution"
SUMMARY_FILE="$EXEC_DIR/_pid-summary.jsonl"
EMPTY_BRIEFING='{"swept":[],"alive":[],"malformed":[],"briefing":""}'

emit_empty() {
    echo "$EMPTY_BRIEFING"
    exit 0
}

# ---- Parse mode ----
MODE="briefing"
case "${1:-}" in
    --gc)        MODE="gc" ;;
    --gc-cheap)  MODE="gc-cheap" ;;
    "")          MODE="briefing" ;;
    *)
        echo "pid-orphan-detector: unknown mode: $1" >&2
        echo "  usage: run.sh [--gc|--gc-cheap]" >&2
        exit 1
        ;;
esac

# ---- Source the shared liveness helper -------------------------------------
# Resolve the lib path: prefer in-repo dev path, fall back to deployed copy.
_PID_LIB=""
_THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd)"
for cand in \
    "$_THIS_DIR/../../scripts/lib/pid-liveness.sh" \
    "$_THIS_DIR/../../../claude-template/scripts/lib/pid-liveness.sh" \
    "$HOME/.claude/scripts/lib/pid-liveness.sh"
do
    if [ -n "$cand" ] && [ -f "$cand" ]; then
        _PID_LIB="$cand"
        break
    fi
done
if [ -n "$_PID_LIB" ]; then
    # shellcheck disable=SC1090
    . "$_PID_LIB" 2>/dev/null || _PID_LIB=""
fi
# Defensive inline fallback if the helper is unavailable (matches the
# dispatchable-sessions pattern; failure-closed -> treats unknown as alive).
if ! command -v pid_is_alive >/dev/null 2>&1; then
    _uldf_pid_probe="kill"
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*) _uldf_pid_probe="powershell" ;;
    esac
    pid_is_alive() {
        local pid="$1"
        [ -n "$pid" ] || return 1
        case "$pid" in (*[!0-9]*) return 1 ;; esac
        [ "$pid" -gt 0 ] || return 1
        if [ "$_uldf_pid_probe" = "powershell" ]; then
            powershell.exe -NoProfile -Command "if (Get-Process -Id $pid -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1
        else
            kill -0 "$pid" 2>/dev/null
        fi
    }
fi

# ---- Locate the exec dir; absent => graceful nothing-to-do -----------------
if [ ! -d "$EXEC_DIR" ]; then
    if [ "$MODE" = "briefing" ] || [ "$MODE" = "gc" ]; then
        emit_empty
    else
        # --gc-cheap is silent on graceful absence
        exit 0
    fi
fi

# ---- Helpers ---------------------------------------------------------------

_iso_mtime() {
    local f="$1"
    [ -f "$f" ] || return 0
    local epoch
    # GNU stat
    epoch=$(stat -c %Y "$f" 2>/dev/null) && [ -n "$epoch" ] && {
        date -u -d "@$epoch" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null && return 0
    }
    # macOS BSD stat
    epoch=$(stat -f %m "$f" 2>/dev/null) && [ -n "$epoch" ] && {
        date -u -r "$epoch" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null && return 0
    }
    # python fallback
    for py in python3 python; do
        if command -v "$py" >/dev/null 2>&1; then
            "$py" -c "
import os, sys
from datetime import datetime, timezone
try:
    m = os.path.getmtime(sys.argv[1])
    print(datetime.fromtimestamp(int(m), tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))
except Exception:
    pass
" "$f" 2>/dev/null && return 0
        fi
    done
}

_ms_now() {
    local ms
    ms=$(date -u +%s%3N 2>/dev/null)
    if [ -n "$ms" ]; then
        case "$ms" in (*[!0-9]*) ;; *) echo "$ms"; return 0 ;; esac
    fi
    if command -v perl >/dev/null 2>&1; then
        perl -MTime::HiRes=time -e 'printf("%d\n", time*1000)' 2>/dev/null && return 0
    fi
    for py in python3 python; do
        if command -v "$py" >/dev/null 2>&1; then
            "$py" -c 'import time; print(int(time.time()*1000))' 2>/dev/null && return 0
        fi
    done
}

# JSON escape (path / string fields). Backslash, quote, CR, LF.
_json_esc() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\r'/}"
    s="${s//$'\n'/ }"
    printf '%s' "$s"
}

# Atomic append + verify. Returns 0 on success, 1 on failure.
_append_summary() {
    local line="$1"
    mkdir -p "$EXEC_DIR" 2>/dev/null || true
    if ! printf '%s\n' "$line" >> "$SUMMARY_FILE" 2>/dev/null; then
        return 1
    fi
    local last
    last=$(tail -n 1 "$SUMMARY_FILE" 2>/dev/null)
    [ "$last" = "$line" ]
}

# Build summary line for a swept entry (delete-time format).
_build_summary_line() {
    local pid_file="$1"
    local pid="$2"
    local mtime="$3"
    local swept_at="$4"
    local jp jm
    jp=$(_json_esc "$pid_file")
    jm=$(_json_esc "$mtime")
    printf '{"pid_file":"%s","referenced_pid":%s,"liveness_at_sweep":false,"mtime":%s,"sweptAt":"%s"}' \
        "$jp" "$pid" \
        "$( [ -n "$mtime" ] && printf '"%s"' "$jm" || printf 'null' )" \
        "$swept_at"
}

# Enumerate target .pid files (worker-shell-*.pid + legacy worker-shell.pid).
# Excludes the summary file itself and any non-regular files.
_list_pid_files() {
    shopt -s nullglob 2>/dev/null || true
    local f
    for f in "$EXEC_DIR"/worker-shell-*.pid "$EXEC_DIR"/worker-shell.pid; do
        [ -f "$f" ] || continue
        printf '%s\n' "$f"
    done
}

NOW_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# =============================================================================
# Mode dispatch
# =============================================================================

# Common scan: builds three lists -- SWEPT_ITEMS, ALIVE_ITEMS, MALFORMED_ITEMS
# (each newline-separated; SWEPT_ITEMS holds "pid_file<TAB>pid<TAB>mtime").
SWEPT_ITEMS=""
ALIVE_ITEMS=""
MALFORMED_ITEMS=""

BUDGET_MS=0
# Cheap-mode budget: 500ms is the cap for the per-file probe loop. Linux/macOS
# `kill -0` finishes in microseconds, so this is generous. Git Bash on Windows
# spawns powershell.exe per probe (~80-200ms cold-start) -- the larger envelope
# keeps cheap mode useful on Windows without inflating it past "fire-and-forget"
# intent. Worker-shell .pid populations are typically <5 entries; the budget
# absorbs ~3-5 cold-start probes plus filesystem cost. dispatchable-sessions'
# 100ms budget is fine for it because PID liveness is checked against an
# already-pre-filtered candidate list, not a fresh enumeration; this oracle's
# cost profile is fundamentally different.
if [ "$MODE" = "gc-cheap" ]; then BUDGET_MS=500; fi

START_MS=$(_ms_now)
BUDGET_EXCEEDED=""

while IFS= read -r pid_file; do
    [ -n "$pid_file" ] || continue
    [ -f "$pid_file" ] || continue

    if [ "$BUDGET_MS" -gt 0 ]; then
        NOW_MS=$(_ms_now)
        if [ -n "$START_MS" ] && [ -n "$NOW_MS" ] && [ "$((NOW_MS - START_MS))" -gt "$BUDGET_MS" ]; then
            BUDGET_EXCEEDED=1
            break
        fi
    fi

    # Read content; trim CR/LF/whitespace.
    raw=$(head -n 1 "$pid_file" 2>/dev/null | tr -d '[:space:]\r')
    case "$raw" in
        ''|*[!0-9]*)
            MALFORMED_ITEMS="$MALFORMED_ITEMS$pid_file"$'\n'
            continue
            ;;
    esac
    if [ "$raw" -le 0 ] 2>/dev/null; then
        MALFORMED_ITEMS="$MALFORMED_ITEMS$pid_file"$'\n'
        continue
    fi

    if pid_is_alive "$raw"; then
        ALIVE_ITEMS="$ALIVE_ITEMS$pid_file"$'\t'"$raw"$'\n'
    else
        mt=$(_iso_mtime "$pid_file")
        SWEPT_ITEMS="$SWEPT_ITEMS$pid_file"$'\t'"$raw"$'\t'"$mt"$'\n'
    fi
done < <(_list_pid_files)

# In sweep modes, perform the deletes (with pre-delete JSONL append).
if [ "$MODE" = "gc" ] || [ "$MODE" = "gc-cheap" ]; then
    NEW_SWEPT=""
    while IFS=$'\t' read -r pid_file pid mt; do
        [ -n "$pid_file" ] || continue
        line=$(_build_summary_line "$pid_file" "$pid" "$mt" "$NOW_ISO")
        if ! _append_summary "$line"; then
            echo "pid-orphan-detector: summary write failed for $pid_file; preserved" >&2
            # On summary failure, keep file out of swept[] in gc mode -- the
            # file still exists and was not deleted. Move it to alive[] so
            # downstream consumers see the truth (though strictly the PID
            # was probed dead). Simplest: drop from output entirely.
            continue
        fi
        if rm -f "$pid_file" 2>/dev/null; then
            NEW_SWEPT="$NEW_SWEPT$pid_file"$'\t'"$pid"$'\t'"$mt"$'\n'
        else
            echo "pid-orphan-detector: delete failed for $pid_file (summary already appended)" >&2
        fi
    done <<EOF
$SWEPT_ITEMS
EOF
    SWEPT_ITEMS="$NEW_SWEPT"

    # --gc-cheap: silent on success.
    if [ "$MODE" = "gc-cheap" ]; then
        exit 0
    fi
fi

# =============================================================================
# Emit JSON output (briefing OR --gc summary; same shape).
# =============================================================================

_emit_array_swept() {
    local first=1
    local pid_file pid mt jp jm
    while IFS=$'\t' read -r pid_file pid mt; do
        [ -n "$pid_file" ] || continue
        jp=$(_json_esc "$pid_file")
        jm=$(_json_esc "$mt")
        if [ "$first" -eq 1 ]; then first=0; else printf ','; fi
        printf '{"pid_file":"%s","referenced_pid":%s,"liveness_at_sweep":false,"mtime":%s}' \
            "$jp" "$pid" \
            "$( [ -n "$mt" ] && printf '"%s"' "$jm" || printf 'null' )"
    done <<EOF
$SWEPT_ITEMS
EOF
}

_emit_array_alive() {
    local first=1
    local pid_file pid jp
    while IFS=$'\t' read -r pid_file pid; do
        [ -n "$pid_file" ] || continue
        jp=$(_json_esc "$pid_file")
        if [ "$first" -eq 1 ]; then first=0; else printf ','; fi
        printf '{"pid_file":"%s","referenced_pid":%s}' "$jp" "$pid"
    done <<EOF
$ALIVE_ITEMS
EOF
}

_emit_array_malformed() {
    local first=1
    local pid_file jp
    while IFS= read -r pid_file; do
        [ -n "$pid_file" ] || continue
        jp=$(_json_esc "$pid_file")
        if [ "$first" -eq 1 ]; then first=0; else printf ','; fi
        printf '"%s"' "$jp"
    done <<EOF
$MALFORMED_ITEMS
EOF
}

# Briefing line: empty when swept[] is empty.
SWEPT_COUNT=$(printf '%s' "$SWEPT_ITEMS" | grep -c $'\t' 2>/dev/null || true)
[ -n "$SWEPT_COUNT" ] || SWEPT_COUNT=0
case "$SWEPT_COUNT" in (*[!0-9]*) SWEPT_COUNT=0 ;; esac
if [ "$SWEPT_COUNT" -gt 0 ]; then
    BRIEFING="[pid-orphans] $SWEPT_COUNT stale worker-shell PIDs, run /0-uldf-oracle pid-orphan-detector --gc to clean"
else
    BRIEFING=""
fi

printf '{"swept":['
_emit_array_swept
printf '],"alive":['
_emit_array_alive
printf '],"malformed":['
_emit_array_malformed
printf '],"briefing":"%s"' "$(_json_esc "$BRIEFING")"
if [ -n "$BUDGET_EXCEEDED" ] && [ "$MODE" = "gc" ]; then
    printf ',"budgetExceeded":true'
fi
printf '}\n'

exit 0
