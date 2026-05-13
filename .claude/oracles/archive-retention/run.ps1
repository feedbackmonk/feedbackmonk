# archive-retention oracle (Windows PowerShell)
# Answers: which archived PODS sessions exist, and which are old enough to sweep?
#
# Operates on .claude/collaboration/archived/collab-*/ directories.
# Default threshold: 90 days. KEEP file at <dir>/KEEP exempts the dir from sweep.
#
# Modes:
#   (default)    : list all collab-* dirs with metadata + sweepability flags
#   --gc-cheap   : session-start hygiene sweep, ~100ms budget, defers if exceeded
#   --gc         : on-demand hygiene sweep, no time budget, prints {swept,before,after,...}
#
# Sweep criteria (--gc / --gc-cheap):
#   basename matches /^collab-\d{8}-\d{6}$/ AND no KEEP file present
#   AND createdAt (parsed from basename) is older than (now - threshold).
#   Action: append JSON line to _summary.jsonl, verify write, Remove-Item -Recurse the dir.
#   Threshold: .claude/config.json archiveRetention.threshold (numeric days or PnD), default 90.
#   Design lineage: CSI-05 (claude-template/oracles/dispatchable-sessions/run.ps1).

param(
    [switch]$gc,
    [switch]$gcCheap
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# Sync .NET CWD with PowerShell's current location -- defends against the
# WriteAllText/ReadAllBytes class of CWD bug surfaced during CSI-01 smoke.
try { [Environment]::CurrentDirectory = (Get-Location).Path } catch { }

# Accept --gc / --gc-cheap as positional argv tokens (PowerShell's param block
# turns -gc into a switch, but bash-style "--gc" arrives in $args).
$mode = "briefing"
if ($gc) { $mode = "gc" }
if ($gcCheap) { $mode = "gc-cheap" }
foreach ($a in $args) {
    switch ($a) {
        "--gc"        { $mode = "gc" }
        "--gc-cheap"  { $mode = "gc-cheap" }
        default {
            if ($a -is [string] -and $a.StartsWith("--")) {
                Write-Error "archive-retention: unknown mode: $a"
                exit 1
            }
        }
    }
}

$archivedDir = ".claude/collaboration/archived"
$summaryFile = Join-Path $archivedDir "_summary.jsonl"
$emptyOutput = '{"count":0,"dirs":[],"threshold":"P90D","thresholdSource":"default","summary":"No archived PODS sessions."}'

function Emit-Empty-Briefing {
    Write-Output $emptyOutput
    exit 0
}

# ---- Locate the archived dir ----
if (-not (Test-Path $archivedDir)) {
    if ($mode -eq "briefing") { Emit-Empty-Briefing }
    if ($mode -eq "gc") {
        Write-Output '{"swept":0,"before":0,"after":0,"threshold":"P90D","thresholdSource":"default","summarized":0,"note":"no archived dir"}'
    }
    exit 0
}

# ---- Threshold resolution ----
$thresholdDays    = 90
$thresholdSource  = "default"
$thresholdDisplay = "P90D"

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

if (Test-Path ".claude/config.json") {
    $cfg = Read-JsonFile -Path ".claude/config.json"
    if ($null -ne $cfg -and $cfg.PSObject.Properties.Name -contains "archiveRetention" -and $null -ne $cfg.archiveRetention) {
        if ($cfg.archiveRetention.PSObject.Properties.Name -contains "threshold") {
            $raw = $cfg.archiveRetention.threshold
            if ($null -ne $raw) {
                if ($raw -is [int] -or $raw -is [long] -or $raw -is [double]) {
                    $thresholdDays = [int]$raw
                    $thresholdSource = "config"
                    $thresholdDisplay = "P${thresholdDays}D"
                } elseif ($raw -is [string]) {
                    $rs = $raw.Trim()
                    if ($rs -match '^[0-9]+$') {
                        $thresholdDays = [int]$rs
                        $thresholdSource = "config"
                        $thresholdDisplay = "P${thresholdDays}D"
                    } elseif ($rs -match '^P([0-9]+)D$') {
                        $thresholdDays = [int]$Matches[1]
                        $thresholdSource = "config"
                        $thresholdDisplay = $rs
                    }
                }
            }
        }
    }
}

$now    = (Get-Date).ToUniversalTime()
$cutoff = $now.AddDays(-1 * $thresholdDays)
$nowIso = $now.ToString('yyyy-MM-ddTHH:mm:ssZ')

# ---- Helpers ----

# Parse "collab-YYYYMMDD-HHMMSS" -> [DateTime] UTC, or $null.
function Parse-CollabBasename {
    param([string]$Base)
    if ($Base -notmatch '^collab-([0-9]{4})([0-9]{2})([0-9]{2})-([0-9]{2})([0-9]{2})([0-9]{2})$') {
        return $null
    }
    try {
        $dt = [DateTime]::new(
            [int]$Matches[1], [int]$Matches[2], [int]$Matches[3],
            [int]$Matches[4], [int]$Matches[5], [int]$Matches[6],
            [DateTimeKind]::Utc
        )
        return $dt
    } catch {
        return $null
    }
}

# Recursive byte count.
function Get-DirSize {
    param([string]$Path)
    try {
        $sum = 0L
        Get-ChildItem -Path $Path -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
            $sum += $_.Length
        }
        return [int64]$sum
    } catch {
        return 0L
    }
}

