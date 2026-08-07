#!/bin/bash
# ldis-contexturgy oracle (Unix) -- CTXY-01 (scrutiny 03 ADD-1).
#
# Code-verified Contexturgy: did recent LDIS invocations in THIS project
# leave crystallized planning artifacts? The LDIS SKILLs mandate it in
# prose ("This step is MANDATORY. Do not skip it."); this oracle is the
# deterministic detector for the named primary anti-pattern (Ephemeral
# Planning). ADVISORY by design: status is pass|warn, NEVER fail -- a
# ledger/mtime false positive must not block a commit.
#
# Sources:
#   usage ledger  -- $CLAUDE_USAGE_LEDGER_DIR override, else
#                    ./claude-usage/*.jsonl (framework repo), else
#                    ~/.claude/command-usage/*.jsonl (deployed tracker home)
#   artifacts     -- docs/planning/{intakes,plans,ideations}/ + docs/specs/
#
# Contract: for each type:skill record of an LDIS skill in this project
# within the lookback window (default 24h; CLAUDE_CTXY_WINDOW_HOURS
# override), some file must exist in the skill's artifact home with
# mtime >= the invocation time:
#   0-uldf-ldis-intake -> docs/planning/intakes/
#   0-uldf-ldis-plan   -> docs/planning/plans/
#   0-uldf-ldis-ideate -> docs/planning/ideations/
#   0-uldf-ldis-spec   -> docs/specs/ (any *.md, recursive)
# No ledger / no matching records -> pass with a NO-DATA note (never a
# false all-clear claim: details.note says what was unavailable).
# Read-only; no JSON parser dependency (flat one-line records, sed-parsed).

set +e

WINDOW_HOURS="${CLAUDE_CTXY_WINDOW_HOURS:-24}"
NOW="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

emit() { # $1 status, $2 checked, $3 gaps-json, $4 note, $5 briefing
    printf '{"status":"%s","details":{"checked":%d,"window_hours":%s,"gaps":[%s],"note":"%s"},"briefing":"%s"}\n' \
        "$1" "$2" "$WINDOW_HOURS" "$3" "$(esc "$4")" "$(esc "$5")"
    exit 0
}

# ---- Project identity (ledger records carry the project dir name) -----------
PROJECT_NAME="$(basename "$(pwd)")"

