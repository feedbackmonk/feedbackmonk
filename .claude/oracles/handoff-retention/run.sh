#!/bin/bash
# handoff-retention oracle (Unix)
# Answers: which handoff briefs are older than the configured TTL, and which are KEEP-pinned?
#
# Operates on .claude/handoff/handoff-*.md files.
# Default threshold: 30 days (DEC-54). Sibling <file>.KEEP exempts indefinitely (HANDOFF-01).
#
# Modes:
#   (default)    : full inventory JSON with `briefing` field
#   --gc-cheap   : session-start AUTO-SWEEP (DEC-79) + sweep-report JSON with
#                  `briefing` field (empty when nothing swept -> silent line);
#                  ~100ms budget, defers remainder if exceeded
#   --gc         : destructive sweep + JSONL audit; emits summary JSON
#
# Sweep criteria (--gc / --gc-cheap):
#   filename matches /^handoff-.+\.md$/ AND no sibling <file>.KEEP present
#   AND mtime is older than (now - threshold).
#   Action: append JSON line to _summary.jsonl, verify write, rm the file (SWEEP-08).
#   Threshold: .claude/config.json handoffRetention.threshold (numeric days or PnD), default 30.
#
# DEC-79 (2026-06-12): --gc-cheap performs the sweep instead of nagging. The
# Arc 2.5 retrospective showed the nag-only briefing line fired every session
# for ~a month and was never acted on once; the safety rails (SWEEP-08
# pre-delete audit + KEEP pins) are proven by archive-retention's clean
# audited sweeps. The briefing line is now a report of what was swept and is
# SILENT when nothing was swept.
#
# Spec: SPECIFICATION.md § SWEEP-01 (as amended by DEC-79), SWEEP-07, SWEEP-08; DEC-52, DEC-54, DEC-79
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
    --dry-run)   MODE="dry-run" ;;
    "")          MODE="briefing" ;;
    *)
        echo "handoff-retention: unknown mode: $1" >&2
        echo "  usage: run.sh [--gc|--gc-cheap|--dry-run]" >&2
        exit 1
        ;;
esac

# ---- OLOG fire record (ORA-FIRE-01, DEC-166 / DISC-ORA-02) ------------------
# Actuator leg: fires as a session-start --gc-cheap sweep, silent on success. Without
# this record the oracle is invisible to oracle-consultations.jsonl and DEC-82's
# retirement criterion scores it as unused no matter how hard it works. A fire is not
# a consultation -- it carries via:"sweep". Append-failure is swallowed.
case "$MODE" in
  gc|gc-cheap)
    _OFL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    for _ofl_cand in \
        "$_OFL_DIR/../../scripts/lib/oracle-fire-log.sh" \
        "$_OFL_DIR/../../../claude-template/scripts/lib/oracle-fire-log.sh" \
        "$HOME/.claude/scripts/lib/oracle-fire-log.sh"; do
        if [ -f "$_ofl_cand" ]; then
            # shellcheck source=/dev/null
            . "$_ofl_cand" && oracle_fire_log "handoff-retention" "sweep"
            break
        fi
    done
    ;;
esac

# ---- Locate handoff dir; absent => graceful nothing-to-do ----
if [ ! -d "$HANDOFF_DIR" ]; then
    if [ "$MODE" = "briefing" ]; then
        emit_empty_briefing
    elif [ "$MODE" = "gc" ] || [ "$MODE" = "dry-run" ]; then
        echo '{"swept":0,"before":0,"after":0,"threshold":"P30D","thresholdSource":"default","summarized":0,"note":"no handoff dir"}'
        exit 0
    else
        # --gc-cheap: nothing to sweep -> empty briefing (silent line)
        echo '{"swept":0,"before":0,"after":0,"threshold":"P30D","thresholdSource":"default","summarized":0,"briefing":""}'
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
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
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
# --gc-cheap (DEC-79): falls through to the sweep block below alongside --gc,
# with a ~100ms time budget and a sweep-report briefing field.

