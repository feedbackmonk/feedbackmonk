# ltads-state oracle self-test (Windows PowerShell)
$ErrorActionPreference = "Stop"
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$output = & powershell -NoProfile -File (Join-Path $oracleDir "run.ps1")

try {
    $parsed = $output | ConvertFrom-Json
} catch {
    Write-Error "FAIL: output is not valid JSON"
    exit 1
}

$requiredFields = @("state", "has_ltads_dir", "is_tracked", "config_exists", "is_temporary", "session_id", "session_status", "summary")
foreach ($field in $requiredFields) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

Write-Output "PASS: ltads-state oracle validates"
exit 0
