# project-type oracle self-test (Windows PowerShell)
# Verifies the oracle runs successfully and produces valid JSON matching the schema.

$ErrorActionPreference = "Stop"

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
try {
    $output = & powershell -NoProfile -File (Join-Path $oracleDir "run.ps1")
} catch {
    Write-Error "FAIL: run.ps1 exited with error: $_"
    exit 1
}

# Must be valid JSON
try {
    $parsed = $output | ConvertFrom-Json
} catch {
    Write-Error "FAIL: output is not valid JSON"
    Write-Error $output
    exit 1
}

# Must have all required schema fields
$requiredFields = @("languages", "frameworks", "build_systems", "test_command", "dev_command", "package_managers")
foreach ($field in $requiredFields) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

Write-Output "PASS: project-type oracle validates"
exit 0
