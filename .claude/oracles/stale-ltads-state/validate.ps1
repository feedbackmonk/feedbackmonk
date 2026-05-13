# stale-ltads-state oracle self-test (Windows)
$ErrorActionPreference = 'Stop'
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$output = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir "run.ps1")
if ($LASTEXITCODE -ne 0) {
    Write-Error "FAIL: run.ps1 exited non-zero"
    exit 1
}

try {
    $obj = $output | ConvertFrom-Json -ErrorAction Stop
} catch {
    Write-Error "FAIL: output is not valid JSON: $_"
    exit 1
}

foreach ($field in @('stale', 'details', 'briefing')) {
    if (-not ($obj.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}
foreach ($sub in @('current_session_status', 'current_session_id', 'registry_status', 'registry_pid_alive', 'inconsistency_kind')) {
    if (-not ($obj.details.PSObject.Properties.Name -contains $sub)) {
        Write-Error "FAIL: missing details sub-field '$sub'"
        exit 1
    }
}

Write-Output "PASS: stale-ltads-state oracle validates"
exit 0
