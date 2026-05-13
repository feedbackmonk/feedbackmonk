#!/bin/bash
# handoff-retention oracle (Unix)
# Answers: which handoff briefs are older than the configured TTL, and which are KEEP-pinned?
#
# Operates on .claude/handoff/handoff-*.md files.
# Default threshold: 30 days (DEC-54). Sibling <file>.KEEP exempts indefinitely (HANDOFF-01).
#
# Modes:
#   (default)    : full inventory JSON with `briefing` field
#   --gc-cheap   : silent (read-only per SWEEP-01); never blocks briefing
#   --gc         : destructive sweep + JSONL audit; emits summary JSON
#
# Sweep criteria (--gc):
#   filename matches /^handoff-.+\.md$/ AND no sibling <file>.KEEP present
#   AND mtime is older than (now - threshold).
#   Action: append JSON line to _summary.jsonl, verify write, rm the file (SWEEP-08).
#   Threshold: .claude/config.json handoffRetention.threshold (numeric days or PnD), default 30.
#
# Spec: SPECIFICATION.md § SWEEP-01, SWEEP-07, SWEEP-08; DEC-52, DEC-54
# Substrate: claude-template/oracles/archive-retention/ (RETENTION-01..06)

set -e

HANDOFF_DIR=".claude/handoff"
SUMMARY_FILE="$HANDOFF_DIR/_summary.jsonl"
EMPTY_OUTPUT='{"swept":[],"retained_keep_pinned":[],"retained_under_ttl":[],"threshold_days":30,"threshold_source":"default","briefing":""}'

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
        echo "handoff-retention: unknown mode: $1" >&2
        echo "  usage: run.sh [--gc|--gc-cheap]" >&2
        exit 1
        ;;
esac

# ---- Locate handoff dir; absent => graceful nothing-to-do ----
if [ ! -d "$HANDOFF_DIR" ]; then
    if [ "$MODE" = "briefing" ]; then
        emit_empty_briefing
    elif [ "$MODE" = "gc" ]; then
        echo '{"swept":0,"before":0,"after":0,"threshold":"P30D","thresholdSource":"default","summarized":0,"note":"no handoff dir"}'
        exit 0
    else
        # --gc-cheap is silent on graceful absence
        exit 0
    fi
fi

# ---- Pick a JSON parser (probe-verify python; Microsoft Store stub on Windows
# returns 0 from `command -v` but errors with "install from Store" on real use).
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
THRESHOLD_DAYS=30
THRESHOLD_SOURCE="default"
THRESHOLD_DISPLAY="P30D"

CONFIG=""
if [ -f ".claude/config.json" ]; then
    CONFIG=".claude/config.json"
fi

if [ -n "$CONFIG" ] && [ -n "$PARSER" ]; then
    if [ "$PARSER" = "jq" ]; then
        # `|| true` defangs malformed-JSON failures so set -e does not abort.
        CFG_RAW=$(jq -r '.handoffRetention.threshold // empty' "$CONFIG" 2>/dev/null || true)
    else
        CFG_RAW=$("$PARSER" - "$CONFIG" <<'PY' 2>/dev/null || true
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
v = (d.get("handoffRetention") or {}).get("threshold")
if v is not None:
    print(v)
PY
        )
    fi
    if [ -n "$CFG_RAW" ]; then
        CFG_RAW="${CFG_RAW%$'\r'}"
        case "$CFG_RAW" in
            ''|*[!0-9]*)
                # ISO-8601 duration: PnD only (PnH not allowed -- handoff timescale is days)
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

# File mtime as epoch seconds. GNU stat (-c %Y) preferred; macOS BSD stat (-f %m) fallback.
_file_mtime() {
    local f="$1"
    local m
    m=$(stat -c %Y "$f" 2>/dev/null) && [ -n "$m" ] && { echo "$m"; return 0; }
    m=$(stat -f %m "$f" 2>/dev/null) && [ -n "$m" ] && { echo "$m"; return 0; }
    # Python fallback
    if [ -n "$PARSER" ] && [ "$PARSER" != "jq" ]; then
        "$PARSER" -c "import os, sys; print(int(os.path.getmtime(sys.argv[1])))" "$f" 2>/dev/null
    fi
}

