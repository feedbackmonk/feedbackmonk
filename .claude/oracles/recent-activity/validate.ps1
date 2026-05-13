# recent-activity oracle self-test (Windows PowerShell)
$ErrorActionPreference = "Stop"
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$output = & powershell -NoProfile -File (Join-Path $oracleDir "run.ps1")

try {
    $parsed = $output | ConvertFrom-Json
} catch {
    Write-Error "FAIL: output is not valid JSON"
    exit 1
}

$requiredFields = @("last_commits", "touched_directories_last_5", "commits_last_7_days", "commits_last_30_days")
foreach ($field in $requiredFields) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

Write-Output "PASS: recent-activity oracle validates"
exit 0
