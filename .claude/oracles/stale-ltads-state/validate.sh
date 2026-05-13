#!/bin/bash
# stale-ltads-state oracle self-test (Unix)
set -e
ORACLE_DIR="$(dirname "$0")"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

# Probe-verify python (Microsoft Store stub on Windows exits non-zero silently).
PY=""
if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
    PY="python3"
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
    PY="python"
fi

if [ -n "$PY" ]; then
    if ! echo "$OUTPUT" | "$PY" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        exit 1
    fi
fi

for field in stale details briefing; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

for sub in current_session_status current_session_id registry_status registry_pid_alive inconsistency_kind; do
    if ! echo "$OUTPUT" | grep -q "\"$sub\""; then
        echo "FAIL: missing details sub-field '$sub'" >&2
        exit 1
    fi
done

echo "PASS: stale-ltads-state oracle validates"
exit 0
