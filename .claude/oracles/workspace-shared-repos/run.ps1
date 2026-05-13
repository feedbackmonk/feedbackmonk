# workspace-shared-repos oracle (Windows PowerShell)
# Answers: which sibling git repos does this project consume via workspace declarations?
#
# Operates from the project root (CWD). Discovers shared repos from four sources, in
# priority order:
#   1. .claude/config.json  ->  sharedRepos: [{path, role?}] OR [string, ...]   (always wins)
#   2. pnpm-workspace.yaml  ->  packages: list (literal paths or glob patterns)
#   3. Cargo.toml           ->  [workspace] members = [...]
#   4. package.json         ->  workspaces: array OR {packages: [...]}
#
# Each candidate path is:
#   - resolved to absolute (relative paths resolve against the project root)
#   - filtered to those with their own <path>/.git/ directory
#   - skipped if it IS the local working tree (degenerate self-reference)
#   - skipped if it is INSIDE the local working tree (nested workspace package, not a sibling)
#   - deduplicated across sources by absolute path; explicit > pnpm > cargo > npm
#
# Output: JSON object per the FROZEN schema documented in README.md and oracle.json:
#   {count, repos:[{path, declarationSource, hasGit, hasClaudeDir}], discoveryMethod, _meta}
#
# Compute budget: 80ms (typical case 1-3 shared repos; bounded by file reads + git checks).
# Strategy: trigger-invalidate. Read-only on the filesystem (no mutation).
#
# Modes: default only (no --gc / --gc-cheap; this oracle does not sweep state).
#
# Spec: SHARED-CSI-01 in docs/specs/SPECIFICATION.md
# Decision: DEC-35 (discovery format scope) in docs/specs/DECISIONS.md
# Lineage: CSI-05 (dispatchable-sessions) and RETENTION-01..06 (archive-retention) per DISC-CSI-09.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# Sync .NET CWD with PowerShell's current location -- defends against the
# WriteAllText/ReadAllBytes class of CWD bug surfaced during CSI-01 smoke.
try { [Environment]::CurrentDirectory = (Get-Location).Path } catch { }

# Reject unknown modes (this oracle only has the default briefing path).
foreach ($a in $args) {
    if ($a -is [string] -and $a.StartsWith("--")) {
        Write-Error "workspace-shared-repos: unknown mode: $a"
        exit 1
    }
}

$ORACLE_VERSION = "1.0"
$SCHEMA_VERSION = 1
$startTicks     = [System.Diagnostics.Stopwatch]::StartNew()

