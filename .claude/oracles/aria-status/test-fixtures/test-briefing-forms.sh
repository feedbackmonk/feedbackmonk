#!/bin/bash
# aria-status briefing-form fixtures (Unix)
#
# Validates that each acceptance #5 briefing form is produced under the right conditions.
# Run from any directory; creates ephemeral synthetic project trees in a temp dir.
#
# Tested forms:
#   1. no-surface                 (ULDF root: no Cargo.toml, no package.json)            -> briefing=""
#   2. surface-but-no-instrumentation (synthetic Tauri without .claude/aria.json)        -> "ARIA: UI/runtime surface detected; no ARIA instrumentation. /0-uldf-ldis-plan can scaffold."
#   3. instrumented-but-unreachable (synthetic Tauri WITH .claude/aria.json, server down) -> "ARIA: configured but endpoint unreachable at <url>"
#
# Forms 4 & 5 (present-and-healthy, present-but-degraded) require a live ARIA-07 server
# and are exercised at PODS Sync Point 1 against Track B's live SessionHelm server.

set -e

ORACLE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ORACLE_RUN="$ORACLE_DIR/run.sh"
SURFACE_RUN="$ORACLE_DIR/../ui-surface-detector/run.sh"

if [ ! -f "$ORACLE_RUN" ]; then echo "FAIL: $ORACLE_RUN missing" >&2; exit 1; fi
if [ ! -f "$SURFACE_RUN" ]; then echo "FAIL: $SURFACE_RUN missing" >&2; exit 1; fi

# Temp project root
ROOT=$(mktemp -d)
trap "rm -rf '$ROOT' 2>/dev/null || true" EXIT

PASS=0
FAIL=0

run_case() {
    local case_name="$1"
    local expected_substr="$2"
    local subdir="$3"

    cd "$ROOT/$subdir"
    # Place oracle dir relatively so its self-resolution finds ui-surface-detector
    local out
    out=$(bash "$ORACLE_RUN" 2>&1) || { echo "FAIL [$case_name]: oracle exited non-zero"; FAIL=$((FAIL+1)); return; }
    local briefing
    briefing=$(printf '%s' "$out" | grep -oE '"briefing"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed -E 's/^"briefing"[[:space:]]*:[[:space:]]*"(.*)"$/\1/')
    if [ -z "$expected_substr" ]; then
        if [ -z "$briefing" ]; then
            echo "PASS [$case_name]: briefing empty as expected"
            PASS=$((PASS+1))
        else
            echo "FAIL [$case_name]: expected empty briefing, got: '$briefing'"
            FAIL=$((FAIL+1))
        fi
    else
        if printf '%s' "$briefing" | grep -qF "$expected_substr"; then
            echo "PASS [$case_name]: briefing matches"
            PASS=$((PASS+1))
        else
            echo "FAIL [$case_name]: expected substring '$expected_substr' not found in briefing: '$briefing'"
            FAIL=$((FAIL+1))
        fi
    fi
}

# Case 1: no-surface (cli-tool with bin/ directory; surface_kind=cli-tool -> surface_present=false)
mkdir -p "$ROOT/case-no-surface/bin"
echo '{"name":"foo","bin":"./bin/foo"}' > "$ROOT/case-no-surface/package.json"
run_case "no-surface (cli-tool)" "" "case-no-surface"

# Case 2: surface-but-no-instrumentation (Tauri without aria.json)
mkdir -p "$ROOT/case-no-instr/src-tauri"
touch "$ROOT/case-no-instr/Cargo.toml" "$ROOT/case-no-instr/src-tauri/Cargo.toml"
run_case "surface-but-no-instrumentation (Tauri)" "no ARIA instrumentation" "case-no-instr"

# Case 3: instrumented-but-unreachable (Tauri WITH aria.json; default endpoint at unused port -> unreachable)
mkdir -p "$ROOT/case-configured/src-tauri" "$ROOT/case-configured/.claude"
touch "$ROOT/case-configured/Cargo.toml" "$ROOT/case-configured/src-tauri/Cargo.toml"
# Use a port unlikely to be in use (14599 is at the top of the reserved ARIA range)
printf '{"endpoint_url":"http://127.0.0.1:14599/aria/health"}' > "$ROOT/case-configured/.claude/aria.json"
run_case "instrumented-but-unreachable (Tauri+aria.json)" "configured but endpoint unreachable" "case-configured"

echo ""
echo "Summary: $PASS pass, $FAIL fail"
exit $FAIL
