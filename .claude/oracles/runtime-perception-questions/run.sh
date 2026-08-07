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
#     "briefing": string }   # empty string suppresses the [runtime-perception] line
#
# NO-DATA / empty: missing or empty log -> count 0, questions [], briefing "".
# Read-only + idempotent. bash-3.2 portable (no jq/grep -P dependency).

set -e

LOG=".claude/session-state/aria-probe-candidates.jsonl"
SESSION_ID="${CLAUDE_SESSION_ID:-}"

emit_empty() {
    printf '{"count":0,"questions":[],"briefing":""}'
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
    OUT=$("$_py" - "$LOG" "$SESSION_ID" <<'PYEOF'
import json, sys
path, sess = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else "")
cats = {"navigation", "errors", "async", "other"}
qs = []
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
            # Session scoping: when this session's id is known and the record
            # carries a non-null sessionId, surface only this session's rows.
            if sess and d.get("sessionId") not in (None, "", sess):
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
if count == 0:
    briefing = ""
else:
    answerable = sum(1 for r in qs if r["aria_could_answer"])
    briefing = ("runtime-perception: %d human-relay probe candidate(s) logged"
                " (%d AOR-answerable) -- surfaced at /0-uldf-finalize Phase 11.5"
                % (count, answerable))
    if len(briefing) > 200:
        briefing = briefing[:197] + "..."
sys.stdout.write(json.dumps({"count": count, "questions": qs, "briefing": briefing},
                            separators=(",", ":")))
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
COUNT=$(awk 'NF { c++ } END { print c+0 }' "$LOG" 2>/dev/null || echo 0)
[ -z "$COUNT" ] && COUNT=0
if [ "$COUNT" -eq 0 ]; then
    emit_empty
fi
BRIEF="runtime-perception: $COUNT human-relay probe candidate(s) logged (detail unavailable without python) -- surfaced at /0-uldf-finalize Phase 11.5"
ESC_BRIEF=$(printf '%s' "$BRIEF" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')
printf '{"count":%d,"questions":[],"briefing":"%s"}' "$COUNT" "$ESC_BRIEF"
exit 0
