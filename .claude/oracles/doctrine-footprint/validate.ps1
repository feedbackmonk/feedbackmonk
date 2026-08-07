# doctrine-footprint oracle -- self-test (Windows). DAUD-01.
#
# Runs run.ps1 inside each test-fixtures/<name>/ and exact-matches stdout against
# that fixture's expected-output.json (the SAME golden file the bash validator
# uses -- run.sh and run.ps1 must be byte-identical). Exit 0 iff all match.

$ErrorActionPreference = 'Stop'

$scriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$oracleRun  = Join-Path $scriptDir 'run.ps1'
$fixturesDir = Join-Path $scriptDir 'test-fixtures'

if (-not (Test-Path -LiteralPath $fixturesDir -PathType Container)) {
    Write-Error "validate.ps1: no test-fixtures directory at $fixturesDir"
    exit 2
}

$pass = 0
$fail = 0
$failed = @()

Get-ChildItem -LiteralPath $fixturesDir -Directory | ForEach-Object {
    $name = $_.Name
    $expected = Join-Path $_.FullName 'expected-output.json'
    if (-not (Test-Path -LiteralPath $expected)) {
        Write-Warning "validate.ps1: fixture '$name' missing expected-output.json -- skipping"
        return
    }
    Push-Location $_.FullName
    try {
        # Pin the global-claude fallback inside the fixture so the JIG-05 wire
        # check never reads the real ~/.claude (hermetic golden files).
        $env:DOCTRINE_FOOTPRINT_GLOBAL_CLAUDE = (Join-Path $_.FullName '.claude')
        $actual  = (& powershell -NoProfile -File $oracleRun) -join ''
        $actual2 = (& powershell -NoProfile -File $oracleRun) -join ''
    } finally {
        Remove-Item Env:DOCTRINE_FOOTPRINT_GLOBAL_CLAUDE -ErrorAction SilentlyContinue
        Pop-Location
    }
    $expectedContent = (Get-Content -LiteralPath $expected -Raw)
    $actualTrim   = ($actual   -replace "[`r`n]",'')
    $actual2Trim  = ($actual2  -replace "[`r`n]",'')
    $expectedTrim = ($expectedContent -replace "[`r`n]",'')

    if ($actualTrim -ne $actual2Trim) {
        $fail++; $failed += "$name(non-deterministic)"
        Write-Output "FAIL: $name -- non-deterministic across two runs"
        return
    }
    if ($actualTrim -eq $expectedTrim) {
        $pass++; Write-Output "PASS: $name"
    } else {
        $fail++; $failed += $name
        Write-Output "FAIL: $name"
        Write-Output "  expected: $expectedTrim"
        Write-Output "  actual:   $actualTrim"
    }
}

Write-Output "---"
Write-Output "validate.ps1: $pass passed, $fail failed"
if ($fail -gt 0) {
    Write-Output ("Failed fixtures: " + ($failed -join ', '))
    exit 1
}
exit 0
