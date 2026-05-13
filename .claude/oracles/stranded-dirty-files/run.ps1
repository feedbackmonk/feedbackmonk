# stranded-dirty-files oracle (Windows)
#
# CSI-15 (Phase 1.7): emit a [stranded-dirty-files] briefing line when this
# project's working tree contains dirty files older than the most recent
# finalize commit AND no live peer (per dispatchable-sessions registry) owns
# them.
#
# Visibility-only — never mutates state. Cleanup is user-driven via
# /0-uldf-finalize --include-stranded (FINALIZE-04, same Arc 2).
#
# Output: single-line JSON matching the FROZEN output schema (oracle.json).
# Briefing field is empty string when count == 0 so the session-start hook
# gracefully suppresses the line (parallel to stale-ltads-state's silence pattern).

$ErrorActionPreference = 'SilentlyContinue'

$registry         = ".claude/collaboration/active-sessions.json"
$scopeGuardMax    = 2000
$sampleCap        = 10
$largeThreshold   = 50

function Emit-Json($obj) {
    # Compact one-line JSON, dictionaries preserve insertion order via [ordered]
    $json = $obj | ConvertTo-Json -Compress -Depth 6
    Write-Output $json
    exit 0
}

function Emit-Empty([string]$lastFinalize, [int]$livePeerCount) {
    $obj = [ordered]@{
        has_stranded     = $false
        count            = 0
        oldest_mtime     = $null
        sample           = @()
        live_peer_count  = $livePeerCount
        last_finalize_at = $lastFinalize
        briefing         = ""
    }
    Emit-Json $obj
}

function Iso-Utc([datetime]$dt) {
    $u = $dt.ToUniversalTime()
    return $u.ToString("yyyy-MM-ddTHH:mm:ssZ")
}

# ---- Graceful absence: not in a git repo -----------------------------------
& git rev-parse --git-dir 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) {
    Emit-Empty $null 0
}

# ---- last_finalize_at = HEAD's commit timestamp ----------------------------
$lastFinalizeIsoRaw = & git log -1 --format=%aI HEAD 2>$null
$lastFinalizeIso    = $null
$lastFinalizeUtc    = $null
if ($null -ne $lastFinalizeIsoRaw) {
    $lastFinalizeIsoRaw = ($lastFinalizeIsoRaw | Out-String).Trim()
}
if (-not [string]::IsNullOrEmpty($lastFinalizeIsoRaw)) {
    try {
        $dto = [DateTimeOffset]::Parse($lastFinalizeIsoRaw)
        $lastFinalizeUtc = $dto.UtcDateTime
        # Re-emit normalized to UTC Z form for the briefing schema.
        $lastFinalizeIso = Iso-Utc $lastFinalizeUtc
    } catch {
        $lastFinalizeIso = $lastFinalizeIsoRaw  # Best-effort echo of original
    }
}

# ---- Dirty file set --------------------------------------------------------
$dirtyRaw = & git status --porcelain 2>$null
$dirtyFiles = New-Object System.Collections.Generic.List[string]
if ($null -ne $dirtyRaw) {
    foreach ($line in @($dirtyRaw)) {
        if ([string]::IsNullOrEmpty($line)) { continue }
        if ($line.Length -lt 3) { continue }
        $xy = $line.Substring(0, 2)
        $rest = $line.Substring(3)
        # Working-tree deletion -> no mtime; skip
        if ($xy -match '^[ AMRC]D$' -or $xy -eq 'D ' -or $xy -eq ' D' -or $xy -eq 'DD') { continue }
        # Rename: "old -> new" - keep new
        if ($xy -match '^R' -or $xy -match '^.R$') {
            if ($rest -match ' -> ') {
                $rest = ($rest -split ' -> ')[-1]
            }
        }
        $dirtyFiles.Add($rest) | Out-Null
    }
}

$dirtyCount = $dirtyFiles.Count

# ---- Scope guard: too many dirty files -> detection skipped ----------------
if ($dirtyCount -gt $scopeGuardMax) {
    $briefing = "stranded-dirty-files: detection skipped - too many dirty files (>${scopeGuardMax}); run /0-uldf-finalize --include-stranded for full audit"
    $obj = [ordered]@{
        has_stranded     = $false
        count            = -1
        oldest_mtime     = $null
        sample           = @()
        live_peer_count  = 0
        last_finalize_at = $lastFinalizeIso
        briefing         = $briefing
    }
    Emit-Json $obj
}

# ---- Empty dirty set -> graceful empty -------------------------------------
if ($dirtyCount -eq 0) {
    Emit-Empty $lastFinalizeIso 0
}

