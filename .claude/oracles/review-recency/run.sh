#!/bin/bash
# review-recency oracle (Unix) — RECENCY-03/04, DEC-139.
#
# Emits a [review-recency] briefing line when a heavy periodic review/refactor
# skill (covered set: 0-uldf-scrutinize, 0-uldf-uladp-compliance by default)
# ran on THIS project within recentDays — so you don't re-run it having
# forgotten. Delegates ALL aggregation to the single-source-of-truth reader
# scripts/review-recency.sh (RECENCY-01); this wrapper only scopes to the
# current project and shapes the briefing.
#
# Output: single-line JSON (always-fresh; ~1.5s budget).
# Gracefully absent: empty `briefing` when nothing recent, or on NO-DATA (no
# usage registry) / reader-not-found — the session-start fan-out then emits no
# line (parallel to stale-ltads-state's empty-result silence).

set +e

emit_quiet() {
    local proj_json="$1"
    cat <<EOF
{"recent":false,"details":{"project":$proj_json,"recentDays":14,"recentSkills":[]},"briefing":""}
EOF
    exit 0
}

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

PROJECT="$(basename "$PWD")"
PROJECT_JSON="\"$(esc "$PROJECT")\""

command -v jq >/dev/null 2>&1 || emit_quiet "$PROJECT_JSON"

# ---- Locate the reader (deployed project -> template repo -> global) --------
_THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
READER=""
for _cand in \
    "$_THIS_DIR/../../scripts/review-recency.sh" \
    "$_THIS_DIR/../../../claude-template/scripts/review-recency.sh" \
    "$HOME/.claude/scripts/review-recency.sh"; do
    if [ -f "$_cand" ]; then READER="$_cand"; break; fi
done
[ -z "$READER" ] && emit_quiet "$PROJECT_JSON"

# ---- Run the reader, project-scoped -----------------------------------------
OUT="$(bash "$READER" --project "$PROJECT" --json 2>/dev/null)"
rc=$?
# rc 2 = NO-DATA (no registry). Any non-zero or empty -> graceful absence.
if [ "$rc" -ne 0 ] || [ -z "$OUT" ]; then emit_quiet "$PROJECT_JSON"; fi

# ---- Extract the recent covered-skill cells ---------------------------------
RECENT_DAYS="$(printf '%s' "$OUT" | jq -r '.recentDays // 14')"
RECENT_JSON="$(printf '%s' "$OUT" | jq -c '
    (.projects[0].skills // {})
    | to_entries
    | map(select(.value.status == "recent")
          | {skill: .key, lastUsed: .value.lastUsed, lastArg1: .value.lastArg1})
' 2>/dev/null)"
[ -z "$RECENT_JSON" ] && RECENT_JSON="[]"

COUNT="$(printf '%s' "$RECENT_JSON" | jq 'length' 2>/dev/null)"
case "$COUNT" in ''|*[!0-9]*) COUNT=0 ;; esac

if [ "$COUNT" -eq 0 ]; then
    cat <<EOF
{"recent":false,"details":{"project":$PROJECT_JSON,"recentDays":$RECENT_DAYS,"recentSkills":[]},"briefing":""}
EOF
    exit 0
fi

# ---- Compose the briefing ----------------------------------------------------
# e.g. "0-uldf-scrutinize ran here 2026-07-01; 0-uldf-uladp-compliance 2026-06-30"
SUMMARY="$(printf '%s' "$RECENT_JSON" | jq -r '
    map(.skill + " " + (.lastUsed // "?")) | join("; ")')"
BRIEFING="$(esc "reviewed on this project within ${RECENT_DAYS}d — ${SUMMARY}; a re-run may be redundant (see /0-uldf-portfolio recency)")"

cat <<EOF
{"recent":true,"details":{"project":$PROJECT_JSON,"recentDays":$RECENT_DAYS,"recentSkills":$RECENT_JSON},"briefing":"$BRIEFING"}
EOF
exit 0
