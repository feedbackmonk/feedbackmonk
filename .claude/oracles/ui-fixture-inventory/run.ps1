# ui-fixture-inventory oracle (Windows PowerShell)
# Answers: what fixture/smoke-test infrastructure does this project have?
#
# Output (frozen schema, TGFP-02): see run.sh header.
#
# Filesystem stat + glob only — never executes project scripts.

$ErrorActionPreference = "Stop"

# ---- Surface detection (composes with ui-surface-detector) ----
$SurfaceKind = "none"
$SurfaceOracleDir = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "..\ui-surface-detector"
if (Test-Path (Join-Path $SurfaceOracleDir "run.ps1")) {
    try {
        $surfaceOut = & powershell -NoProfile -File (Join-Path $SurfaceOracleDir "run.ps1") 2>$null
        if ($surfaceOut) {
            $parsed = $surfaceOut | ConvertFrom-Json -ErrorAction SilentlyContinue
            if ($parsed -and $parsed.surface_kind) {
                $SurfaceKind = $parsed.surface_kind
            }
        }
    } catch { }
}

# Surface presence (UI surfaces qualify; backend/cli do not)
$SurfacePresent = switch ($SurfaceKind) {
    "none" { $false }
    "cli-tool" { $false }
    "backend-service" { $false }
    default { $true }
}

# ---- No-surface short-circuit ----
if (-not $SurfacePresent) {
    Write-Output '{"has_fixtures":false,"patterns":[],"counts":{"fixtures":0,"smoke_specs":0,"e2e_specs":0},"conventions":[],"briefing":""}'
    exit 0
}

# ---- Pattern detection ----
$Patterns = New-Object System.Collections.Generic.List[string]
$Conventions = New-Object System.Collections.Generic.List[string]
$FixtureCount = 0
$SmokeCount = 0
$E2eCount = 0

function Get-FileCount {
    param([string]$Root, [string]$Filter, [int]$Depth = 6)
    try {
        $items = Get-ChildItem -Path $Root -Filter $Filter -Recurse -Depth $Depth -File -ErrorAction SilentlyContinue
        if ($items) { return @($items).Count } else { return 0 }
    } catch { return 0 }
}

# Detect: tests/fixtures/*-smoke.{ts,js,py}
if (Test-Path "tests\fixtures") {
    $n = (Get-FileCount "tests\fixtures" "*-smoke.ts") + `
         (Get-FileCount "tests\fixtures" "*-smoke.js") + `
         (Get-FileCount "tests\fixtures" "*-smoke.py")
    if ($n -gt 0) {
        $Patterns.Add("tests/fixtures/*-smoke.{ts,js,py}")
        $Conventions.Add("co-located smoke")
        $FixtureCount += $n
    }
}

# Detect: tests/smoke/*.spec.{ts,js,py}
if (Test-Path "tests\smoke") {
    $n = (Get-FileCount "tests\smoke" "*.spec.ts") + `
         (Get-FileCount "tests\smoke" "*.spec.js") + `
         (Get-FileCount "tests\smoke" "*.spec.py")
    if ($n -gt 0) {
        $Patterns.Add("tests/smoke/*.spec.{ts,js,py}")
        $SmokeCount += $n
    }
}

# Detect: e2e/**/*.spec.{ts,js,py}
if (Test-Path "e2e") {
    $n = (Get-FileCount "e2e" "*.spec.ts") + `
         (Get-FileCount "e2e" "*.spec.js") + `
         (Get-FileCount "e2e" "*.spec.py")
    if ($n -gt 0) {
        $Patterns.Add("e2e/**/*.spec.{ts,js,py}")
        $E2eCount += $n
    }
}

# Detect: __tests__/fixtures/**
$jestFix = Get-ChildItem -Path "." -Recurse -Depth 6 -Directory -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match "__tests__[\\/]fixtures" }
if ($jestFix) {
    $Patterns.Add("__tests__/fixtures/**")
    $Conventions.Add("jest fixtures")
}

# Detect: tests/visual/**
if (Test-Path "tests\visual") {
    $Patterns.Add("tests/visual/**")
    $Conventions.Add("visual regression")
}

# Detect: playwright.config.*
foreach ($ext in @("ts", "js", "mjs", "cjs")) {
    if (Test-Path "playwright.config.$ext") {
        $Patterns.Add("playwright.config.$ext")
        $Conventions.Add("playwright")
        break
    }
}

# Detect: cypress/
if (Test-Path "cypress") {
    $Patterns.Add("cypress/**")
    $Conventions.Add("cypress")
}

# Detect: vitest fixtures
foreach ($ext in @("ts", "js", "mjs", "cjs")) {
    if (Test-Path "vitest.config.$ext") {
        if ((Test-Path "tests\fixtures") -or (Test-Path "src\__tests__\fixtures")) {
            $Conventions.Add("vitest fixtures")
        }
        break
    }
}

# ---- Compose has_fixtures ----
$HasFixtures = ($FixtureCount -gt 0) -or ($SmokeCount -gt 0) -or ($E2eCount -gt 0) -or ($Patterns.Count -gt 0)

# ---- Compose briefing ----
$Briefing = ""
if (-not $HasFixtures) {
    $Briefing = "fixture-inventory: UI surface detected; no fixture infrastructure. /0-uldf-ldis-plan can scaffold."
}

# Cap briefing at 200 chars
if ($Briefing.Length -gt 200) {
    $Briefing = $Briefing.Substring(0, 197) + "..."
}

# ---- Emit JSON ----
# Dedupe conventions
$UniqueConventions = $Conventions | Select-Object -Unique

$obj = [ordered]@{
    has_fixtures = $HasFixtures
    patterns = @($Patterns)
    counts = [ordered]@{
        fixtures = $FixtureCount
        smoke_specs = $SmokeCount
        e2e_specs = $E2eCount
    }
    conventions = @($UniqueConventions)
    briefing = $Briefing
}

# Use Compress to keep on single line (briefing-line conventions)
$json = $obj | ConvertTo-Json -Depth 4 -Compress
Write-Output $json
exit 0
