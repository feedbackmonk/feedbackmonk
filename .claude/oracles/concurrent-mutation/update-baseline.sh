#!/bin/bash
# concurrent-mutation baseline writer (Unix) -- CSI-10, CSI Phase 3.
#
# Captures / refreshes this session's known-good snapshot of the tracked LTADS
# path set into .claude/session-state/this-session.json under the csi10Baseline
# key (read-merge-write; other top-level keys are preserved so this never
# clobbers any legacy identity fields). The read-only run.{sh,ps1} compares
# against this snapshot.
#
# WHY a separate script: Verification Oracles are read-only (ORACULURGY Part 11
# sec 11.3.4); run.sh must not write. This writer is invoked by the session-
# start hook (establish/refresh at session start) and by /0-uldf-finalize, and
# is intended to also be called after this session's own writes to tracked
# paths so the mtime baseline tracks self-authored edits (which then do NOT
# read as external mutations).
#
# Usage:
#   update-baseline.sh [--root <dir>] [--session-id <id>] [--session-start <iso>]
# Defaults: root=cwd; session-id resolved env-first (CSI-16); session-start
# preserved from an existing same-session baseline, else now.
#
# Fail-open: every error path is a silent no-op return (never breaks the
# session-start hot path or the finalize flow). Cheap: gated behind a baseline-
# dir existence/creation check; no work when no tracked file exists.

set +e

ROOT=""
ARG_SID=""
ARG_START=""
while [ $# -gt 0 ]; do
    case "$1" in
        --root) ROOT="${2:-}"; shift 2 ;;
        --session-id) ARG_SID="${2:-}"; shift 2 ;;
        --session-start) ARG_START="${2:-}"; shift 2 ;;
        *) shift ;;
    esac
done
[ -n "$ROOT" ] || ROOT="$(pwd)"
cd "$ROOT" 2>/dev/null || exit 0

BASELINE_FILE=".claude/session-state/this-session.json"
TRACKED_FILES="ltads/arc-state.json ltads/arc-state.archive.json ltads/sessions/session-history.md ltads/sessions/blockers.md"
TRACKED_DIRS="ltads/execution"

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

file_mtime() {
    local f="$1" m
    m=$(stat -c %Y "$f" 2>/dev/null)
    case "$m" in (''|*[!0-9]*) m=$(stat -f %m "$f" 2>/dev/null) ;; esac
    case "$m" in (''|*[!0-9]*) m="" ;; esac
    printf '%s' "$m"
}

# ---- Resolve sessionId (arg, then env-first lib, then env, then unknown) ----
MY_SID="$ARG_SID"
if [ -z "$MY_SID" ]; then
    _THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
    for _cand in \
        "$_THIS_DIR/../../scripts/lib/session-identity.sh" \
        "$_THIS_DIR/../../../claude-template/scripts/lib/session-identity.sh" \
        "$HOME/.claude/scripts/lib/session-identity.sh"; do
        if [ -f "$_cand" ]; then
            # shellcheck source=/dev/null
            . "$_cand"
            break
        fi
    done
    if command -v csi_identity_resolve >/dev/null 2>&1; then
        MY_SID=$(csi_identity_resolve "$ROOT" "$PPID" | cut -d'|' -f1)
    fi
    [ -n "$MY_SID" ] || MY_SID="${CLAUDE_SESSION_ID:-}"
fi
[ -n "$MY_SID" ] || MY_SID="unknown"

# ---- Collect the tracked-file list -----------------------------------------
FILES=""
for f in $TRACKED_FILES; do
    [ -f "$f" ] && FILES="$FILES$f
"
done
for d in $TRACKED_DIRS; do
    if [ -d "$d" ]; then
        while IFS= read -r f; do
            [ -n "$f" ] && FILES="$FILES$f
"
        done <<EOF
$(find "$d" -type f 2>/dev/null)
EOF
    fi
done

# ---- Build the mtimes JSON object ------------------------------------------
MT_OBJ=""
while IFS= read -r f; do
    [ -z "$f" ] && continue
    fr="${f#./}"
    m=$(file_mtime "$fr")
    [ -z "$m" ] && continue
    pair="\"$(esc "$fr")\":$m"
    if [ -z "$MT_OBJ" ]; then MT_OBJ="$pair"; else MT_OBJ="$MT_OBJ,$pair"; fi
