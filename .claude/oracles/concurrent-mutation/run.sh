#!/bin/bash
# concurrent-mutation oracle (Unix) -- CSI-10, CSI Phase 3.
#
# Verification Oracle (kind: "verification", ORACULURGY_DESIGN.md Part 11).
# Question: have LTADS lifecycle files or ltads/execution/<arc>/ been
# externally mutated since this session's last known-good snapshot?
#
# READ-ONLY by contract (Part 11 sec 11.3.4): this script NEVER writes. The
# per-session baseline it compares against is persisted by the sibling
# update-baseline.sh (wired into the session-start hook + /0-uldf-finalize).
# Splitting the write out keeps run.sh inside the read-only-oracle invariant.
#
# Hybrid baseline (DEC-22):
#   - mtime delta  : per-file last-observed-mtime in
#                    .claude/session-state/this-session.json (csi10Baseline.mtimes)
#                    vs current file mtime -> uncommitted concurrent writes.
#   - git-log delta: git diff over csi10Baseline.headSha..HEAD on the tracked
#                    paths -> committed external mutations.
# Either signal firing reports external_mutation:true.
#
# Output: single-line JSON, CSI-10 domain schema:
#   {external_mutation, mutations:[{path,source,since,by_session}],
#    baseline_age_seconds, summary, briefing}
# The briefing field is empty when external_mutation=false (the session-start
# hook then emits no line -- gracefully absent, same convention as
# stale-ltads-state). Failure-open: any unparseable/absent baseline ->
# external_mutation:false, baseline_age_seconds:null, log + continue.

set +e

BASELINE_FILE=".claude/session-state/this-session.json"
TRACKED_FILES="ltads/arc-state.json ltads/arc-state.archive.json ltads/sessions/session-history.md ltads/sessions/blockers.md"
TRACKED_DIRS="ltads/execution"
MAX_MUT=50

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

emit_absent() {
    # external_mutation false, baseline_age_seconds null (cannot deduce).
    printf '{"external_mutation":false,"mutations":[],"baseline_age_seconds":null,"summary":"%s","briefing":""}\n' "$(esc "$1")"
    exit 0
}

# ---- Portability helpers ----------------------------------------------------
# epoch mtime of a file (GNU stat -c, then BSD stat -f).
file_mtime() {
    local f="$1" m
    m=$(stat -c %Y "$f" 2>/dev/null)
    case "$m" in (''|*[!0-9]*) m=$(stat -f %m "$f" 2>/dev/null) ;; esac
    case "$m" in (''|*[!0-9]*) m="" ;; esac
    printf '%s' "$m"
}

