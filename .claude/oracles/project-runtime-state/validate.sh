#!/bin/bash
# project-runtime-state oracle self-test (Unix)
# Verifies the oracle runs successfully and produces valid JSON matching the
# frozen v1 schema. Also asserts deterministic output under repeated runs.

set -e

ORACLE_DIR="$(dirname "$0")"

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

OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

# Output must be valid JSON
if [ -n "$PYBIN" ]; then
    if ! echo "$OUTPUT" | "$PYBIN" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        echo "Output: $OUTPUT" >&2
        exit 1
    fi
    pass "output is valid JSON"
fi

# Required schema fields
for field in schemaVersion hasLiveDevServer devPortRegistryEntries sharedBuildArtifacts fileWatchers statefulRuntime antiFitScore antiFitReasons; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        fail "schema: missing field '$field'"
    fi
done
[ "$FAIL" -eq 0 ] && pass "schema: all v1 fields present"

# schemaVersion must be 1
if ! echo "$OUTPUT" | grep -q '"schemaVersion":1'; then
    fail "schemaVersion is not 1 (frozen)"
else
    pass "schemaVersion=1 (frozen)"
fi

# antiFitScore must be 0..5
if [ -n "$PYBIN" ]; then
    SCORE=$(echo "$OUTPUT" | "$PYBIN" -c "import sys, json; print(json.load(sys.stdin)['antiFitScore'])" 2>/dev/null)
    if [ -n "$SCORE" ] && [ "$SCORE" -ge 0 ] 2>/dev/null && [ "$SCORE" -le 5 ] 2>/dev/null; then
        pass "antiFitScore=$SCORE in [0,5]"
    else
        fail "antiFitScore out of range or unparsable: '$SCORE'"
    fi
fi

# Determinism: two consecutive runs produce identical output. This depends on
# port-bind state being stable across two ~immediate invocations, which it
# essentially always is in a validation context.
OUTPUT2="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: second run.sh exited non-zero" >&2; exit 1; }
if [ "$OUTPUT" = "$OUTPUT2" ]; then
    pass "determinism: two consecutive runs produce identical output"
else
    fail "determinism: outputs differ between runs"
    echo "Run 1: $OUTPUT" >&2
    echo "Run 2: $OUTPUT2" >&2
fi

echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
