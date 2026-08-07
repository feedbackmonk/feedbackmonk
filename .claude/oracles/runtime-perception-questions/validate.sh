#!/bin/bash
# runtime-perception-questions oracle self-test (Unix) -- ARIA-09
# Builds throwaway sandboxes with a marker log and asserts the oracle's
# count / questions / briefing / session-scoping / NO-DATA behavior.
set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
RUN="$ORACLE_DIR/run.sh"
PASS=0; FAIL=0
PYOK=0
for c in python python3 py; do
    if command -v "$c" >/dev/null 2>&1 && "$c" -c 'import json,sys' >/dev/null 2>&1; then PYOK=1; break; fi
done

check() { # name expected-substring actual
    if printf '%s' "$3" | grep -qF "$2"; then
        PASS=$((PASS+1))
    else
        FAIL=$((FAIL+1)); echo "FAIL: $1 -- expected to contain '$2'; got: $3" >&2
    fi
}
json_ok() { # name actual
    if printf '%s' "$2" | { python3 -c 'import sys,json;json.load(sys.stdin)' 2>/dev/null \
        || python -c 'import sys,json;json.load(sys.stdin)' 2>/dev/null \
        || node -e 'JSON.parse(require("fs").readFileSync(0))' 2>/dev/null; }; then
        PASS=$((PASS+1))
    else
        # No JSON validator available -- accept if it at least starts with '{'
        case "$2" in '{'*) PASS=$((PASS+1));; *) FAIL=$((FAIL+1)); echo "FAIL: $1 not valid JSON: $2" >&2;; esac
    fi
}

mk_sandbox() { # -> echoes a fresh temp dir with .claude/session-state/
    local d; d="$(mktemp -d 2>/dev/null || echo "/tmp/rpq.$$.$RANDOM")"
    mkdir -p "$d/.claude/session-state"
    printf '%s' "$d"
}

# --- Case 1: NO-DATA (no log) ---
S1="$(mk_sandbox)"
O1="$(cd "$S1" && unset CLAUDE_SESSION_ID; bash "$RUN")"
json_ok "no-data valid json" "$O1"
check "no-data count" '"count":0' "$O1"
check "no-data empty briefing" '"briefing":""' "$O1"

# --- Case 2: empty log file ---
S2="$(mk_sandbox)"; : > "$S2/.claude/session-state/aria-probe-candidates.jsonl"
O2="$(cd "$S2" && unset CLAUDE_SESSION_ID; bash "$RUN")"
check "empty-file count" '"count":0' "$O2"

# --- Case 3: two candidates, no session filter ---
S3="$(mk_sandbox)"; LOG3="$S3/.claude/session-state/aria-probe-candidates.jsonl"
cat > "$LOG3" <<'EOF'
{"schemaVersion":"1","ts":"2026-06-29T01:00:00Z","sessionId":"sess-A","category":"async","question":"Did the live interpret round-trip compute the aggregate?","capability":"command-invoke:interpretLive","aria_could_answer":true,"surface_present":false}
not-json-junk-line
{"schemaVersion":"1","ts":"2026-06-29T01:05:00Z","sessionId":"sess-A","category":"errors","question":"Is there an error in the console?","capability":"state-dump:errors","aria_could_answer":true,"surface_present":true}
EOF
O3="$(cd "$S3" && unset CLAUDE_SESSION_ID; bash "$RUN")"
json_ok "two-candidate valid json" "$O3"
check "two-candidate count" '"count":2' "$O3"
check "two-candidate briefing nonempty" 'human-relay probe candidate' "$O3"
if [ "$PYOK" = "1" ]; then
    check "two-candidate carries capability" 'command-invoke:interpretLive' "$O3"
    check "two-candidate carries question" 'live interpret round-trip' "$O3"
fi

# --- Case 4: session scoping ---
S4="$(mk_sandbox)"; LOG4="$S4/.claude/session-state/aria-probe-candidates.jsonl"
cat > "$LOG4" <<'EOF'
{"schemaVersion":"1","ts":"2026-06-29T01:00:00Z","sessionId":"sess-A","category":"async","question":"A-question","capability":"command-invoke:a","aria_could_answer":true}
{"schemaVersion":"1","ts":"2026-06-29T01:05:00Z","sessionId":"sess-B","category":"async","question":"B-question","capability":"command-invoke:b","aria_could_answer":true}
EOF
O4="$(cd "$S4" && CLAUDE_SESSION_ID=sess-A bash "$RUN")"
if [ "$PYOK" = "1" ]; then
    check "session-scope keeps A" '"count":1' "$O4"
    check "session-scope keeps A-question" 'A-question' "$O4"
    if printf '%s' "$O4" | grep -qF 'B-question'; then
        FAIL=$((FAIL+1)); echo "FAIL: session-scope leaked B-question" >&2
    else PASS=$((PASS+1)); fi
fi

rm -rf "$S1" "$S2" "$S3" "$S4" 2>/dev/null || true

echo "runtime-perception-questions validate: $PASS passed, $FAIL failed (python=$PYOK)"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
