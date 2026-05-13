#!/bin/bash
# archive-retention oracle (Unix)
# Answers: which archived PODS sessions exist, and which are old enough to sweep?
#
# Operates on .claude/collaboration/archived/collab-*/ directories.
# Default threshold: 90 days. KEEP file at <dir>/KEEP exempts the dir from sweep.
#
# Modes:
#   (default)    : list all collab-* dirs with metadata + sweepability flags
#   --gc-cheap   : session-start hygiene sweep, ~100ms budget, defers if exceeded
#   --gc         : on-demand hygiene sweep, no time budget, prints {swept,before,after,...}
#
# Sweep criteria (--gc / --gc-cheap):
#   basename matches /^collab-\d{8}-\d{6}$/ AND no KEEP file present
#   AND createdAt (parsed from basename) is older than (now - threshold).
#   Action: append JSON line to _summary.jsonl, verify write, rm -rf the dir.
#   Threshold: .claude/config.json archiveRetention.threshold (numeric days or PnD), default 90.
#   Design lineage: CSI-05 (claude-template/oracles/dispatchable-sessions/run.sh).

set -e

ARCHIVED_DIR=".claude/collaboration/archived"
SUMMARY_FILE="$ARCHIVED_DIR/_summary.jsonl"
EMPTY_OUTPUT='{"count":0,"dirs":[],"threshold":"P90D","thresholdSource":"default","summary":"No archived PODS sessions."}'

emit_empty_briefing() {
    echo "$EMPTY_OUTPUT"
    exit 0
}

# ---- Parse mode ----
MODE="briefing"
case "${1:-}" in
    --gc)        MODE="gc" ;;
    --gc-cheap)  MODE="gc-cheap" ;;
    "")          MODE="briefing" ;;
    *)
        echo "archive-retention: unknown mode: $1" >&2
        echo "  usage: run.sh [--gc|--gc-cheap]" >&2
        exit 1
        ;;
esac

# ---- Locate the archived dir; absent => graceful nothing-to-do ----
if [ ! -d "$ARCHIVED_DIR" ]; then
    if [ "$MODE" = "briefing" ]; then
        emit_empty_briefing
    elif [ "$MODE" = "gc" ]; then
        echo '{"swept":0,"before":0,"after":0,"threshold":"P90D","thresholdSource":"default","summarized":0,"note":"no archived dir"}'
        exit 0
    else
        # --gc-cheap is silent on graceful absence
        exit 0
    fi
fi

# ---- Pick a JSON parser. Prefer jq; fall back to python; else degrade. ----
# Probe python actually runs (Windows-Store python3 shim returns 0 from
# `command -v` but errors out with "install from Store" message on real use).
PARSER=""
if command -v jq >/dev/null 2>&1; then
    PARSER="jq"
else
    for _cand in python3 python; do
        if command -v "$_cand" >/dev/null 2>&1; then
            if "$_cand" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
                PARSER="$_cand"
                break
            fi
        fi
    done
fi

# ---- Threshold resolution -------------------------------------------------
# Accepts numeric days OR ISO-8601 PnD.
THRESHOLD_DAYS=90
THRESHOLD_SOURCE="default"
THRESHOLD_DISPLAY="P90D"

CONFIG=""
if [ -f ".claude/config.json" ]; then
    CONFIG=".claude/config.json"
fi

if [ -n "$CONFIG" ] && [ -n "$PARSER" ]; then
    if [ "$PARSER" = "jq" ]; then
        CFG_RAW=$(jq -r '.archiveRetention.threshold // empty' "$CONFIG" 2>/dev/null)
    else
        CFG_RAW=$("$PARSER" - "$CONFIG" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
v = (d.get("archiveRetention") or {}).get("threshold")
if v is not None:
    print(v)
PY
        )
    fi
    if [ -n "$CFG_RAW" ]; then
        CFG_RAW="${CFG_RAW%$'\r'}"
        case "$CFG_RAW" in
            ''|*[!0-9]*)
                # ISO-8601 duration: PnD only (PnH not allowed -- archive timescale is days)
                case "$CFG_RAW" in
                    P*D)
                        v="${CFG_RAW#P}"; v="${v%D}"
                        case "$v" in (''|*[!0-9]*) ;; *) THRESHOLD_DAYS="$v"; THRESHOLD_SOURCE="config"; THRESHOLD_DISPLAY="$CFG_RAW" ;; esac
                        ;;
                esac
                ;;
            *)
                THRESHOLD_DAYS="$CFG_RAW"
                THRESHOLD_SOURCE="config"
                THRESHOLD_DISPLAY="P${CFG_RAW}D"
                ;;
        esac
    fi
fi

NOW_EPOCH=$(date -u +%s)
CUTOFF_EPOCH=$((NOW_EPOCH - THRESHOLD_DAYS * 86400))
NOW_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# ---- Helpers --------------------------------------------------------------

