#!/bin/bash
# runtime-perception-questions oracle (Unix) — ARIA-09 (Leg-D substrate)
#
# Answers: "in this session, did the agent ask a human to run-and-report a
# runtime-perception question that an AOR surface should have answered itself?"
# (the Human-as-I/O-Relay anti-pattern — FOUNDATIONS/AGENT_OPERABLE_RUNTIME_DOCTRINE.md).
#
# Deterministic input (preferred): the probe-candidate marker log the Leg-C
# reflex appends to when it hits the anti-pattern. Path:
#   .claude/session-state/aria-probe-candidates.jsonl   (append-only JSONL)
# Each line (schemaVersion "1"):
#   {"schemaVersion":"1","ts":ISO8601,"sessionId":str|null,
#    "category":"navigation"|"errors"|"async"|"other",
#    "question":str,"capability":str,            # the concrete AOR verb the gap names
#    "aria_could_answer":bool,"surface_present":bool}
#
# Output (matches oracle.json schema):
#   { "count": int,
#     "questions": [ {question, category, aria_could_answer, capability?,
#                     suggested_endpoint?, surface_present?} ],
#     "scoped": bool,        # was the "this session" filter actually applied?
#     "idSource": string,    # which rung of the ladder named this session
#     "briefing": string }   # empty string suppresses the [runtime-perception] line
#
# NO-DATA / empty: missing or empty log -> count 0, questions [], briefing "".
# Read-only + idempotent. bash-3.2 portable (no jq/grep -P dependency).
#
# ---------------------------------------------------------------------------
# SESSION SCOPING -- an unlabelled record is UNKNOWN, never MINE (ARIA-25,
# DEC-356; brief DEFER-179)
# ---------------------------------------------------------------------------
# This oracle asserts "candidates logged THIS session". It used to measure
# "null-or-this-session candidates": the filter read
#     if sess and d["sessionId"] not in (None, "", sess): continue
# so any record whose writer could not name itself matched EVERY session. The
# writer could not name itself in every session where the framework had not
# exported CLAUDE_SESSION_ID -- which, measured on SessionHelm's 18-record log,
# is every record since 2026-07-03. Phase 11.5 was handed a growing pile of
# other sessions' relay events labelled as its own, correctly declined to
# promote them on every one of four autopilot finalizes across nine days, and
# the mechanism promoted nothing. The self-test could not see it: written in
# the measurement's own vocabulary, it had no unlabelled-record cell at all.
#
# Two changes, and the second is what makes the first honest:
#   * a record with a null/absent/empty sessionId is OUT of scope whenever this
#     session HAS an identity. Unknown is unknown, not mine.
#   * when this session has NO identity, no filter is applied and the output
#     says so (`scoped:false`) rather than passing an unscoped population off
#     as a scoped one.
# The identity itself comes from lib/aria-session-id.sh -- the SAME ladder the
# writer uses, which is the only reason the two ends agree -- and it is
# compared as an ALIAS SET, not one string (DEC-337's "self is a set").
#
# The legacy null backlog is not deleted, only descoped. `--all` (or
# ARIA_PROBE_SCOPE=all) reports the whole log, which is how you reach it.

set -e

LOG=".claude/session-state/aria-probe-candidates.jsonl"

SCOPE_MODE="${ARIA_PROBE_SCOPE:-session}"
for _arg in "$@"; do
    case "$_arg" in
        --all) SCOPE_MODE="all" ;;
    esac
done

# ---- Resolve this session's alias set via the shared ladder ----------------
_THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
for _cand in \
    "$_THIS_DIR/../../scripts/lib/aria-session-id.sh" \
    "$_THIS_DIR/../../../claude-template/scripts/lib/aria-session-id.sh" \
    "$HOME/.claude/scripts/lib/aria-session-id.sh"; do
    if [ -f "$_cand" ]; then
        # shellcheck source=/dev/null
        . "$_cand"
        break
    fi
done
ID_SOURCE="none"
SESSION_ALIASES=""
if command -v aria_session_id >/dev/null 2>&1; then
    _ARIA_ID="$(aria_session_id)"
    ID_SOURCE="${_ARIA_ID##*|}"
    SESSION_ALIASES="$(aria_session_id_aliases)"
else
    # Lib absent: degrade to exactly the pre-fix identity (env only) and SAY SO,
    # rather than inline a second ladder that could drift from the writer's.
    SESSION_ALIASES="${CLAUDE_SESSION_ID:-}"
    if [ -n "$SESSION_ALIASES" ]; then
        ID_SOURCE="env (aria-session-id lib not found)"
    else
        ID_SOURCE="none (aria-session-id lib not found)"
    fi
fi
if [ "$SCOPE_MODE" = "all" ]; then
    SESSION_ALIASES=""
    ID_SOURCE="unscoped (--all)"
fi
if [ -n "$SESSION_ALIASES" ]; then SCOPED="true"; else SCOPED="false"; fi

