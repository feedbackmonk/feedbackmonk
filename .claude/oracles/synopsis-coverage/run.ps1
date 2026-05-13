# synopsis-coverage Verification Oracle (Windows PowerShell)
# Reports the fraction of module READMEs conforming to the HCT Synopsis discipline:
# presence of a `## Synopsis` H2 section AND content between 1 and 5 non-empty lines.
#
# Output schema: see oracle.json. Spec: HCT-04 (docs/specs/SPECIFICATION.md).
# Verification Oracle category: FOUNDATIONS/ORACULURGY_DESIGN.md Part 11.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$excludes = @(
    'node_modules', 'target', '.git', '.vscode', '.idea', 'dist', 'build', 'out',
    'coverage', '__pycache__', '.venv', 'venv', '.claude/oracles/cache', '.claude/checkpoints'
)

function IsExcluded([string]$path) {
    $normalized = $path.Replace('\', '/').TrimStart('./')
    foreach ($ex in $excludes) {
        if ($normalized -eq $ex -or $normalized.StartsWith("$ex/") -or $normalized -match "/$ex(/|$)") {
            return $true
        }
    }
    return $false
}

function Extract-Synopsis([string]$readmePath) {
    # Returns array of non-empty content lines between `## Synopsis` and the next `## ` heading.
    # Strips HTML comment blocks (the template's authoring guidance is wrapped in <!-- -->).
    if (-not (Test-Path $readmePath)) { return $null }
    $lines = @(Get-Content $readmePath -Encoding UTF8 -ErrorAction SilentlyContinue)
    $inSection = $false
    $inComment = $false
    $found = $false
    $content = @()
    foreach ($rawLine in $lines) {
        [string]$line = "$rawLine"
        if (-not $inSection) {
            if ($line -match '^##\s+Synopsis\s*$') {
                $inSection = $true
                $found = $true
                continue
            }
            continue
        }
        if ($line -match '^##\s') { break }
        if ($line -match '^\s*<!--') {
            $inComment = $true
            if ($line -match '-->\s*$') { $inComment = $false }
            continue
        }
        if ($inComment) {
            if ($line -match '-->\s*$') { $inComment = $false }
            continue
        }
        if (-not [string]::IsNullOrWhiteSpace($line)) {
            $content += $line
        }
    }
    if (-not $found) { return $null }
    return ,$content
}

# ---- Walk: collect all module READMEs ----
$total = 0
$conformant = 0
$missing = @()
$overLength = @()

# Root README
if (Test-Path "README.md") {
    $total++
    $syn = Extract-Synopsis "README.md"
    if ($null -eq $syn) {
        $missing += "."
    } elseif ($syn.Count -lt 1 -or $syn.Count -gt 5) {
        if ($syn.Count -gt 5) { $overLength += "." }
        # Less than 1 non-empty line is also non-conformant; falls through to neither bucket
        # but is implicitly missing-content (we treat it as non-conformant by not incrementing).
        if ($syn.Count -lt 1) { $missing += "." }
    } else {
        $conformant++
    }
}

# Subdirectory READMEs
$candidates = Get-ChildItem -Path "." -Directory -Recurse -ErrorAction SilentlyContinue
foreach ($dir in $candidates) {
    $relPath = Resolve-Path -Relative $dir.FullName -ErrorAction SilentlyContinue
    if (-not $relPath) { continue }
    $relPath = $relPath -replace '^\.\\', '' -replace '^\.\/', ''
    $normalized = $relPath.Replace('\', '/')
    if ($normalized -eq '.' -or $normalized -eq '') { continue }
    if (IsExcluded $normalized) { continue }

    $readmePath = Join-Path $dir.FullName "README.md"
    if (-not (Test-Path $readmePath)) { continue }

    $total++
    $syn = Extract-Synopsis $readmePath
    if ($null -eq $syn) {
        $missing += $normalized
    } elseif ($syn.Count -gt 5) {
        $overLength += $normalized
    } elseif ($syn.Count -lt 1) {
        $missing += $normalized
    } else {
        $conformant++
    }
}

# ---- Compute stats ----
[int]$coveragePct = if ($total -eq 0) { 100 } else { [math]::Floor(($conformant * 100) / $total) }

# Briefing line per HCT-05 spec format. Empty when coverage_pct == 100 -> gracefully absent.
[string]$briefing = ""
if ($total -gt 0 -and $coveragePct -lt 100) {
    $missingCount = $missing.Count
    $overCount = $overLength.Count
    $briefing = "$coveragePct% ($missingCount missing, $overCount over-length). Run /0-uldf-uladp-compliance for details."
}

$result = [ordered]@{
    coverage_pct = $coveragePct
    conformant_count = $conformant
    total_modules = $total
    missing = @($missing | Sort-Object)
    over_length = @($overLength | Sort-Object)
    briefing_summary = $briefing
    briefing = $briefing
}

$result | ConvertTo-Json -Compress -Depth 5
