#!/bin/bash
# run.sh -- Bash entrypoint for the code-graph oracle (scrutiny Arc 3 A1).
# Thin wrapper: resolves a working python (PW-005 -- the WindowsApps python3
# shim is broken) and forwards to code-graph.py, so a bash caller and a
# PowerShell caller (run.ps1) drive BYTE-IDENTICAL JSON. The query logic lives
# in code-graph.py; the frozen schema + coverage-honesty rules + the reuse of
# the dependency-drift --emit-edges seam live in README.md.
#
# Usage:
#   run.sh [--deps M | --consumers M | --impact M | --cycles]
#          [--transitive] [--root DIR] [--config FILE] [--compact]
# Advisory: exits 0 always (non-zero only on a hard interpreter-absence path,
# which itself emits an honest NO-DATA JSON and still exits 0).
set -u
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PY=""
for c in python python3 py; do
  if command -v "$c" >/dev/null 2>&1 && "$c" -c 'import json,sys' >/dev/null 2>&1; then PY="$c"; break; fi
done
if [ -z "$PY" ]; then
  echo '{ "status": "no-data", "schemaVersion": "1.0", "query": { "verb": "summary", "target": null, "transitive": false }, "result": [], "coverage": "none", "coverageNote": "", "truncated": false, "reason": "no working python interpreter", "extractor": { "configured": false, "coveredModules": [] }, "briefing": "" }'
  exit 0
fi
exec "$PY" "$SCRIPT_DIR/code-graph.py" "$@"
