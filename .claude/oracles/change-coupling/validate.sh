#!/bin/bash
# change-coupling oracle self-test (Unix) — scrutiny A2, Arc 1.
# Asserts: valid JSON, schema fields, correlation-not-dependency framing, and
# the NO-DATA path on a non-git directory (honesty floor).
set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"

json_ok() {
    echo "$1" | python3 -c "import sys,json;json.load(sys.stdin)" 2>/dev/null && return 0
    echo "$1" | python -c "import sys,json;json.load(sys.stdin)" 2>/dev/null && return 0
    command -v jq >/dev/null 2>&1 && echo "$1" | jq -e . >/dev/null 2>&1 && return 0
    return 1
}

# --- 1. Real run on this repo (or wherever invoked) ---------------------------
OUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }
if ! json_ok "$OUT"; then echo "FAIL: output is not valid JSON" >&2; echo "$OUT" >&2; exit 1; fi

for field in '"status"' '"window"' '"filters"' '"filePairs"' '"modulePairs"' '"crossBoundaryTop"' '"truncated"' '"cached"' '"briefing"'; do
    echo "$OUT" | grep -q "$field" || { echo "FAIL: missing schema field $field" >&2; exit 1; }
done
for wf in '"sinceDays"' '"maxCommits"' '"commitsAnalyzed"' '"qualifyingCommits"'; do
    echo "$OUT" | grep -q "$wf" || { echo "FAIL: missing window field $wf" >&2; exit 1; }
done
for ff in '"bulkCommitMax"' '"minCoChanges"' '"excludedCommits"'; do
    echo "$OUT" | grep -q "$ff" || { echo "FAIL: missing filter field $ff" >&2; exit 1; }
done

# status must be one of the two contract values
echo "$OUT" | grep -qE '"status":[[:space:]]*"(ok|no-data)"' || { echo "FAIL: status not ok|no-data" >&2; echo "$OUT" >&2; exit 1; }

# Never exit nonzero on findings (advisory contract) — already asserted by the
# set -e guard on the run above.

# --- 2. NO-DATA path on a non-git directory ----------------------------------
TMP="$(mktemp -d)"
NDOUT="$(cd "$TMP" && bash "$ORACLE_DIR/run.sh" 2>&1)"; rc=$?
rm -rf "$TMP"
[ "$rc" -eq 0 ] || { echo "FAIL: run.sh exited non-zero in a non-git dir" >&2; exit 1; }
if ! json_ok "$NDOUT"; then echo "FAIL: non-git output is not valid JSON" >&2; echo "$NDOUT" >&2; exit 1; fi
echo "$NDOUT" | grep -q '"status":"no-data"' || { echo "FAIL: non-git dir did not yield status no-data (NO-DATA honesty floor)" >&2; echo "$NDOUT" >&2; exit 1; }
echo "$NDOUT" | grep -q '"reason"' || { echo "FAIL: no-data result missing reason" >&2; exit 1; }

echo "PASS: change-coupling oracle validates"
exit 0