function Count-Entries {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return 0 }
    try {
        return @(Get-ChildItem -Path $Path -Force -ErrorAction SilentlyContinue).Count
    } catch {
        return 0
    }
}

function Get-GuideHeadline {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return $null }
    try {
        $line = Get-Content -Path $Path -ErrorAction SilentlyContinue | Where-Object { $_ -match '^#' -and $_.Trim().Length -gt 1 } | Select-Object -First 1
        if ($null -eq $line) { return $null }
        # Strip leading hashes and spaces
        $clean = $line -replace '^#+\s*', ''
        if ($clean.Length -gt 200) { $clean = $clean.Substring(0, 200) }
        return $clean
    } catch {
        return $null
    }
}

function Get-CriticVerdict {
    param([string]$DirPath)
    $f = Join-Path $DirPath "CRITIC_VERDICT.md"
    if (-not (Test-Path $f)) { return $null }
    try {
        $content = Get-Content -Path $f -Raw -ErrorAction SilentlyContinue
        if ($content -match '\b(VETO|CONCERN|PASS)\b') {
            return $Matches[1]
        }
        return $null
    } catch {
        return $null
    }
}

# JSON-escape a string. Returns "null" string if input is null/empty (caller decides).
function ConvertTo-JsonString {
    param([string]$Input)
    if ($null -eq $Input -or $Input -eq "") { return $null }
    $s = $Input
    $s = $s -replace '\\', '\\'
    $s = $s -replace '"', '\"'
    $s = $s -replace "`r", ''
    $s = $s -replace "`n", ' '
    return $s
}

# Append a JSON line atomically; verify by re-reading last line.
function Append-Summary-Line {
    param([string]$Line)
    try {
        if (-not (Test-Path $archivedDir)) {
            New-Item -ItemType Directory -Path $archivedDir -Force -ErrorAction Stop | Out-Null
        }
        # PowerShell's Add-Content default uses local encoding; force UTF-8 NoBOM.
        $utf8NoBom = New-Object System.Text.UTF8Encoding $false
        # File.AppendAllText is atomic enough for <PIPE_BUF; cross-platform.
        [System.IO.File]::AppendAllText(
            (Resolve-Path -Path (Split-Path -Parent $summaryFile) -ErrorAction Stop).Path + [System.IO.Path]::DirectorySeparatorChar + (Split-Path -Leaf $summaryFile),
            ($Line + "`n"),
            $utf8NoBom
        )
        # Verify: last non-empty line of file == $Line
        $allLines = [System.IO.File]::ReadAllLines($summaryFile)
        $lastNonEmpty = $null
        for ($i = $allLines.Length - 1; $i -ge 0; $i--) {
            if ($allLines[$i].Trim().Length -gt 0) { $lastNonEmpty = $allLines[$i]; break }
        }
        return ($lastNonEmpty -eq $Line)
    } catch {
        return $false
    }
}