# Read first non-empty line of a file (cap at 200 chars after trim).
_brief_first_line() {
    local f="$1"
    [ -f "$f" ] || return 0
    awk 'NF { sub(/^[[:space:]]+/, ""); sub(/[[:space:]]+$/, ""); print; exit }' "$f" 2>/dev/null \
        | head -c 200
}

# JSON-escape a string. Caller wraps in quotes or emits null.
_json_esc() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\r'/}"
    s="${s//$'\n'/ }"
    s="${s//$'\t'/ }"
    printf '%s' "$s"
}

# Append a JSON line atomically to $SUMMARY_FILE; verify by re-reading last line.
# Returns 0 on success, 1 on failure (caller MUST NOT delete the file on failure).
_append_summary() {
    local line="$1"
    mkdir -p "$HANDOFF_DIR"
    if ! printf '%s\n' "$line" >> "$SUMMARY_FILE" 2>/dev/null; then
        return 1
    fi
    local last
    last=$(tail -n 1 "$SUMMARY_FILE" 2>/dev/null)
    [ "$last" = "$line" ]
}

# Build the JSON line for a sweep entry per SWEEP-08 schema.
#   $1 = file path (relative)
#   $2 = swept_at (ISO-8601)
#   $3 = age_days
_build_summary_line() {
    local file="$1"
    local sweptAt="$2"
    local ageDays="$3"
    local firstLine
    firstLine=$(_brief_first_line "$file")
    local jFile jFirstLine
    jFile=$(_json_esc "$file")
    jFirstLine=$(_json_esc "$firstLine")
    printf '{"file":"%s","swept_at":"%s","age_days":%s,"brief_first_line":%s}' \
        "$jFile" \
        "$sweptAt" \
        "$ageDays" \
        "$( [ -n "$jFirstLine" ] && printf '"%s"' "$jFirstLine" || printf 'null' )"
}

# =============================================================================
# Mode dispatch
# =============================================================================

# --gc-cheap: silent no-op per SWEEP-01 (briefing only via default-mode iteration).
# Exists for symmetry with archive-retention's hook wiring; not destructive.
if [ "$MODE" = "gc-cheap" ]; then
    exit 0
fi

# Collect handoff files matching pattern (glob expansion in alphabetical order).
shopt -s nullglob 2>/dev/null || true

# Build classification arrays.
SWEPT_JSON=""
KEEP_PINNED_JSON=""
UNDER_TTL_JSON=""
COUNT_STALE=0
COUNT_PINNED=0
COUNT_UNDER=0
BEFORE_FILES=0

for file in "$HANDOFF_DIR"/handoff-*.md; do
    [ -f "$file" ] || continue
    BEFORE_FILES=$((BEFORE_FILES + 1))

    # KEEP-pin check (sibling file)
    if [ -f "$file.KEEP" ]; then
        local_jpath=$(_json_esc "$file")
        if [ -z "$KEEP_PINNED_JSON" ]; then
            KEEP_PINNED_JSON="\"$local_jpath\""
        else
            KEEP_PINNED_JSON="$KEEP_PINNED_JSON,\"$local_jpath\""
        fi
        COUNT_PINNED=$((COUNT_PINNED + 1))
        continue
    fi

    # Age check via mtime
    mtime=$(_file_mtime "$file")
    if [ -z "$mtime" ]; then
        # Failure-open: cannot read mtime -> skip (treat as recent)
        continue
    fi
    age_days=$(( (NOW_EPOCH - mtime) / 86400 ))

    if [ "$mtime" -gt "$CUTOFF_EPOCH" ]; then
        # under TTL
        local_jpath=$(_json_esc "$file")
        entry=$(printf '{"file":"%s","age_days":%s}' "$local_jpath" "$age_days")
        if [ -z "$UNDER_TTL_JSON" ]; then
            UNDER_TTL_JSON="$entry"
        else
            UNDER_TTL_JSON="$UNDER_TTL_JSON,$entry"
        fi
        COUNT_UNDER=$((COUNT_UNDER + 1))
    else
        # stale candidate
        firstLine=$(_brief_first_line "$file")
        jFile=$(_json_esc "$file")
        jFirstLine=$(_json_esc "$firstLine")
        entry=$(printf '{"file":"%s","swept_at":null,"age_days":%s,"brief_first_line":%s}' \
            "$jFile" \
            "$age_days" \
            "$( [ -n "$jFirstLine" ] && printf '"%s"' "$jFirstLine" || printf 'null' )")
        if [ -z "$SWEPT_JSON" ]; then
            SWEPT_JSON="$entry"
        else
            SWEPT_JSON="$SWEPT_JSON,$entry"
        fi
        COUNT_STALE=$((COUNT_STALE + 1))
    fi
