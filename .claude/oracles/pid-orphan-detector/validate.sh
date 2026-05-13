#!/bin/bash
# pid-orphan-detector oracle self-test (Unix + Git Bash on Windows)
#
# Phase 1: validate the read-only briefing path against the real ltads/execution.
# Phase 2: validate --gc and --gc-cheap sweep semantics in a sandbox:
#   T1. Default mode lists swept[]/alive[]/malformed[] without deleting.
#   T2. --gc deletes only dead-PID files; alive PIDs preserved.
#   T3. Malformed .pid content surfaces in malformed[] and is NEVER deleted.
#   T4. --gc is idempotent (second run sweeps zero).
#   T5. _pid-summary.jsonl receives one JSON line per swept file BEFORE delete.
#   T6. --gc-cheap is silent on success and performs the sweep.
#   T7. Empty ltads/execution/ produces empty briefing (gracefully absent).

set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

PYBIN=""
for c in python3 python; do
    if command -v "$c" >/dev/null 2>&1; then
        if "$c" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
            PYBIN="$c"; break
        fi
    fi
done

# Resolve a real, currently-alive Windows PID (for cross-MSYS visibility) when
# possible; otherwise use $$ (works on real Linux/macOS).
_pick_alive_pid() {
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*)
            local p
            p=$(powershell.exe -NoProfile -Command "(Get-Process -Name bash | Select-Object -First 1).Id" 2>/dev/null | tr -d '[:space:]')
            if [ -n "$p" ] && [ "$p" -gt 0 ] 2>/dev/null; then
                echo "$p"; return 0
            fi
            ;;
    esac
    echo "$$"
}

# =============================================================================
# Phase 1 — briefing path (best-effort)
# =============================================================================
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }
if [ -n "$PYBIN" ]; then
    if ! echo "$OUTPUT" | "$PYBIN" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        fail "briefing: output is not valid JSON ($OUTPUT)"
    fi
fi
for field in swept alive malformed briefing; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        fail "briefing: missing schema field '$field'"
    fi
done
pass "Phase1: briefing emits frozen schema"

# =============================================================================
# Phase 2 — sandbox sweep semantics
# =============================================================================
SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'pidoracle')"
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

mkdir -p "$SANDBOX/ltads/execution"
mkdir -p "$SANDBOX/.claude/oracles/pid-orphan-detector"
mkdir -p "$SANDBOX/.claude/scripts/lib"

# Place the oracle + helper at .claude/oracles paths so its lib-resolution
# walks `../../scripts/lib/pid-liveness.sh` and finds it under the sandbox.
cp "$ORACLE_DIR/run.sh"     "$SANDBOX/.claude/oracles/pid-orphan-detector/run.sh"
cp "$ORACLE_DIR/oracle.json" "$SANDBOX/.claude/oracles/pid-orphan-detector/oracle.json"

LIB_SRC=""
for cand in \
    "$ORACLE_DIR/../../scripts/lib/pid-liveness.sh" \
    "$ORACLE_DIR/../../../claude-template/scripts/lib/pid-liveness.sh"
do
    if [ -f "$cand" ]; then LIB_SRC="$cand"; break; fi
done
if [ -n "$LIB_SRC" ]; then
    cp "$LIB_SRC" "$SANDBOX/.claude/scripts/lib/pid-liveness.sh"
fi

ALIVE_PID=$(_pick_alive_pid)
[ -n "$ALIVE_PID" ] || { fail "Phase2: could not pick an alive PID for fixture"; exit 1; }
DEAD_PID=999999  # Effectively never-allocated

EXEC="$SANDBOX/ltads/execution"
echo "$ALIVE_PID" > "$EXEC/worker-shell-20260508-100000-001.pid"
echo "$DEAD_PID"  > "$EXEC/worker-shell-20260101-100000-002.pid"
echo "garbage"    > "$EXEC/worker-shell-20260101-100000-bogus.pid"

# T1: default mode lists, does not delete
DEFAULT_OUT=$(cd "$SANDBOX" && bash .claude/oracles/pid-orphan-detector/run.sh 2>&1)
echo "[default]: $DEFAULT_OUT"
echo "$DEFAULT_OUT" | grep -q "\"referenced_pid\":$ALIVE_PID" \
    && pass "T1: alive PID surfaces in alive[]" \
    || fail "T1: alive PID missing from output ($DEFAULT_OUT)"
echo "$DEFAULT_OUT" | grep -q "\"referenced_pid\":$DEAD_PID" \
    && pass "T1: dead PID surfaces in swept[]" \
    || fail "T1: dead PID missing from output ($DEFAULT_OUT)"
echo "$DEFAULT_OUT" | grep -q "worker-shell-20260101-100000-bogus.pid" \
    && pass "T1: malformed file surfaces in malformed[]" \
    || fail "T1: malformed file missing ($DEFAULT_OUT)"
[ -f "$EXEC/worker-shell-20260101-100000-002.pid" ] \
    && pass "T1: default mode does NOT delete dead-PID file" \
    || fail "T1: default mode deleted dead-PID file"

