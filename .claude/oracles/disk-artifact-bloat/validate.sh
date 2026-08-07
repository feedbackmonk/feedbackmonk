#!/bin/bash
# disk-artifact-bloat oracle self-test (Unix / Git Bash)
#
# The oracle reads a LIVE drive's free space, so the tests drive the verdict by
# forcing thresholds (env overrides) against the real free value and by crafting
# a sandbox baseline for the drift path:
#
#   T1. ok            -> warn/alert below real free  -> level=ok, briefing=""
#   T2. warn          -> warn above real free        -> level=warn, briefing has "(warn"
#   T3. alert         -> alert above real free        -> level=alert, briefing has "ALERT"
#   T4. absence       -> ROOT = nonexistent path      -> free_gb=null, briefing=""
#   T5. drift         -> sandbox baseline, huge prior free -> drift_gb!=null, "free dropped"
#
# All schema fields are asserted present on every run.

set +e
ORACLE_DIR="$(cd "$(dirname "$0")" 2>/dev/null && pwd -P)"
RUN="$ORACLE_DIR/run.sh"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

PY=""
if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then PY="python3";
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then PY="python"; fi

SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'dabix')"
cleanup() { [ -n "$SANDBOX" ] && [ -d "$SANDBOX" ] && rm -rf "$SANDBOX"; }
trap cleanup EXIT

json_field() {  # out field
    [ -n "$PY" ] || { echo ""; return; }
    printf '%s' "$1" | "$PY" -c "import sys,json
d=json.load(sys.stdin); v=d.get('$2')
print('' if v is None else v)" 2>/dev/null
}

SCHEMA_FIELDS="tripped level drive free_gb total_gb warn_gb alert_gb drift_gb baseline_age_days briefing"
assert_schema() {  # out label
    for f in $SCHEMA_FIELDS; do
        printf '%s' "$1" | grep -q "\"$f\"" || { fail "$2: missing schema field '$f' in: $1"; return 1; }
    done
    return 0
}
assert_json() {  # out label
    [ -n "$PY" ] || return 0
    printf '%s' "$1" | "$PY" -c "import sys,json;json.load(sys.stdin)" 2>/dev/null || { fail "$2: invalid JSON: $1"; return 1; }
}

# Run from the sandbox so the real project's .claude/config.json is not read.
run_oracle() { ( cd "$SANDBOX" && env "$@" bash "$RUN" 2>&1 ); }

# ---- T1. ok ----------------------------------------------------------------
out="$(run_oracle ULDF_BLOAT_WARN_GB=1 ULDF_BLOAT_ALERT_GB=1)"
assert_json "$out" T1; assert_schema "$out" T1
lvl="$(json_field "$out" level)"; br="$(json_field "$out" briefing)"
if [ "$lvl" = "ok" ] && [ -z "$br" ]; then pass "T1: ok -> level=ok briefing=\"\""
else fail "T1: expected ok/empty; got level=$lvl br='$br'"; fi

# ---- T2. warn --------------------------------------------------------------
out="$(run_oracle ULDF_BLOAT_WARN_GB=999999 ULDF_BLOAT_ALERT_GB=1)"
assert_json "$out" T2; assert_schema "$out" T2
lvl="$(json_field "$out" level)"
if [ "$lvl" = "warn" ] && printf '%s' "$out" | grep -q '(warn'; then pass "T2: warn trip"
else fail "T2: expected warn + '(warn' clause; got level=$lvl out=$out"; fi

# ---- T3. alert -------------------------------------------------------------
out="$(run_oracle ULDF_BLOAT_ALERT_GB=999999)"
assert_json "$out" T3; assert_schema "$out" T3
lvl="$(json_field "$out" level)"
if [ "$lvl" = "alert" ] && printf '%s' "$out" | grep -q 'ALERT'; then pass "T3: alert trip"
else fail "T3: expected alert + 'ALERT'; got level=$lvl out=$out"; fi

# ---- T4. graceful absence (nonexistent root) -------------------------------
out="$(run_oracle ULDF_BLOAT_ROOT="$SANDBOX/does-not-exist-xyz" ULDF_BLOAT_WARN_GB=999999)"
assert_json "$out" T4; assert_schema "$out" T4
fg="$(json_field "$out" free_gb)"; br="$(json_field "$out" briefing)"
if [ -z "$fg" ] && [ -z "$br" ]; then pass "T4: absence -> free_gb=null briefing=\"\""
else fail "T4: expected free_gb null + empty briefing; got fg='$fg' br='$br'"; fi

# ---- T5. drift (sandbox HOME, baseline = live free + 100 GiB) --------------
# Prior free is a normal-magnitude integer (jq-safe; a 16-digit literal would
# render in scientific notation and fail the parse). No 'drive' field -> the
# same-drive check defaults true. +100 GiB guarantees a drop >= driftGb (50).
FAKE_HOME="$SANDBOX/home"
mkdir -p "$FAKE_HOME/.claude/session-state"
AVAIL_K_NOW="$(df -P -k "$SANDBOX" 2>/dev/null | awk 'NR==2{print $4}')"
PRIOR_FREE=$(( AVAIL_K_NOW * 1024 + 100 * 1073741824 ))
cat > "$FAKE_HOME/.claude/session-state/disk-artifact-baseline.json" <<EOF
{"scanned_at":"2026-06-01T00:00:00Z","free_bytes":$PRIOR_FREE,"total_artifact_bytes":0}
EOF
out="$( cd "$SANDBOX" && env HOME="$FAKE_HOME" USERPROFILE="$FAKE_HOME" bash "$RUN" 2>&1 )"
assert_json "$out" T5; assert_schema "$out" T5
df="$(json_field "$out" drift_gb)"
if [ -n "$df" ] && printf '%s' "$out" | grep -q 'free dropped'; then pass "T5: drift -> drift_gb=$df, 'free dropped' clause"
else fail "T5: expected non-null drift_gb + 'free dropped'; got drift_gb='$df' out=$out"; fi

echo
echo "================================================================"
echo "  disk-artifact-bloat validate: $PASS PASS / $FAIL FAIL"
echo "================================================================"
[ "$FAIL" -gt 0 ] && exit 1
exit 0
