#!/bin/bash
# harness-capabilities oracle -- self-test (Unix). HAL-01.
#
# Runs run.sh under each golden fixture's pinned environment and exact-matches
# stdout against the fixture file. Each fixture runs twice (determinism).
# The env-per-fixture mapping is hardcoded here by design: fixtures + validator
# ship together, and the test seams (HARNESS_CAPS_FORCE_VERSION /
# HARNESS_CAPS_NOW) keep the goldens hermetic -- the real `claude --version`
# and real clock are never consulted during validation.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ORACLE_RUN="$SCRIPT_DIR/run.sh"
FIXTURES_DIR="$SCRIPT_DIR/test-fixtures"

if [ ! -d "$FIXTURES_DIR" ]; then
    echo "validate.sh: no test-fixtures directory at $FIXTURES_DIR" >&2
    exit 2
fi

# run_fixture <name> : run the oracle under the fixture's pinned env.
run_fixture() {
    case "$1" in
        claude-full-2.1.206)
            env -u CLAUDE_CODE_DISABLE_WORKFLOWS -u CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS \
                CLAUDECODE=1 HARNESS_CAPS_FORCE_VERSION="2.1.206 (Claude Code)" \
                HARNESS_CAPS_NOW=2026-07-10T00:00:00Z bash "$ORACLE_RUN" 2>&1 ;;
        claude-teams-opt-in)
            env -u CLAUDE_CODE_DISABLE_WORKFLOWS \
                CLAUDECODE=1 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 \
                HARNESS_CAPS_FORCE_VERSION="2.1.206 (Claude Code)" \
                HARNESS_CAPS_NOW=2026-07-10T00:00:00Z bash "$ORACLE_RUN" 2>&1 ;;
        claude-workflows-disabled)
            env -u CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS \
                CLAUDECODE=1 CLAUDE_CODE_DISABLE_WORKFLOWS=1 \
                HARNESS_CAPS_FORCE_VERSION="2.1.206 (Claude Code)" \
                HARNESS_CAPS_NOW=2026-07-10T00:00:00Z bash "$ORACLE_RUN" 2>&1 ;;
        claude-old-2.1.150)
            env -u CLAUDE_CODE_DISABLE_WORKFLOWS -u CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS \
                CLAUDECODE=1 HARNESS_CAPS_FORCE_VERSION="2.1.150 (Claude Code)" \
                HARNESS_CAPS_NOW=2026-07-10T00:00:00Z bash "$ORACLE_RUN" 2>&1 ;;
        claude-version-unknown)
            env -u CLAUDE_CODE_DISABLE_WORKFLOWS -u CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS \
                CLAUDECODE=1 HARNESS_CAPS_FORCE_VERSION="garbage" \
                HARNESS_CAPS_NOW=2026-07-10T00:00:00Z bash "$ORACLE_RUN" 2>&1 ;;
        non-claude-all-false)
            env -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID \
                -u CLAUDE_CODE_DISABLE_WORKFLOWS -u CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS \
                HARNESS_CAPS_NOW=2026-07-10T00:00:00Z bash "$ORACLE_RUN" 2>&1 ;;
        *)  return 3 ;;
    esac
}

PASS=0
FAIL=0
FAILED_NAMES=()

for expected in "$FIXTURES_DIR"/*.json; do
    name=$(basename "$expected" .json)
    actual=$(run_fixture "$name") || { echo "SKIP: $name (no env mapping in validate.sh -- add one)"; continue; }
    actual2=$(run_fixture "$name")
    expected_trim=$(tr -d '\r\n' < "$expected")
    actual_trim=$(printf '%s' "$actual" | tr -d '\r\n')
    actual2_trim=$(printf '%s' "$actual2" | tr -d '\r\n')

    if [ "$actual_trim" != "$actual2_trim" ]; then
        FAIL=$((FAIL + 1)); FAILED_NAMES+=("$name(non-deterministic)")
        echo "FAIL: $name -- non-deterministic across two runs"
        continue
    fi
    if [ "$actual_trim" = "$expected_trim" ]; then
        PASS=$((PASS + 1)); echo "PASS: $name"
    else
        FAIL=$((FAIL + 1)); FAILED_NAMES+=("$name")
        echo "FAIL: $name"
        echo "  expected: $expected_trim"
        echo "  actual:   $actual_trim"
    fi
done

echo "---"
echo "validate.sh: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    echo "Failed fixtures: ${FAILED_NAMES[*]}"
    exit 1
fi
exit 0
