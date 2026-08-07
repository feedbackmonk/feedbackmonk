#!/bin/bash
# dispatchable-sessions oracle self-test (Unix)
#
# Phase 1: validate the read-only briefing path against the real registry.
# Phase 2: validate --gc and --gc-cheap sweep semantics in a sandbox:
#   T1. Sweep flips dead-PID + old-spawnedAt entries to status=expired and moves them to closed[].
#   T2. Sweep does NOT touch live-PID entries (regardless of age).
#   T3. Sweep does NOT touch dead-PID entries that are younger than threshold (age guard).
#   T4. Sweep is idempotent: re-running on the post-sweep registry sweeps zero more.
#   T5. On-demand --gc emits a JSON summary {swept,before,after,threshold,thresholdSource}.
#   T6. .claude/config.json csi.registryHygieneThreshold is honored.
#   T7. --gc-cheap is silent on success and performs the sweep.
#   T8. Minimum-progress guarantee: even under a starved budget (ULDF_DS_GC_BUDGET_MS=1)
#       the FIRST candidate is probed and swept -- setup cost can never starve the
#       whole sweep (DEFER-064: pre-fix, the budget clock started before the parser
#       fork and MSYS setup cost alone deferred every sweep, silently).
#   T9. Bounded tail deferral: budget exhaustion after the first probe defers the
#       TAIL only; already-collected candidates are still swept (MSYS-only cell --
#       needs multi-second probe cost to be deterministic; SKIPs elsewhere).
#   T10. Convergence: repeated cheap passes drain the deferred backlog to zero
#       (DISC-ORA-05 rule; MSYS-only, rides the T9 fixture).
# Phase 3: validate --duplicate-of semantics (RESUME-03) in the same sandbox:
#   D1. No active entry for the identity -> duplicate:false.
#   D2. Live non-self holder, same workDir -> duplicate:true.
#   D3. Dead-PID holder -> duplicate:false (stale).
#   D4. Holder pid in the caller's own ancestor chain -> duplicate:false, isSelf:true.
#   D5. Live non-self holder, DIFFERENT workDir -> duplicate:false (cross-project guard).

set -e
ORACLE_DIR="$(dirname "$0")"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }
SKIP=0

# DEFER-086 / DEC-247: T7 asserts the outcome of a --gc-cheap sweep, whose
# subject runs under a wall-clock budget. Under machine load the budget can
# expire before the sweep completes, and the cell then reports a true statement
# about a starved run as a failure of sweep semantics -- a red that is not
# evidence. NOTE this oracle already has SWEEP-09's minimum-progress guarantee
# (it is the DEFER-064 precedent), so a *total* starvation is structurally
# impossible here; what remains exposed is a multi-entry sweep truncated
# mid-way. The budget is NOT widened (OVALID-05: defer, never weaken).
_TG_LIB="$ORACLE_DIR/../../scripts/lib/timing-guard.sh"
[ -f "$_TG_LIB" ] || _TG_LIB="$ORACLE_DIR/../../../claude-template/scripts/lib/timing-guard.sh"
[ -f "$_TG_LIB" ] || _TG_LIB="$HOME/.claude/scripts/lib/timing-guard.sh"
if [ -f "$_TG_LIB" ]; then
    . "$_TG_LIB"
else
    # Graceful absence: grade normally (status quo ante), never silently skip.
    timing_guard_declare() { :; }; timing_guard_deferred() { return 1; }
    timing_guard_note() { :; }; timing_guard_summary() { :; }
fi
timing_guard_declare "dispatchable-sessions --gc-cheap 1000ms probe budget (T7)"
fail_timed() {
    if timing_guard_deferred; then
        SKIP=$((SKIP+1)); echo "SKIP: $1" >&2; echo "      $(timing_guard_note)" >&2
    else
        fail "$1"
    fi
}

PYBIN=""
# Avoid the Windows-Store python3 stub (`python3 --version` errors out with the "install from Store" message).
# Probe by running a trivial python command and checking exit code.
for _candidate in python3 python; do
    if command -v "$_candidate" >/dev/null 2>&1; then
        if "$_candidate" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
            PYBIN="$_candidate"
            break
        fi
    fi
