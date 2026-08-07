#!/bin/bash
# validate.sh -- self-test for the overseer-wake oracle (CSO Phase 1, C1).
# Two layers: (1) schema-field presence on the real tree; (2) fixture-driven
# wake/clear behavior in disposable sandbox projects.
set -u
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
RUN="$ORACLE_DIR/run.sh"
pass=0; fail=0
ok()  { echo "PASS: $1"; pass=$((pass+1)); }
no()  { echo "FAIL: $1"; fail=$((fail+1)); }

PY=""
if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then PY="python3";
elif command -v python  >/dev/null 2>&1 && python  -c "pass" >/dev/null 2>&1; then PY="python"; fi

valid_json() { [ -n "$PY" ] && printf '%s' "$1" | "$PY" -c "import sys,json;json.load(sys.stdin)" >/dev/null 2>&1; }
field() {  # field <json> <key> -> value via python (graceful)
    [ -n "$PY" ] || { echo ""; return; }
    printf '%s' "$1" | "$PY" -c "import sys,json
try: d=json.load(sys.stdin)
except Exception: print(''); sys.exit(0)
v=d.get('$2')
print(json.dumps(v) if isinstance(v,(list,dict,bool)) else (v if v is not None else ''))" 2>/dev/null
}

# ---- Layer 1: real-tree schema presence -------------------------------------
OUT="$(bash "$RUN" 2>&1)"
if valid_json "$OUT"; then ok "L1.1 emits valid JSON on the real tree"; else no "L1.1 invalid JSON: $OUT"; fi
for f in wake signals summary briefing; do
    if printf '%s' "$OUT" | grep -q "\"$f\""; then ok "L1.2 schema field '$f' present"; else no "L1.2 missing field '$f'"; fi
done

# ---- Sandbox helpers --------------------------------------------------------
mk_sandbox() { mktemp -d 2>/dev/null || mktemp -d -t cso-ow; }
write_registry() { # write_registry <dir> <json>
    mkdir -p "$1/.claude/collaboration"
    printf '%s' "$2" > "$1/.claude/collaboration/active-sessions.json"
}
run_in() { ( cd "$1" && bash "$RUN" 2>/dev/null ); }

# ---- Layer 2a: dead-PID active entry -> wake=true, stall-monitor ------------
SB="$(mk_sandbox)"
write_registry "$SB" '{"registryVersion":2,"sessions":[{"id":"worker-dead","status":"active","claudeShellPid":999999,"sessionRole":"orchestrated-worker"}],"closed":[]}'
O="$(run_in "$SB")"
if [ "$(field "$O" wake)" = "true" ]; then ok "L2a.1 dead-PID active entry -> wake=true"; else no "L2a.1 expected wake=true, got: $O"; fi
if printf '%s' "$O" | grep -q '"detectorId":"stall-monitor"'; then ok "L2a.2 emits a stall-monitor signal"; else no "L2a.2 no stall-monitor signal: $O"; fi
if printf '%s' "$O" | grep -q 'worker-dead'; then ok "L2a.3 signal names the dead session"; else no "L2a.3 dead session not named: $O"; fi
B="$(field "$O" briefing)"; if [ -n "$B" ] && [ "$B" != '""' ]; then ok "L2a.4 briefing non-empty when wake=true"; else no "L2a.4 briefing empty when wake=true"; fi
rm -rf "$SB"

# ---- Layer 2b: clean registry -> wake=false, briefing empty -----------------
SB="$(mk_sandbox)"
write_registry "$SB" '{"registryVersion":2,"sessions":[],"closed":[]}'
O="$(run_in "$SB")"
if [ "$(field "$O" wake)" = "false" ]; then ok "L2b.1 empty registry -> wake=false"; else no "L2b.1 expected wake=false, got: $O"; fi
B="$(field "$O" briefing)"; if [ -z "$B" ] || [ "$B" = '""' ]; then ok "L2b.2 briefing empty when wake=false"; else no "L2b.2 briefing should be empty: $B"; fi
# never a false-clear CLAIM: summary must mention sources observed or NO-DATA, not "all clear"
S="$(field "$O" summary)"; if printf '%s' "$S" | grep -qi 'no airspace signals\|NO-DATA'; then ok "L2b.3 honest no-signal summary (no fabricated all-clear)"; else no "L2b.3 summary suspicious: $S"; fi
rm -rf "$SB"