emit_empty() {
    printf '{"count":0,"questions":[],"scoped":%s,"idSource":"%s","briefing":""}' \
        "$SCOPED" "$ID_SOURCE"
    exit 0
}

[ -f "$LOG" ] || emit_empty
[ -s "$LOG" ] || emit_empty

# Prefer a working python (the WindowsApps `python3` shim is broken — probe it,
# same pattern as aria-status). Python gives faithful per-record parsing; the
# pure-shell fallback below still emits valid JSON (count-only, details NO-DATA).
_py=""
for _c in python python3 py; do
    if command -v "$_c" >/dev/null 2>&1 && "$_c" -c 'import json,sys' >/dev/null 2>&1; then _py="$_c"; break; fi
done

if [ -n "$_py" ]; then
    OUT=$(ARIA_SELF_ALIASES="$SESSION_ALIASES" ARIA_ID_SOURCE="$ID_SOURCE" \
        "$_py" - "$LOG" <<'PYEOF'
import json, os, sys
path = sys.argv[1]
aliases = set(a for a in os.environ.get("ARIA_SELF_ALIASES", "").split("\n") if a)
id_source = os.environ.get("ARIA_ID_SOURCE", "none")
cats = {"navigation", "errors", "async", "other"}
qs = []
skipped_unlabelled = 0
try:
    with open(path, "r", encoding="utf-8-sig", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                d = json.loads(line)
            except Exception:
                continue
            if not isinstance(d, dict):
                continue
            # Session scoping (ARIA-25): when this session has an identity,
            # surface ONLY records this session wrote. An unlabelled record is
            # UNKNOWN, not mine -- counting it as mine is the DEFER-179 defect
            # that made this oracle report other sessions' rows as its own.
            if aliases:
                rid = d.get("sessionId")
                if rid in (None, ""):
                    skipped_unlabelled += 1
                    continue
                if rid not in aliases:
                    continue
            q = d.get("question")
            if not isinstance(q, str) or not q:
                continue
            cat = d.get("category")
            if cat not in cats:
                cat = "other"
            rec = {"question": q, "category": cat,
                   "aria_could_answer": bool(d.get("aria_could_answer", False))}
            cap = d.get("capability")
            if isinstance(cap, str) and cap:
                rec["capability"] = cap
            ep = d.get("suggested_endpoint")
            if isinstance(ep, str) and ep:
                rec["suggested_endpoint"] = ep
            if "surface_present" in d:
                rec["surface_present"] = bool(d.get("surface_present"))
            qs.append(rec)
except Exception:
    qs = []
count = len(qs)
scoped = bool(aliases)
if count == 0:
    briefing = ""
else:
    answerable = sum(1 for r in qs if r["aria_could_answer"])
    briefing = ("runtime-perception: %d human-relay probe candidate(s) logged"
                " (%d AOR-answerable) -- surfaced at /0-uldf-finalize Phase 11.5"
                % (count, answerable))
    if not scoped:
        # Never pass an unscoped population off as "this session's" (ARIA-25).
        briefing += " [WHOLE LOG -- not scoped to this session: %s]" % id_source
    if len(briefing) > 200:
        briefing = briefing[:197] + "..."
out = {"count": count, "questions": qs, "scoped": scoped,
       "idSource": id_source, "briefing": briefing}
if skipped_unlabelled:
    out["skippedUnlabelled"] = skipped_unlabelled
sys.stdout.write(json.dumps(out, separators=(",", ":")))
PYEOF
)
    if [ -n "$OUT" ]; then
        printf '%s' "$OUT"
        exit 0
    fi
fi

# ---- No-python fallback: count non-empty lines; details are NO-DATA ----
# (Counts every record; session-scoping needs faithful JSON parsing, so the
# fallback is intentionally conservative — it reports the count honestly and
# omits per-question detail rather than risk a malformed parse.)
# ARIA-25: that conservatism is exactly why this path reports scoped:false even
# when this session HAS an identity — the count is over the whole log, and
# labelling an unscoped number as scoped is the defect one level up.
COUNT=$(awk 'NF { c++ } END { print c+0 }' "$LOG" 2>/dev/null || echo 0)
[ -z "$COUNT" ] && COUNT=0
SCOPED="false"
ID_SOURCE="$ID_SOURCE (no python -- whole log, unscoped)"
if [ "$COUNT" -eq 0 ]; then
    emit_empty
fi
BRIEF="runtime-perception: $COUNT human-relay probe candidate(s) logged (WHOLE LOG -- detail and session scoping unavailable without python) -- surfaced at /0-uldf-finalize Phase 11.5"
ESC_BRIEF=$(printf '%s' "$BRIEF" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')
printf '{"count":%d,"questions":[],"scoped":false,"idSource":"%s","briefing":"%s"}' \
    "$COUNT" "$ID_SOURCE" "$ESC_BRIEF"
exit 0