# Millisecond clock for the --gc-cheap budget (mirrors archive-retention).
# Empty result disables the budget check (sweep set is TTL-bounded anyway).
_ms_now() {
    # Zero-fork fast path (DEFER-064 rule 2, propagated by DEC-250): a forked
    # clock costs ~300-500ms per call on a busy MSYS host, so the budget
    # measurement consumes the budget it is measuring.
    if [ -n "${EPOCHREALTIME:-}" ]; then
        local _er_s="${EPOCHREALTIME%%[.,]*}" _er_f="${EPOCHREALTIME#*[.,]}"
        case "$_er_s$_er_f" in (*[!0-9]*) : ;; *)
            _er_f="${_er_f}000"
            echo "$(( _er_s * 1000 + 10#${_er_f:0:3} ))"
            return 0
            ;;
        esac
    fi
    local ms
    ms=$(date +%s%3N 2>/dev/null)
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
# --gc / --gc-cheap: actually sweep stale candidates (DEC-79: cheap mode
#            sweeps too, under a ~100ms budget, and reports via `briefing`)
# --dry-run: identical candidate computation, ZERO mutation (SWEEPER-05) --
#            no JSONL append, no rm; reports the would-sweep set.
# =============================================================================
if [ "$MODE" = "gc" ] || [ "$MODE" = "gc-cheap" ] || [ "$MODE" = "dry-run" ]; then
    SWEEP_COUNT=0
    SUMMARIZED=0
    SWEPT_FILES=""
    BUDGET_EXCEEDED=""

    # Budget calibrated for MSYS/Git-Bash, where each per-file sweep costs
    # ~0.5s in subprocess spawns (measured 7.3s for 14 files, 2026-06-12).
    # 1000ms sweeps 50+ files on real Unix, ~2 per session on MSYS; the
    # deferred remainder drains across sessions and is reported honestly
    # via budgetExceeded + the briefing line.
    BUDGET_MS=0
    if [ "$MODE" = "gc-cheap" ]; then BUDGET_MS=1000; fi
    # ULDF_HR_GC_BUDGET_MS: test seam (DEC-250), mirroring ULDF_DS_GC_BUDGET_MS.
    case "${ULDF_HR_GC_BUDGET_MS:-}" in
        ''|*[!0-9]*) : ;;
        *) [ "$MODE" = "gc-cheap" ] && BUDGET_MS="$ULDF_HR_GC_BUDGET_MS" ;;
    esac
    START_MS=$(_ms_now)
    WORKED_ANY=""   # minimum-progress guarantee (DEFER-064 rule 3)

    for file in "$HANDOFF_DIR"/handoff-*.md; do
        [ -f "$file" ] || continue

        # --gc-cheap budget: defer the remainder to the next session-start
        # rather than stalling the briefing (mirrors archive-retention) -- but
        # NEVER before one unit of real work (DEFER-064 rule 3, DEC-250). The
        # cheap filters below (KEEP pin, unreadable mtime, too-young file) are
        # not progress, so a prefix of them must not starve the sweep.
        if [ "$BUDGET_MS" -gt 0 ] && [ -n "$WORKED_ANY" ]; then
            NOW_MS=$(_ms_now)
            if [ -n "$START_MS" ] && [ -n "$NOW_MS" ] && [ "$((NOW_MS - START_MS))" -gt "$BUDGET_MS" ]; then
                BUDGET_EXCEEDED=1
                break
            fi
        fi

        # KEEP-pin
        [ -f "$file.KEEP" ] && continue

        # mtime check
        mtime=$(_file_mtime "$file")
        [ -n "$mtime" ] || continue
        [ "$mtime" -gt "$CUTOFF_EPOCH" ] && continue

        age_days=$(( (NOW_EPOCH - mtime) / 86400 ))

        # Past every cheap filter: this file IS a sweep candidate, so from here
        # on the invocation has done real work and the budget may bind again.
        WORKED_ANY=1

        if [ "$MODE" = "dry-run" ]; then
            SWEEP_COUNT=$((SWEEP_COUNT + 1))
            base=$(basename "$file")
            if [ -z "$SWEPT_FILES" ]; then
                SWEPT_FILES="$base"
            else
                SWEPT_FILES="$SWEPT_FILES,$base"
            fi
            continue
        fi

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

    if [ "$MODE" = "dry-run" ]; then
        printf '{"dryRun":true,"wouldSweep":%s,"before":%s,"after":%s,"threshold":"%s","thresholdSource":"%s"' \
            "$SWEEP_COUNT" "$BEFORE_FILES" "$BEFORE_FILES" "$THRESHOLD_DISPLAY" "$THRESHOLD_SOURCE"
        if [ -n "$SWEPT_FILES" ]; then
            jSwept=$(_json_esc "$SWEPT_FILES")
            printf ',"wouldSweepFiles":"%s"' "$jSwept"
        fi
        printf '}\n'
        exit 0
    fi

    AFTER_FILES=$((BEFORE_FILES - SWEEP_COUNT))

    printf '{"swept":%s,"before":%s,"after":%s,"threshold":"%s","thresholdSource":"%s","summarized":%s' \
        "$SWEEP_COUNT" "$BEFORE_FILES" "$AFTER_FILES" "$THRESHOLD_DISPLAY" "$THRESHOLD_SOURCE" "$SUMMARIZED"
    if [ -n "$BUDGET_EXCEEDED" ]; then
        printf ',"budgetExceeded":true'
    fi
    if [ -n "$SWEPT_FILES" ]; then
        jSwept=$(_json_esc "$SWEPT_FILES")
        printf ',"sweptFiles":"%s"' "$jSwept"
    fi
    if [ "$MODE" = "gc-cheap" ]; then
        # DEC-79 sweep-report briefing: silent (empty) when nothing swept.
        BRIEFING=""
        if [ "$SWEEP_COUNT" -gt 0 ]; then
            noun="briefs"
            [ "$SWEEP_COUNT" -eq 1 ] && noun="brief"
            BRIEFING="swept $SWEEP_COUNT expired handoff $noun (>${THRESHOLD_DAYS}d, KEEP-pinned exempt) -- audit: $SUMMARY_FILE"
            if [ -n "$BUDGET_EXCEEDED" ]; then
                BRIEFING="$BRIEFING; budget hit, remainder next session"
            fi
        fi
        printf ',"briefing":"%s"' "$(_json_esc "$BRIEFING")"
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
