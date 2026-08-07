#!/bin/bash
# module-size oracle self-test (Unix). Asserts run.sh emits valid JSON with the
# frozen schema fields, a legal status, and the advisory contract (exit 0).
set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if ! echo "$OUTPUT" | python -c "import sys,json;json.load(sys.stdin)" 2>/dev/null; then
    if ! echo "$OUTPUT" | python3 -c "import sys,json;json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2; echo "$OUTPUT" >&2; exit 1
    fi
fi

for field in '"status"' '"modules_scanned"' '"over_band"' '"over_band_total"' \
             '"no_readme"' '"no_data"' '"band"' '"softTokens"' '"enum_mode"' '"briefing"'; do
    if ! echo "$OUTPUT" | grep -q "$field"; then
        echo "FAIL: missing schema field $field" >&2; exit 1
    fi
done

# status must be one of the three legal values.
if ! echo "$OUTPUT" | grep -qE '"status":"(pass|warn|no-data)"'; then
    echo "FAIL: illegal status value" >&2; echo "$OUTPUT" >&2; exit 1
fi

# A clean pass MUST carry an empty briefing (suppressed-line convention).
if echo "$OUTPUT" | grep -q '"status":"pass"'; then
    if ! echo "$OUTPUT" | grep -qE '"briefing":""'; then
        echo "FAIL: status=pass but briefing is non-empty (quiet-path invariant)" >&2; exit 1
    fi
fi

echo "PASS: module-size oracle validates"
exit 0
