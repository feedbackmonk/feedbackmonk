#!/bin/bash
# Self-test for hook-verification-coverage (Unix).
# Builds synthetic fixtures and asserts each detector path fires:
#   1. new uncovered hook       -> fail
#   2. hook with tests/ file    -> pass
#   3. hook referenced by smoke -> pass
#   4. baseline (legacy) hook   -> warn
#   5. no settings file         -> pass, applicable=false
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
RUN="$HERE/run.sh"
fails=0
check() { # label, expect_substr, output
    if printf '%s' "$3" | grep -q "$2"; then
        echo "PASS: $1"
    else
        echo "FAIL: $1 -- expected '$2' in: $3"
        fails=$((fails+1))
    fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/claude-template/hooks/tests" "$tmp/claude-template/scripts/csi-tests"

# 1. new uncovered hook -> fail
printf '{"hooks":{"X":[{"command":"pwsh hooks/brand-new-gate.ps1"}]}}' > "$tmp/claude-template/settings.json"
out="$(HVC_ROOT="$tmp" bash "$RUN")"
check "new uncovered hook fails" '"status":"fail"' "$out"
check "new uncovered hook named" 'brand-new-gate' "$out"

# 2. covered via tests/ file -> pass
touch "$tmp/claude-template/hooks/tests/brand-new-gate.test.ps1"
out="$(HVC_ROOT="$tmp" bash "$RUN")"
check "tests/-file coverage passes" '"status":"pass"' "$out"

# 3. covered via smoke reference -> pass
rm "$tmp/claude-template/hooks/tests/brand-new-gate.test.ps1"
printf 'exercises brand-new-gate.sh end to end\n' > "$tmp/claude-template/scripts/csi-tests/gate-smoke.sh"
out="$(HVC_ROOT="$tmp" bash "$RUN")"
check "smoke-reference coverage passes" '"status":"pass"' "$out"

# 4. baseline hook uncovered -> warn
printf '{"hooks":{"X":[{"command":"pwsh hooks/pre-compact.ps1"}]}}' > "$tmp/claude-template/settings.json"
rm "$tmp/claude-template/scripts/csi-tests/gate-smoke.sh"
out="$(HVC_ROOT="$tmp" bash "$RUN")"
check "baseline hook warns" '"status":"warn"' "$out"

# 5. no settings -> graceful absence
rm "$tmp/claude-template/settings.json"
out="$(HVC_ROOT="$tmp" bash "$RUN")"
check "graceful absence" '"applicable":false' "$out"

if [ "$fails" -eq 0 ]; then
    echo "hook-verification-coverage validate: ALL PASS"
    exit 0
else
    echo "hook-verification-coverage validate: $fails FAILURE(S)"
    exit 1
fi
