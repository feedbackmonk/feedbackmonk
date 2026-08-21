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
O1="$(cd "$S1" && env -u CLAUDE_SESSION_ID -u CLAUDE_CODE_SESSION_ID -u CLAUDE_PID -u ARIA_PROBE_SCOPE bash "$RUN")"
json_ok "no-data valid json" "$O1"
check "no-data count" '"count":0' "$O1"
check "no-data empty briefing" '"briefing":""' "$O1"

# --- Case 2: empty log file ---
S2="$(mk_sandbox)"; : > "$S2/.claude/session-state/aria-probe-candidates.jsonl"
O2="$(cd "$S2" && env -u CLAUDE_SESSION_ID -u CLAUDE_CODE_SESSION_ID -u CLAUDE_PID -u ARIA_PROBE_SCOPE bash "$RUN")"
check "empty-file count" '"count":0' "$O2"

# --- Case 3: two candidates, no session filter ---
# NOTE (ARIA-25): "no session filter" now means NO RUNG resolves, not merely
# that CLAUDE_SESSION_ID is unset -- the ladder also reads CLAUDE_CODE_SESSION_ID
# and CLAUDE_PID, both of which a validate run inherits from its caller. Every
# cell below scrubs all three, or it would be graded against a scoped read.
S3="$(mk_sandbox)"; LOG3="$S3/.claude/session-state/aria-probe-candidates.jsonl"
cat > "$LOG3" <<'EOF'
{"schemaVersion":"1","ts":"2026-06-29T01:00:00Z","sessionId":"sess-A","category":"async","question":"Did the live interpret round-trip compute the aggregate?","capability":"command-invoke:interpretLive","aria_could_answer":true,"surface_present":false}
not-json-junk-line
{"schemaVersion":"1","ts":"2026-06-29T01:05:00Z","sessionId":"sess-A","category":"errors","question":"Is there an error in the console?","capability":"state-dump:errors","aria_could_answer":true,"surface_present":true}
EOF
O3="$(cd "$S3" && env -u CLAUDE_SESSION_ID -u CLAUDE_CODE_SESSION_ID -u CLAUDE_PID -u ARIA_PROBE_SCOPE bash "$RUN")"
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
O4="$(cd "$S4" && env -u CLAUDE_CODE_SESSION_ID -u CLAUDE_PID -u ARIA_PROBE_SCOPE CLAUDE_SESSION_ID=sess-A bash "$RUN")"
if [ "$PYOK" = "1" ]; then
    check "session-scope keeps A" '"count":1' "$O4"
    check "session-scope keeps A-question" 'A-question' "$O4"
    if printf '%s' "$O4" | grep -qF 'B-question'; then
        FAIL=$((FAIL+1)); echo "FAIL: session-scope leaked B-question" >&2
    else PASS=$((PASS+1)); fi
fi

# --- Case 5: ARIA-25 -- an unlabelled record is UNKNOWN, never MINE ---------
# The defect this replaces: the filter admitted a null-sessionId record into
# EVERY session's output, so Phase 11.5 was handed other sessions' relay events
# labelled as its own and correctly refused to promote them, forever. The
# pre-ARIA-25 self-test had no cell here at all -- written in the measurement's
# own vocabulary, it could not see the gap (OVALID-03).
#
# Every must-NOT-fire cell below is PAIRED with a must-STILL-fire control in the
# SAME run: a "fix" that merely stops reporting passes the negative cells
# perfectly and deletes the mechanism.
S5="$(mk_sandbox)"; LOG5="$S5/.claude/session-state/aria-probe-candidates.jsonl"
cat > "$LOG5" <<'EOF'
{"schemaVersion":"1","ts":"2026-07-01T01:00:00Z","sessionId":"sess-OTHER","category":"async","question":"OTHER-question","aria_could_answer":true}
{"schemaVersion":"1","ts":"2026-07-03T01:00:00Z","sessionId":null,"category":"errors","question":"NULL-legacy-question","aria_could_answer":true}
{"schemaVersion":"1","ts":"2026-08-12T01:00:00Z","sessionId":"sess-ME","category":"navigation","question":"MINE-question","aria_could_answer":true}
EOF
# Drive with ONLY the rung under test reachable, so a cell cannot pass by
# accident through a rung the harness happens to inherit from its caller.
O5="$(cd "$S5" && env -u CLAUDE_CODE_SESSION_ID -u CLAUDE_PID -u ARIA_PROBE_SCOPE \
        CLAUDE_SESSION_ID=sess-ME bash "$RUN")"