# ---- Ledger resolution -------------------------------------------------------
LEDGER_DIR="${CLAUDE_USAGE_LEDGER_DIR:-}"
if [ -z "$LEDGER_DIR" ]; then
    if [ -d "claude-usage" ] && ls claude-usage/*.jsonl >/dev/null 2>&1; then
        LEDGER_DIR="claude-usage"
    elif [ -n "${HOME:-}" ] && ls "$HOME/.claude/command-usage/"*.jsonl >/dev/null 2>&1; then
        LEDGER_DIR="$HOME/.claude/command-usage"
    fi
fi
if [ -z "$LEDGER_DIR" ] || ! ls "$LEDGER_DIR"/*.jsonl >/dev/null 2>&1; then
    emit "pass" 0 "" "NO-DATA: no command-usage ledger found (tracker hook not installed?); Contexturgy not verifiable this session" ""
fi

# ---- Window start -- one per zone form (DEC-302 / LEDGER-ZONE-02) -----------
# The comment that stood here until DEC-302 declared this window to be in local
# time and claimed the ledger's `at` format agreed with it. It asserted a zone
# the ledger never guaranteed: the `.ps1` producer stamped LOCAL
# and the `.sh` producer stamped UTC, both unmarked, so this window was right on
# Windows and a full-offset error on any POSIX host -- and the sibling consumers
# (usage-stats, review-recency) carried the OPPOSITE assumption.
#
# From DEC-302 the field is UTC-with-`Z`; rows written before it are BARE and
# legacy-LOCAL, and one ledger file holds both across the transition. So compute
# the window start ONCE in each form and let the filter pick per row by the row's
# own marker -- exact for both forms, and no arithmetic inside awk.
WINDOW_START_LOCAL="$(date -d "$WINDOW_HOURS hours ago" +%Y-%m-%dT%H:%M:%S 2>/dev/null)"
WINDOW_START_UTC="$(date -u -d "$WINDOW_HOURS hours ago" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null)"
if [ -z "$WINDOW_START_LOCAL" ]; then
    # BSD date fallback; degrade to pass rather than misreport.
    WINDOW_START_LOCAL="$(date -v-"$WINDOW_HOURS"H +%Y-%m-%dT%H:%M:%S 2>/dev/null)"
    WINDOW_START_UTC="$(date -u -v-"$WINDOW_HOURS"H +%Y-%m-%dT%H:%M:%SZ 2>/dev/null)"
fi
[ -n "$WINDOW_START_LOCAL" ] && [ -n "$WINDOW_START_UTC" ] \
    || emit "pass" 0 "" "NO-DATA: could not compute lookback window on this platform" ""

# ---- Scan ledger for LDIS records in-window for this project -----------------
# Flat one-line records: sed-extract at/cmd/project. Lexical ISO compare.
artifact_home() {
    case "$1" in
        *ldis-intake) printf 'docs/planning/intakes' ;;
        *ldis-plan)   printf 'docs/planning/plans' ;;
        *ldis-ideate) printf 'docs/planning/ideations' ;;
        *ldis-spec)   printf 'docs/specs' ;;
    esac
}

CHECKED=0
GAPS_JSON=""
GAP_COUNT=0

# One awk pass per ledger file (per-line sed forks cost ~30s on Git Bash
# against a multi-thousand-record ledger; awk does the whole filter in-process).
# Emits "cmd\tat" for in-window LDIS records of this project only.
_ldis_rows() {
    awk -v proj="$PROJECT_NAME" -v wlocal="$WINDOW_START_LOCAL" -v wutc="$WINDOW_START_UTC" '
        function val(s, key,   r) {
            if (match(s, "\"" key "\"[ \t]*:[ \t]*\"[^\"]*\"")) {
                r = substr(s, RSTART, RLENGTH)
                sub(/^[^:]*:[ \t]*"/, "", r)
                sub(/"$/, "", r)
                return r
            }
            return ""
        }
        /ldis-/ {
            c = val($0, "cmd")
            if (c !~ /ldis-(intake|plan|ideate|spec)$/) next
            if (val($0, "project") != proj) next
            a = val($0, "at")
            if (a == "") next
            # Pick the window in the OWN zone form of the row (DEC-302): a
            # trailing Z means UTC as written; bare means legacy LOCAL. Both
            # compares are lexical between two values of identical shape.
            if (a ~ /Z$/) { if (a < wutc) next } else { if (a < wlocal) next }
            print c "\t" a
        }
    ' "$1" 2>/dev/null
}

for f in "$LEDGER_DIR"/*.jsonl; do
    [ -f "$f" ] || continue
    while IFS=$'\t' read -r _cmd _at; do
        [ -n "$_cmd" ] || continue
        CHECKED=$((CHECKED + 1))
        _home="$(artifact_home "$_cmd")"
        _found=""
        if [ -d "$_home" ]; then
            # Any file in the home with mtime >= invocation time. find -newermt
            # takes the ISO stamp directly (GNU findutils; Git Bash included).
            #
            # ZONE (DEC-302 / LEDGER-ZONE-02) -- this is the SECOND zone coupling
            # in this oracle and the one that decides the verdict, not just a
            # window edge. It needs no conversion, which is worth stating because
            # it looks like it should: `tr T ' '` preserves a trailing Z, and GNU
            # find honours it as UTC, while a BARE stamp is parsed as LOCAL --
            # which is exactly right for a legacy row. Measured both ways
            # 2026-08-06 (see the receipt). Do not "normalize" the stamp here:
            # stripping the Z would silently reintroduce a full-offset error
            # against file mtimes, which are local.
            if [ "$_home" = "docs/specs" ]; then
                _found="$(find "$_home" -name '*.md' -newermt "$(printf '%s' "$_at" | tr 'T' ' ')" -print -quit 2>/dev/null)"
            else
                _found="$(find "$_home" -type f -newermt "$(printf '%s' "$_at" | tr 'T' ' ')" -print -quit 2>/dev/null)"
            fi
        fi
        if [ -z "$_found" ]; then
            _entry="{\"skill\":\"$(esc "$_cmd")\",\"at\":\"$(esc "$_at")\",\"expected\":\"$(esc "$_home")\"}"
            if [ "$GAP_COUNT" -eq 0 ]; then GAPS_JSON="$_entry"; else GAPS_JSON="$GAPS_JSON,$_entry"; fi
            GAP_COUNT=$((GAP_COUNT + 1))
        fi
    done <<EOF
$(_ldis_rows "$f")
EOF
done

if [ "$GAP_COUNT" -gt 0 ]; then
    emit "warn" "$CHECKED" "$GAPS_JSON" "" "ldis-contexturgy: $GAP_COUNT LDIS invocation(s) in the last ${WINDOW_HOURS}h left no crystallized artifact (Ephemeral Planning?) -- advisory, crystallize before finalize"
fi
if [ "$CHECKED" -eq 0 ]; then
    emit "pass" 0 "" "no LDIS invocations recorded for this project in the last ${WINDOW_HOURS}h" ""
fi
emit "pass" "$CHECKED" "" "" ""
