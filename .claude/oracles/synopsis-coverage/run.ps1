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

# Walk-time prune (WinDirFul 2026-07-10, DEFER-windirful § 2; twin of the
# run.sh _SC_PRUNE block): Get-ChildItem -Recurse descended node_modules /
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
    Write-Output '{"coverage_pct":100,"conformant_count":0,"total_modules":0,"missing":[],"over_length":[],"briefing_summary":"","briefing":"","cache":"cold-refreshing"}'
    exit 0
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

# Store cache only when the trigger set didn't mutate mid-compute, then emit.
$jsonOut = $result | ConvertTo-Json -Compress -Depth 5
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