function Build-SummaryLine {
    param([string]$DirPath, [string]$SweptAt)

    $base = Split-Path -Leaf $DirPath
    $createdAtDt = Parse-CollabBasename -Base $base
    if ($null -ne $createdAtDt) {
        $createdAt = $createdAtDt.ToString('yyyy-MM-ddTHH:mm:ssZ')
        $ageDays = [int][Math]::Floor(($now - $createdAtDt).TotalDays)
    } else {
        $createdAt = $null
        $ageDays = $null
    }

    $sizeBytes      = Get-DirSize -Path $DirPath
    $workerCount    = Count-Entries -Path (Join-Path $DirPath "workers")
    $taskCount      = Count-Entries -Path (Join-Path $DirPath "tasks")
    $criticVerdict  = Get-CriticVerdict -DirPath $DirPath
    $hasOverrideVeto = (Test-Path (Join-Path $DirPath "OVERRIDE_VETO.md"))
    $guideHeadline  = Get-GuideHeadline -Path (Join-Path $DirPath "GUIDE.md")

    $jHeadline = ConvertTo-JsonString -Input $guideHeadline
    $jVerdict  = ConvertTo-JsonString -Input $criticVerdict

    $createdAtJson  = if ($null -ne $createdAt)   { '"' + $createdAt + '"' }   else { 'null' }
    $ageDaysJson    = if ($null -ne $ageDays)     { [string]$ageDays }         else { 'null' }
    $verdictJson    = if ($null -ne $jVerdict)    { '"' + $jVerdict + '"' }    else { 'null' }
    $headlineJson   = if ($null -ne $jHeadline)   { '"' + $jHeadline + '"' }   else { 'null' }
    $hasOverrideJson = if ($hasOverrideVeto) { 'true' } else { 'false' }

    return ('{"sessionId":"' + $base + '","sweptAt":"' + $SweptAt + '","createdAt":' + $createdAtJson + ',"ageDays":' + $ageDaysJson + ',"sizeBytes":' + $sizeBytes + ',"workerCount":' + $workerCount + ',"taskCount":' + $taskCount + ',"criticVerdict":' + $verdictJson + ',"hasOverrideVeto":' + $hasOverrideJson + ',"guideHeadline":' + $headlineJson + '}')
}

