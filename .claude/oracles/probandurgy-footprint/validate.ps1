# probandurgy-footprint oracle -- self-test (Windows PowerShell)
#
# Iterates over test-fixtures/<name>/, runs the oracle inside each fixture, and
# compares stdout against the fixture's expected-output.json. Exit 0 iff all
# fixtures match. Emits per-fixture diffs on failure.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$OracleRun = Join-Path $ScriptDir "run.ps1"
$FixturesDir = Join-Path $ScriptDir "test-fixtures"

if (-not (Test-Path -LiteralPath $FixturesDir -PathType Container)) {
    Write-Error "validate.ps1: no test-fixtures directory at $FixturesDir"
    exit 2
}

$Pass = 0
$Fail = 0
$FailedNames = @()

Get-ChildItem -LiteralPath $FixturesDir -Directory | ForEach-Object {
    $name = $_.Name
    $fixturePath = $_.FullName
    $expectedPath = Join-Path $fixturePath "expected-output.json"
    if (-not (Test-Path -LiteralPath $expectedPath -PathType Leaf)) {
        Write-Warning "validate.ps1: fixture '$name' missing expected-output.json -- skipping"
        return
    }
    $expected = (Get-Content -LiteralPath $expectedPath -Raw) -replace "`r", "" -replace "`n", ""
    Push-Location $fixturePath
    try {
        $actual = & powershell -NoProfile -ExecutionPolicy Bypass -File $OracleRun 2>&1 | Out-String
    } finally {
        Pop-Location
    }
    $actual = $actual -replace "`r", "" -replace "`n", ""
    if ($actual -eq $expected) {
        $Pass++
        Write-Output "PASS: $name"
    } else {
        $Fail++
        $script:FailedNames += $name
        Write-Output "FAIL: $name"
        Write-Output "  expected: $expected"
        Write-Output "  actual:   $actual"
    }
}

Write-Output "---"
Write-Output "validate.ps1: $Pass passed, $Fail failed"
if ($Fail -gt 0) {
    Write-Output ("Failed fixtures: " + ($FailedNames -join ", "))
    exit 1
}
exit 0