done <<EOF
$FILES
EOF
MT_OBJ="{$MT_OBJ}"

# ---- headSha + timestamps --------------------------------------------------
HEAD_SHA=""
if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    HEAD_SHA=$(git rev-parse HEAD 2>/dev/null)
    case "$HEAD_SHA" in (*[!0-9a-fA-F]*) HEAD_SHA="" ;; esac
fi
NOW_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null)

# Preserve original sessionStart when the existing baseline is THIS session's
# (so baseline_age_seconds accumulates across the session); else start now.
SESSION_START="$ARG_START"
if [ -z "$SESSION_START" ] && [ -f "$BASELINE_FILE" ]; then
    if command -v jq >/dev/null 2>&1; then
        prev_sid=$(jq -r '.csi10Baseline.sessionId // ""' "$BASELINE_FILE" 2>/dev/null | tr -d '\r')
        prev_start=$(jq -r '.csi10Baseline.sessionStart // ""' "$BASELINE_FILE" 2>/dev/null | tr -d '\r')
        if [ "$prev_sid" = "$MY_SID" ] && [ -n "$prev_start" ]; then SESSION_START="$prev_start"; fi
    fi
fi
[ -n "$SESSION_START" ] || SESSION_START="$NOW_ISO"

# ---- Compose the csi10Baseline object --------------------------------------
B_OBJ="{\"sessionId\":\"$(esc "$MY_SID")\",\"sessionStart\":\"$(esc "$SESSION_START")\",\"capturedAt\":\"$(esc "$NOW_ISO")\",\"headSha\":\"$(esc "$HEAD_SHA")\",\"mtimes\":$MT_OBJ}"

mkdir -p "$(dirname "$BASELINE_FILE")" 2>/dev/null || exit 0
TMP="$BASELINE_FILE.csi10.tmp.$$"

# ---- Merge-write: preserve all other top-level keys ------------------------
if command -v python3 >/dev/null 2>&1 && python3 -c "import sys" >/dev/null 2>&1; then
    PYBIN="python3"
elif command -v python >/dev/null 2>&1 && python -c "import sys" >/dev/null 2>&1; then
    PYBIN="python"
else
    PYBIN=""
fi

if [ -n "$PYBIN" ]; then
    CSI10_B="$B_OBJ" CSI10_F="$BASELINE_FILE" CSI10_T="$TMP" "$PYBIN" - <<'PY' 2>/dev/null
import json, os, sys
f = os.environ["CSI10_F"]; t = os.environ["CSI10_T"]
b = json.loads(os.environ["CSI10_B"])
d = {}
try:
    with open(f, "r", encoding="utf-8-sig") as fh:
        d = json.load(fh)
    if not isinstance(d, dict):
        d = {}
except Exception:
    d = {}
d["csi10Baseline"] = b
try:
    with open(t, "w", encoding="utf-8") as fh:
        json.dump(d, fh, separators=(",", ":"))
    os.replace(t, f)
except Exception:
    try: os.remove(t)
    except Exception: pass
PY
elif command -v jq >/dev/null 2>&1; then
    if [ -f "$BASELINE_FILE" ]; then
        if jq --argjson b "$B_OBJ" '.csi10Baseline = $b' "$BASELINE_FILE" > "$TMP" 2>/dev/null; then
            mv -f "$TMP" "$BASELINE_FILE" 2>/dev/null || rm -f "$TMP" 2>/dev/null
        else
            rm -f "$TMP" 2>/dev/null
        fi
    else
        printf '{"csi10Baseline":%s}\n' "$B_OBJ" > "$TMP" 2>/dev/null && mv -f "$TMP" "$BASELINE_FILE" 2>/dev/null
        rm -f "$TMP" 2>/dev/null
    fi
else
    # No JSON parser: only safe to write when the file is absent (no key to
    # preserve). If it exists, skip rather than risk clobbering legacy fields.
    if [ ! -f "$BASELINE_FILE" ]; then
        printf '{"csi10Baseline":%s}\n' "$B_OBJ" > "$TMP" 2>/dev/null && mv -f "$TMP" "$BASELINE_FILE" 2>/dev/null
        rm -f "$TMP" 2>/dev/null
    fi
fi
exit 0