# =============================================================================
# Mode dispatch
# =============================================================================
if ($mode -eq "gc" -or $mode -eq "gc-cheap") {

    $budgetMs = 100
    if ($mode -eq "gc") { $budgetMs = 0 }
    $startTicks = [System.Diagnostics.Stopwatch]::StartNew()

    $allDirs = @()
    try {
        $allDirs = @(Get-ChildItem -Path $archivedDir -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^collab-[0-9]{8}-[0-9]{6}$' })
    } catch { $allDirs = @() }

    $before = $allDirs.Count
    $sweepCount = 0
    $summarized = 0
    $sweepIds = New-Object System.Collections.ArrayList
    $budgetExceeded = $false

    foreach ($d in $allDirs) {
        if ($budgetMs -gt 0 -and $startTicks.ElapsedMilliseconds -gt $budgetMs) {
            $budgetExceeded = $true
            break
        }

        $base = $d.Name
        $dirPath = $d.FullName

        # KEEP file pin
        if (Test-Path (Join-Path $dirPath "KEEP")) { continue }

        $createdAtDt = Parse-CollabBasename -Base $base
        if ($null -eq $createdAtDt) { continue }    # unparsable -> never sweep
        if ($createdAtDt -gt $cutoff) { continue }  # too young

        $line = Build-SummaryLine -DirPath $dirPath -SweptAt $nowIso
        if (-not (Append-Summary-Line -Line $line)) {
            Write-Warning "archive-retention: summary write failed for $base; dir preserved"
            continue
        }
        $summarized++

        try {
            Remove-Item -Path $dirPath -Recurse -Force -ErrorAction Stop
            $sweepCount++
            [void]$sweepIds.Add($base)
        } catch {
            Write-Warning "archive-retention: Remove-Item failed for $base; summary line was already appended"
        }
    }

    $after = $before - $sweepCount

    if ($mode -eq "gc") {
        $idsCsv = ""
        if ($sweepIds.Count -gt 0) { $idsCsv = ($sweepIds -join ",") }
        $parts = @(
            "`"swept`":$sweepCount",
            "`"before`":$before",
            "`"after`":$after",
            "`"threshold`":`"$thresholdDisplay`"",
            "`"thresholdSource`":`"$thresholdSource`"",
            "`"summarized`":$summarized"
        )
        if ($budgetExceeded) { $parts += "`"budgetExceeded`":true" }
        if ($idsCsv) { $parts += "`"sweptIds`":`"$idsCsv`"" }
        Write-Output ("{" + ($parts -join ",") + "}")
    }
    exit 0
}

# =============================================================================
# Default mode: briefing path
# =============================================================================

$allDirs = @()
try {
    $allDirs = @(Get-ChildItem -Path $archivedDir -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^collab-[0-9]{8}-[0-9]{6}$' })
} catch { $allDirs = @() }

if ($allDirs.Count -eq 0) { Emit-Empty-Briefing }

$entries = New-Object System.Collections.ArrayList
$sweepableCount = 0
$keptCount = 0

foreach ($d in $allDirs) {
    $base = $d.Name
    $dirPath = $d.FullName
    $createdAtDt = Parse-CollabBasename -Base $base
    if ($null -ne $createdAtDt) {
        $createdAt = $createdAtDt.ToString('yyyy-MM-ddTHH:mm:ssZ')
        $ageDays = [int][Math]::Floor(($now - $createdAtDt).TotalDays)
    } else {
        $createdAt = $null
        $ageDays = $null
    }
    $sizeBytes = Get-DirSize -Path $dirPath

    $kept = (Test-Path (Join-Path $dirPath "KEEP"))
    if ($kept) {
        $sweepable = $false
        $reason = "kept"
        $keptCount++
    } elseif ($null -eq $createdAtDt) {
        $sweepable = $false
        $reason = "unparsable-age"
    } elseif ($createdAtDt -gt $cutoff) {
        $sweepable = $false
        $reason = "too-young"
    } else {
        $sweepable = $true
        $reason = "sweepable"
        $sweepableCount++
    }

    $createdAtJson = if ($null -ne $createdAt) { '"' + $createdAt + '"' } else { 'null' }
    $ageDaysJson   = if ($null -ne $ageDays)   { [string]$ageDays }       else { 'null' }
    $keptJson      = if ($kept) { 'true' } else { 'false' }
    $sweepableJson = if ($sweepable) { 'true' } else { 'false' }

    $entry = '{"sessionId":"' + $base + '","createdAt":' + $createdAtJson + ',"ageDays":' + $ageDaysJson + ',"sizeBytes":' + $sizeBytes + ',"kept":' + $keptJson + ',"sweepable":' + $sweepableJson + ',"reason":"' + $reason + '"}'
    [void]$entries.Add($entry)
}

$count = $entries.Count
$dirsJson = ($entries -join ",")
$summary = "$count archived session(s); $sweepableCount sweepable, $keptCount kept (threshold $thresholdDisplay)"

Write-Output ('{"count":' + $count + ',"dirs":[' + $dirsJson + '],"threshold":"' + $thresholdDisplay + '","thresholdSource":"' + $thresholdSource + '","summary":"' + $summary + '"}')