# ---- Layer 2c: live-PID active entry -> NOT a stall ------------------------
# Supply a genuinely-alive PID in the namespace the oracle's probe checks. On
# MSYS the probe is `powershell Get-Process` (Windows PIDs), so $$ (an MSYS PID)
# is the WRONG namespace -- spawn a hidden Windows process and use its Windows
# PID. On real Unix the probe is `kill -0` and $$ is correct.
LIVE_PID=""; LIVE_KILL=""
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        LIVE_PID="$(powershell.exe -NoProfile -Command "(Start-Process -FilePath ping -ArgumentList '-n','30','127.0.0.1' -PassThru -WindowStyle Hidden).Id" 2>/dev/null | tr -d '\r ')"
        LIVE_KILL="win"
        ;;
    *)
        LIVE_PID="$$"
        ;;
esac
if [ -n "$LIVE_PID" ]; then
    SB="$(mk_sandbox)"
    write_registry "$SB" "{\"registryVersion\":2,\"sessions\":[{\"id\":\"worker-live\",\"status\":\"active\",\"claudeShellPid\":$LIVE_PID,\"sessionRole\":\"orchestrated-worker\"}],\"closed\":[]}"
    O="$(run_in "$SB")"
    if printf '%s' "$O" | grep -q '"detectorId":"stall-monitor"'; then no "L2c.1 live PID wrongly flagged as stall: $O"; else ok "L2c.1 live-PID active entry is NOT a stall"; fi
    rm -rf "$SB"
else
    echo "SKIP: L2c.1 (could not obtain a live PID for the probe namespace)"
fi

# ---- Layer 2d: touches.json multi-claimant screen, liveness-gated -----------
# DEC-215 (DEFER-039 phantom fix): claimants only count when LIVE — backed by
# the collab dir's workers/<id>/shell.pid or an active registry entry. A
# claimant with no liveness evidence (the 5-month-dead Feb-dir shape) must NOT
# signal; two live claimants must. LIVE_PID (from L2c) is reused; it is killed
# after this layer.
mk_touches_sandbox() {  # <pidA> <pidB> (empty = no shell.pid) -> sandbox path
    local sb pa="$1" pb="$2"
    sb="$(mk_sandbox)"
    mkdir -p "$sb/.claude/collaboration/collab-test/file-tracking"
    printf '%s' '{"schemaVersion":"1.0","files":{"src/main.ts":{"lastTouchedBy":"CLAUDE-B","agents":["CLAUDE-A","CLAUDE-B"],"action":"MODIFIED"},"src/solo.ts":{"agents":["CLAUDE-A"],"action":"MODIFIED"}}}' \
        > "$sb/.claude/collaboration/collab-test/file-tracking/touches.json"
    if [ -n "$pa" ]; then mkdir -p "$sb/.claude/collaboration/collab-test/workers/CLAUDE-A"; printf '%s' "$pa" > "$sb/.claude/collaboration/collab-test/workers/CLAUDE-A/shell.pid"; fi
    if [ -n "$pb" ]; then mkdir -p "$sb/.claude/collaboration/collab-test/workers/CLAUDE-B"; printf '%s' "$pb" > "$sb/.claude/collaboration/collab-test/workers/CLAUDE-B/shell.pid"; fi
    write_registry "$sb" '{"registryVersion":2,"sessions":[],"closed":[]}'
    echo "$sb"
}

# Dead-claimants shape (no liveness evidence at all) -> NO signal
SB="$(mk_touches_sandbox "" "")"
O="$(run_in "$SB")"
if printf '%s' "$O" | grep -q '"detectorId":"touches-conflict"'; then no "L2d.0 dead claimants wrongly signalled (phantom): $O"; else ok "L2d.0 claimants without liveness evidence -> NO signal (DEC-215)"; fi
rm -rf "$SB"

if [ -n "$LIVE_PID" ]; then
    SB="$(mk_touches_sandbox "$LIVE_PID" "$LIVE_PID")"
    O="$(run_in "$SB")"
    if printf '%s' "$O" | grep -q '"detectorId":"touches-conflict"'; then ok "L2d.1 multi-claimant touches path (live claimants) -> touches-conflict signal"; else no "L2d.1 no touches-conflict signal: $O"; fi
    if printf '%s' "$O" | grep -q 'src/main.ts'; then ok "L2d.2 names the contested path"; else no "L2d.2 contested path not named: $O"; fi
    if printf '%s' "$O" | grep -q 'src/solo.ts'; then no "L2d.3 single-claimant path wrongly flagged"; else ok "L2d.3 single-claimant path NOT flagged"; fi
    rm -rf "$SB"
else
    echo "SKIP: L2d.1-3 (could not obtain a live PID for the probe namespace)"
fi
if [ "$LIVE_KILL" = "win" ] && [ -n "$LIVE_PID" ]; then powershell.exe -NoProfile -Command "Stop-Process -Id $LIVE_PID -Force -ErrorAction SilentlyContinue" >/dev/null 2>&1; fi

echo ""
echo "================================================================"
echo "overseer-wake validate: $pass passed, $fail failed"
echo "================================================================"
[ "$fail" -eq 0 ]