done

# =============================================================================
# --gc: actually sweep stale candidates
# =============================================================================
if [ "$MODE" = "gc" ]; then
    SWEEP_COUNT=0
    SUMMARIZED=0
    SWEPT_FILES=""

    for file in "$HANDOFF_DIR"/handoff-*.md; do
        [ -f "$file" ] || continue

        # KEEP-pin
        [ -f "$file.KEEP" ] && continue

        # mtime check
        mtime=$(_file_mtime "$file")
        [ -n "$mtime" ] || continue
        [ "$mtime" -gt "$CUTOFF_EPOCH" ] && continue

        age_days=$(( (NOW_EPOCH - mtime) / 86400 ))

        # Build summary line BEFORE delete (SWEEP-08 invariant)
        line=$(_build_summary_line "$file" "$NOW_ISO" "$age_days")
        if ! _append_summary "$line"; then
            echo "handoff-retention: summary write failed for $file; preserved" >&2
            continue
        fi
        SUMMARIZED=$((SUMMARIZED + 1))

        # Delete
        if rm -f "$file" 2>/dev/null; then
            SWEEP_COUNT=$((SWEEP_COUNT + 1))
            base=$(basename "$file")
            if [ -z "$SWEPT_FILES" ]; then
                SWEPT_FILES="$base"
            else
                SWEPT_FILES="$SWEPT_FILES,$base"
            fi
        else
            echo "handoff-retention: rm failed for $file; summary line was already appended" >&2
        fi
    done

    AFTER_FILES=$((BEFORE_FILES - SWEEP_COUNT))

    printf '{"swept":%s,"before":%s,"after":%s,"threshold":"%s","thresholdSource":"%s","summarized":%s' \
        "$SWEEP_COUNT" "$BEFORE_FILES" "$AFTER_FILES" "$THRESHOLD_DISPLAY" "$THRESHOLD_SOURCE" "$SUMMARIZED"
    if [ -n "$SWEPT_FILES" ]; then
        jSwept=$(_json_esc "$SWEPT_FILES")
        printf ',"sweptFiles":"%s"' "$jSwept"
    fi
    printf '}\n'
    exit 0
fi

# =============================================================================
# Default mode: emit full inventory JSON with briefing field
# =============================================================================

if [ "$BEFORE_FILES" -eq 0 ]; then
    emit_empty_briefing
fi

# Compose briefing line
if [ "$COUNT_STALE" -eq 0 ]; then
    BRIEFING=""
else
    if [ "$COUNT_STALE" -eq 1 ]; then
        BRIEFING="$COUNT_STALE brief older than ${THRESHOLD_DAYS}d, run /0-uldf-oracle handoff-retention --gc to sweep"
    else
        BRIEFING="$COUNT_STALE briefs older than ${THRESHOLD_DAYS}d, run /0-uldf-oracle handoff-retention --gc to sweep"
    fi
fi

jBriefing=$(_json_esc "$BRIEFING")

printf '{"swept":[%s],"retained_keep_pinned":[%s],"retained_under_ttl":[%s],"threshold_days":%s,"threshold_source":"%s","briefing":"%s"}\n' \
    "$SWEPT_JSON" \
    "$KEEP_PINNED_JSON" \
    "$UNDER_TTL_JSON" \
    "$THRESHOLD_DAYS" \
    "$THRESHOLD_SOURCE" \
    "$jBriefing"
