# module-tree-map oracle self-test (Windows PowerShell)
# Asserts: T1 single-module, T2 multi-module hierarchical, T3 missing-synopsis,
#          T4 graceful empty-project, T5 file-index extraction, T6 cross-platform parity
# (T6 is asserted by the validate.sh counterpart producing identical JSON.)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$runScript = Join-Path $oracleDir "run.ps1"

$tmpRoot = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpRoot | Out-Null

# Cache isolation: run.ps1 keeps its trigger-invalidate cache in a cache/ dir
# NEXT TO ITSELF -- copy the script into the sandbox so validation never
# touches the production cache.
$sandboxOracle = Join-Path $tmpRoot "oracle"
New-Item -ItemType Directory -Path $sandboxOracle | Out-Null
Copy-Item $runScript (Join-Path $sandboxOracle "run.ps1")
$runScript = Join-Path $sandboxOracle "run.ps1"

try {
    # ---- T4: empty project ----
    $t4 = Join-Path $tmpRoot "t4-empty"
    New-Item -ItemType Directory -Path $t4 | Out-Null
    Push-Location $t4
    try {
        $out = & powershell -NoProfile -File $runScript
        $d = $out | ConvertFrom-Json
        if ($d.root.path -ne ".") { throw "T4 root path expected '.' got '$($d.root.path)'" }
        if ($null -ne $d.root.synopsis) { throw "T4 root synopsis expected null" }
        if ($d.root.children.Count -ne 0) { throw "T4 root children expected empty" }
        if ($d.stats.total_modules -ne 0) { throw "T4 total_modules expected 0" }
        if ($d.stats.synopsized -ne 0) { throw "T4 synopsized expected 0" }
        if ($d.stats.missing_synopsis.Count -ne 0) { throw "T4 missing_synopsis expected empty" }
        if ($d.briefing -ne "") { throw "T4 briefing expected empty (graceful absence) got '$($d.briefing)'" }
        Write-Output "PASS T4: graceful empty-project (briefing empty -> gracefully absent)"
    } finally { Pop-Location }

    # ---- T1: single-module project ----
    $t1 = Join-Path $tmpRoot "t1-single"
    New-Item -ItemType Directory -Path $t1 | Out-Null
    Push-Location $t1
    try {
        @"
# Project

## Synopsis

Single-module project for testing. Come here for triage tests.

## Purpose & Responsibilities

Test fixture.
"@ | Set-Content -Path "README.md" -Encoding UTF8
        $out = & powershell -NoProfile -File $runScript
        $d = $out | ConvertFrom-Json
        if (-not $d.root.synopsis) { throw "T1 root synopsis missing" }
        if ($d.root.synopsis -notmatch 'Single-module') { throw "T1 synopsis content wrong: $($d.root.synopsis)" }
        if ($d.stats.total_modules -ne 1) { throw "T1 total expected 1" }
        if ($d.stats.synopsized -ne 1) { throw "T1 synopsized expected 1" }
        if ($d.stats.missing_synopsis.Count -ne 0) { throw "T1 missing expected empty" }
        if ($d.briefing -notmatch '1 module' -or $d.briefing -notmatch '1/1 with Synopsis' -or $d.briefing -notmatch '/0-uldf-oracle module-tree-map') {
            throw "T1 briefing format wrong: '$($d.briefing)'"
        }
        Write-Output "PASS T1: single-module (briefing populated per HCT-05 format)"
    } finally { Pop-Location }

    # ---- T2: multi-module hierarchical + T3 missing + T5 file-index ----
    $t2 = Join-Path $tmpRoot "t2-hierarchical"
    New-Item -ItemType Directory -Path (Join-Path $t2 "src/auth/session") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $t2 "src/billing") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $t2 "src/legacy") | Out-Null
    Push-Location $t2
    try {
        @"
# Root

## Synopsis

Root.

## File Index

- [src/](./src/): Application source
"@ | Set-Content -Path "README.md" -Encoding UTF8
        @"
# Auth

## Synopsis

Auth module. Come here for token issuance.

## File Index

- [tokens.ts](./tokens.ts): Token generation
- [session.ts](./session.ts): Session lifecycle
"@ | Set-Content -Path "src/auth/README.md" -Encoding UTF8
        @"
# Session

## Synopsis

Session lifecycle helpers.
"@ | Set-Content -Path "src/auth/session/README.md" -Encoding UTF8
        @"
# Billing

## Synopsis

Billing.
"@ | Set-Content -Path "src/billing/README.md" -Encoding UTF8
        @"
# Legacy

## Purpose & Responsibilities

Old code.
"@ | Set-Content -Path "src/legacy/README.md" -Encoding UTF8

        $out = & powershell -NoProfile -File $runScript
        $d = $out | ConvertFrom-Json
        if ($d.stats.total_modules -ne 5) { throw "T2 total_modules expected 5 got $($d.stats.total_modules)" }
        if ($d.stats.synopsized -ne 4) { throw "T2 synopsized expected 4 got $($d.stats.synopsized)" }
        if ($d.stats.missing_synopsis -notcontains "src/legacy") { throw "T3 src/legacy expected in missing_synopsis" }
        $rootChildPaths = @($d.root.children | ForEach-Object { $_.path } | Sort-Object)
        $expected = @("src/auth", "src/billing", "src/legacy")
        if (($rootChildPaths -join ",") -ne ($expected -join ",")) {
            throw "T2 root child paths expected '$($expected -join ",")' got '$($rootChildPaths -join ",")'"
        }
        $auth = $d.root.children | Where-Object { $_.path -eq "src/auth" } | Select-Object -First 1
        if ($auth.children.Count -ne 1) { throw "T2 src/auth expected 1 child" }
        if ($auth.children[0].path -ne "src/auth/session") { throw "T2 child path expected src/auth/session" }
        if (-not $auth.file_index) { throw "T5 src/auth expected file_index" }
        if ($auth.file_index.Count -ne 2) { throw "T5 src/auth file_index expected 2 entries" }
        $names = @($auth.file_index | ForEach-Object { $_.name } | Sort-Object)
        if (($names -join ",") -ne "session.ts,tokens.ts") { throw "T5 file_index names expected 'session.ts,tokens.ts' got '$($names -join ",")'" }
        if (-not $d.root.file_index) { throw "T5 root expected file_index" }
        Write-Output "PASS T2 + T3 + T5: multi-module hierarchical, missing-synopsis, file-index extraction"

        # ---- T7: cache warm-hit (freshness contract) ----
        $cacheFile = Join-Path $sandboxOracle "cache/latest.json"
        if (-not (Test-Path $cacheFile)) { throw "T7 cache not stored after compute" }
        $outWarm = (& powershell -NoProfile -File $runScript) -join "`n"
        if (($outWarm | ConvertFrom-Json).stats.total_modules -ne 5) { throw "T7 warm-hit content wrong" }
        Write-Output "PASS T7: cache warm-hit serves stored output"

        # ---- T8: invalidation -- edit / add / delete all recompute ----
        Start-Sleep -Seconds 1
        (Get-Item "src/auth/README.md").LastWriteTime = Get-Date
        $d8 = ((& powershell -NoProfile -File $runScript) -join "`n") | ConvertFrom-Json
        if ($d8.stats.total_modules -ne 5) { throw "T8a recompute total" }
        Write-Output "PASS T8a: mtime edit invalidates"
        New-Item -ItemType Directory -Path "src/newmod" -Force | Out-Null
        Set-Content -Path "src/newmod/README.md" -Value "# New`n`n## Synopsis`n`nNew module." -Encoding UTF8
        $d8b = ((& powershell -NoProfile -File $runScript) -join "`n") | ConvertFrom-Json
        if ($d8b.stats.total_modules -ne 6) { throw "T8b add not seen: $($d8b.stats.total_modules)" }
        Write-Output "PASS T8b: added README invalidates"
        Remove-Item -Force "src/newmod/README.md"
        $d8c = ((& powershell -NoProfile -File $runScript) -join "`n") | ConvertFrom-Json
        if ($d8c.stats.total_modules -ne 5) { throw "T8c delete not seen: $($d8c.stats.total_modules)" }
        Write-Output "PASS T8c: deleted README invalidates"

        # ---- T9: briefing-context cold cache -- graceful absence + detached warm ----
        Remove-Item -Recurse -Force (Join-Path $sandboxOracle "cache")
        $env:ULDF_BRIEFING = "1"
        try {
            $d9 = ((& powershell -NoProfile -File $runScript) -join "`n") | ConvertFrom-Json
            if ($d9.briefing -ne "") { throw "T9 cold briefing must be empty (graceful absence)" }
            if ($d9.cache -ne "cold-refreshing") { throw "T9 cache marker missing" }
            Write-Output "PASS T9: briefing-context cold cache gracefully absents (no stale serve)"
            $warmed = $false
            foreach ($i in 1..15) {
                if (Test-Path $cacheFile) { $warmed = $true; break }
                Start-Sleep -Seconds 1
            }
            if (-not $warmed) { throw "T9b detached refresh did not warm the cache" }
            Start-Sleep -Seconds 1
            $d9b = ((& powershell -NoProfile -File $runScript) -join "`n") | ConvertFrom-Json
            if ($d9b.stats.total_modules -ne 5) { throw "T9b warmed briefing serve wrong: $($d9b.stats.total_modules)" }
            Write-Output "PASS T9b: detached refresh warmed the cache; next briefing serves it"
        } finally {
            $env:ULDF_BRIEFING = $null
        }
    } finally { Pop-Location }

    Write-Output "PASS: module-tree-map oracle validates (T1, T2, T3, T4, T5, T7, T8a-c, T9)"
    exit 0
} catch {
    Write-Error "FAIL: $_"
    exit 1
} finally {
    Remove-Item -Recurse -Force $tmpRoot -ErrorAction SilentlyContinue
}
