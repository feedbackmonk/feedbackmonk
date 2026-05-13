# project-runtime-state oracle self-test (Windows PowerShell)
# Verifies the oracle runs successfully and produces valid JSON matching v1 schema.

$ErrorActionPreference = "Stop"

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
try {
    $output = & powershell -NoProfile -File (Join-Path $oracleDir "run.ps1")
} catch {
    Write-Error "FAIL: run.ps1 exited with error: $_"
    exit 1
}

try {
    $parsed = $output | ConvertFrom-Json
} catch {
    Write-Error "FAIL: output is not valid JSON"
    Write-Error $output
    exit 1
}

$requiredFields = @(
    "schemaVersion","hasLiveDevServer","devPortRegistryEntries",
    "sharedBuildArtifacts","fileWatchers","statefulRuntime",
    "antiFitScore","antiFitReasons"
)
foreach ($field in $requiredFields) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

if ($parsed.schemaVersion -ne 1) {
    Write-Error "FAIL: schemaVersion is not 1 (frozen) - got $($parsed.schemaVersion)"
    exit 1
}

if ($parsed.antiFitScore -lt 0 -or $parsed.antiFitScore -gt 5) {
    Write-Error "FAIL: antiFitScore out of range - got $($parsed.antiFitScore)"
    exit 1
}

# Determinism check
$output2 = & powershell -NoProfile -File (Join-Path $oracleDir "run.ps1")
if ($output -ne $output2) {
    Write-Error "FAIL: determinism - outputs differ between runs"
    Write-Error "Run 1: $output"
    Write-Error "Run 2: $output2"
    exit 1
}

Write-Output "PASS: project-runtime-state oracle validates"
exit 0
