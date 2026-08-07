# oracle-template-drift oracle (Windows PowerShell)
# REFRESH-01 -- detect when an oracle installed in this project's
# .claude/oracles/ differs from the current framework baseline (a
# starter-pack fix the add-only /0-uldf-setup-project never delivered).
#
# Output schema (FROZEN base + PACK-02 additive extension -- see oracle.json):
#   { drifted: bool, drifted_oracles: [{name, files:[...]}],
#     drift_count: int, compared_count: int,
#     missing_starter: [name,...], missing_count: int, briefing: string }
# missing_starter = PACK_MANIFEST.json packs.starter entries with no installed
# project dir (the F10 gap: the drift comparison only sees INSTALLED oracles).
#
# Read-only, idempotent. Compares the CR-normalized content of the 5
# functional files (oracle.json, run.sh, run.ps1, validate.sh,
# validate.ps1). README.md and test-fixtures/ are NOT compared. A project
# oracle dir with a .local-customized marker is skipped.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# ---- Tracked functional files ----------------------------------------------
$Tracked = @("oracle.json", "run.sh", "run.ps1", "validate.sh", "validate.ps1")

# ---- Source resolution ------------------------------------------------------
$baselineDir = $env:CLAUDE_ORACLE_BASELINE_DIR
$projectDir  = $env:CLAUDE_ORACLE_PROJECT_DIR
if ([string]::IsNullOrEmpty($projectDir)) { $projectDir = ".claude/oracles" }

if ([string]::IsNullOrEmpty($baselineDir)) {
    $homeDir = $env:USERPROFILE
    if ([string]::IsNullOrEmpty($homeDir)) { $homeDir = $env:HOME }
    if (-not [string]::IsNullOrEmpty($homeDir) -and (Test-Path (Join-Path $homeDir ".claude/oracles"))) {
        $baselineDir = Join-Path $homeDir ".claude/oracles"
    } else {
        # Framework-dev fallback: walk up looking for claude-template/oracles
        $probeDir = (Get-Location).Path
        for ($i = 0; $i -lt 6; $i++) {
            $candidate = Join-Path $probeDir "claude-template/oracles"
            if (Test-Path $candidate) {
                $baselineDir = $candidate
                break
            }
            $parent = Split-Path -Parent $probeDir
            if ([string]::IsNullOrEmpty($parent) -or $parent -eq $probeDir) { break }
            $probeDir = $parent
        }
    }
}

# ---- Graceful absent --------------------------------------------------------
function Write-Empty {
    $r = [ordered]@{
        drifted = $false
        drifted_oracles = @()
        drift_count = 0
        compared_count = 0
        missing_starter = @()
        missing_count = 0
        briefing = ""
    }
    $r | ConvertTo-Json -Compress -Depth 5
    exit 0
}
if ([string]::IsNullOrEmpty($baselineDir) -or -not (Test-Path $baselineDir)) { Write-Empty }
if (-not (Test-Path $projectDir)) { Write-Empty }

# ---- CR-normalized content hash --------------------------------------------
# Strips CR so CRLF<->LF copies never raise false drift. Missing file yields
# the sentinel "MISSING" so a project lacking a baseline file drifts.
$sha = [System.Security.Cryptography.SHA256]::Create()
function Get-NormHash([string]$Path) {
    if (-not (Test-Path $Path)) { return "MISSING" }
    $text = [System.IO.File]::ReadAllText($Path)
    $norm = $text -replace "`r", ""
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($norm)
    return ([System.BitConverter]::ToString($sha.ComputeHash($bytes)) -replace '-', '')
}

# ---- Compare each project oracle against baseline ---------------------------
$driftedOracles = New-Object System.Collections.Generic.List[object]
$comparedCount = 0

Get-ChildItem -LiteralPath $projectDir -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $name = $_.Name
    if ($name -eq "shared" -or $name -eq "candidates" -or $name -eq "cache") { return }
    $subdir = $_.FullName
    # Customization opt-out: skip pinned oracles entirely.
    if (Test-Path (Join-Path $subdir ".local-customized")) { return }
    $baseOracle = Join-Path $baselineDir $name
    # Project-only oracle (no baseline counterpart): never compared/touched.
    if (-not (Test-Path $baseOracle -PathType Container)) { return }

    $script:comparedCount++

    $driftedFiles = New-Object System.Collections.Generic.List[string]
    foreach ($f in $Tracked) {
        $baseFile = Join-Path $baseOracle $f
        # Only compare files the baseline actually ships for this oracle.
        if (-not (Test-Path $baseFile -PathType Leaf)) { continue }
        $bh = Get-NormHash $baseFile
        $ph = Get-NormHash (Join-Path $subdir $f)
        if ($bh -ne $ph) { $driftedFiles.Add($f) | Out-Null }
    }

    if ($driftedFiles.Count -gt 0) {
        $driftedOracles.Add([ordered]@{ name = $name; files = @($driftedFiles.ToArray()) }) | Out-Null
    }
}

$driftCount = $driftedOracles.Count
$drifted = ($driftCount -gt 0)

# ---- Missing starter oracles (PACK-02 additive extension) --------------------
# packs.starter entries in the baseline PACK_MANIFEST.json with no installed
# project dir. Manifest absent (pre-PACK-01 baseline) -> empty (graceful).
$missingStarter = New-Object System.Collections.Generic.List[string]
$manifestPath = Join-Path $baselineDir "PACK_MANIFEST.json"
if (Test-Path $manifestPath -PathType Leaf) {
    try {
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        foreach ($name in @($manifest.packs.starter)) {
            if ([string]::IsNullOrEmpty($name)) { continue }
            # Phantom manifest entries are validate-pack-manifest's finding.
            if (-not (Test-Path (Join-Path $baselineDir $name) -PathType Container)) { continue }
            if (-not (Test-Path (Join-Path $projectDir $name) -PathType Container)) {
                $missingStarter.Add([string]$name) | Out-Null
            }
        }
    } catch { }
}
$missingCount = $missingStarter.Count

# `drifted` keeps its frozen semantics (content drift of INSTALLED oracles);
# the briefing fires on drift OR missing-starter gaps.
$emdash = [char]0x2014
if ($driftCount -gt 0 -and $missingCount -gt 0) {
    $briefing = "oracle-template-drift: $driftCount oracle(s) drifted from framework baseline, $missingCount starter oracle(s) not installed $emdash run /0-uldf-migrate-oracles"
} elseif ($driftCount -gt 0) {
    $briefing = "oracle-template-drift: $driftCount oracle(s) drifted from framework baseline $emdash run /0-uldf-migrate-oracles to refresh"
} elseif ($missingCount -gt 0) {
    $briefing = "oracle-template-drift: $missingCount starter oracle(s) not installed $emdash run /0-uldf-migrate-oracles to install"
} else {
    $briefing = ""
}

$result = [ordered]@{
    drifted = $drifted
    drifted_oracles = @($driftedOracles.ToArray())
    drift_count = $driftCount
    compared_count = $comparedCount
    missing_starter = @($missingStarter.ToArray())
    missing_count = $missingCount
    briefing = $briefing
}
$result | ConvertTo-Json -Compress -Depth 5
exit 0
