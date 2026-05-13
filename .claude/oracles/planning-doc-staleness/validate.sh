#!/bin/bash
# planning-doc-staleness oracle self-test (Unix)
#
# Cases:
#   T1. shipped-via-commit  — slug appears in commit log; no spec refs DONE → "commit-hash-found"
#   T2. shipped-via-spec    — body refs spec entries that are all [DONE]; no commit match → "all-spec-entries-done"
#   T3. shipped-via-both    — both signals fire → "both"
#   T4. in-flight           — recent mtime, no signals → fresh[]
#   T5. mixed               — multiple files spanning all partitions in one run
#   T6. no-planning-dir     — neither dir exists → empty arrays + empty briefing
#   T7. malformed           — zero-byte file with old mtime → unknown[] (signal silent, age > 14d)

set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

PYBIN=""
for _candidate in python3 python; do
    if command -v "$_candidate" >/dev/null 2>&1; then
        if "$_candidate" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
            PYBIN="$_candidate"
            break
        fi
    fi
done

if [ -z "$PYBIN" ]; then
    echo "SKIP: validate requires python for JSON inspection"
    exit 0
fi

# --- sandbox helpers ---------------------------------------------------------
make_sandbox() {
    SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'pds')"
    mkdir -p "$SANDBOX/.claude/oracles/planning-doc-staleness"
    cp "$ORACLE_DIR/run.sh" "$SANDBOX/.claude/oracles/planning-doc-staleness/run.sh"
    cp "$ORACLE_DIR/oracle.json" "$SANDBOX/.claude/oracles/planning-doc-staleness/oracle.json"
    mkdir -p "$SANDBOX/docs/planning/intakes" "$SANDBOX/docs/planning/plans" "$SANDBOX/docs/specs"
    # Init a git repo so `git log` returns something deterministic (even if empty).
    (cd "$SANDBOX" && git init -q && git config user.email t@t && git config user.name t)
}
sandbox_cleanup() { rm -rf "$SANDBOX"; }

run_oracle() {
    (cd "$SANDBOX" && bash .claude/oracles/planning-doc-staleness/run.sh)
}

# Set mtime to N days ago (portable across GNU touch + BSD touch).
backdate() {
    local target="$1"; local days="$2"
    local epoch=$(( $(date +%s) - days * 86400 ))
    # GNU touch:
    if touch -d "@$epoch" "$target" 2>/dev/null; then return 0; fi
    # BSD touch:
    local stamp=$(date -r "$epoch" '+%Y%m%d%H%M.%S' 2>/dev/null)
    if [ -n "$stamp" ] && touch -t "$stamp" "$target" 2>/dev/null; then return 0; fi
    return 1
}

assert_json() {
    local out="$1"; local pyq="$2"; local expect="$3"; local label="$4"
    local actual
    actual=$(printf '%s' "$out" | "$PYBIN" -c "import json,sys; d=json.load(sys.stdin); $pyq" 2>/dev/null || echo "<<error>>")
    if [ "$actual" = "$expect" ]; then
        pass "$label"
    else
        fail "$label (expected '$expect', got '$actual')"
        echo "Output: $out" >&2
    fi
}

# =============================================================================
# T1: shipped-via-commit (commit message references slug, spec absent)
# =============================================================================
make_sandbox
echo "stub" > "$SANDBOX/docs/planning/intakes/20260101T000000-feature-foo.md"
backdate "$SANDBOX/docs/planning/intakes/20260101T000000-feature-foo.md" 30
(cd "$SANDBOX" && git add -A && git commit -q -m "feat(foo): ship feature-foo" --allow-empty)
T1_OUT=$(run_oracle)
assert_json "$T1_OUT" "print(d['stale'][0]['staleness_signal'] if d['stale'] else 'NONE')" "commit-hash-found" "T1: commit-hash-found signal"
assert_json "$T1_OUT" "print(len(d['stale']))" "1" "T1: exactly one stale entry"
assert_json "$T1_OUT" "print(len(d['fresh']))" "0" "T1: no fresh entries"
sandbox_cleanup

# =============================================================================
# T2: shipped-via-spec-status (body refs DONE entries; no commit signal)
# =============================================================================
make_sandbox
cat > "$SANDBOX/docs/specs/SPECIFICATION.md" <<'EOF'
# Spec
#### TEST-01: Test entry [DONE]
**Description**: shipped.
#### TEST-02: Other [DONE]
**Description**: shipped too.
EOF
cat > "$SANDBOX/docs/planning/plans/20260201T000000-bar.md" <<'EOF'
# Plan
References: TEST-01 and TEST-02.
EOF
backdate "$SANDBOX/docs/planning/plans/20260201T000000-bar.md" 60
(cd "$SANDBOX" && git commit -q --allow-empty -m "unrelated commit message")
T2_OUT=$(run_oracle)
assert_json "$T2_OUT" "print(d['stale'][0]['staleness_signal'] if d['stale'] else 'NONE')" "all-spec-entries-done" "T2: all-spec-entries-done signal"
assert_json "$T2_OUT" "print(len(d['stale']))" "1" "T2: exactly one stale entry"
sandbox_cleanup

