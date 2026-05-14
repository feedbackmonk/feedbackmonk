#!/usr/bin/env pwsh
# tier-enforcement-status Verification Oracle (Windows shim).
# Delegates to oracle.py (canonical implementation). Python 3.8+ required.
# Forwards all arguments (notably `--full`) to the Python entrypoint.
$ErrorActionPreference = 'Stop'
$scriptDir = $PSScriptRoot
$py = $null
foreach ($candidate in @('python', 'python3', 'py')) {
    $cmd = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($cmd) { $py = $cmd.Source; break }
}
if (-not $py) {
    Write-Output "FAIL tier-enforcement-status (python not found)"
    exit 2
}
$oracle = Join-Path $scriptDir 'oracle.py'
& $py $oracle @args
exit $LASTEXITCODE
