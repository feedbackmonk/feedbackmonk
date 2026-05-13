#!/bin/bash
# Self-test for ui-fixture-inventory oracle (Unix).
#
# Exercises three scenarios in a sandbox:
#   1. No surface → empty briefing
#   2. Surface present, no fixtures → "scaffold via /0-uldf-ldis-plan" briefing
#   3. Surface present, fixtures present → empty briefing (suppressed; healthy project)
#
# Usage: bash validate.sh

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORACLE="$SCRIPT_DIR/run.sh"
SURFACE_ORACLE_DIR="$SCRIPT_DIR/../ui-surface-detector"
SURFACE_ORACLE="$SURFACE_ORACLE_DIR/run.sh"

[ -f "$ORACLE" ] || { echo "FATAL: oracle run.sh missing" >&2; exit 1; }

PASS=0; FAIL=0
ok()  { echo "PASS: $1"; PASS=$((PASS+1)); }
bad() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }

SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t fixture-inv-validate)"
[ -d "$SANDBOX" ] || { echo "FATAL: cannot create sandbox" >&2; exit 1; }
trap "rm -rf '$SANDBOX'" EXIT

# Set up sibling oracle dir for surface detection
mkdir -p "$SANDBOX/.claude/oracles/ui-surface-detector"
mkdir -p "$SANDBOX/.claude/oracles/ui-fixture-inventory"
cp "$ORACLE" "$SANDBOX/.claude/oracles/ui-fixture-inventory/run.sh"

# Stub surface oracle: configurable via env var SURFACE_KIND_STUB
cat > "$SANDBOX/.claude/oracles/ui-surface-detector/run.sh" <<'EOF'
#!/bin/bash
SK="${SURFACE_KIND_STUB:-none}"
printf '{"surface_kind":"%s","confidence":"high","evidence":["stub"]}' "$SK"
EOF
chmod +x "$SANDBOX/.claude/oracles/ui-surface-detector/run.sh"

run_oracle() {
    (cd "$SANDBOX" && SURFACE_KIND_STUB="$1" bash .claude/oracles/ui-fixture-inventory/run.sh)
}

# --- Scenario 1: no surface ---
out=$(run_oracle "none")
if printf '%s' "$out" | grep -q '"has_fixtures":false' && \
   printf '%s' "$out" | grep -q '"briefing":""'; then
    ok "Scenario 1: no-surface emits empty briefing"
else
    bad "Scenario 1: expected empty briefing for no-surface; got: $out"
fi

# --- Scenario 2: surface present, no fixtures ---
out=$(run_oracle "tauri-desktop")
if printf '%s' "$out" | grep -q '"has_fixtures":false' && \
   printf '%s' "$out" | grep -q "scaffold"; then
    ok "Scenario 2: surface-no-fixtures emits scaffold briefing"
else
    bad "Scenario 2: expected scaffold briefing; got: $out"
fi

# --- Scenario 3: surface present, fixtures present ---
mkdir -p "$SANDBOX/tests/fixtures" "$SANDBOX/tests/smoke"
echo "// fixture stub" > "$SANDBOX/tests/fixtures/example-smoke.ts"
echo "// smoke stub" > "$SANDBOX/tests/smoke/example.spec.ts"

out=$(run_oracle "tauri-desktop")
if printf '%s' "$out" | grep -q '"has_fixtures":true' && \
   printf '%s' "$out" | grep -q '"briefing":""'; then
    ok "Scenario 3: surface-with-fixtures emits empty briefing (healthy suppression)"
else
    bad "Scenario 3: expected empty briefing for healthy project; got: $out"
fi

# --- Summary ---
echo "---"
echo "Passed: $PASS / $((PASS+FAIL))"
if [ "$FAIL" -eq 0 ]; then
    exit 0
else
    exit 1
fi