# Parse "collab-YYYYMMDD-HHMMSS" basename into epoch seconds (UTC).
# Echo nothing on parse failure.
_basename_to_epoch() {
    local base="$1"
    case "$base" in
        collab-[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9])
            ;;
        *)
            return 0
            ;;
    esac
    local ymd="${base#collab-}"
    local hms="${ymd#*-}"
    ymd="${ymd%-*}"
    local iso="${ymd:0:4}-${ymd:4:2}-${ymd:6:2}T${hms:0:2}:${hms:2:2}:${hms:4:2}Z"
    # GNU date
    local epoch
    epoch=$(date -u -d "$iso" +%s 2>/dev/null) && [ -n "$epoch" ] && { echo "$epoch"; return 0; }
    # macOS BSD date
    epoch=$(date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$iso" +%s 2>/dev/null) && [ -n "$epoch" ] && { echo "$epoch"; return 0; }
    # python fallback
    if [ -n "$PARSER" ] && [ "$PARSER" != "jq" ]; then
        "$PARSER" -c "
import sys
from datetime import datetime, timezone
try:
    dt = datetime.strptime(sys.argv[1], '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
    print(int(dt.timestamp()))
except Exception:
    pass
" "$iso" 2>/dev/null
    fi
}

_epoch_to_iso() {
    local epoch="$1"
    [ -n "$epoch" ] || return 0
    date -u -d "@$epoch" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null && return 0
    date -u -r "$epoch" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null && return 0
    if [ -n "$PARSER" ] && [ "$PARSER" != "jq" ]; then
        "$PARSER" -c "
import sys
from datetime import datetime, timezone
print(datetime.fromtimestamp(int(sys.argv[1]), tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))
" "$epoch" 2>/dev/null
    fi
}

# Recursive byte count; null-tolerant; avoids du's filesystem-block rounding.
_dir_bytes() {
    local d="$1"
    if command -v du >/dev/null 2>&1; then
        # -sb is GNU; -k is portable. Prefer GNU bytes for accuracy.
        local out
        out=$(du -sb "$d" 2>/dev/null | awk '{print $1}')
        if [ -n "$out" ]; then echo "$out"; return 0; fi
        out=$(du -sk "$d" 2>/dev/null | awk '{print $1}')
        if [ -n "$out" ]; then echo $((out * 1024)); return 0; fi
    fi
    # Pure-find fallback
    find "$d" -type f -printf '%s\n' 2>/dev/null | awk '{s+=$1} END {print s+0}'
}

# Count files at top of subdir (e.g. workers/, tasks/).
_count_entries() {
    local d="$1"
    [ -d "$d" ] || { echo 0; return; }
    find "$d" -mindepth 1 -maxdepth 1 2>/dev/null | wc -l | tr -d ' '
}

# Extract first non-empty markdown heading from GUIDE.md (1-2 line scan).
_guide_headline() {
    local g="$1"
    [ -f "$g" ] || return 0
    awk '/^#/ && NF>=2 {sub(/^#+ */, ""); print; exit}' "$g" 2>/dev/null \
        | head -c 200
}

_critic_verdict() {
    local d="$1"
    local f="$d/CRITIC_VERDICT.md"
    [ -f "$f" ] || { return 0; }
    awk '
        /(^|[^A-Za-z])(PASS|CONCERN|VETO)([^A-Za-z]|$)/ {
            for (i=1;i<=NF;i++) {
                if ($i=="PASS"||$i=="CONCERN"||$i=="VETO") { print $i; exit }
            }
        }
    ' "$f" 2>/dev/null | head -1
}

# Append a JSON line atomically to $SUMMARY_FILE; verify by re-reading last line.
# Returns 0 on success, 1 on failure (caller MUST NOT delete the dir on failure).
_append_summary() {
    local line="$1"
    mkdir -p "$ARCHIVED_DIR"
    # Atomic append of <PIPE_BUF (~4KB on POSIX). Each line is well under that.
    if ! printf '%s\n' "$line" >> "$SUMMARY_FILE" 2>/dev/null; then
        return 1
    fi
    # Verify read-back: last line of file matches what we wrote.
    local last
    last=$(tail -n 1 "$SUMMARY_FILE" 2>/dev/null)
    [ "$last" = "$line" ]
}

# Build the JSON line for an archived dir (all fields per oracle.json schema).
_build_summary_line() {
    local dir="$1"
    local sweptAt="$2"
    local base
    base=$(basename "$dir")
    local createdAtEpoch createdAt ageDays
    createdAtEpoch=$(_basename_to_epoch "$base")
    if [ -n "$createdAtEpoch" ]; then
        createdAt=$(_epoch_to_iso "$createdAtEpoch")
        ageDays=$(( (NOW_EPOCH - createdAtEpoch) / 86400 ))
    else
        createdAt=""
        ageDays=""
    fi
    local sizeBytes workerCount taskCount criticVerdict hasOverrideVeto guideHeadline
    sizeBytes=$(_dir_bytes "$dir")
    workerCount=$(_count_entries "$dir/workers")
    taskCount=$(_count_entries "$dir/tasks")
    criticVerdict=$(_critic_verdict "$dir")
    if [ -f "$dir/OVERRIDE_VETO.md" ]; then
        hasOverrideVeto="true"
    else
        hasOverrideVeto="false"
    fi
    guideHeadline=$(_guide_headline "$dir/GUIDE.md")

    # JSON-escape strings (only need basic escaping: \\, \", control chars rare)
    _json_esc() {
        local s="$1"
        s="${s//\\/\\\\}"
        s="${s//\"/\\\"}"
        # Strip CR/LF
        s="${s//$'\r'/}"
        s="${s//$'\n'/ }"
        echo "$s"
    }

    local jHeadline jVerdict
    jHeadline=$(_json_esc "$guideHeadline")
    jVerdict=$(_json_esc "$criticVerdict")

    # Build object
    printf '{"sessionId":"%s","sweptAt":"%s","createdAt":%s,"ageDays":%s,"sizeBytes":%s,"workerCount":%s,"taskCount":%s,"criticVerdict":%s,"hasOverrideVeto":%s,"guideHeadline":%s}' \
        "$base" \
        "$sweptAt" \
        "$( [ -n "$createdAt" ] && printf '"%s"' "$createdAt" || printf 'null' )" \
        "$( [ -n "$ageDays" ] && printf '%s' "$ageDays" || printf 'null' )" \
        "$sizeBytes" \
        "$workerCount" \
        "$taskCount" \
        "$( [ -n "$jVerdict" ] && printf '"%s"' "$jVerdict" || printf 'null' )" \
        "$hasOverrideVeto" \
        "$( [ -n "$jHeadline" ] && printf '"%s"' "$jHeadline" || printf 'null' )"
}

_ms_now() {
    local ms
    ms=$(date -u +%s%3N 2>/dev/null)
    if [ -n "$ms" ] && [ "${ms#*N}" = "$ms" ]; then
        case "$ms" in (*[!0-9]*) ;; *) echo "$ms"; return 0 ;; esac
    fi
    if command -v perl >/dev/null 2>&1; then
        perl -MTime::HiRes=time -e 'printf("%d\n", time*1000)' 2>/dev/null && return 0
    fi
    if [ -n "$PARSER" ] && [ "$PARSER" != "jq" ]; then
        "$PARSER" -c 'import time; print(int(time.time()*1000))' 2>/dev/null
    fi
}

