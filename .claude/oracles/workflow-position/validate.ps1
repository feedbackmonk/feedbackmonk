# workflow-position oracle self-test (Windows PowerShell)

$ErrorActionPreference = "Stop"
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path

try {
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir "run.ps1") 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Error "FAIL: run.ps1 exited non-zero"
        exit 1
    }
} catch {
    Write-Error "FAIL: run.ps1 threw: $_"
    exit 1
}

# Parse JSON
try {
    $parsed = $output | ConvertFrom-Json
} catch {
    Write-Error "FAIL: output is not valid JSON: $_"
    exit 1
}

foreach ($field in @("position","latest_intake","latest_plan","spec_exists","ltads_active","ltads_session_status","suggested_next_command","proceed_hint")) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

$validPositions = @("NONE","POST-IDEATE","POST-INTAKE","POST-SPEC","POST-PLAN","IN-EXECUTION","POST-IMPLEMENTATION","UNKNOWN")
if ($parsed.position -notin $validPositions) {
    Write-Error "FAIL: position '$($parsed.position)' not in declared enum"
    exit 1
}

Write-Host "PASS: workflow-position oracle validates (position=$($parsed.position))"
exit 0