if [ "$PYOK" = "1" ]; then
    # must-NOT-fire: the unlabelled record does not belong to this session
    if printf '%s' "$O5" | grep -qF 'NULL-legacy-question'; then
        FAIL=$((FAIL+1)); echo "FAIL: ARIA-25 unlabelled record leaked into an identified session" >&2
    else PASS=$((PASS+1)); fi
    # must-STILL-fire control: this session's own record is still surfaced
    check "ARIA-25 own record still surfaces" 'MINE-question' "$O5"
    check "ARIA-25 count excludes the unlabelled row" '"count":1' "$O5"
    check "ARIA-25 exclusion is reported, not silent" '"skippedUnlabelled":1' "$O5"
    # must-NOT-fire control: a peer's LABELLED record is still excluded
    if printf '%s' "$O5" | grep -qF 'OTHER-question'; then
        FAIL=$((FAIL+1)); echo "FAIL: ARIA-25 peer record leaked" >&2
    else PASS=$((PASS+1)); fi
fi
check "ARIA-25 scoped flag true when identified" '"scoped":true' "$O5"

# --- Case 6: no identity at all -> no filter, and the output SAYS so --------
# Honest, but NOT the assertion: the whole log is returned, so scoped MUST be
# false. A consumer reading the list while ignoring this flag re-creates the
# DEFER-179 defect, which is why the flag is asserted here and not just the count.
O6="$(cd "$S5" && env -u CLAUDE_SESSION_ID -u CLAUDE_CODE_SESSION_ID -u CLAUDE_PID \
        -u ARIA_PROBE_SCOPE bash "$RUN")"
check "no-identity scoped flag false" '"scoped":false' "$O6"
check "no-identity briefing declares the whole-log read" 'WHOLE LOG' "$O6"
if [ "$PYOK" = "1" ]; then
    check "no-identity surfaces the unlabelled row" 'NULL-legacy-question' "$O6"
    check "no-identity count is the whole log" '"count":3' "$O6"
fi

# --- Case 7: --all reaches the descoped legacy backlog ----------------------
O7="$(cd "$S5" && env -u CLAUDE_CODE_SESSION_ID -u CLAUDE_PID -u ARIA_PROBE_SCOPE \
        CLAUDE_SESSION_ID=sess-ME bash "$RUN" --all)"
check "--all reports unscoped" '"scoped":false' "$O7"
if [ "$PYOK" = "1" ]; then
    check "--all reaches the legacy null record" 'NULL-legacy-question' "$O7"
    check "--all reaches a peer's record" 'OTHER-question' "$O7"
fi

# --- Case 8: the harness rung labels a session with no CLAUDE_SESSION_ID ----
# This is the rung the whole defect turned on: SessionHelm's records 9-18 were
# null precisely because the framework had not exported CLAUDE_SESSION_ID.
S8="$(mk_sandbox)"; LOG8="$S8/.claude/session-state/aria-probe-candidates.jsonl"
cat > "$LOG8" <<'EOF'
{"schemaVersion":"1","ts":"2026-08-12T01:00:00Z","sessionId":"uuid-HARNESS","category":"other","question":"HARNESS-question","aria_could_answer":false}
{"schemaVersion":"1","ts":"2026-08-12T02:00:00Z","sessionId":null,"category":"other","question":"NULL-question","aria_could_answer":false}
EOF
O8="$(cd "$S8" && env -u CLAUDE_SESSION_ID -u CLAUDE_PID -u ARIA_PROBE_SCOPE \
        CLAUDE_CODE_SESSION_ID=uuid-HARNESS bash "$RUN")"
check "harness-rung identifies the session" '"idSource":"harness"' "$O8"
if [ "$PYOK" = "1" ]; then
    check "harness-rung surfaces its own record" 'HARNESS-question' "$O8"
    if printf '%s' "$O8" | grep -qF 'NULL-question'; then
        FAIL=$((FAIL+1)); echo "FAIL: ARIA-25 unlabelled record leaked under the harness rung" >&2
    else PASS=$((PASS+1)); fi