# ---- Resolve project root (absolute, normalized) ----
$projectRoot = (Get-Location).Path
# Normalize separators to forward slashes for consistent string compare with bash output.
$projectRoot = $projectRoot.TrimEnd(@([char]'\', [char]'/')).Replace('\', '/')

function Get-ComputeMs { return [int]$startTicks.ElapsedMilliseconds }

function Emit-Empty {
    $computeMs = Get-ComputeMs
    Write-Output ('{"count":0,"repos":[],"discoveryMethod":"none","_meta":{"oracleVersion":"' + $ORACLE_VERSION + '","computeMs":' + $computeMs + ',"schemaVersion":' + $SCHEMA_VERSION + '}}')
    exit 0
}

# ---- Read JSON from a file with UTF-8 BOM tolerance ----
function Read-JsonFile {
    param([string]$Path)
    try {
        $bytes = [System.IO.File]::ReadAllBytes($Path)
        if ($bytes.Length -eq 0) { return $null }
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            $jsonText = [System.Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
        } else {
            $jsonText = [System.Text.Encoding]::UTF8.GetString($bytes)
        }
        if ([string]::IsNullOrWhiteSpace($jsonText)) { return $null }
        return $jsonText | ConvertFrom-Json -ErrorAction Stop
    } catch {
        return $null
    }
}

# ---- Resolve relative paths against the project root; normalize forward-slashes ----
function Resolve-AbsPath {
    param(
        [string]$Path,
        [string]$Base
    )
    if ([string]::IsNullOrEmpty($Path)) { return $null }
    if (-not $Base) { $Base = $projectRoot }

    # Already absolute?  POSIX (/) or Windows (X:\ or X:/)
    $abs = $null
    if ($Path -match '^/' -or $Path -match '^[A-Za-z]:[\\/]') {
        $abs = $Path
    } else {
        $abs = Join-Path $Base $Path
    }

    # Normalize: prefer Resolve-Path for existing dirs; fall back to lexical normalization.
    try {
        $resolved = (Resolve-Path -Path $abs -ErrorAction Stop).Path
        $abs = $resolved
    } catch {
        # Lexical normalize (path may not exist yet)
        try { $abs = [System.IO.Path]::GetFullPath($abs) } catch { }
    }

    return ($abs.TrimEnd(@([char]'\', [char]'/'))).Replace('\', '/')
}

# ---- Test for self / nested-inside-project ----
function Test-SelfOrNested {
    param([string]$Abs)
    if ([string]::IsNullOrEmpty($Abs)) { return $true }
    # Case-insensitive on Windows.
    $a = $Abs.ToLowerInvariant()
    $r = $projectRoot.ToLowerInvariant()
    if ($a -eq $r) { return $true }
    if ($a.StartsWith($r + "/")) { return $true }
    return $false
}

# ---- Priority rank ----
function Get-PriorityRank {
    param([string]$Source)
    switch ($Source) {
        "explicit" { return 4 }
        "pnpm"     { return 3 }
        "cargo"    { return 2 }
        "npm"      { return 1 }
        default    { return 0 }
    }
}

# ---- State for dedup ----
# We store entries as parallel arrays in $script: scope so helper functions can append.
$script:RegisteredPaths   = New-Object System.Collections.ArrayList
$script:RegisteredSources = New-Object System.Collections.ArrayList
$script:SourcesUsed       = New-Object System.Collections.ArrayList

function Register-Repo {
    param([string]$Abs, [string]$Source)
    if ([string]::IsNullOrEmpty($Abs)) { return }

    # Filter: must have .git/
    if (-not (Test-Path (Join-Path $Abs ".git"))) { return }

    # Filter: skip self / nested
    if (Test-SelfOrNested -Abs $Abs) { return }

    # Dedup: see if already registered
    for ($i = 0; $i -lt $script:RegisteredPaths.Count; $i++) {
        if ($script:RegisteredPaths[$i].ToLowerInvariant() -eq $Abs.ToLowerInvariant()) {
            $newRank      = Get-PriorityRank -Source $Source
            $existingRank = Get-PriorityRank -Source $script:RegisteredSources[$i]
            if ($newRank -gt $existingRank) {
                $script:RegisteredSources[$i] = $Source
                if (-not $script:SourcesUsed.Contains($Source)) {
                    [void]$script:SourcesUsed.Add($Source)
                }
            }
            return
        }
    }

    [void]$script:RegisteredPaths.Add($Abs)
    [void]$script:RegisteredSources.Add($Source)
    if (-not $script:SourcesUsed.Contains($Source)) {
        [void]$script:SourcesUsed.Add($Source)
    }
}

# ---- Glob expansion + register ----
function Expand-AndRegister {
    param([string]$Pattern, [string]$Source, [string]$Base)

    if ([string]::IsNullOrEmpty($Pattern)) { return }
    if (-not $Base) { $Base = $projectRoot }

    # Strip a single trailing slash if present
    $Pattern = $Pattern.TrimEnd(@([char]'/', [char]'\'))

    if ($Pattern -match '[\*\?\[]') {
        # Glob expansion.
        $absPattern = $null
        if ($Pattern -match '^/' -or $Pattern -match '^[A-Za-z]:[\\/]') {
            $absPattern = $Pattern
        } else {
            $absPattern = Join-Path $Base $Pattern
        }

        try {
            $matched = Resolve-Path -Path $absPattern -ErrorAction SilentlyContinue
            if ($matched) {
                foreach ($m in $matched) {
                    if (Test-Path -Path $m.Path -PathType Container) {
                        $abs = Resolve-AbsPath -Path $m.Path -Base $Base
                        Register-Repo -Abs $abs -Source $Source
                    }
                }
            }
        } catch { }
    } else {
        $abs = Resolve-AbsPath -Path $Pattern -Base $Base
        Register-Repo -Abs $abs -Source $Source
    }
}

# ============================================================================
# Source 1: explicit list (.claude/config.json sharedRepos)
# ============================================================================
function Discover-Explicit {
    if (-not (Test-Path ".claude/config.json")) { return }
    $cfg = Read-JsonFile -Path ".claude/config.json"
    if ($null -eq $cfg) { return }
    if (-not ($cfg.PSObject.Properties.Name -contains "sharedRepos")) { return }
    $sr = $cfg.sharedRepos
    if ($null -eq $sr) { return }
    # sharedRepos may be a single value (object/string) -- coerce to enumerable.
    foreach ($entry in @($sr)) {
        $p = $null
        if ($entry -is [string]) {
            $p = $entry
        } elseif ($entry -is [pscustomobject] -or $entry -is [hashtable]) {
            try {
                if ($entry.PSObject.Properties.Name -contains "path") {
                    $p = [string]$entry.path
                }
            } catch {
                # hashtable
                if ($entry.ContainsKey("path")) { $p = [string]$entry["path"] }
            }
        }
        if (-not [string]::IsNullOrEmpty($p)) {
            # Paths in .claude/config.json resolve against the project root, NOT the
            # .claude/ directory.  Documented in README.md "Path resolution".
            Expand-AndRegister -Pattern $p -Source "explicit" -Base $projectRoot
        }
    }
}

# ============================================================================
# Source 2: pnpm-workspace.yaml
# ============================================================================
function Discover-Pnpm {
    if (-not (Test-Path "pnpm-workspace.yaml")) { return }
    $lines = Get-Content -Path "pnpm-workspace.yaml" -ErrorAction SilentlyContinue
    if ($null -eq $lines) { return }

    $inPackages = $false
    foreach ($raw in $lines) {
        # Strip CR
        $line = $raw -replace "`r$", ""
        # Skip comments / blank
        if ($line -match '^\s*$') { continue }
        if ($line -match '^\s*#') { continue }

        if (-not $inPackages) {
            if ($line -match '^\s*packages\s*:') { $inPackages = $true }
            continue
        }

        # in packages: block.  A non-indented non-list line at column 0 ends the block.
        if ($line -match '^[^\s\-]') {
            $inPackages = $false
            continue
        }

        $trimmed = $line.Trim()
        if (-not ($trimmed.StartsWith('-'))) { continue }
        $value = $trimmed.Substring(1).TrimStart()
        # Strip a trailing inline comment
        $value = ($value -replace '\s+#.*$', '').Trim()
        # Strip surrounding quotes
        if ($value.Length -ge 2) {
            if (($value.StartsWith('"') -and $value.EndsWith('"')) -or
                ($value.StartsWith("'") -and $value.EndsWith("'"))) {
                $value = $value.Substring(1, $value.Length - 2)
            }
        }
        if (-not [string]::IsNullOrEmpty($value)) {
            Expand-AndRegister -Pattern $value -Source "pnpm" -Base $projectRoot
        }
    }
}

# ============================================================================
# Source 3: Cargo.toml [workspace] members
# ============================================================================
function Discover-Cargo {
    if (-not (Test-Path "Cargo.toml")) { return }
    $lines = Get-Content -Path "Cargo.toml" -ErrorAction SilentlyContinue
    if ($null -eq $lines) { return }

    $inWorkspace = $false
    $inMembers   = $false
    $buf         = ""

    foreach ($raw in $lines) {
        $line = $raw -replace "`r$", ""
        # Strip line-level # comments (naive: doesn't handle # inside quotes, fine for paths)
        if ($line -match '#') { $line = $line -replace '\s*#.*$', '' }
        $trimmed = $line.Trim()

        if (-not $inWorkspace) {
            if ($trimmed -eq '[workspace]') { $inWorkspace = $true }
            continue
        }

        # New section header inside [workspace]?
        if ($trimmed -match '^\[.*\]\s*$') {
            if ($trimmed -like '[workspace.*]') {
                # Sub-section; stay in [workspace]
            } else {
                $inWorkspace = $false
                $inMembers = $false
                continue
            }
        }

        if (-not $inMembers) {
            if ($trimmed -match '^members\s*=\s*\[') {
                $inMembers = $true
                $rest = $trimmed -replace '^members\s*=\s*\[', ''
                if ($rest -match '\]') {
                    $buf = ($rest -replace '\].*$', '')
                    $inMembers = $false
                } else {
                    $buf = $rest
                }
            }
        } else {
            if ($line -match '\]') {
                $buf = $buf + " " + ($line -replace '\].*$', '')
                $inMembers = $false
            } else {
                $buf = $buf + " " + $line
            }
        }
    }

    if ([string]::IsNullOrEmpty($buf)) { return }

    # Split on commas
    $pieces = $buf -split ','
    foreach ($piece in $pieces) {
        $p = $piece.Trim()
        if ($p.Length -ge 2) {
            if (($p.StartsWith('"') -and $p.EndsWith('"')) -or
                ($p.StartsWith("'") -and $p.EndsWith("'"))) {
                $p = $p.Substring(1, $p.Length - 2)
            }
        }
        if (-not [string]::IsNullOrEmpty($p)) {
            Expand-AndRegister -Pattern $p -Source "cargo" -Base $projectRoot
        }
    }
}

# ============================================================================
# Source 4: package.json workspaces
# ============================================================================
function Discover-Npm {
    if (-not (Test-Path "package.json")) { return }
    $pkg = Read-JsonFile -Path "package.json"
    if ($null -eq $pkg) { return }
    if (-not ($pkg.PSObject.Properties.Name -contains "workspaces")) { return }
    $ws = $pkg.workspaces
    if ($null -eq $ws) { return }

    $items = @()
    if ($ws -is [System.Collections.IEnumerable] -and -not ($ws -is [string])) {
        $items = @($ws)
    } elseif ($ws -is [pscustomobject] -and ($ws.PSObject.Properties.Name -contains "packages")) {
        if ($null -ne $ws.packages) { $items = @($ws.packages) }
    }

    foreach ($item in $items) {
        if ($item -is [string] -and -not [string]::IsNullOrEmpty($item)) {
            Expand-AndRegister -Pattern $item -Source "npm" -Base $projectRoot
        }
    }
}

# ---- Run all four sources in priority order ----
Discover-Explicit
Discover-Pnpm
Discover-Cargo
Discover-Npm

# ---- Empty? ----
if ($script:RegisteredPaths.Count -eq 0) {
    Emit-Empty
}

# ---- Build entries ----
$entries = New-Object System.Collections.ArrayList
$contributingSources = New-Object System.Collections.ArrayList

for ($i = 0; $i -lt $script:RegisteredPaths.Count; $i++) {
    $abs = $script:RegisteredPaths[$i]
    $src = $script:RegisteredSources[$i]

    # Track sources that contributed surviving entries (post-dedup).
    if (-not $contributingSources.Contains($src)) {
        [void]$contributingSources.Add($src)
    }

    $hasClaude = (Test-Path (Join-Path $abs ".claude"))
    $hasClaudeJson = if ($hasClaude) { 'true' } else { 'false' }

    # JSON-escape path: backslashes already normalized to /, only " and control chars left.
    $escPath = $abs -replace '\\', '\\' -replace '"', '\"' -replace "`r", '' -replace "`n", ' '

    $entry = '{"path":"' + $escPath + '","declarationSource":"' + $src + '","hasGit":true,"hasClaudeDir":' + $hasClaudeJson + '}'
    [void]$entries.Add($entry)
}

# ---- Compute discoveryMethod (only sources that contributed surviving entries, in priority order) ----
$ordered = New-Object System.Collections.ArrayList
foreach ($src in @("explicit", "pnpm", "cargo", "npm")) {
    if ($contributingSources.Contains($src)) {
        [void]$ordered.Add($src)
    }
}
$discoveryMethod = if ($ordered.Count -gt 0) { ($ordered -join ',') } else { 'none' }

if ($entries.Count -eq 0) {
    Emit-Empty
}

$count = $entries.Count
$reposJson = ($entries -join ',')
$computeMs = Get-ComputeMs

Write-Output ('{"count":' + $count + ',"repos":[' + $reposJson + '],"discoveryMethod":"' + $discoveryMethod + '","_meta":{"oracleVersion":"' + $ORACLE_VERSION + '","computeMs":' + $computeMs + ',"schemaVersion":' + $SCHEMA_VERSION + '}}')
