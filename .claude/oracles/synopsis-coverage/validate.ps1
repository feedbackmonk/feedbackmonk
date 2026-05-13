# synopsis-coverage Verification Oracle self-test (Windows PowerShell)
# Asserts: T1 all-conformant -> 100%, T2 missing -> correct missing[],
#          T3 over-length -> correct over_length[], T4 graceful empty,
#          T5 cross-platform parity (asserted by validate.sh producing identical JSON)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$runScript = Join-Path $oracleDir "run.ps1"

$tmpRoot = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpRoot | Out-Null

try {
    # T4: empty project
    $t4 = Join-Path $tmpRoot "t4-empty"
    New-Item -ItemType Directory -Path $t4 | Out-Null
    Push-Location $t4
    try {
        $out = & powershell -NoProfile -File $runScript
        $d = $out | ConvertFrom-Json
        if ($d.coverage_pct -ne 100) { throw "T4 coverage_pct expected 100 got $($d.coverage_pct)" }
        if ($d.total_modules -ne 0) { throw "T4 total expected 0" }
        if ($d.conformant_count -ne 0) { throw "T4 conformant expected 0" }
        if ($d.missing.Count -ne 0) { throw "T4 missing expected empty" }
        if ($d.over_length.Count -ne 0) { throw "T4 over_length expected empty" }
        if ($d.briefing -ne "") { throw "T4 briefing expected empty got '$($d.briefing)'" }
        Write-Output "PASS T4: graceful empty-project"
    } finally { Pop-Location }

    # T1: all conformant
    $t1 = Join-Path $tmpRoot "t1-all-conformant"
    New-Item -ItemType Directory -Path (Join-Path $t1 "src/auth") | Out-Null
    Push-Location $t1
    try {
        @"
# Root

## Synopsis

Root module.
"@ | Set-Content -Path "README.md" -Encoding UTF8
        @"
# Auth

## Synopsis

Auth module.
Comes here for tokens.
"@ | Set-Content -Path "src/auth/README.md" -Encoding UTF8
        $out = & powershell -NoProfile -File $runScript
        $d = $out | ConvertFrom-Json
        if ($d.coverage_pct -ne 100) { throw "T1 coverage_pct expected 100 got $($d.coverage_pct)" }
        if ($d.total_modules -ne 2) { throw "T1 total expected 2 got $($d.total_modules)" }
        if ($d.conformant_count -ne 2) { throw "T1 conformant expected 2 got $($d.conformant_count)" }
        if ($d.missing.Count -ne 0) { throw "T1 missing expected empty" }
        if ($d.over_length.Count -ne 0) { throw "T1 over_length expected empty" }
        if ($d.briefing -ne "") { throw "T1 briefing expected empty got '$($d.briefing)'" }
        Write-Output "PASS T1: all conformant -> 100%, no briefing"
    } finally { Pop-Location }

    # T2: some missing
    $t2 = Join-Path $tmpRoot "t2-some-missing"
    New-Item -ItemType Directory -Path (Join-Path $t2 "src/auth") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $t2 "src/billing") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $t2 "src/legacy") | Out-Null
    Push-Location $t2
    try {
        @"
# Root

## Synopsis

Root.
"@ | Set-Content -Path "README.md" -Encoding UTF8
        @"
# Auth

## Synopsis

Auth.
"@ | Set-Content -Path "src/auth/README.md" -Encoding UTF8
        @"
# Billing

## Synopsis

Billing.
"@ | Set-Content -Path "src/billing/README.md" -Encoding UTF8
        @"
# Legacy

## Purpose & Responsibilities

Old.
"@ | Set-Content -Path "src/legacy/README.md" -Encoding UTF8

        $out = & powershell -NoProfile -File $runScript
        $d = $out | ConvertFrom-Json
        if ($d.total_modules -ne 4) { throw "T2 total expected 4 got $($d.total_modules)" }
        if ($d.conformant_count -ne 3) { throw "T2 conformant expected 3 got $($d.conformant_count)" }
        if ($d.coverage_pct -ne 75) { throw "T2 coverage_pct expected 75 got $($d.coverage_pct)" }
        if ($d.missing -notcontains "src/legacy") { throw "T2 src/legacy expected in missing got $($d.missing -join ',')" }
        if ($d.over_length.Count -ne 0) { throw "T2 over_length expected empty" }
        if (-not $d.briefing.StartsWith("75%")) { throw "T2 briefing expected to start with 75% got '$($d.briefing)'" }
        if ($d.briefing -notmatch '1 missing') { throw "T2 briefing should report 1 missing got '$($d.briefing)'" }
        Write-Output "PASS T2: missing surfaced in missing[] and briefing"
    } finally { Pop-Location }

    # T3: over-length
    $t3 = Join-Path $tmpRoot "t3-over-length"
    New-Item -ItemType Directory -Path (Join-Path $t3 "src/big") | Out-Null
    Push-Location $t3
    try {
        @"
# Root

## Synopsis

Root.
"@ | Set-Content -Path "README.md" -Encoding UTF8
        @"
# Big

## Synopsis

Line 1.
Line 2.
Line 3.
Line 4.
Line 5.
Line 6.
Line 7.
"@ | Set-Content -Path "src/big/README.md" -Encoding UTF8
        $out = & powershell -NoProfile -File $runScript
        $d = $out | ConvertFrom-Json
        if ($d.total_modules -ne 2) { throw "T3 total expected 2 got $($d.total_modules)" }
        if ($d.conformant_count -ne 1) { throw "T3 conformant expected 1 got $($d.conformant_count)" }
        if ($d.over_length -notcontains "src/big") { throw "T3 src/big expected in over_length got $($d.over_length -join ',')" }
        if ($d.missing.Count -ne 0) { throw "T3 missing expected empty got $($d.missing -join ',')" }
        if ($d.coverage_pct -ne 50) { throw "T3 coverage_pct expected 50 got $($d.coverage_pct)" }
        if ($d.briefing -notmatch '1 over-length') { throw "T3 briefing should report over-length got '$($d.briefing)'" }
        Write-Output "PASS T3: over-length surfaced in over_length[] and briefing"
    } finally { Pop-Location }

    Write-Output "PASS: synopsis-coverage oracle validates (T1, T2, T3, T4)"
    exit 0
} catch {
    Write-Error "FAIL: $_"
    exit 1
} finally {
    Remove-Item -Recurse -Force $tmpRoot -ErrorAction SilentlyContinue
}
