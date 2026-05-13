#!/bin/bash
# Leg B (ARIA-04) condition fixtures — verify the oracle outputs that drive
# the /0-uldf-ldis-plan Phase 4 testability gate's Stage 0 auto-proposal.
#
# Acceptance per ARIA-04:
#   - Auto-propose Stage 0 IFF: ui-surface-detector.surface_kind != "none"
#     AND ui-surface-detector.confidence == "high" AND any aria-status.foundation_layer.* == false
#   - Silence rule: silent if foundation_layer.* all true (already instrumented)
#   - Silence rule: silent if surface_kind == "none" (no surface)

set -e

ORACLE_DIR_BASE="$(cd "$(dirname "$0")/.." && pwd)"
ARIA_RUN="$ORACLE_DIR_BASE/run.sh"
SURFACE_RUN="$ORACLE_DIR_BASE/../ui-surface-detector/run.sh"

ROOT=$(mktemp -d)
trap "rm -rf '$ROOT' 2>/dev/null || true" EXIT

PASS=0
FAIL=0

# Helpers
expect_propose() {
    local case_name="$1"
    local subdir="$2"
    cd "$ROOT/$subdir"
    local sout aout sk conf fl_err
    sout=$(bash "$SURFACE_RUN" 2>/dev/null)
    aout=$(bash "$ARIA_RUN" 2>/dev/null)
    sk=$(printf '%s' "$sout" | grep -oE '"surface_kind"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
    conf=$(printf '%s' "$sout" | grep -oE '"confidence"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
    fl_err=$(printf '%s' "$aout" | grep -oE '"errors"[[:space:]]*:[[:space:]]*(true|false)' | grep -oE '(true|false)$' | head -1)
    fl_async=$(printf '%s' "$aout" | grep -oE '"async"[[:space:]]*:[[:space:]]*(true|false)' | grep -oE '(true|false)$' | head -1)
    fl_nav=$(printf '%s' "$aout" | grep -oE '"navigation"[[:space:]]*:[[:space:]]*(true|false)' | grep -oE '(true|false)$' | head -1)
    # Decision: auto-propose iff sk != none AND conf == high AND any fl == false
    local would_propose="false"
    if [ "$sk" != "none" ] && [ "$conf" = "high" ]; then
        if [ "$fl_err" = "false" ] || [ "$fl_async" = "false" ] || [ "$fl_nav" = "false" ]; then
            would_propose="true"
        fi
    fi
    if [ "$would_propose" = "true" ]; then
        echo "PASS [$case_name]: auto-propose triggered (surface_kind=$sk, confidence=$conf, fl=$fl_err/$fl_async/$fl_nav)"
        PASS=$((PASS+1))
    else
        echo "FAIL [$case_name]: expected auto-propose but conditions not met (surface_kind=$sk, confidence=$conf, fl=$fl_err/$fl_async/$fl_nav)"
        FAIL=$((FAIL+1))
    fi
}

expect_silent() {
    local case_name="$1"
    local subdir="$2"
    local reason="$3"
    cd "$ROOT/$subdir"
    local sout aout sk conf fl_err fl_async fl_nav
    sout=$(bash "$SURFACE_RUN" 2>/dev/null)
    aout=$(bash "$ARIA_RUN" 2>/dev/null)
    sk=$(printf '%s' "$sout" | grep -oE '"surface_kind"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
    conf=$(printf '%s' "$sout" | grep -oE '"confidence"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
    fl_err=$(printf '%s' "$aout" | grep -oE '"errors"[[:space:]]*:[[:space:]]*(true|false)' | grep -oE '(true|false)$' | head -1)
    fl_async=$(printf '%s' "$aout" | grep -oE '"async"[[:space:]]*:[[:space:]]*(true|false)' | grep -oE '(true|false)$' | head -1)
    fl_nav=$(printf '%s' "$aout" | grep -oE '"navigation"[[:space:]]*:[[:space:]]*(true|false)' | grep -oE '(true|false)$' | head -1)
    local would_propose="false"
    if [ "$sk" != "none" ] && [ "$conf" = "high" ]; then
        if [ "$fl_err" = "false" ] || [ "$fl_async" = "false" ] || [ "$fl_nav" = "false" ]; then
            would_propose="true"
        fi
    fi
    if [ "$would_propose" = "false" ]; then
        echo "PASS [$case_name]: silent ($reason): surface_kind=$sk, confidence=$conf, fl=$fl_err/$fl_async/$fl_nav"
        PASS=$((PASS+1))
    else
        echo "FAIL [$case_name]: expected silent ($reason) but auto-propose would trigger"
        FAIL=$((FAIL+1))
    fi
}

# NEGATIVE: CLI tool — refactor a utility module (no UI, no ARIA need)
mkdir -p "$ROOT/cli-tool/bin"
echo '{"name":"cli-foo","bin":"./bin/cli-foo"}' > "$ROOT/cli-tool/package.json"
expect_silent "negative: CLI-tool refactor" "cli-tool" "surface_kind=cli-tool -> aria-status returns surface_present=false"

# POSITIVE: Tauri-desktop — build settings page (UI task on non-instrumented project)
mkdir -p "$ROOT/tauri-uninstrumented/src-tauri"
touch "$ROOT/tauri-uninstrumented/Cargo.toml" "$ROOT/tauri-uninstrumented/src-tauri/Cargo.toml"
expect_propose "positive: Tauri-desktop UI task" "tauri-uninstrumented"

# SILENT: project with surface_kind=none (e.g., empty dir)
mkdir -p "$ROOT/empty-dir"
expect_silent "silent: no surface" "empty-dir" "surface_kind=none"

echo ""
echo "Summary: $PASS pass, $FAIL fail"
exit $FAIL