# =============================================================================
# Mode dispatch
# =============================================================================
if [ "$MODE" = "gc" ] || [ "$MODE" = "gc-cheap" ]; then
    # -------------------------------------------------------------------------
    # RETENTION-01..06 sweep
    # -------------------------------------------------------------------------
    BUDGET_MS=100
    if [ "$MODE" = "gc" ]; then BUDGET_MS=0; fi

    START_MS=$(_ms_now)

    BEFORE=0
    SWEEP_COUNT=0
    SUMMARIZED=0
    SWEEP_IDS=""
    BUDGET_EXCEEDED=""

    # Glob expansion. Count only dirs matching the strict collab-YYYYMMDD-HHMMSS pattern.
    shopt -s nullglob 2>/dev/null || true
    for dir in "$ARCHIVED_DIR"/collab-*; do
        [ -d "$dir" ] || continue
        b=$(basename "$dir")
        case "$b" in
            collab-[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9])
                BEFORE=$((BEFORE + 1))
                ;;
        esac
    done

    for dir in "$ARCHIVED_DIR"/collab-*; do
        [ -d "$dir" ] || continue

        if [ "$BUDGET_MS" -gt 0 ]; then
            NOW_MS=$(_ms_now)
            if [ -n "$START_MS" ] && [ -n "$NOW_MS" ] && [ "$((NOW_MS - START_MS))" -gt "$BUDGET_MS" ]; then
                BUDGET_EXCEEDED=1
                break
            fi
        fi

        local_base=$(basename "$dir")

        # Validate basename pattern (defensive — must match collab-YYYYMMDD-HHMMSS).
        case "$local_base" in
            collab-[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9])
                ;;
            *)
                continue
                ;;
        esac

        # KEEP file pin: skip
        [ -f "$dir/KEEP" ] && continue

        # Age check: createdAt must parse and be older than cutoff
        createdAtEpoch=$(_basename_to_epoch "$local_base")
        if [ -z "$createdAtEpoch" ]; then
            # Unparsable -- never sweep (failure-open per Probandurgy)
            continue
        fi
        if [ "$createdAtEpoch" -gt "$CUTOFF_EPOCH" ]; then
            continue
        fi

        # Build summary line BEFORE delete
        line=$(_build_summary_line "$dir" "$NOW_ISO")
        if ! _append_summary "$line"; then
            # Summary write failed -- DO NOT DELETE. Log and skip.
            echo "archive-retention: summary write failed for $local_base; dir preserved" >&2
            continue
        fi
        SUMMARIZED=$((SUMMARIZED + 1))

        # Delete
        if rm -rf "$dir" 2>/dev/null; then
            SWEEP_COUNT=$((SWEEP_COUNT + 1))
            if [ -z "$SWEEP_IDS" ]; then
                SWEEP_IDS="$local_base"
            else
                SWEEP_IDS="$SWEEP_IDS,$local_base"
            fi
        else
            echo "archive-retention: rm -rf failed for $local_base; summary line was already appended" >&2
        fi
    done

    AFTER=$((BEFORE - SWEEP_COUNT))

    if [ "$MODE" = "gc" ]; then
        printf '{"swept":%s,"before":%s,"after":%s,"threshold":"%s","thresholdSource":"%s","summarized":%s' \
            "$SWEEP_COUNT" "$BEFORE" "$AFTER" "$THRESHOLD_DISPLAY" "$THRESHOLD_SOURCE" "$SUMMARIZED"
        if [ -n "$BUDGET_EXCEEDED" ]; then
            printf ',"budgetExceeded":true'
        fi
        if [ -n "$SWEEP_IDS" ]; then
            printf ',"sweptIds":"%s"' "$SWEEP_IDS"
        fi
        printf '}\n'
    fi
    exit 0
