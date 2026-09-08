#!/usr/bin/env bash
# multi-tenant-isolation-check Verification Oracle (Unix shim).
# Delegates to oracle.py (canonical implementation). Python 3.8+ required.
set -u
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Probe each candidate by actually invoking --version so the Windows
# Microsoft Store python stub (which exits non-zero) is rejected.
PY=""
for c in python3 python py; do
    if command -v "$c" >/dev/null 2>&1; then
        if "$c" --version >/dev/null 2>&1; then
            PY="$c"
            break
        fi
    fi
done
if [ -z "$PY" ]; then
    echo "FAIL multi-tenant-isolation-check (python3 not found)"
    exit 2
fi
exec "$PY" "$script_dir/oracle.py"
