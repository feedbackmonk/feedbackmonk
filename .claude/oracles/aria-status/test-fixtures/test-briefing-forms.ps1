# aria-status briefing-form fixtures (Windows PowerShell)
# See test-briefing-forms.sh for the cross-platform spec.

$ErrorActionPreference = "Stop"
$oracleDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$oracleRun = Join-Path $oracleDir "run.ps1"
$surfaceRun = Join-Path (Split-Path -Parent $oracleDir) "ui-surface-detector/run.ps1"

if (-not (Test-Path $oracleRun)) { Write-Error "FAIL: $oracleRun missing"; exit 1 }
if (-not (Test-Path $surfaceRun)) { Write-Error "FAIL: $surfaceRun missing"; exit 1 }

$root = Join-Path $env:TEMP ("aria-status-test-" + [Guid]::NewGuid().ToString("N"))
New-Item -Path $root -ItemType Directory -Force | Out-Null

$pass = 0
$fail = 0
$origDir = Get-Location

function Run-Case {
    param([string]$CaseName, [string]$ExpectedSubstr, [string]$Subdir)
    Set-Location (Join-Path $script:root $Subdir)
    try {
        $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $script:oracleRun 2>&1
        [string]$outStr = if ($out -is [array]) { $out -join "" } else { "$out" }
        $parsed = $outStr.Trim() | ConvertFrom-Json -ErrorAction Stop
        $briefing = if ($parsed.briefing) { [string]$parsed.briefing } else { "" }
        if (-not $ExpectedSubstr) {
            if (-not $briefing) {
                Write-Host "PASS [$CaseName]: briefing empty as expected"
                $script:pass++
            } else {
                Write-Host "FAIL [$CaseName]: expected empty briefing, got: '$briefing'"
                $script:fail++
            }
        } else {
            if ($briefing.Contains($ExpectedSubstr)) {
                Write-Host "PASS [$CaseName]: briefing matches"
                $script:pass++
            } else {
                Write-Host "FAIL [$CaseName]: expected substring '$ExpectedSubstr' not found in briefing: '$briefing'"
                $script:fail++
            }
        }
    } catch {
        Write-Host "FAIL [$CaseName]: oracle threw: $_"
        $script:fail++
    }
}

try {
    # Case 1: no-surface (cli-tool)
    $caseDir = Join-Path $root "case-no-surface"
    New-Item -Path (Join-Path $caseDir "bin") -ItemType Directory -Force | Out-Null
    Set-Content -Path (Join-Path $caseDir "package.json") -Value '{"name":"foo","bin":"./bin/foo"}'
    Run-Case "no-surface (cli-tool)" "" "case-no-surface"

    # Case 2: surface-but-no-instrumentation (Tauri)
    $caseDir = Join-Path $root "case-no-instr"
    New-Item -Path (Join-Path $caseDir "src-tauri") -ItemType Directory -Force | Out-Null
    Set-Content -Path (Join-Path $caseDir "Cargo.toml") -Value ""
    Set-Content -Path (Join-Path $caseDir "src-tauri/Cargo.toml") -Value ""
    Run-Case "surface-but-no-instrumentation (Tauri)" "no ARIA instrumentation" "case-no-instr"

    # Case 3: instrumented-but-unreachable
    $caseDir = Join-Path $root "case-configured"
    New-Item -Path (Join-Path $caseDir "src-tauri") -ItemType Directory -Force | Out-Null
    New-Item -Path (Join-Path $caseDir ".claude") -ItemType Directory -Force | Out-Null
    Set-Content -Path (Join-Path $caseDir "Cargo.toml") -Value ""
    Set-Content -Path (Join-Path $caseDir "src-tauri/Cargo.toml") -Value ""
    Set-Content -Path (Join-Path $caseDir ".claude/aria.json") -Value '{"endpoint_url":"http://127.0.0.1:14599/aria/health"}'
    Run-Case "instrumented-but-unreachable (Tauri+aria.json)" "configured but endpoint unreachable" "case-configured"
} finally {
    Set-Location $origDir
    Remove-Item -Path $root -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "Summary: $pass pass, $fail fail"
exit $fail