done

# =============================================================================
# Phase 1 — briefing path against the real registry
# =============================================================================

OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

# Output must be valid JSON
if [ -n "$PYBIN" ]; then
    if ! echo "$OUTPUT" | "$PYBIN" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        echo "Output: $OUTPUT" >&2
        exit 1
    fi
else
    echo "(skip JSON validity check; no python)" >&2
fi

# Required schema fields
for field in count peers briefing; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        fail "briefing: missing schema field '$field'"
    fi
done

# count must be a non-negative integer
COUNT=$(echo "$OUTPUT" | grep -oE '"count"[[:space:]]*:[[:space:]]*[0-9]+' | grep -oE '[0-9]+$')
if [ -z "$COUNT" ]; then
    fail "briefing: 'count' is not a non-negative integer"
else
    pass "briefing: count=$COUNT"
fi

# When count==0, briefing must include "No live siblings"
if [ "$COUNT" = "0" ]; then
    if ! echo "$OUTPUT" | grep -q '"briefing":"No live siblings'; then
        fail "briefing: count=0 but briefing does not start with 'No live siblings'"
    fi
fi

# =============================================================================
# Phase 2 — --gc / --gc-cheap sweep semantics in a sandbox
# =============================================================================

if [ -z "$PYBIN" ]; then
    echo "SKIP: Phase 2 (--gc tests) requires python for fixture build"
    if [ "$FAIL" -gt 0 ]; then exit 1; fi
    exit 0
fi

SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'csi05')"
ALIVE_PID=""
ALIVE_PID_OS=""  # "win" or "posix" -- governs cleanup
cleanup() {
    rm -rf "$SANDBOX"
    if [ -n "$ALIVE_PID" ]; then
        if [ "$ALIVE_PID_OS" = "win" ]; then
            powershell.exe -NoProfile -Command "Stop-Process -Id $ALIVE_PID -Force -ErrorAction SilentlyContinue" >/dev/null 2>&1 || true
        else
            kill "$ALIVE_PID" 2>/dev/null || true
        fi
    fi
}
trap cleanup EXIT

# ---- Liveness probe helper (needed by the sleeper guard below) --------------
PID_PROBE="kill"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*) PID_PROBE="powershell" ;;
esac
is_alive_check() {
    if [ "$PID_PROBE" = "powershell" ]; then
        powershell.exe -NoProfile -Command "if (Get-Process -Id $1 -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1
    else
        kill -0 "$1" 2>/dev/null
    fi
}

# ---- Find an alive PID and a guaranteed-dead PID ----------------------------
# Spawn a sleeper process. $$ (bash PID) is invisible to PowerShell's
# Get-Process on Windows -- the Git-Bash PID lives in a separate namespace from
# Windows PIDs -- so we can't use it as the alive marker. Spawn a real OS
# process and clean it up via the EXIT trap.
#
# FIXTURE-LIFETIME CONTRACT (DEFER-064): the sleeper must outlive the WHOLE
# validator run. At 120s it did not -- on MSYS every liveness probe / ancestor
# walk forks powershell.exe (~1.5s each) and the full run takes ~4.5 min, so
# the sleeper expired between D2 and D3 and the later liveness cells (D5,
# S1-S3) asserted "alive" about a genuinely dead PID. The oracle answered
# correctly; the harness lied about its own fixture -- pseudo-deterministically,
# because the fork count is fixed. Two defenses: a 1800s lifetime (headroom for
# future cells; the EXIT trap still reaps it) and ensure_sleeper(), which
# re-verifies liveness at every point a fixture write embeds $ALIVE_PID and
# re-spawns on expiry, so a dead fixture can never again masquerade as an
# oracle liveness bug.
SLEEPER_SECONDS=1800
spawn_sleeper() {
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*)
            ALIVE_PID=$(powershell.exe -NoProfile -Command "(Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds $SLEEPER_SECONDS' -PassThru -WindowStyle Hidden).Id" 2>/dev/null | tr -d '\r\n ')
            ALIVE_PID_OS="win"
            ;;
        *)
            sleep "$SLEEPER_SECONDS" &
            ALIVE_PID=$!
            ALIVE_PID_OS="posix"
            ;;
    esac
    [ -n "$ALIVE_PID" ] || return 1
    # Sanity: confirm the PID is actually alive before using it.
    sleep 1
    is_alive_check "$ALIVE_PID"
}
ensure_sleeper() {
    # $1 = label of the cell/write about to rely on the sleeper
    if is_alive_check "$ALIVE_PID"; then return 0; fi
    echo "[harness] fixture sleeper (pid $ALIVE_PID) not alive before $1 -- re-spawning." >&2
    echo "[harness] This is a fixture-lifetime event, NOT an oracle verdict (DEFER-064)." >&2
    if spawn_sleeper; then
        echo "[harness] re-spawned fixture sleeper as pid $ALIVE_PID" >&2
        return 0
    fi
    fail "$1: fixture sleeper died and could not be re-spawned -- harness defect, not an oracle verdict"
    return 1
}