fi

# =============================================================================
# Default mode: briefing path
# =============================================================================

# Iterate dirs and emit metadata + sweepability for each.
DIRS_JSON=""
COUNT=0
shopt -s nullglob 2>/dev/null || true
for dir in "$ARCHIVED_DIR"/collab-*; do
    [ -d "$dir" ] || continue
    base=$(basename "$dir")

    case "$base" in
        collab-[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9])
            ;;
        *)
            continue
            ;;
    esac

    createdAtEpoch=$(_basename_to_epoch "$base")
    if [ -n "$createdAtEpoch" ]; then
        createdAt=$(_epoch_to_iso "$createdAtEpoch")
        ageDays=$(( (NOW_EPOCH - createdAtEpoch) / 86400 ))
    else
        createdAt=""
        ageDays=""
    fi

    sizeBytes=$(_dir_bytes "$dir")
    if [ -f "$dir/KEEP" ]; then
        kept="true"
        sweepable="false"
        reason="kept"
    elif [ -z "$createdAtEpoch" ]; then
        kept="false"
        sweepable="false"
        reason="unparsable-age"
    elif [ "$createdAtEpoch" -gt "$CUTOFF_EPOCH" ]; then
        kept="false"
        sweepable="false"
        reason="too-young"
    else
        kept="false"
        sweepable="true"
        reason="sweepable"
    fi

    entry=$(printf '{"sessionId":"%s","createdAt":%s,"ageDays":%s,"sizeBytes":%s,"kept":%s,"sweepable":%s,"reason":"%s"}' \
        "$base" \
        "$( [ -n "$createdAt" ] && printf '"%s"' "$createdAt" || printf 'null' )" \
        "$( [ -n "$ageDays" ] && printf '%s' "$ageDays" || printf 'null' )" \
        "$sizeBytes" \
        "$kept" \
        "$sweepable" \
        "$reason")

    if [ -z "$DIRS_JSON" ]; then
        DIRS_JSON="$entry"
    else
        DIRS_JSON="$DIRS_JSON,$entry"
    fi
    COUNT=$((COUNT + 1))
done

if [ "$COUNT" -eq 0 ]; then
    emit_empty_briefing
fi

# Count sweepable and kept for the human-readable summary.
SWEEPABLE_COUNT=$(echo "$DIRS_JSON" | grep -o '"sweepable":true' | wc -l | tr -d ' ')
KEPT_COUNT=$(echo "$DIRS_JSON" | grep -o '"kept":true' | wc -l | tr -d ' ')

SUMMARY="$COUNT archived session(s); ${SWEEPABLE_COUNT} sweepable, ${KEPT_COUNT} kept (threshold $THRESHOLD_DISPLAY)"

printf '{"count":%s,"dirs":[%s],"threshold":"%s","thresholdSource":"%s","summary":"%s"}\n' \
    "$COUNT" "$DIRS_JSON" "$THRESHOLD_DISPLAY" "$THRESHOLD_SOURCE" "$SUMMARY"
