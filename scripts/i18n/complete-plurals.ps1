#!/usr/bin/env pwsh
# complete-plurals shim (Windows shim).
# Delegates to complete-plurals.py (canonical implementation). Python 3.8+ required.
# Forwards all arguments to the Python entrypoint.
$ErrorActionPreference = 'Stop'
$scriptDir = $PSScriptRoot
$py = $null
foreach ($candidate in @('python', 'python3', 'py')) {
    $cmd = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($cmd) { $py = $cmd.Source; break }
}
if (-not $py) {
    Write-Output "FAIL complete-plurals (python not found)"
    exit 2
}
$script = Join-Path $scriptDir 'complete-plurals.py'
& $py $script @args
exit $LASTEXITCODE
