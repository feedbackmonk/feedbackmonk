# module-tree-map oracle (Windows PowerShell)
# Walks the project tree and emits a hierarchical JSON map of modules
# with their Synopsis sections (HCT § 3) and File Index entries.
#
# Output schema: FOUNDATIONS/HIERARCHICAL_CONTEXT_TRIAGE.md § 4.2
# Spec: HCT-03 (docs/specs/SPECIFICATION.md)

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

function Extract-Section([string[]]$lines, [string]$heading) {
    # Returns content lines between `## $heading` and the next `## ` heading.
    # Strips leading/trailing blank lines and HTML comment blocks.
    $result = @()
    $inSection = $false
    $inComment = $false
    foreach ($rawLine in $lines) {
        [string]$line = "$rawLine"
        if (-not $inSection) {
            if ($line -match "^##\s+$([regex]::Escape($heading))\s*$") {
                $inSection = $true
                continue
            }
            continue
        }
        if ($line -match '^##\s') { break }
        # Skip HTML comments inside the section
        if ($line -match '^\s*<!--') {
            $inComment = $true
            if ($line -match '-->\s*$') { $inComment = $false }
            continue
        }
        if ($inComment) {
            if ($line -match '-->\s*$') { $inComment = $false }
            continue
        }
        $result += $line
    }
    # Trim leading/trailing empty lines
    while ($result.Count -gt 0 -and [string]::IsNullOrWhiteSpace($result[0])) {
        $result = $result[1..($result.Count - 1)]
    }
    while ($result.Count -gt 0 -and [string]::IsNullOrWhiteSpace($result[-1])) {
        if ($result.Count -eq 1) { $result = @() } else { $result = $result[0..($result.Count - 2)] }
    }
    return ,$result
}

function Parse-FileIndex([string[]]$indexLines) {
    # Parses llms.txt-compatible file-index entries: `- [name](./path): purpose`
    # Returns array of {name, purpose} objects.
    $entries = @()
    foreach ($rawLine in $indexLines) {
        [string]$line = "$rawLine"
        if ($line -match '^\s*-\s*\[([^\]]+)\]\([^)]+\)\s*[:\-]\s*(.+)$') {
            $entries += [ordered]@{
                name = [string]$matches[1]
                purpose = ([string]$matches[2]).Trim()
            }
            continue
        }
        # Fallback: legacy `- **`name`** - purpose` shape
        if ($line -match '^\s*-\s*\*\*[`"]?([^`"\*]+)[`"]?\*\*\s*[-:]\s*(.+)$') {
            $entries += [ordered]@{
                name = ([string]$matches[1]).Trim()
                purpose = ([string]$matches[2]).Trim()
            }
        }
    }
    return ,$entries
}

# ---- Walk: collect all module READMEs ----
$modulesByPath = @{}
$missingSynopsis = @()
$totalModules = 0
$synopsized = 0

# Root README (project-level) is the tree root
$rootReadme = "README.md"
$rootSynopsis = $null
$rootFileIndex = $null
if (Test-Path $rootReadme) {
    $totalModules++
    $lines = @(Get-Content $rootReadme -Encoding UTF8 -ErrorAction SilentlyContinue)
    $synLines = Extract-Section $lines 'Synopsis'
    if ($synLines.Count -gt 0) {
        $rootSynopsis = ($synLines -join "`n").Trim()
        $synopsized++
    } else {
        $missingSynopsis += "."
    }
    $idxLines = Extract-Section $lines 'File Index'
    if ($idxLines.Count -gt 0) {
        $rootFileIndex = Parse-FileIndex $idxLines
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

    $totalModules++
    $lines = @(Get-Content $readmePath -Encoding UTF8 -ErrorAction SilentlyContinue)
    $synLines = Extract-Section $lines 'Synopsis'
    [string]$synopsis = $null
    if ($synLines.Count -gt 0) {
        $synopsis = ($synLines -join "`n").Trim()
        $synopsized++
    } else {
        $missingSynopsis += $normalized
    }
    $idxLines = Extract-Section $lines 'File Index'
    $fileIndex = $null
    if ($idxLines.Count -gt 0) {
        $fileIndex = Parse-FileIndex $idxLines
    }
    $modulesByPath[$normalized] = [ordered]@{
        path = $normalized
        synopsis = $synopsis
        file_index = $fileIndex
    }
}

# ---- Build hierarchical tree ----
# Each path's parent is the longest existing module path that is a prefix.
# Modules without a module ancestor attach to the root.
$childrenMap = @{}
foreach ($path in $modulesByPath.Keys) {
    $childrenMap[$path] = @()
}
$rootChildren = @()

$sortedPaths = @($modulesByPath.Keys | Sort-Object)
foreach ($path in $sortedPaths) {
    $parent = $null
    $segments = $path.Split('/')
    for ($i = $segments.Length - 1; $i -ge 1; $i--) {
        $candidate = ($segments[0..($i - 1)] -join '/')
        if ($modulesByPath.ContainsKey($candidate)) {
            $parent = $candidate
            break
        }
    }
    if ($parent) {
        $childrenMap[$parent] += $path
    } else {
        $rootChildren += $path
    }
}

function Build-Node([string]$path) {
    $mod = $modulesByPath[$path]
    $children = @()
    foreach ($childPath in ($childrenMap[$path] | Sort-Object)) {
        $children += (Build-Node $childPath)
    }
    $node = [ordered]@{
        path = $mod.path
        synopsis = $mod.synopsis
    }
    if ($mod.file_index -and $mod.file_index.Count -gt 0) {
        $node.file_index = @($mod.file_index)
    }
    $node.children = @($children)
    return $node
}

$rootNode = [ordered]@{
    path = "."
    synopsis = $rootSynopsis
}
if ($rootFileIndex -and $rootFileIndex.Count -gt 0) {
    $rootNode.file_index = @($rootFileIndex)
}
$builtChildren = @()
foreach ($childPath in ($rootChildren | Sort-Object)) {
    $builtChildren += (Build-Node $childPath)
}
$rootNode.children = @($builtChildren)

# Briefing line per HCT-05 acceptance: empty when total_modules == 0 (graceful absence).
[string]$briefing = ""
if ($totalModules -gt 0) {
    $modWord = if ($totalModules -eq 1) { "module" } else { "modules" }
    $briefing = "$totalModules $modWord, $synopsized/$totalModules with Synopsis. Invoke: /0-uldf-oracle module-tree-map"
}

$result = [ordered]@{
    root = $rootNode
    stats = [ordered]@{
        total_modules = $totalModules
        synopsized = $synopsized
        missing_synopsis = @($missingSynopsis | Sort-Object)
    }
    briefing = $briefing
}

$result | ConvertTo-Json -Compress -Depth 32
