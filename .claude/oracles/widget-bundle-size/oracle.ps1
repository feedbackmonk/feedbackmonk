#!/usr/bin/env pwsh
# widget-bundle-size Verification Oracle (Windows shim).
# Delegates to oracle.py (canonical implementation). Python 3.8+ required.
$ErrorActionPreference = 'Stop'
$scriptDir = $PSScriptRoot
$py = $null
foreach ($candidate in @('python', 'python3', 'py')) {
    $cmd = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($cmd) { $py = $cmd.Source; break }
}
if (-not $py) {
    Write-Output "FAIL widget-bundle-size (python not found)"
    exit 2
}
$oracle = Join-Path $scriptDir 'oracle.py'
& $py $oracle
exit $LASTEXITCODE