# T2: --gc deletes only dead PIDs
GC_OUT=$(cd "$SANDBOX" && bash .claude/oracles/pid-orphan-detector/run.sh --gc 2>&1)
echo "[--gc]: $GC_OUT"
[ -f "$EXEC/worker-shell-20260508-100000-001.pid" ] \
    && pass "T2: alive-PID file preserved by --gc" \
    || fail "T2: alive-PID file was deleted by --gc"
[ ! -f "$EXEC/worker-shell-20260101-100000-002.pid" ] \
    && pass "T2: dead-PID file deleted by --gc" \
    || fail "T2: dead-PID file was NOT deleted by --gc"

# T3: malformed file is preserved even in --gc
[ -f "$EXEC/worker-shell-20260101-100000-bogus.pid" ] \
    && pass "T3: malformed .pid preserved by --gc (failure-open)" \
    || fail "T3: malformed .pid was deleted (must NOT happen)"

# T5: _pid-summary.jsonl received the entry BEFORE delete (we can verify shape)
SUMMARY="$EXEC/_pid-summary.jsonl"
if [ -f "$SUMMARY" ]; then
    LINES=$(wc -l < "$SUMMARY" | tr -d ' ')
    [ "$LINES" = "1" ] && pass "T5: _pid-summary.jsonl has 1 line" \
        || fail "T5: _pid-summary.jsonl has $LINES lines, expected 1"
    LINE=$(head -1 "$SUMMARY")
    if [ -n "$PYBIN" ]; then
        echo "$LINE" | "$PYBIN" -c "import sys, json; json.loads(sys.stdin.read())" 2>/dev/null \
            && pass "T5: _pid-summary.jsonl line is valid JSON" \
            || fail "T5: _pid-summary.jsonl line is not valid JSON: $LINE"
    fi
    for f in pid_file referenced_pid liveness_at_sweep mtime sweptAt; do
        echo "$LINE" | grep -q "\"$f\"" || fail "T5: summary line missing field '$f'"
    done
    echo "$LINE" | grep -q "\"referenced_pid\":$DEAD_PID" \
        && pass "T5: summary references dead PID $DEAD_PID" \
        || fail "T5: summary missing referenced_pid=$DEAD_PID"
    echo "$LINE" | grep -q '"liveness_at_sweep":false' \
        && pass "T5: summary records liveness_at_sweep=false" \
        || fail "T5: summary missing liveness_at_sweep=false"
else
    fail "T5: _pid-summary.jsonl was not created"
fi

# T4: idempotence
GC_OUT2=$(cd "$SANDBOX" && bash .claude/oracles/pid-orphan-detector/run.sh --gc 2>&1)
SWEPT_COUNT2=$(echo "$GC_OUT2" | grep -oE '"swept":\[[^]]*\]' | grep -oc '"pid_file"' || true)
[ -z "$SWEPT_COUNT2" ] && SWEPT_COUNT2=0
if [ "$SWEPT_COUNT2" = "0" ]; then
    pass "T4: idempotence (second --gc swept zero)"
else
    fail "T4: second --gc swept $SWEPT_COUNT2 (expected 0)"
fi

# T6: --gc-cheap silent on success and performs sweep
echo "$DEAD_PID" > "$EXEC/worker-shell-20260102-100000-003.pid"
CHEAP_OUT=$(cd "$SANDBOX" && bash .claude/oracles/pid-orphan-detector/run.sh --gc-cheap 2>&1)
if [ -z "$(echo "$CHEAP_OUT" | tr -d '[:space:]')" ]; then
    pass "T6: --gc-cheap silent on success"
else
    fail "T6: --gc-cheap emitted output (should be silent): $CHEAP_OUT"
fi
[ ! -f "$EXEC/worker-shell-20260102-100000-003.pid" ] \
    && pass "T6: --gc-cheap performed the sweep" \
    || fail "T6: --gc-cheap did not delete dead-PID file"

# T7: empty exec dir -> empty briefing
EMPTY_SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'pidoracle2')"
trap "rm -rf '$SANDBOX' '$EMPTY_SANDBOX'" EXIT
mkdir -p "$EMPTY_SANDBOX/ltads/execution"
mkdir -p "$EMPTY_SANDBOX/.claude/oracles/pid-orphan-detector"
mkdir -p "$EMPTY_SANDBOX/.claude/scripts/lib"
cp "$ORACLE_DIR/run.sh"     "$EMPTY_SANDBOX/.claude/oracles/pid-orphan-detector/run.sh"
cp "$ORACLE_DIR/oracle.json" "$EMPTY_SANDBOX/.claude/oracles/pid-orphan-detector/oracle.json"
[ -n "$LIB_SRC" ] && cp "$LIB_SRC" "$EMPTY_SANDBOX/.claude/scripts/lib/pid-liveness.sh"

EMPTY_OUT=$(cd "$EMPTY_SANDBOX" && bash .claude/oracles/pid-orphan-detector/run.sh 2>&1)
echo "$EMPTY_OUT" | grep -q '"briefing":""' \
    && pass "T7: empty exec dir -> empty briefing field" \
    || fail "T7: empty exec dir did not yield empty briefing ($EMPTY_OUT)"
echo "$EMPTY_OUT" | grep -q '"swept":\[\]' \
    && pass "T7: empty exec dir -> swept[] empty" \
    || fail "T7: empty exec dir non-empty swept[] ($EMPTY_OUT)"

echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -gt 0 ] && exit 1
exit 0