fi

# --- Case 9: writer <-> reader round trip (the acceptance criterion) --------
# "A candidate logged in session X surfaces in session X's Phase 11.5 and in no
# other session's." Driven through the REAL logger, not a hand-written fixture,
# because the defect lived in the disagreement between the two ends.
LOGGER=""
for _l in "$ORACLE_DIR/../../scripts/aria/log-probe-candidate.sh" \
          "$ORACLE_DIR/../../../claude-template/scripts/aria/log-probe-candidate.sh" \
          "$HOME/.claude/scripts/aria/log-probe-candidate.sh"; do
    [ -f "$_l" ] && { LOGGER="$_l"; break; }
done
S9="$(mk_sandbox)"
if [ -n "$LOGGER" ] && [ "$PYOK" = "1" ]; then
    ( cd "$S9" && env -u CLAUDE_SESSION_ID -u ARIA_PROBE_SCOPE \
        CLAUDE_CODE_SESSION_ID=uuid-RT CLAUDE_PID=4242 \
        bash "$LOGGER" --category errors --question "ROUNDTRIP-question" >/dev/null 2>&1 )
    O9A="$(cd "$S9" && env -u CLAUDE_SESSION_ID -u ARIA_PROBE_SCOPE \
            CLAUDE_CODE_SESSION_ID=uuid-RT CLAUDE_PID=4242 bash "$RUN")"
    O9B="$(cd "$S9" && env -u CLAUDE_SESSION_ID -u ARIA_PROBE_SCOPE \
            CLAUDE_CODE_SESSION_ID=uuid-PEER CLAUDE_PID=9999 bash "$RUN")"
    check "round-trip: the writing session sees it" 'ROUNDTRIP-question' "$O9A"
    if printf '%s' "$O9B" | grep -qF 'ROUNDTRIP-question'; then
        FAIL=$((FAIL+1)); echo "FAIL: round-trip record visible to a DIFFERENT session" >&2
    else PASS=$((PASS+1)); fi
    check "round-trip: the writer stamped a source" 'sessionIdSource' "$(cat "$S9/.claude/session-state/aria-probe-candidates.jsonl")"
else
    echo "SKIP: round-trip cells (logger=${LOGGER:-<not found>}, python=$PYOK)" >&2
fi

# --- Case 10: SELF IS A SET, not the top rung (DEC-337's other half) --------
# A record stamped under rung 2 while the READER also has rung 1 must still be
# recognised: one session can write from a process that sees a different subset
# of the env variables than the process that reads. Comparing against the top
# rung alone would silently lose the session's own candidate -- the DEFER-179
# defect turned inside out (invisible-to-self instead of visible-to-everyone).
S10="$(mk_sandbox)"; LOG10="$S10/.claude/session-state/aria-probe-candidates.jsonl"
cat > "$LOG10" <<'EOF'
{"schemaVersion":"1","ts":"2026-08-12T03:00:00Z","sessionId":"uuid-BOTH","sessionIdSource":"harness","category":"other","question":"ALIAS-question","aria_could_answer":false}
{"schemaVersion":"1","ts":"2026-08-12T03:05:00Z","sessionId":"uuid-FOREIGN","sessionIdSource":"harness","category":"other","question":"FOREIGN-question","aria_could_answer":false}
EOF
O10="$(cd "$S10" && env -u CLAUDE_PID -u ARIA_PROBE_SCOPE \
        CLAUDE_SESSION_ID=agent-X CLAUDE_CODE_SESSION_ID=uuid-BOTH bash "$RUN")"
if [ "$PYOK" = "1" ]; then
    check "alias-set: a lower-rung stamp is still recognised" 'ALIAS-question' "$O10"
    # must-NOT-fire control: widening to a SET must not widen to everybody
    if printf '%s' "$O10" | grep -qF 'FOREIGN-question'; then
        FAIL=$((FAIL+1)); echo "FAIL: alias set matched a foreign harness id" >&2
    else PASS=$((PASS+1)); fi
fi

rm -rf "$S1" "$S2" "$S3" "$S4" "$S5" "$S8" "$S9" "$S10" 2>/dev/null || true

echo "runtime-perception-questions validate: $PASS passed, $FAIL failed (python=$PYOK)"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
