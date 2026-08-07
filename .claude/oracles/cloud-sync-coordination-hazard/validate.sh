#!/bin/bash
# cloud-sync-coordination-hazard oracle self-test (Unix / Git Bash)
#
# T1. Emits valid single-line JSON with the frozen schema fields.
# T2. A clean (non-synced) root => hosted=false and an EMPTY briefing
#     (empty briefing is what suppresses the session-start line).
# T3. A synthetic OneDrive-segment root => hosted=true, provider reported,
#     non-empty briefing.
# T4. Segment-awareness: a path merely CONTAINING the provider name as a
#     substring of a larger segment (e.g. "OneDriveTools") must NOT trip.
#     This is the false-positive guard -- an advisory that cries wolf gets
#     ignored, which is worse than no advisory.
# T5. Provider list is configurable via ULDF_CLOUD_SYNC_PROVIDERS.
# T6. at_risk_paths names only coordination surfaces that actually exist.
# T7. Graceful absence: an unresolvable root must not fabricate a verdict.

set -uo pipefail
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"

PASS=0; FAIL=0
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }

PYBIN=""
for c in python3 python; do
    if command -v "$c" >/dev/null 2>&1 && "$c" -c "import sys" >/dev/null 2>&1; then PYBIN="$c"; break; fi
done

SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'csch')"
cleanup() {
    if [ -n "${SANDBOX:-}" ] && [ -d "$SANDBOX" ]; then rm -rf "$SANDBOX"; fi
    # Must return 0: called explicitly AND by the EXIT trap, and an EXIT trap's
    # status overrides an explicit `exit 0` (the exact defect DEFER-022 found in
    # workspace-shared-repos, which reported 37/37 while exiting 1).
    return 0
}
trap cleanup EXIT

run_in() {  # run_in <dir>  -> stdout JSON
    ( cd "$1" && bash "$ORACLE_DIR/run.sh" 2>/dev/null )
}

mk_root() {  # mk_root <path> ; makes a git-less dir with a coordination store
    mkdir -p "$1/.claude/collaboration" "$1/.claude/session-state"
    echo '{"sessions":[]}' > "$1/.claude/collaboration/active-sessions.json"
}

# --- T1 + T2: clean root -------------------------------------------------------
CLEAN="$SANDBOX/plainlocal/project"
mk_root "$CLEAN"
OUT="$(run_in "$CLEAN")"
if [ -n "$PYBIN" ]; then
    echo "$OUT" | "$PYBIN" -c "import sys,json; json.load(sys.stdin)" 2>/dev/null \
        && pass "T1: output is valid JSON" || fail "T1: not valid JSON: $OUT"
fi
for f in hosted provider root matched_segment at_risk_paths briefing; do
    echo "$OUT" | grep -q "\"$f\"" || fail "T1: missing schema field '$f'"
done
pass "T1: frozen schema fields present"

echo "$OUT" | grep -q '"hosted":false' && pass "T2: clean root -> hosted=false" \
    || fail "T2: clean root reported hosted!=false ($OUT)"
echo "$OUT" | grep -q '"briefing":""' && pass "T2: clean root -> empty briefing (line suppressed)" \
    || fail "T2: clean root emitted a non-empty briefing ($OUT)"

# --- T3: synthetic hosted root -------------------------------------------------
HOSTED="$SANDBOX/Users/x/OneDrive/Developer/project"
mk_root "$HOSTED"
OUT="$(run_in "$HOSTED")"
echo "$OUT" | grep -q '"hosted":true' && pass "T3: OneDrive segment -> hosted=true" \
    || fail "T3: OneDrive segment NOT detected ($OUT)"
echo "$OUT" | grep -q '"provider":"OneDrive"' && pass "T3: provider reported" \
    || fail "T3: provider not reported ($OUT)"
echo "$OUT" | grep -q '"briefing":""' && fail "T3: hosted root emitted an EMPTY briefing (line would be suppressed)" \
    || pass "T3: hosted root emits a non-empty briefing"

# --- T4: segment-awareness (false-positive guard) ------------------------------
NEAR="$SANDBOX/Developer/OneDriveTools/project"
mk_root "$NEAR"
OUT="$(run_in "$NEAR")"
echo "$OUT" | grep -q '"hosted":false' \
    && pass "T4: 'OneDriveTools' segment does NOT trip (no false positive)" \
    || fail "T4: FALSE POSITIVE on a path merely containing the provider name ($OUT)"

# --- T5: configurable provider list --------------------------------------------
CUSTOM="$SANDBOX/Users/x/MegaSync/project"
mk_root "$CUSTOM"
OUT="$(ULDF_CLOUD_SYNC_PROVIDERS="MegaSync" run_in "$CUSTOM")"
echo "$OUT" | grep -q '"hosted":true' && pass "T5: ULDF_CLOUD_SYNC_PROVIDERS extends detection" \
    || fail "T5: custom provider not honored ($OUT)"
OUT="$(run_in "$CUSTOM")"
echo "$OUT" | grep -q '"hosted":false' && pass "T5: unlisted provider does not trip by default" \
    || fail "T5: MegaSync tripped without being configured ($OUT)"

# --- T6: at_risk_paths reflects reality ----------------------------------------
BARE="$SANDBOX/Users/x/OneDrive/bare"
mkdir -p "$BARE"
OUT="$(run_in "$BARE")"
echo "$OUT" | grep -q '"at_risk_paths":\[\]' \
    && pass "T6: no coordination store -> at_risk_paths empty" \
    || fail "T6: at_risk_paths should be empty with no .claude present ($OUT)"
OUT="$(run_in "$HOSTED")"
echo "$OUT" | grep -q 'active-sessions.json' \
    && pass "T6: existing coordination surfaces are named" \
    || fail "T6: existing active-sessions.json not named ($OUT)"

# --- T7: graceful absence ------------------------------------------------------
# An unresolvable root must not fabricate a verdict in EITHER direction. The
# oracle's silence must mean "no recognized provider segment", never a
# synthesized "verified safe".
GONE="$SANDBOX/vanishing"
mkdir -p "$GONE"
OUT="$( cd "$GONE" && rm -rf "$GONE" 2>/dev/null; bash "$ORACLE_DIR/run.sh" 2>/dev/null )"
if [ -z "$OUT" ]; then
    fail "T7: emitted nothing on an unresolvable root (must still emit valid JSON)"
else
    echo "$OUT" | grep -q '"hosted":' && pass "T7: unresolvable root still emits valid schema" \
        || fail "T7: malformed output on unresolvable root ($OUT)"
fi

cleanup
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -gt 0 ] && exit 1
exit 0
