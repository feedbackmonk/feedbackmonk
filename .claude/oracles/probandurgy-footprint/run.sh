#!/bin/bash
# probandurgy-footprint oracle (Unix)
# Verification Oracle: validates per-project manifest of declared Probandurgy
# mechanisms against actual build-config gates.
#
# Thin wrapper: all logic lives in run.py for cross-platform parity with run.ps1.
# Output: single JSON object matching FOOTPRINT-04 frozen output schema.
#
# Spec: SPECIFICATION.md § FOOTPRINT-04; DEC-62/63; DISC-FOOTPRINT-01

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PY_IMPL="$SCRIPT_DIR/run.py"

# Probe-verified Python interpreter selection. Microsoft Store stub on Windows
# returns 0 from `command -v python3` but errors on real use — exec a no-op
# import first to confirm the interpreter actually runs.
PYTHON=""
for cand in python3 python; do
    if command -v "$cand" >/dev/null 2>&1; then
        if "$cand" -c "import sys" >/dev/null 2>&1; then
            PYTHON="$cand"
            break
        fi
    fi
done

if [ -z "$PYTHON" ]; then
    # Fail-soft: emit graceful-absent shape with a manifest-malformed violation
    # explaining the dependency. Phase 11 is advisory so this doesn't block; an
    # agent reading the output learns exactly what's wrong.
    cat <<'EOF'
{"schemaVersion":"1","manifest_present":false,"mechanisms_declared":[],"violations":[{"mechanism":null,"kind":"manifest-malformed","evidence":"python interpreter","remediation":"Install Python 3 (python3 or python on PATH). The probandurgy-footprint oracle uses Python for cross-platform JSON parsing parity with run.ps1."}],"advisory":true}
EOF
    exit 0
fi

exec "$PYTHON" "$PY_IMPL" "$@"
