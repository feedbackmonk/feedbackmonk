# feedback-parity-status shim (Windows native) — delegates to the canonical Python oracle.
# Mirrors oracle.sh: passes through args and the exit code.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$py = (Get-Command python -ErrorAction SilentlyContinue) ?? (Get-Command python3 -ErrorAction SilentlyContinue)
if (-not $py) { Write-Error "python not found on PATH"; exit 2 }
& $py.Source (Join-Path $here "oracle.py") @args
exit $LASTEXITCODE
