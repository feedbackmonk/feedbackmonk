#!/bin/bash
# machine-quiescence oracle self-test (Unix + Git Bash on Windows)
#
# Sandbox self-test: verifies the oracle runs, emits schema-conforming JSON, and
# that its exit code agrees with its own `verdict` field. Uses the fixture seam
# (ULDF_QUIESCE_FIXTURE) where fixtures are reachable so the self-test does not
# depend on the state of the machine it happens to run on.
#
# Full behavioural coverage (verdict classes, falsifiability cells, twin parity)
# lives in scripts/smoke-tests/machine-quiescence-smoke.sh. This file is the
# oracle-local "does it run and conform" gate that /0-uldf-oracle invokes.

set -u

ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
PASS=0; FAIL=0
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }

# `command -v python3` finds the Microsoft Store execution-alias stub on Windows,
# which is not a Python. Probe that it actually runs (project-runtime-state's
# validate.sh established this pattern; copying it, not re-learning it).
PYBIN=""
for c in python3 python; do
    if command -v "$c" >/dev/null 2>&1 && "$c" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
        PYBIN="$c"; break
    fi
done

# --- 1. Runs, and its exit code matches its verdict -------------------------
OUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)"; RC=$?
case "$RC" in 0|1|2|3) ;; *) fail "run.sh exited $RC (expected 0|1|2|3)";; esac

V="$(printf '%s' "$OUT" | grep -o '"verdict":"[a-z]*"' | head -1 | sed 's/.*":"//; s/"$//')"
EXPECT=""
case "$V" in quiet) EXPECT=0;; noisy) EXPECT=1;; unknown) EXPECT=2;; blocking) EXPECT=3;; esac
if [ -n "$EXPECT" ] && [ "$RC" = "$EXPECT" ]; then
    pass "exit code $RC agrees with verdict '$V'"
else
    fail "exit code $RC does not agree with verdict '$V'"
fi

# --- 2. Valid JSON carrying every required field ----------------------------
if [ -n "$PYBIN" ]; then
    if printf '%s' "$OUT" | "$PYBIN" -c "
import sys, json
d = json.load(sys.stdin)
req = ['schemaVersion','verdict','checkedAt','requiredPorts','timingSensitive',
       'counts','residue','blockingReasons','degraded','summary']
missing = [k for k in req if k not in d]
assert not missing, 'missing fields: %s' % missing
assert d['schemaVersion'] == 1, 'schemaVersion must be 1'
assert d['verdict'] in ('quiet','noisy','blocking','unknown'), 'bad verdict'
for k in ('devPortListeners','agentSessions','buildProcesses'):
    assert k in d['counts'], 'counts.%s missing' % k
n = sum(d['counts'][k] for k in ('devPortListeners','agentSessions','buildProcesses'))
assert n == len(d['residue']), 'counts (%d) disagree with residue length (%d)' % (n, len(d['residue']))
# Fail-closed invariant: a declared degradation must never read as quiet.
assert not (d['degraded'] and d['verdict'] != 'unknown'), 'degraded but verdict is not unknown'
assert not (d['blockingReasons'] and d['verdict'] not in ('blocking','unknown')), 'blocking reasons but verdict is not blocking'
" 2>/dev/null; then
        pass "output is schema-conforming JSON, counts agree with residue, fail-closed invariant holds"
    else
        fail "schema/invariant check failed: $OUT"
    fi
else
    echo "SKIP: no python available - JSON schema check not run (NOT a pass)"
fi

# --- 3. Fail-closed on an unreadable probe ---------------------------------
OUT2="$(ULDF_QUIESCE_FIXTURE="$ORACLE_DIR/__no_such_fixture__" bash "$ORACLE_DIR/run.sh" 2>&1)"; RC2=$?
if [ "$RC2" = "2" ] && printf '%s' "$OUT2" | grep -q '"verdict":"unknown"'; then
    pass "an unreadable probe yields unknown/exit 2 - never quiet"
else
    fail "unreadable probe yielded exit $RC2: $OUT2"
fi

# --- 4. Never actuates ------------------------------------------------------
if grep -Ei 'Stop-Process|taskkill|pkill|kill -' "$ORACLE_DIR/run.sh" "$ORACLE_DIR/run.ps1" "$ORACLE_DIR/probe.ps1" \
   | grep -qvE ':[[:space:]]*#'; then
    fail "a process-termination verb appears in the oracle - it must only report"
else
    pass "no process-termination verb present (never actuates)"
fi

echo "machine-quiescence validate: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
