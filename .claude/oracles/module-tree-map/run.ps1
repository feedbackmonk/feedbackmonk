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

# Walk-time prune (WinDirFul 2026-07-10, DEFER-windirful § 2; twin of the
# run.sh _MTM_PRUNE block): Get-ChildItem -Recurse descended node_modules /
# target etc. with IsExcluded filtering only afterwards. This walker skips
# excluded directory NAMES at descent time; the two path-based excludes stay
# covered by the IsExcluded post-filter callers keep. Output set identical.
$script:PruneNames = @('node_modules','target','.git','.vscode','.idea','dist','build','out','coverage','__pycache__','.venv','venv')
function Get-PrunedChildItems([string]$Root, [switch]$Directories, [string]$Filter) {
    $out = [System.Collections.Generic.List[object]]::new()
    $stack = [System.Collections.Stack]::new()
    $rootItem = Get-Item -LiteralPath $Root -ErrorAction SilentlyContinue
    if (-not $rootItem) { return @() }
    $stack.Push($rootItem)
    while ($stack.Count -gt 0) {
        $dir = $stack.Pop()
        foreach ($child in (Get-ChildItem -LiteralPath $dir.FullName -ErrorAction SilentlyContinue)) {
            if ($child.PSIsContainer) {
                if ($script:PruneNames -contains $child.Name) { continue }
                if ($Directories) { $out.Add($child) }
                $stack.Push($child)
            } elseif (-not $Directories) {
                if (-not $Filter -or $child.Name -like $Filter) { $out.Add($child) }
            }
        }
    }
    return ($out | Sort-Object FullName)
}


# =============================================================================
# Trigger-invalidate cache -- implements the freshness contract the manifest
# declares (previously declared-but-unimplemented; the multi-second-to-minute
# full recompute per call is why DEC-86 deferred this oracle from the
# session-start briefing budget). Set-level digest (path+mtime+size of every
# **/README.md, excludes-filtered) so edits, adds, deletes, AND renames all
# invalidate; the cache is stored only when pre/post-compute digests agree
# (mid-compute mutation can never freeze into a fresh-looking cache).
# --refresh / --no-cache force a recompute. Twin contract: run.sh cache block.
# =============================================================================
$script:OracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$script:CacheDir = Join-Path $script:OracleDir "cache"
$script:CacheFile = Join-Path $script:CacheDir "latest.json"
$script:DigestFile = Join-Path $script:CacheDir "trigger-digest.txt"

$script:ForceRefresh = $false
foreach ($a in $args) {
    if ($a -eq '--refresh' -or $a -eq '--no-cache') { $script:ForceRefresh = $true }
}

function Get-TriggerDigest {
    $entries = Get-PrunedChildItems -Root . -Filter 'README.md' |
        Where-Object { -not (IsExcluded $_.FullName.Substring((Get-Location).Path.Length).TrimStart('\','/')) } |
        Sort-Object FullName |
        ForEach-Object { "{0}`t{1}`t{2}" -f $_.FullName, $_.LastWriteTimeUtc.Ticks, $_.Length }
    $joined = ($entries -join "`n")
    $md5 = [System.Security.Cryptography.MD5]::Create()
    $hash = $md5.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($joined))
    return ([System.BitConverter]::ToString($hash) -replace '-', '').ToLowerInvariant()
}

$script:DigestPre = Get-TriggerDigest
if (-not $script:ForceRefresh -and $script:DigestPre -and
    (Test-Path $script:CacheFile) -and (Test-Path $script:DigestFile) -and
    ((Get-Content $script:DigestFile -Raw -ErrorAction SilentlyContinue) -eq $script:DigestPre)) {
    Get-Content $script:CacheFile -Raw -Encoding UTF8
    exit 0
}

$script:RefreshMark = Join-Path $script:CacheDir "refresh-in-progress"

# Cold cache in briefing context (ULDF_BRIEFING=1): detached background
# refresh + graceful absence this session (declared cost is the WARM cost, so
# an inline cold compute would be killed at 3x-declared and the cache could
# never warm via the briefing path). Stale cache is never served. Stampede
# guard: skip the spawn when a refresh started <10 min ago.
if (-not $script:ForceRefresh -and $env:ULDF_BRIEFING -eq '1') {
    $spawn = $true
    if (Test-Path $script:RefreshMark) {
        $age = ((Get-Date) - (Get-Item $script:RefreshMark).LastWriteTime).TotalSeconds
        if ($age -lt 600) { $spawn = $false }
    }
    if ($spawn) {
        try {
            if (-not (Test-Path $script:CacheDir)) { New-Item -ItemType Directory -Path $script:CacheDir -Force | Out-Null }
            [System.IO.File]::WriteAllText($script:RefreshMark, "", [System.Text.UTF8Encoding]::new($false))
            $self = $MyInvocation.MyCommand.Path -replace "'", "''"
            $wd = (Get-Location).Path -replace "'", "''"
            $mark = $script:RefreshMark -replace "'", "''"
            $cmd = "`$env:ULDF_BRIEFING=`$null; Set-Location -LiteralPath '$wd'; & '$self' --refresh | Out-Null; Remove-Item -Force '$mark' -ErrorAction SilentlyContinue"
            Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-Command',$cmd -WindowStyle Hidden | Out-Null
        } catch { }
    }
    Write-Output '{"root":{"path":".","synopsis":null,"children":[]},"stats":{"total_modules":0,"synopsized":0,"missing_synopsis":[]},"briefing":"","cache":"cold-refreshing"}'
    exit 0
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
$candidates = Get-PrunedChildItems -Root "." -Directories
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

# Store cache only when the trigger set didn't mutate mid-compute, then emit.
$jsonOut = $result | ConvertTo-Json -Compress -Depth 32
$digestPost = Get-TriggerDigest
if ($digestPost -and $digestPost -eq $script:DigestPre) {
    try {
        if (-not (Test-Path $script:CacheDir)) { New-Item -ItemType Directory -Path $script:CacheDir -Force | Out-Null }
        $utf8NoBom = New-Object System.Text.UTF8Encoding $false
        [System.IO.File]::WriteAllText($script:CacheFile, $jsonOut, $utf8NoBom)
        [System.IO.File]::WriteAllText($script:DigestFile, $digestPost, $utf8NoBom)
    } catch { }
}
$jsonOut