if ! spawn_sleeper; then
    echo "FAIL: could not spawn alive sleeper for fixture (or it died immediately)" >&2
    exit 1
fi

DEAD_PID=999999

# Confirm the dead PID is actually dead. If not (very unlikely), bump it.
while is_alive_check "$DEAD_PID"; do
    DEAD_PID=$((DEAD_PID + 1))
done

# Timestamps: now-25h (older than default 24h threshold) and now (younger).
NOW_EPOCH=$(date -u +%s)
OLD_EPOCH=$((NOW_EPOCH - 25 * 3600))   # 25 hours ago, beyond 24h threshold
RECENT_EPOCH=$((NOW_EPOCH - 60))       # 60 seconds ago

OLD_ISO=$("$PYBIN" -c "
import sys
from datetime import datetime, timezone
print(datetime.fromtimestamp(int(sys.argv[1]), tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))
" "$OLD_EPOCH")
RECENT_ISO=$("$PYBIN" -c "
import sys
from datetime import datetime, timezone
print(datetime.fromtimestamp(int(sys.argv[1]), tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))
" "$RECENT_EPOCH")

# ---- Build fixture registry -------------------------------------------------
mkdir -p "$SANDBOX/.claude/collaboration"
mkdir -p "$SANDBOX/.claude/oracles/dispatchable-sessions"

# Copy oracle files into the sandbox so relative-path lookups work.
cp "$ORACLE_DIR/run.sh" "$SANDBOX/.claude/oracles/dispatchable-sessions/run.sh"
cp "$ORACLE_DIR/oracle.json" "$SANDBOX/.claude/oracles/dispatchable-sessions/oracle.json"

REG="$SANDBOX/.claude/collaboration/active-sessions.json"

"$PYBIN" - "$REG" "$ALIVE_PID" "$DEAD_PID" "$OLD_ISO" "$RECENT_ISO" <<'PY'
import json, sys
reg, alive_pid, dead_pid, old_iso, recent_iso = sys.argv[1:6]
data = {
  "sessions": [
    # 0: dead PID, OLD spawnedAt -> SHOULD be swept
    {"id":"DEAD-OLD","sessionRole":"pods-worker","claudeShellPid":int(dead_pid),"status":"active","dispatchable":True,"spawnedAt":old_iso,"role":"to-sweep"},
    # 1: alive PID, OLD spawnedAt -> NEVER swept (live PIDs are protected regardless of age)
    {"id":"ALIVE-OLD","sessionRole":"pods-worker","claudeShellPid":int(alive_pid),"status":"active","dispatchable":True,"spawnedAt":old_iso,"role":"alive-guard"},
    # 2: dead PID, RECENT spawnedAt -> protected by age guard
    {"id":"DEAD-RECENT","sessionRole":"pods-worker","claudeShellPid":int(dead_pid),"status":"active","dispatchable":True,"spawnedAt":recent_iso,"role":"age-guard"},
    # 3: status=ended -> not a sweep candidate (only status=active is considered)
    {"id":"ENDED","sessionRole":"pods-worker","claudeShellPid":int(dead_pid),"status":"ended","dispatchable":True,"spawnedAt":old_iso,"role":"non-active"}
  ],
  "stale": [],
  "closed": [],
  "lastUpdated": None
}
with open(reg, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
PY

# ---- T1+T2+T3+T5: run --gc and inspect the summary + registry ---------------
GC_OUT=$(cd "$SANDBOX" && bash .claude/oracles/dispatchable-sessions/run.sh --gc 2>&1)
echo "[--gc summary]: $GC_OUT"

# T5: summary shape
for f in swept before after threshold thresholdSource; do
    if ! echo "$GC_OUT" | grep -q "\"$f\""; then
        fail "T5: --gc summary missing field '$f' (got: $GC_OUT)"
    fi
done
echo "$GC_OUT" | grep -q '"swept":1' && pass "T1: --gc swept=1 (only DEAD-OLD)" || fail "T1: --gc swept != 1 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"before":4' && pass "T5: --gc before=4" || fail "T5: --gc before != 4 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"after":3'  && pass "T5: --gc after=3"  || fail "T5: --gc after != 3 (got: $GC_OUT)"

# Inspect post-sweep registry
POST_T1=$("$PYBIN" - "$REG" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8-sig") as f: d=json.load(f)
ids = [s.get("id") for s in d.get("sessions") or []]
closed_ids = [c.get("id") for c in d.get("closed") or []]
closed_status = [c.get("status") for c in d.get("closed") or []]
closed_swept = [bool(c.get("sweptAt")) for c in d.get("closed") or []]
print(json.dumps({"sessions_ids": ids, "closed_ids": closed_ids, "closed_status": closed_status, "closed_sweptAt_present": closed_swept}))
PY
)
echo "[post-T1 registry]: $POST_T1"

echo "$POST_T1" | grep -q '"sessions_ids": \["ALIVE-OLD", "DEAD-RECENT", "ENDED"\]' \
    && pass "T1+T2+T3: sessions[] is ALIVE-OLD + DEAD-RECENT + ENDED" \
    || fail "T1+T2+T3: sessions[] unexpected: $POST_T1"
echo "$POST_T1" | grep -q '"closed_ids": \["DEAD-OLD"\]' \
    && pass "T1: closed[] received DEAD-OLD" \
    || fail "T1: closed[] missing DEAD-OLD: $POST_T1"
echo "$POST_T1" | grep -q '"closed_status": \["expired"\]' \
    && pass "T1: closed[].status == expired" \
    || fail "T1: closed[].status not expired: $POST_T1"
echo "$POST_T1" | grep -q '"closed_sweptAt_present": \[true\]' \
    && pass "T1: closed[].sweptAt set" \
    || fail "T1: closed[].sweptAt missing: $POST_T1"

# ---- T4: idempotence -- re-running --gc should now sweep zero ---------------
GC_OUT2=$(cd "$SANDBOX" && bash .claude/oracles/dispatchable-sessions/run.sh --gc 2>&1)
echo "[second --gc]: $GC_OUT2"
echo "$GC_OUT2" | grep -q '"swept":0' \
    && pass "T4: idempotence (second --gc swept=0)" \
    || fail "T4: idempotence violated (got: $GC_OUT2)"

# ---- T6: config.json threshold honored --------------------------------------
# Set threshold to 12h via config.json, rebuild fixture so DEAD-RECENT (60s) is
# still protected, but a 13-hour-old entry would be swept too. Use a new
# 13-hour-old entry to exercise it.
mkdir -p "$SANDBOX/.claude"
echo '{"csi":{"registryHygieneThreshold":12}}' > "$SANDBOX/.claude/config.json"

THIRTEEN_EPOCH=$((NOW_EPOCH - 13 * 3600))
THIRTEEN_ISO=$("$PYBIN" -c "
import sys
from datetime import datetime, timezone
print(datetime.fromtimestamp(int(sys.argv[1]), tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))
" "$THIRTEEN_EPOCH")

"$PYBIN" - "$REG" "$DEAD_PID" "$THIRTEEN_ISO" <<'PY'
import json, sys
reg, dead_pid, thirteen_iso = sys.argv[1:4]
data = {
  "sessions": [
    {"id":"DEAD-13H","sessionRole":"pods-worker","claudeShellPid":int(dead_pid),"status":"active","dispatchable":True,"spawnedAt":thirteen_iso,"role":"threshold-test"}
  ],
  "stale": [],
  "closed": [],
  "lastUpdated": None
}
with open(reg, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
PY

GC_OUT3=$(cd "$SANDBOX" && bash .claude/oracles/dispatchable-sessions/run.sh --gc 2>&1)
echo "[--gc with config 12h]: $GC_OUT3"
echo "$GC_OUT3" | grep -q '"swept":1'                         && pass "T6: config 12h threshold sweeps DEAD-13H" || fail "T6: 13h entry NOT swept under 12h threshold (got: $GC_OUT3)"
echo "$GC_OUT3" | grep -q '"thresholdSource":"config"'        && pass "T6: thresholdSource=config"               || fail "T6: thresholdSource != config (got: $GC_OUT3)"

# Re-set fixture and verify --gc-cheap is silent on success and sweeps too.
"$PYBIN" - "$REG" "$DEAD_PID" "$OLD_ISO" <<'PY'
import json, sys
reg, dead_pid, old_iso = sys.argv[1:4]
data = {
  "sessions": [
    {"id":"DEAD-OLD-C","sessionRole":"pods-worker","claudeShellPid":int(dead_pid),"status":"active","dispatchable":True,"spawnedAt":old_iso,"role":"cheap-test"}
  ],
  "stale": [],
  "closed": [],
  "lastUpdated": None
}
with open(reg, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
PY
# Drop config.json to fall back to default 24h.
rm -f "$SANDBOX/.claude/config.json"

CHEAP_OUT=$(cd "$SANDBOX" && bash .claude/oracles/dispatchable-sessions/run.sh --gc-cheap 2>&1)
if [ -z "$(echo "$CHEAP_OUT" | tr -d '[:space:]')" ]; then
    pass "T7: --gc-cheap silent on success"
else
    fail "T7: --gc-cheap emitted output (should be silent): $CHEAP_OUT"
fi

POST_CHEAP=$("$PYBIN" - "$REG" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8-sig") as f: d=json.load(f)
print(json.dumps({"n_active": len(d.get("sessions") or []), "n_closed": len(d.get("closed") or [])}))
PY
)
echo "$POST_CHEAP" | grep -q '"n_active": 0' && echo "$POST_CHEAP" | grep -q '"n_closed": 1' \
    && pass "T7: --gc-cheap performed the sweep (active=0, closed=1)" \
    || fail_timed "T7: --gc-cheap did not sweep correctly: $POST_CHEAP"

# ---- T8: minimum-progress guarantee under a starved budget (DEFER-064) ------
# Seed a 1ms budget via the ULDF_DS_GC_BUDGET_MS seam. Setup cost alone dwarfs
# 1ms, so pre-fix code (budget clock started before the candidates() parser
# fork; no first-probe exemption) deferred BEFORE the first probe -- this cell
# is the DEFER-064 T7 mechanism made deterministic, red under the old code and
# green under the fix (first candidate is always probed).
"$PYBIN" - "$REG" "$DEAD_PID" "$OLD_ISO" <<'PY'
import json, sys
reg, dead_pid, old_iso = sys.argv[1:4]
data = {
  "sessions": [
    {"id":"DEAD-OLD-T8","sessionRole":"pods-worker","claudeShellPid":int(dead_pid),"status":"active","dispatchable":True,"spawnedAt":old_iso,"role":"min-progress-test"}
  ],
  "stale": [],
  "closed": [],
  "lastUpdated": None
}
with open(reg, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
PY

T8_OUT=$(cd "$SANDBOX" && ULDF_DS_GC_BUDGET_MS=1 bash .claude/oracles/dispatchable-sessions/run.sh --gc-cheap 2>&1)
if [ -z "$(echo "$T8_OUT" | tr -d '[:space:]')" ]; then
    pass "T8: starved-budget --gc-cheap still silent"
else
    fail "T8: starved-budget --gc-cheap emitted output: $T8_OUT"
fi
POST_T8=$("$PYBIN" - "$REG" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8-sig") as f: d=json.load(f)
print(json.dumps({"n_active": len(d.get("sessions") or []), "n_closed": len(d.get("closed") or [])}))
PY
)
echo "$POST_T8" | grep -q '"n_active": 0' && echo "$POST_T8" | grep -q '"n_closed": 1' \
    && pass "T8: min-progress guarantee -- first candidate swept even at 1ms budget" \
    || fail "T8: starved budget starved the WHOLE sweep (setup cost consumed the budget): $POST_T8"

# ---- T9: bounded tail deferral (MSYS-only; probe cost makes it deterministic) ----
# Two dead+old entries, 1ms budget: the first is probed (min-progress) and the
# multi-second powershell probe guarantees the budget is exhausted before the
# second -- so exactly the TAIL defers and the collected head still sweeps.
# On POSIX, kill -0 probes are sub-millisecond and both entries may lawfully
# sweep inside the budget, so the cell SKIPs there (never a silent pass).
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        "$PYBIN" - "$REG" "$DEAD_PID" "$OLD_ISO" <<'PY'
import json, sys
reg, dead_pid, old_iso = sys.argv[1:4]
data = {
  "sessions": [
    {"id":"DEAD-T9-A","sessionRole":"pods-worker","claudeShellPid":int(dead_pid),"status":"active","dispatchable":True,"spawnedAt":old_iso,"role":"tail-deferral-a"},
    {"id":"DEAD-T9-B","sessionRole":"pods-worker","claudeShellPid":int(dead_pid),"status":"active","dispatchable":True,"spawnedAt":old_iso,"role":"tail-deferral-b"}
  ],
  "stale": [],
  "closed": [],
  "lastUpdated": None
}
with open(reg, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
PY
        (cd "$SANDBOX" && ULDF_DS_GC_BUDGET_MS=1 bash .claude/oracles/dispatchable-sessions/run.sh --gc-cheap >/dev/null 2>&1)
        POST_T9=$("$PYBIN" - "$REG" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8-sig") as f: d=json.load(f)
print(json.dumps({"n_active": len(d.get("sessions") or []), "n_closed": len(d.get("closed") or [])}))
PY
)
        echo "$POST_T9" | grep -q '"n_active": 1' && echo "$POST_T9" | grep -q '"n_closed": 1' \
            && pass "T9: tail deferred, head swept (bounded deferral)" \
            || fail "T9: expected head swept + tail deferred, got: $POST_T9"

        # ---- T10: convergence -- repeated cheap passes drain the deferred tail ----
        # DISC-ORA-05's rule: test that a backlog drains to zero across repeated
        # invocations, not just that one item sweeps under a lucky budget.
        (cd "$SANDBOX" && ULDF_DS_GC_BUDGET_MS=1 bash .claude/oracles/dispatchable-sessions/run.sh --gc-cheap >/dev/null 2>&1)
        POST_T10=$("$PYBIN" - "$REG" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8-sig") as f: d=json.load(f)
print(json.dumps({"n_active": len(d.get("sessions") or []), "n_closed": len(d.get("closed") or [])}))
PY
)
        echo "$POST_T10" | grep -q '"n_active": 0' && echo "$POST_T10" | grep -q '"n_closed": 2' \
            && pass "T10: second cheap pass drains the deferred tail (convergence)" \
            || fail "T10: backlog did not converge to zero, got: $POST_T10"
        ;;
    *)
        echo "SKIP: T9/T10 (deterministic only where probes cost multi-second, i.e. MSYS)"
        ;;
esac

# =============================================================================
# Phase 3 -- --duplicate-of semantics (RESUME-03)
# =============================================================================

# The sandbox path as the oracle will see it (Windows form under MSYS).
DUP_SANDBOX_WD="$SANDBOX"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        if command -v cygpath >/dev/null 2>&1; then
            DUP_SANDBOX_WD="$(cygpath -w "$SANDBOX")"
        fi
        ;;
esac

# An ancestor PID of the oracle invocation (for the isSelf test, D4): this
# validate script's own process. Under MSYS, bash PIDs live in a separate
# namespace from Windows PIDs and the oracle's ancestor walk runs in the
# WINDOWS tree -- use this bash's WINPID (column 4 of MSYS ps).
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*) SELF_ANCESTOR="$(ps -p $$ 2>/dev/null | awk 'NR==2 {print $4}')" ;;
    *)                    SELF_ANCESTOR=$$ ;;
esac

write_dup_fixture() {
    # $1=pid  $2=workDir
    "$PYBIN" - "$REG" "$1" "$2" <<'PY'
import json, sys
reg, pid, wd = sys.argv[1:4]
data = {
  "sessions": [
    {"id":"CLAUDE-DUP","sessionRole":"pods-worker","claudeShellPid":int(pid),"status":"active","dispatchable":True,"spawnedAt":"2026-06-12T00:00:00Z","workDir":wd}
  ],
  "stale": [], "closed": [], "lastUpdated": None
}
with open(reg, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
PY
}

run_dup() {
    (cd "$SANDBOX" && bash .claude/oracles/dispatchable-sessions/run.sh "--duplicate-of=$1" 2>&1)
}

# D1: no active entry for the queried identity
ensure_sleeper "D1/D2 fixture write"
write_dup_fixture "$ALIVE_PID" "$DUP_SANDBOX_WD"
D1_OUT="$(run_dup CLAUDE-NOPE)"
echo "$D1_OUT" | grep -q '"duplicate":false' && echo "$D1_OUT" | grep -q 'no active registry entry' \
    && pass "D1: no entry -> duplicate:false" \
    || fail "D1: unexpected output: $D1_OUT"

# D2: live non-self holder in the SAME workDir -> duplicate:true
D2_OUT="$(run_dup CLAUDE-DUP)"
echo "$D2_OUT" | grep -q '"duplicate":true' && echo "$D2_OUT" | grep -q '"workDirMatch":true' && echo "$D2_OUT" | grep -q '"isSelf":false' \
    && pass "D2: live non-self same-workDir holder -> duplicate:true" \
    || fail "D2: unexpected output: $D2_OUT"

# D3: dead-PID holder -> duplicate:false (stale)
write_dup_fixture "$DEAD_PID" "$DUP_SANDBOX_WD"
D3_OUT="$(run_dup CLAUDE-DUP)"
echo "$D3_OUT" | grep -q '"duplicate":false' && echo "$D3_OUT" | grep -q 'holder pid dead' \
    && pass "D3: dead holder -> duplicate:false (stale)" \
    || fail "D3: unexpected output: $D3_OUT"

# D4: holder pid is in the caller's own ancestor chain -> isSelf, duplicate:false
if [ -n "$SELF_ANCESTOR" ]; then
    write_dup_fixture "$SELF_ANCESTOR" "$DUP_SANDBOX_WD"
    D4_OUT="$(run_dup CLAUDE-DUP)"
    echo "$D4_OUT" | grep -q '"duplicate":false' && echo "$D4_OUT" | grep -q '"isSelf":true' \
        && pass "D4: self-held identity -> duplicate:false, isSelf:true" \
        || fail "D4: unexpected output: $D4_OUT"
else
    echo "SKIP: D4 (could not determine own ancestor PID)"
fi

# D5: live non-self holder in a DIFFERENT workDir -> duplicate:false
ensure_sleeper "D5 fixture write"
write_dup_fixture "$ALIVE_PID" "/some/other/project"
D5_OUT="$(run_dup CLAUDE-DUP)"
echo "$D5_OUT" | grep -q '"duplicate":false' && echo "$D5_OUT" | grep -q '"workDirMatch":false' \
    && pass "D5: different-workDir holder -> duplicate:false (cross-project guard)" \
    || fail "D5: unexpected output: $D5_OUT"

# =============================================================================
# Phase 4 -- briefing-path self-exclusion (DISC-CSI-22)
# =============================================================================
# A session's own registry entry must not be reported to it as a live sibling.
# Exclusion keys on ULDF_SELF_SESSION_ID (hook-set) > CLAUDE_SESSION_ID env.

write_self_fixture() {
    # $1=pid  $2=include_peer (1|0)
    "$PYBIN" - "$REG" "$1" "$2" <<'PY'
import json, sys
reg, pid, include_peer = sys.argv[1:4]
sessions = [
  {"id":"SELF-ENTRY","sessionRole":"interactive","claudeShellPid":int(pid),"status":"active","dispatchable":True,"spawnedAt":"2026-07-07T00:00:00Z","workDir":"X","registryVersion":2}
]
if include_peer == "1":
    sessions.append({"id":"PEER-ENTRY","sessionRole":"pods-worker","claudeShellPid":int(pid),"status":"active","dispatchable":True,"spawnedAt":"2026-07-07T00:00:00Z","workDir":"X","registryVersion":2})
data = {"sessions": sessions, "stale": [], "closed": [], "lastUpdated": None}
with open(reg, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
PY
}

run_briefing_env() {
    # $1=ULDF_SELF_SESSION_ID value ("" = unset)  $2=CLAUDE_SESSION_ID value ("" = unset)
    (cd "$SANDBOX" && env -u ULDF_SELF_SESSION_ID -u CLAUDE_SESSION_ID \
        ${1:+ULDF_SELF_SESSION_ID="$1"} ${2:+CLAUDE_SESSION_ID="$2"} \
        bash .claude/oracles/dispatchable-sessions/run.sh 2>&1)
}

if [ -n "$ALIVE_PID" ]; then
    ensure_sleeper "S1-S3 fixture write"
    write_self_fixture "$ALIVE_PID" 1

    # S1: ULDF_SELF_SESSION_ID excludes own entry, peer survives
    S1_OUT="$(run_briefing_env SELF-ENTRY "")"
    echo "$S1_OUT" | grep -q '"count":1' && echo "$S1_OUT" | grep -q 'PEER-ENTRY' && ! echo "$S1_OUT" | grep -q 'SELF-ENTRY' \
        && pass "S1: ULDF_SELF_SESSION_ID excludes self, keeps peer" \
        || fail "S1: unexpected output: $S1_OUT"

    # S2: no identity env -> both entries reported (legacy no-exclusion path)
    S2_OUT="$(run_briefing_env "" "")"
    echo "$S2_OUT" | grep -q '"count":2' \
        && pass "S2: no identity env -> no exclusion (count 2)" \
        || fail "S2: unexpected output: $S2_OUT"

    # S3: CLAUDE_SESSION_ID fallback excludes own entry
    S3_OUT="$(run_briefing_env "" SELF-ENTRY)"
    echo "$S3_OUT" | grep -q '"count":1' && ! echo "$S3_OUT" | grep -q 'SELF-ENTRY' \
        && pass "S3: CLAUDE_SESSION_ID fallback excludes self" \
        || fail "S3: unexpected output: $S3_OUT"

    # S4: self is the ONLY live entry -> canonical empty output
    ensure_sleeper "S4 fixture write"
    write_self_fixture "$ALIVE_PID" 0
    S4_OUT="$(run_briefing_env SELF-ENTRY "")"
    echo "$S4_OUT" | grep -q '"count":0' && echo "$S4_OUT" | grep -q 'No live siblings' \
        && pass "S4: self-only registry -> canonical empty output" \
        || fail "S4: unexpected output: $S4_OUT"
else
    echo "SKIP: Phase 4 (no alive fixture PID)"
fi

# =============================================================================
# Summary
# =============================================================================
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL  DEFERRED=$SKIP"
timing_guard_summary
if [ "$FAIL" -gt 0 ]; then exit 1; fi
exit 0
