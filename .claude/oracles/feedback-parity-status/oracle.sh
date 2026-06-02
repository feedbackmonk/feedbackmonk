#!/usr/bin/env bash
# feedback-parity-status shim — delegates to the canonical Python oracle.
# Passes through args (e.g. --json) and the exit code (0 gate-open / 3 gate-closed / 2 error).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "${HERE}/oracle.py" "$@"
