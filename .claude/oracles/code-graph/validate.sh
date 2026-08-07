#!/bin/bash
# code-graph oracle self-test (Unix) — scrutiny Arc 3 A1.
# Asserts: valid JSON, the frozen schema fields, a legal status, the MANDATORY
# coverage field, and the NO-DATA path on a non-git directory (honesty floor).
set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"

json_ok() {
    echo "$1" | python3 -c "import sys,json;json.load(sys.stdin)" 2>/dev/null && return 0
    echo "$1" | python -c "import sys,json;json.load(sys.stdin)" 2>/dev/null && return 0
    command -v jq >/dev/null 2>&1 && echo "$1" | jq -e . >/dev/null 2>&1 && return 0
    return 1
}

# --- 1. Real run (default summary) on this repo (or wherever invoked) ---------
OUT="$(bash "$ORACLE_DIR/run.sh" --compact 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }
if ! json_ok "$OUT"; then echo "FAIL: output is not valid JSON" >&2; echo "$OUT" >&2; exit 1; fi

for field in '"status"' '"schemaVersion"' '"query"' '"result"' '"coverage"' '"coverageNote"' '"truncated"' '"briefing"'; do
    echo "$OUT" | grep -q "$field" || { echo "FAIL: missing schema field $field" >&2; exit 1; }
done
for qf in '"verb"' '"target"' '"transitive"'; do
    echo "$OUT" | grep -q "$qf" || { echo "FAIL: missing query field $qf" >&2; exit 1; }
done

# status must be one of the two contract values
echo "$OUT" | grep -qE '"status":"(ok|no-data)"' || { echo "FAIL: status not ok|no-data" >&2; echo "$OUT" >&2; exit 1; }
# coverage must be one of the three legal values (MANDATORY field, M6)
echo "$OUT" | grep -qE '"coverage":"(full|grep-only|none)"' || { echo "FAIL: coverage not full|grep-only|none" >&2; echo "$OUT" >&2; exit 1; }

# --- 2. --cycles verb answers with a legal coverage --------------------------
COUT="$(bash "$ORACLE_DIR/run.sh" --cycles --compact 2>&1)" || { echo "FAIL: --cycles exited non-zero" >&2; exit 1; }
if ! json_ok "$COUT"; then echo "FAIL: --cycles output is not valid JSON" >&2; exit 1; fi
echo "$COUT" | grep -qE '"coverage":"(full|grep-only|none)"' || { echo "FAIL: --cycles missing legal coverage" >&2; exit 1; }

# --- 3. NO-DATA path on a non-git directory ----------------------------------
TMP="$(mktemp -d)"
NDOUT="$(cd "$TMP" && bash "$ORACLE_DIR/run.sh" --compact 2>&1)"; rc=$?
rm -rf "$TMP"
[ "$rc" -eq 0 ] || { echo "FAIL: run.sh exited non-zero in a non-git dir" >&2; exit 1; }
if ! json_ok "$NDOUT"; then echo "FAIL: non-git output is not valid JSON" >&2; echo "$NDOUT" >&2; exit 1; fi
echo "$NDOUT" | grep -q '"status":"no-data"' || { echo "FAIL: non-git dir did not yield status no-data (NO-DATA honesty floor)" >&2; echo "$NDOUT" >&2; exit 1; }
echo "$NDOUT" | grep -q '"reason"' || { echo "FAIL: no-data result missing reason" >&2; exit 1; }

echo "PASS: code-graph oracle validates"
exit 0