# ---- Build live-peer ownership set -----------------------------------------
$projRoot = (Get-Location).Path
$projRootNorm = ($projRoot -replace '\\', '/').TrimEnd('/')

$livePeerCount = 0
$ownedSet = New-Object System.Collections.Generic.HashSet[string]

if (Test-Path $registry) {
    $reg = $null
    try {
        $bytes = [System.IO.File]::ReadAllBytes($registry)
        if ($bytes.Length -gt 0) {
            $rawText = $null
            if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
                $rawText = [System.Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
            } else {
                $rawText = [System.Text.Encoding]::UTF8.GetString($bytes)
            }
            if (-not [string]::IsNullOrWhiteSpace($rawText)) {
                $reg = $rawText | ConvertFrom-Json -ErrorAction Stop
            }
        }
    } catch {
        $reg = $null
    }

    if ($null -ne $reg -and $reg.PSObject.Properties.Name -contains 'sessions' -and $reg.sessions) {
        foreach ($s in @($reg.sessions)) {
            if ($null -eq $s) { continue }
            if ($s.status -ne 'active') { continue }
            $sPid = $null
            if ($s.PSObject.Properties.Name -contains 'claudeShellPid' -and $null -ne $s.claudeShellPid) {
                try { $sPid = [int]$s.claudeShellPid } catch { $sPid = $null }
            }
            if ($null -eq $sPid -or $sPid -le 0) { continue }
            $sWd = ""
            if ($s.PSObject.Properties.Name -contains 'workDir' -and $null -ne $s.workDir) {
                $sWd = ([string]$s.workDir -replace '\\', '/').TrimEnd('/')
            }
            if ($sWd -ne $projRootNorm) { continue }
            # Liveness probe
            $alive = $false
            try {
                $proc = Get-Process -Id $sPid -ErrorAction SilentlyContinue
                if ($null -ne $proc) { $alive = $true }
            } catch { $alive = $false }
            if (-not $alive) { continue }

            $livePeerCount++
            if ($s.PSObject.Properties.Name -contains 'dirtyFiles' -and $null -ne $s.dirtyFiles) {
                foreach ($df in @($s.dirtyFiles)) {
                    if ($null -eq $df) { continue }
                    [void]$ownedSet.Add([string]$df)
                }
            }
        }
    }
}

# ---- Walk dirty files, classify, build sample ------------------------------
$nowUtc = [DateTime]::UtcNow
$strandCount = 0
$oldestUtc = $null
$sample = New-Object System.Collections.Generic.List[object]

foreach ($f in $dirtyFiles) {
    if ([string]::IsNullOrEmpty($f)) { continue }
    if (-not (Test-Path -LiteralPath $f)) { continue }
    if ($ownedSet.Contains($f)) { continue }
    if ($null -eq $lastFinalizeUtc) { continue }   # No finalize boundary -> no strands

    try {
        $info = Get-Item -LiteralPath $f -Force -ErrorAction Stop
    } catch { continue }
    if ($info -is [System.IO.DirectoryInfo]) { continue }

    $mtimeUtc = $info.LastWriteTimeUtc
    if ($mtimeUtc -ge $lastFinalizeUtc) { continue }

    $strandCount++
    if ($null -eq $oldestUtc -or $mtimeUtc -lt $oldestUtc) { $oldestUtc = $mtimeUtc }

    if ($strandCount -le $sampleCap) {
        $ageDays = [int][Math]::Floor(($nowUtc - $mtimeUtc).TotalDays)
        $pNorm = ($f -replace '\\', '/')
        $sample.Add([ordered]@{
            path     = $pNorm
            mtime    = Iso-Utc $mtimeUtc
            age_days = $ageDays
        }) | Out-Null
    }
}

# ---- Emit results ----------------------------------------------------------
if ($strandCount -eq 0) {
    Emit-Empty $lastFinalizeIso $livePeerCount
}

$oldestIso  = Iso-Utc $oldestUtc
$oldestAge  = [int][Math]::Floor(($nowUtc - $oldestUtc).TotalDays)

if ($strandCount -lt $largeThreshold) {
    $briefing = "stranded-dirty-files: $strandCount files (oldest $oldestAge days; no live owner) - run /0-uldf-finalize --include-stranded for cleanup"
} else {
    $briefing = "stranded-dirty-files: $strandCount files (oldest $oldestAge days) - significant accumulation; see /0-uldf-oracle stranded-dirty-files for full sample"
}

$obj = [ordered]@{
    has_stranded     = $true
    count            = $strandCount
    oldest_mtime     = $oldestIso
    sample           = $sample
    live_peer_count  = $livePeerCount
    last_finalize_at = $lastFinalizeIso
    briefing         = $briefing
}
Emit-Json $obj
