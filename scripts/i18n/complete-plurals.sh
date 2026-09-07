#!/usr/bin/env bash
# complete-plurals shim (Unix + Git Bash shim).
# Delegates to complete-plurals.py (canonical implementation). Python 3.8+ required.
# Forwards all arguments to the Python entrypoint.
set -u
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
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
    echo "FAIL complete-plurals (python3 not found)"
    exit 2
fi
exec "$PY" "$script_dir/complete-plurals.py" "$@"
