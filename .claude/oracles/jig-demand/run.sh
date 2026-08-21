#!/bin/bash
# jig-demand oracle (Unix) -- JIG-09 (the conversion side of the Jig doctrine)
#
# Answers: "has the SAME capability been asked for across several separate
# sessions and still not been built?"
#
# The Development Jig doctrine was instrumented on collection only. Agents log
# candidates reflexively (223 across 12 projects, measured 2026-08-14) and the
# only reader -- runtime-perception-questions -- is session-scoped BY
# CONSTRUCTION (ARIA-25/DEC-356), while jig-friction clusters within one session
# and /0-uldf-portfolio jigs reports the log as a raw line count (pre-JIG-10).
# Cross-session
# recurrence, the only signal that justifies building, was computed by nobody.
#
# Inputs (both append-only JSONL, both may be absent):
#   .claude/session-state/aria-probe-candidates.jsonl     the demand log
#   .claude/session-state/jig-demand-dispositions.jsonl   what has been drained
#
# Output: see oracle.json `output.schema`. Empty briefing on the quiet path.
# Read-only + idempotent. Advisory ONLY -- never blocks a commit or a session.
#
# The clustering algorithm lives in cluster.py, invoked by BOTH twins. That is
# deliberate: an IDF/union-find pass written twice is a TWIN-01 defect waiting
# to happen, and a green parity cell would not catch it (parity is a consistency
# property, never a correctness one -- DISC-MON-17).

set -u

LOG=".claude/session-state/aria-probe-candidates.jsonl"
DISP=".claude/session-state/jig-demand-dispositions.jsonl"
CFG=".claude/config.json"

MIN_CANDIDATES=""; MIN_OCCASIONS=""; SIMILARITY=""; TOP_N=""

while [ $# -gt 0 ]; do
    case "$1" in
        --min-candidates) MIN_CANDIDATES="${2:-}"; shift 2;;
        --min-occasions)  MIN_OCCASIONS="${2:-}";  shift 2;;
        --similarity)     SIMILARITY="${2:-}";     shift 2;;
        --top-n)          TOP_N="${2:-}";          shift 2;;
        --log)            LOG="${2:-}";            shift 2;;
        --dispositions)   DISP="${2:-}";           shift 2;;
        --self-test)      exec bash "$(dirname "${BASH_SOURCE[0]:-$0}")/validate.sh" "$@";;
        *) shift;;
    esac
done

# Prefer a working python (the WindowsApps `python3` shim is broken -- probe it,
# same pattern as runtime-perception-questions / aria-status).
_py=""
for _c in python python3 py; do
    if command -v "$_c" >/dev/null 2>&1 && "$_c" -c 'import json,sys' >/dev/null 2>&1; then _py="$_c"; break; fi
done

if [ -z "$_py" ]; then
    # NO clustering was performed. status:ok here means "not measured", never
    # "nothing found" -- `clustered:false` is the field that distinguishes them,
    # and the briefing says so rather than passing silence off as a clean bill.
    printf '{"status":"ok","clusters":[],"totals":{"candidates":0,"dispositioned":0,"undispositioned":0},"clustered":false,"briefing":"jig-demand: NOT MEASURED -- no python interpreter available, so no recurrence clustering ran (clustered:false)"}'
    exit 0
fi

# Config precedence: CLI > .claude/config.json jigDemand.* > default (in cluster.py).
if [ -f "$CFG" ]; then
    _cfg="$("$_py" - "$CFG" <<'PYEOF'
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig", errors="replace") as fh:
        d = json.load(fh)
except Exception:
    d = {}
j = d.get("jigDemand") or {}
for k in ("minCandidates", "minOccasions", "similarity", "topN"):
    v = j.get(k)
    if v is not None:
        sys.stdout.write("%s=%s\n" % (k, v))
PYEOF
)" || _cfg=""
    while IFS='=' read -r _k _v; do
        [ -n "${_k:-}" ] || continue
        case "$_k" in
            minCandidates) [ -z "$MIN_CANDIDATES" ] && MIN_CANDIDATES="$_v";;
            minOccasions)  [ -z "$MIN_OCCASIONS" ]  && MIN_OCCASIONS="$_v";;
            similarity)    [ -z "$SIMILARITY" ]     && SIMILARITY="$_v";;
            topN)          [ -z "$TOP_N" ]          && TOP_N="$_v";;
        esac
    done <<EOF
$_cfg
EOF
fi

_THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"

JIGD_MIN_CANDIDATES="$MIN_CANDIDATES" \
JIGD_MIN_OCCASIONS="$MIN_OCCASIONS" \
JIGD_SIMILARITY="$SIMILARITY" \
JIGD_TOP_N="$TOP_N" \
    "$_py" "$_THIS_DIR/cluster.py" "$LOG" "$DISP" 2>/dev/null || {
        printf '{"status":"ok","clusters":[],"totals":{"candidates":0,"dispositioned":0,"undispositioned":0},"clustered":false,"briefing":"jig-demand: NOT MEASURED -- clustering pass failed (clustered:false)"}'
    }
exit 0
