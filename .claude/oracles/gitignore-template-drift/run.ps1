# gitignore-template-drift oracle (Windows PowerShell)
# HYGIENE-03 — detect when project's .gitignore lacks framework-managed
# patterns from the current claude-template baseline.
#
# Output schema (FROZEN — see channels/messages.md [ARC1-W3] / oracle.json):
#   { drifted: bool, missing_patterns: [string], baseline_patterns: int,
#     project_patterns: int, briefing: string }

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# ---- Source resolution ------------------------------------------------------
$baselineFile = $env:CLAUDE_GITIGNORE_BASELINE
$projectFile  = $env:CLAUDE_GITIGNORE_PROJECT
if ([string]::IsNullOrEmpty($projectFile)) { $projectFile = ".gitignore" }

if ([string]::IsNullOrEmpty($baselineFile)) {
    $homeDir = $env:USERPROFILE
    if ([string]::IsNullOrEmpty($homeDir)) { $homeDir = $env:HOME }
    if (-not [string]::IsNullOrEmpty($homeDir) -and (Test-Path (Join-Path $homeDir ".claude/.gitignore"))) {
        $baselineFile = Join-Path $homeDir ".claude/.gitignore"
    } else {
        # Framework-dev fallback: walk up looking for claude-template/.gitignore
        $probeDir = (Get-Location).Path
        for ($i = 0; $i -lt 6; $i++) {
            $candidate = Join-Path $probeDir "claude-template/.gitignore"
            if (Test-Path $candidate) {
                $baselineFile = $candidate
                break
            }
            $parent = Split-Path -Parent $probeDir
            if ([string]::IsNullOrEmpty($parent) -or $parent -eq $probeDir) { break }
            $probeDir = $parent
        }
    }
}

# ---- Header marker ----------------------------------------------------------
# Em-dash (U+2014) constructed via codepoint so PowerShell 5.1 (which reads
# .ps1 as cp1252 unless BOM-marked) parses this line correctly. The baseline
# file is UTF-8; Get-Content -Encoding UTF8 produces the same [char]0x2014
# in memory, so byte-equal Ordinal comparison succeeds.
$emdash = [char]0x2014
$frameworkHeader = "# Claude Code (session artifacts $emdash never commit)"

# ---- Graceful absent: no baseline found -------------------------------------
if ([string]::IsNullOrEmpty($baselineFile) -or -not (Test-Path $baselineFile)) {
    $emptyResult = [ordered]@{
        drifted = $false
        missing_patterns = @()
        baseline_patterns = 0
        project_patterns = 0
        briefing = ""
    }
    $emptyResult | ConvertTo-Json -Compress -Depth 4
    exit 0
}

# ---- Helper: extract patterns -----------------------------------------------
# Reads non-comment, non-blank lines from a file. If $afterHeader is provided,
# only lines AFTER the first occurrence of that exact (line-trimmed) header
# are considered.
function Get-Patterns {
    param(
        [string]$Path,
        [string]$AfterHeader = ""
    )
    if (-not (Test-Path $Path)) { return @() }
    $lines = Get-Content -LiteralPath $Path -Encoding UTF8
    $inSection = [string]::IsNullOrEmpty($AfterHeader)
    $patterns = New-Object System.Collections.Generic.List[string]
    foreach ($raw in $lines) {
        # Strip CR (Get-Content already handles \r\n in most cases, defensive)
        $line = $raw -replace "`r$", ""
        if (-not $inSection) {
            if ($line -eq $AfterHeader) { $inSection = $true }
            continue
        }
        $trimmed = $line.Trim()
        if ([string]::IsNullOrEmpty($trimmed)) { continue }
        if ($trimmed.StartsWith("#")) { continue }
        $patterns.Add($trimmed) | Out-Null
    }
    return ,$patterns.ToArray()
}

$baselinePatterns = Get-Patterns -Path $baselineFile -AfterHeader $frameworkHeader
$projectPatterns = @()
if (Test-Path $projectFile) {
    $projectPatterns = Get-Patterns -Path $projectFile
}

# ---- Compute missing patterns -----------------------------------------------
# Build a HashSet for O(1) project lookup. Use Ordinal compare for byte-exact
# matching (preserves the em-dash and any other UTF-8 characters verbatim).
$projectSet = New-Object System.Collections.Generic.HashSet[string]([System.StringComparer]::Ordinal)
foreach ($p in $projectPatterns) { [void]$projectSet.Add($p) }

$missing = New-Object System.Collections.Generic.List[string]
foreach ($p in $baselinePatterns) {
    if (-not $projectSet.Contains($p)) {
        $missing.Add($p) | Out-Null
    }
}

$missingCount = $missing.Count
$drifted = ($missingCount -gt 0)

if ($drifted) {
    $briefing = "gitignore-template-drift: $missingCount framework patterns missing $emdash run /0-uldf-migrate-hygiene to update"
} else {
    $briefing = ""
}

# ---- Emit JSON --------------------------------------------------------------
# ConvertTo-Json's default for empty arrays is "[]" with -Compress; force
# array semantics on missing_patterns by wrapping with @().
$result = [ordered]@{
    drifted = $drifted
    missing_patterns = @($missing.ToArray())
    baseline_patterns = $baselinePatterns.Count
    project_patterns = $projectPatterns.Count
    briefing = $briefing
}

$result | ConvertTo-Json -Compress -Depth 4
exit 0
