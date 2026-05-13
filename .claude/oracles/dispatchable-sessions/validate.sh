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

set -e
ORACLE_DIR="$(dirname "$0")"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

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

# ---- Find an alive PID and a guaranteed-dead PID ----------------------------
# Spawn a sleeper process. $$ (bash PID) is invisible to PowerShell's
# Get-Process on Windows -- the Git-Bash PID lives in a separate namespace from
# Windows PIDs -- so we can't use it as the alive marker. Spawn a real OS
# process and clean it up via the EXIT trap.
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        ALIVE_PID=$(powershell.exe -NoProfile -Command "(Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 120' -PassThru -WindowStyle Hidden).Id" 2>/dev/null | tr -d '\r\n ')
        ALIVE_PID_OS="win"
        ;;
    *)
        sleep 120 &
        ALIVE_PID=$!
        ALIVE_PID_OS="posix"
        ;;
esac
if [ -z "$ALIVE_PID" ]; then
    echo "FAIL: could not spawn alive sleeper for fixture" >&2
    exit 1
fi
# Sanity: confirm the PID is actually alive before using it.
sleep 1
if [ "$ALIVE_PID_OS" = "win" ]; then
    if ! powershell.exe -NoProfile -Command "if (Get-Process -Id $ALIVE_PID -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1; then
        echo "FAIL: spawned alive sleeper PID $ALIVE_PID was dead immediately after spawn" >&2
        exit 1
    fi
else
    if ! kill -0 "$ALIVE_PID" 2>/dev/null; then
        echo "FAIL: spawned alive sleeper PID $ALIVE_PID was dead immediately after spawn" >&2
        exit 1
    fi
fi

DEAD_PID=999999

# Confirm the dead PID is actually dead. If not (very unlikely), bump it.
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
with open(sys.argv[1], "r", encoding="utf-8") as f: d=json.load(f)
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
with open(sys.argv[1], "r", encoding="utf-8") as f: d=json.load(f)
print(json.dumps({"n_active": len(d.get("sessions") or []), "n_closed": len(d.get("closed") or [])}))
PY
)
echo "$POST_CHEAP" | grep -q '"n_active": 0' && echo "$POST_CHEAP" | grep -q '"n_closed": 1' \
    && pass "T7: --gc-cheap performed the sweep (active=0, closed=1)" \
    || fail "T7: --gc-cheap did not sweep correctly: $POST_CHEAP"

# =============================================================================
# Summary
# =============================================================================
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
if [ "$FAIL" -gt 0 ]; then exit 1; fi
exit 0
