# DEC-FBR-IMPL-05 — Rust→JSON pricing SSOT export shim (Windows / PowerShell).
#
# Runs the `feedbackmonk-core` example binary and writes the JSON output to
# `marketing/src/data/tier_quotas.json` (gitignored — generated, not source).
# Called by `marketing/scripts/run-export.mjs` (which Astro's `prebuild` invokes).

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir '..\..')
$OutPath = Join-Path $RepoRoot 'marketing\src\data\tier_quotas.json'

$OutDir = Split-Path -Parent $OutPath
if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}

Push-Location $RepoRoot
try {
    $Json = & cargo run --quiet -p feedbackmonk-core --example export_tier_quotas
    if ($LASTEXITCODE -ne 0) {
        throw "cargo run --example export_tier_quotas exited with code $LASTEXITCODE"
    }
    # Write as UTF-8 without BOM for cross-platform parity with the .sh shim.
    [System.IO.File]::WriteAllText($OutPath, ($Json -join "`n"))
} finally {
    Pop-Location
}

$Size = (Get-Item $OutPath).Length
Write-Host "wrote marketing/src/data/tier_quotas.json ($Size bytes)"