# =============================================================================
# T3: shipped-via-both (both signals fire)
# =============================================================================
make_sandbox
cat > "$SANDBOX/docs/specs/SPECIFICATION.md" <<'EOF'
#### XYZ-01: Done [DONE]
EOF
cat > "$SANDBOX/docs/planning/intakes/20260101T000000-baz-feature.md" <<'EOF'
# Intake
Refs: XYZ-01
EOF
backdate "$SANDBOX/docs/planning/intakes/20260101T000000-baz-feature.md" 60
(cd "$SANDBOX" && git add -A && git commit -q -m "ship baz-feature work" --allow-empty)
T3_OUT=$(run_oracle)
assert_json "$T3_OUT" "print(d['stale'][0]['staleness_signal'] if d['stale'] else 'NONE')" "both" "T3: both signals"
sandbox_cleanup

# =============================================================================
# T4: in-flight (no signals, recent mtime → fresh)
# =============================================================================
make_sandbox
cat > "$SANDBOX/docs/specs/SPECIFICATION.md" <<'EOF'
#### INF-01: Pending [PLANNED]
EOF
echo "in-flight work" > "$SANDBOX/docs/planning/plans/20260601T000000-active-arc.md"
backdate "$SANDBOX/docs/planning/plans/20260601T000000-active-arc.md" 3
(cd "$SANDBOX" && git commit -q --allow-empty -m "unrelated work")
T4_OUT=$(run_oracle)
assert_json "$T4_OUT" "print(len(d['stale']))" "0" "T4: no stale entries"
assert_json "$T4_OUT" "print(len(d['fresh']))" "1" "T4: one fresh entry"
assert_json "$T4_OUT" "print(d['briefing'])" "" "T4: briefing empty"
sandbox_cleanup

# =============================================================================
# T5: mixed (multiple docs, three partitions)
# =============================================================================
make_sandbox
cat > "$SANDBOX/docs/specs/SPECIFICATION.md" <<'EOF'
#### MIX-01: Done [DONE]
#### MIX-02: Pending [PLANNED]
EOF
# Stale via spec
cat > "$SANDBOX/docs/planning/intakes/20260101T000000-stale-doc.md" <<'EOF'
Refs MIX-01.
EOF
backdate "$SANDBOX/docs/planning/intakes/20260101T000000-stale-doc.md" 60
# Fresh (no signals, recent)
echo "active" > "$SANDBOX/docs/planning/intakes/20260601T000000-fresh-doc.md"
backdate "$SANDBOX/docs/planning/intakes/20260601T000000-fresh-doc.md" 2
# Unknown (no signals, old mtime)
echo "stale-no-signal" > "$SANDBOX/docs/planning/plans/20260101T000000-unknown-doc.md"
backdate "$SANDBOX/docs/planning/plans/20260101T000000-unknown-doc.md" 60
(cd "$SANDBOX" && git commit -q --allow-empty -m "unrelated")
T5_OUT=$(run_oracle)
assert_json "$T5_OUT" "print(len(d['stale']))" "1" "T5: one stale"
assert_json "$T5_OUT" "print(len(d['fresh']))" "1" "T5: one fresh"
assert_json "$T5_OUT" "print(len(d['unknown']))" "1" "T5: one unknown"
assert_json "$T5_OUT" "print('stale' in d['briefing'])" "True" "T5: briefing mentions stale"
sandbox_cleanup

# =============================================================================
# T6: no-planning-dir (graceful absence)
# =============================================================================
make_sandbox
rm -rf "$SANDBOX/docs/planning"
T6_OUT=$(run_oracle)
assert_json "$T6_OUT" "print(len(d['stale']))" "0" "T6: no stale"
assert_json "$T6_OUT" "print(len(d['fresh']))" "0" "T6: no fresh"
assert_json "$T6_OUT" "print(len(d['unknown']))" "0" "T6: no unknown"
assert_json "$T6_OUT" "print(d['briefing'])" "" "T6: briefing empty"
sandbox_cleanup

# =============================================================================
# T7: malformed (zero-byte, old mtime → unknown)
# =============================================================================
make_sandbox
: > "$SANDBOX/docs/planning/intakes/20260101T000000-empty.md"
backdate "$SANDBOX/docs/planning/intakes/20260101T000000-empty.md" 60
(cd "$SANDBOX" && git commit -q --allow-empty -m "unrelated")
T7_OUT=$(run_oracle)
assert_json "$T7_OUT" "print(len(d['stale']))" "0" "T7: malformed not stale"
assert_json "$T7_OUT" "print(len(d['unknown']))" "1" "T7: malformed → unknown"
sandbox_cleanup

# =============================================================================
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
if [ "$FAIL" -gt 0 ]; then exit 1; fi
exit 0
