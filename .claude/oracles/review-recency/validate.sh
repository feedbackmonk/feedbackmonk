#!/bin/bash
# review-recency oracle self-test (Unix) — RECENCY-03/04.
set -e
ORACLE_DIR="$(dirname "$0")"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if ! echo "$OUTPUT" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
    if ! echo "$OUTPUT" | python -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        echo "$OUTPUT" >&2
        exit 1
    fi
fi

for field in '"recent"' '"details"' '"project"' '"recentDays"' '"recentSkills"' '"briefing"'; do
    if ! echo "$OUTPUT" | grep -q "$field"; then
        echo "FAIL: missing schema field $field" >&2
        exit 1
    fi
done

# recent=false MUST carry an empty briefing (quiet-path invariant).
if echo "$OUTPUT" | grep -q '"recent":false'; then
    if ! echo "$OUTPUT" | grep -qE '"briefing":[[:space:]]*""'; then
        echo "FAIL: recent=false but briefing is non-empty (quiet-path invariant)" >&2
        echo "$OUTPUT" >&2
        exit 1
    fi
fi

echo "PASS: review-recency oracle validates"
exit 0
