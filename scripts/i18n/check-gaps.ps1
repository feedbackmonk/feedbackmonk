#!/usr/bin/env pwsh
# check-gaps shim (Windows shim).
# Delegates to check-gaps.py (canonical implementation). Python 3.8+ required.
# Forwards all arguments to the Python entrypoint.
$ErrorActionPreference = 'Stop'
$scriptDir = $PSScriptRoot
$py = $null
foreach ($candidate in @('python', 'python3', 'py')) {
    $cmd = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($cmd) { $py = $cmd.Source; break }
}
if (-not $py) {
    Write-Output "FAIL check-gaps (python not found)"
    exit 2
}
$script = Join-Path $scriptDir 'check-gaps.py'
& $py $script @args
exit $LASTEXITCODE