# ISO-8601 (UTC, ...Z) -> epoch seconds (GNU date -d, then BSD date -j).
iso_to_epoch() {
    local iso="$1" e
    e=$(date -u -d "$iso" +%s 2>/dev/null)
    case "$e" in (''|*[!0-9]*) e=$(date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$iso" +%s 2>/dev/null) ;; esac
    case "$e" in (''|*[!0-9]*) e="" ;; esac
    printf '%s' "$e"
}

# epoch seconds -> ISO-8601 UTC (GNU date -d @, then BSD date -r).
epoch_to_iso() {
    local e="$1" s
    s=$(date -u -d "@$e" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null)
    case "$s" in (''|*N) s=$(date -u -r "$e" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null) ;; esac
    printf '%s' "$s"
}

# ---- Resolve own sessionId (env-first, DEC-71) via the CSI-16 lib -----------
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
MY_SID=""
if command -v csi_identity_resolve >/dev/null 2>&1; then
    MY_SID=$(csi_identity_resolve "$(pwd)" "$PPID" | cut -d'|' -f1)
fi
[ -n "$MY_SID" ] || MY_SID="${CLAUDE_SESSION_ID:-}"

# ---- Graceful absence: no baseline file ------------------------------------
if [ ! -f "$BASELINE_FILE" ]; then
    emit_absent "no baseline (this-session.json absent)"
fi

# ---- Pick a JSON parser (jq preferred, python fallback) --------------------
PARSER=""
if command -v jq >/dev/null 2>&1; then
    PARSER="jq"
elif command -v python3 >/dev/null 2>&1 && python3 -c "import sys" >/dev/null 2>&1; then
    PARSER="python3"
elif command -v python >/dev/null 2>&1 && python -c "import sys" >/dev/null 2>&1; then
    PARSER="python"
fi
[ -n "$PARSER" ] || emit_absent "no json parser (jq/python) for baseline"

# ---- Read csi10Baseline scalar fields + mtimes map -------------------------
B_SID=""; B_START=""; B_HEAD=""; MTIMES=""
if [ "$PARSER" = "jq" ]; then
    # Native Windows jq emits CRLF; strip CR or numeric/sha compares break
    # (the native-jq CRLF class guarded by the SHARED-CSI-07 smoke).
    B_SID=$(jq -r '.csi10Baseline.sessionId // ""' "$BASELINE_FILE" 2>/dev/null | tr -d '\r')
    B_START=$(jq -r '.csi10Baseline.sessionStart // ""' "$BASELINE_FILE" 2>/dev/null | tr -d '\r')
    B_HEAD=$(jq -r '.csi10Baseline.headSha // ""' "$BASELINE_FILE" 2>/dev/null | tr -d '\r')
    MTIMES=$(jq -r '.csi10Baseline.mtimes // {} | to_entries[] | "\(.key)\t\(.value)"' "$BASELINE_FILE" 2>/dev/null | tr -d '\r')
else
    OUT=$("$PARSER" - "$BASELINE_FILE" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
b = d.get("csi10Baseline") if isinstance(d, dict) else None
if not isinstance(b, dict):
    sys.exit(0)
print("SID\t%s" % (b.get("sessionId") or ""))
print("START\t%s" % (b.get("sessionStart") or ""))
print("HEAD\t%s" % (b.get("headSha") or ""))
mt = b.get("mtimes")
if isinstance(mt, dict):
    for k, v in mt.items():
        print("MT\t%s\t%s" % (k, v))
PY
)
    B_SID=$(printf '%s\n' "$OUT" | awk -F'\t' '$1=="SID"{print $2; exit}')
    B_START=$(printf '%s\n' "$OUT" | awk -F'\t' '$1=="START"{print $2; exit}')
    B_HEAD=$(printf '%s\n' "$OUT" | awk -F'\t' '$1=="HEAD"{print $2; exit}')
    MTIMES=$(printf '%s\n' "$OUT" | awk -F'\t' '$1=="MT"{print $2 "\t" $3}')
fi

# No baseline object at all -> graceful absence.
if [ -z "$B_SID" ] && [ -z "$B_START" ] && [ -z "$MTIMES" ]; then
    emit_absent "no csi10 baseline recorded for this workdir"
fi

# Per-session keying: a baseline authored by a DIFFERENT session is not mine
# (the shared this-session.json may carry a peer's or a prior session's
# snapshot). Mismatch -> graceful absence (cannot attribute; failure-open).
if [ -n "$B_SID" ] && [ -n "$MY_SID" ] && [ "$B_SID" != "$MY_SID" ]; then
    emit_absent "baseline belongs to another session ($B_SID)"
fi

# ---- baseline_age_seconds --------------------------------------------------
NOW=$(date -u +%s 2>/dev/null)
START_EPOCH=""
[ -n "$B_START" ] && START_EPOCH=$(iso_to_epoch "$B_START")
AGE_JSON="null"
if [ -n "$START_EPOCH" ] && [ -n "$NOW" ]; then
    AGE=$((NOW - START_EPOCH))
    [ "$AGE" -lt 0 ] && AGE=0
    AGE_JSON="$AGE"
fi

mut_json=""
mut_count=0
mtime_hits=0
gitlog_hits=0

add_mut() {
    # path source since
    [ "$mut_count" -ge "$MAX_MUT" ] && return 0
    local entry
    entry="{\"path\":\"$(esc "$1")\",\"source\":\"$2\",\"since\":\"$(esc "$3")\",\"by_session\":null}"
    if [ -z "$mut_json" ]; then mut_json="$entry"; else mut_json="$mut_json,$entry"; fi
    mut_count=$((mut_count + 1))
}

# ---- mtime leg: per-file last-observed-mtime vs current --------------------
# Only files present in the baseline AND still existing are compared; a current
# mtime strictly greater than the baseline is a write this session did not
# observe (update-baseline refreshes the baseline on this session's own writes).
if [ -n "$MTIMES" ]; then
    while IFS=$'\t' read -r bpath bmt; do
        [ -z "$bpath" ] && continue
        case "$bmt" in (''|*[!0-9]*) continue ;; esac
        [ -f "$bpath" ] || continue
        cur=$(file_mtime "$bpath")
        [ -z "$cur" ] && continue
        if [ "$cur" -gt "$bmt" ]; then
            since=$(epoch_to_iso "$cur")
            [ -n "$since" ] || since="$B_START"
            add_mut "$bpath" "mtime" "$since"
            mtime_hits=$((mtime_hits + 1))
        fi
    done <<EOF
$MTIMES
EOF
fi

# ---- git-log leg: committed external mutations over headSha..HEAD ----------
if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    if [ -n "$B_HEAD" ]; then
        CUR_HEAD=$(git rev-parse HEAD 2>/dev/null | tr -d '\r')
        if [ -n "$CUR_HEAD" ] && [ "$CUR_HEAD" != "$B_HEAD" ]; then
            latest=$(git log -1 --format=%cI "$B_HEAD..HEAD" -- $TRACKED_FILES $TRACKED_DIRS 2>/dev/null)
            [ -n "$latest" ] || latest="$B_START"
            changed=$(git diff --name-only "$B_HEAD..HEAD" -- $TRACKED_FILES $TRACKED_DIRS 2>/dev/null)
            if [ -n "$changed" ]; then
                while IFS= read -r cpath; do
                    [ -z "$cpath" ] && continue
                    add_mut "$cpath" "git-log" "$latest"
                    gitlog_hits=$((gitlog_hits + 1))
                done <<EOF
$changed
EOF
            fi
        fi
    fi
fi

# ---- Compose verdict --------------------------------------------------------
if [ "$mut_count" -gt 0 ]; then
    ext="true"
    summary="$mut_count external LTADS mutation(s) since ${B_START:-baseline}"
    briefing="$mut_count external LTADS-state mutation(s) since ${B_START:-baseline} (mtime: $mtime_hits, git-log: $gitlog_hits) -- another session may be editing this arc; reconcile before committing"
else
    ext="false"
    summary="no external LTADS mutation since ${B_START:-baseline}"
    briefing=""
fi

printf '{"external_mutation":%s,"mutations":[%s],"baseline_age_seconds":%s,"summary":"%s","briefing":"%s"}\n' \
    "$ext" "$mut_json" "$AGE_JSON" "$(esc "$summary")" "$(esc "$briefing")"
exit 0
