#!/bin/bash
# gitignore-template-drift oracle self-test (Unix)
#
# Runs run.sh against each fixture in test-fixtures/ and compares output
# against the expected drift classification. Six cases:
#   1. no-drift                     — drifted=false, missing=0
#   2. 1-pattern-missing            — drifted=true,  missing=1
#   3. 5-patterns-missing           — drifted=true,  missing=5
#   4. project-has-extra-patterns   — drifted=false, missing=0
#   5. no-baseline-found            — drifted=false, missing=0, briefing="" (graceful absent)
#   6. project-no-gitignore         — drifted=true,  missing=13

set -u
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
FIXTURES="$ORACLE_DIR/test-fixtures"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

# Probe for python (Windows-Store stub guard, same pattern as
# dispatchable-sessions validate.sh).
PYBIN=""
for _candidate in python3 python; do
    if command -v "$_candidate" >/dev/null 2>&1; then
        if "$_candidate" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
            PYBIN="$_candidate"
            break
        fi
    fi
done

# JSON field extractor. Uses python when available (precise), else regex
# fallback for environments without python.
get_field() {
    local json="$1" field="$2"
    if [ -n "$PYBIN" ]; then
        printf '%s' "$json" | "$PYBIN" -c "
import sys, json
data = json.load(sys.stdin)
v = data.get('$field')
if isinstance(v, list):
    print(len(v))
elif isinstance(v, bool):
    print('true' if v else 'false')
else:
    print(v if v is not None else '')
"
    else
        # Regex fallback (less robust, but our schema is simple)
        case "$field" in
            drifted)
                printf '%s' "$json" | grep -oE '"drifted":(true|false)' | head -1 | sed 's/.*://'
                ;;
            missing_patterns)
                # Count occurrences of "" inside the array
                local arr
                arr=$(printf '%s' "$json" | grep -oE '"missing_patterns":\[[^]]*\]' | head -1)
                # Count quoted strings inside
                printf '%s' "$arr" | grep -oE '"[^"]*"' | tail -n +2 | wc -l | tr -d ' '
                ;;
            baseline_patterns|project_patterns)
                printf '%s' "$json" | grep -oE "\"$field\":[0-9]+" | head -1 | sed 's/.*://'
                ;;
            briefing)
                printf '%s' "$json" | grep -oE '"briefing":"[^"]*"' | head -1 | sed 's/^"briefing":"//;s/"$//'
                ;;
        esac
    fi
}

# JSON schema check — output must contain all 5 fields.
check_schema() {
    local label="$1" json="$2"
    for field in drifted missing_patterns baseline_patterns project_patterns briefing; do
        if ! printf '%s' "$json" | grep -q "\"$field\""; then
            fail "$label: missing schema field '$field'"
            return 1
        fi
    done
    return 0
}

# Run a fixture and assert expected outcome.
run_case() {
    local name="$1" baseline="$2" project="$3" exp_drifted="$4" exp_missing="$5" exp_briefing_nonempty="$6"
    local out
    out=$(CLAUDE_GITIGNORE_BASELINE="$baseline" CLAUDE_GITIGNORE_PROJECT="$project" bash "$ORACLE_DIR/run.sh" 2>&1)
    local rc=$?
    if [ $rc -ne 0 ]; then
        fail "$name: run.sh exited $rc (output: $out)"
        return
    fi
    if ! check_schema "$name" "$out"; then return; fi

    local got_drifted got_missing got_briefing
    got_drifted=$(get_field "$out" drifted)
    got_missing=$(get_field "$out" missing_patterns)
    got_briefing=$(get_field "$out" briefing)

    if [ "$got_drifted" != "$exp_drifted" ]; then
        fail "$name: drifted got='$got_drifted' want='$exp_drifted' (out=$out)"
        return
    fi
    if [ "$got_missing" != "$exp_missing" ]; then
        fail "$name: missing_patterns count got='$got_missing' want='$exp_missing' (out=$out)"
        return
    fi
    if [ "$exp_briefing_nonempty" = "yes" ]; then
        if [ -z "$got_briefing" ]; then
            fail "$name: briefing expected non-empty, got empty (out=$out)"
            return
        fi
        # When non-empty, must reference the migrate command
        if ! printf '%s' "$got_briefing" | grep -q '/0-uldf-migrate-hygiene'; then
            fail "$name: briefing missing /0-uldf-migrate-hygiene reference: '$got_briefing'"
            return
        fi
    else
        if [ -n "$got_briefing" ]; then
            fail "$name: briefing expected empty, got '$got_briefing' (out=$out)"
            return
        fi
    fi
    pass "$name (drifted=$got_drifted missing=$got_missing)"
}

# ---- Run all 6 fixtures -----------------------------------------------------

run_case "no-drift" \
    "$FIXTURES/no-drift/baseline.gitignore" \
    "$FIXTURES/no-drift/project.gitignore" \
    "false" "0" "no"

run_case "1-pattern-missing" \
    "$FIXTURES/1-pattern-missing/baseline.gitignore" \
    "$FIXTURES/1-pattern-missing/project.gitignore" \
    "true" "1" "yes"

run_case "5-patterns-missing" \
    "$FIXTURES/5-patterns-missing/baseline.gitignore" \
    "$FIXTURES/5-patterns-missing/project.gitignore" \
    "true" "5" "yes"

run_case "project-has-extra-patterns" \
    "$FIXTURES/project-has-extra-patterns/baseline.gitignore" \
    "$FIXTURES/project-has-extra-patterns/project.gitignore" \
    "false" "0" "no"

# no-baseline-found: point CLAUDE_GITIGNORE_BASELINE at a path that doesn't exist
run_case "no-baseline-found" \
    "$FIXTURES/no-baseline-found/__nonexistent_baseline__.gitignore" \
    "$FIXTURES/no-baseline-found/project.gitignore" \
    "false" "0" "no"

# project-no-gitignore: project path doesn't exist; baseline does
run_case "project-no-gitignore" \
    "$FIXTURES/project-no-gitignore/baseline.gitignore" \
    "$FIXTURES/project-no-gitignore/__nonexistent_project__.gitignore" \
    "true" "13" "yes"

# ---- Summary ----------------------------------------------------------------
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -gt 0 ] && exit 1
exit 0
