# concurrent-mutation oracle self-test (Windows) -- CSI-10.
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

foreach ($field in @('external_mutation', 'mutations', 'baseline_age_seconds', 'summary', 'briefing')) {
    if (-not ($obj.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

Write-Output "PASS: concurrent-mutation oracle validates"
exit 0
