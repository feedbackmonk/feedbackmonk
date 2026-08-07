# handoff-retention oracle (Windows PowerShell)
# Answers: which handoff briefs are older than the configured TTL, and which are KEEP-pinned?
#
# Operates on .claude/handoff/handoff-*.md files.
# Default threshold: 30 days (DEC-54). Sibling <file>.KEEP exempts indefinitely (HANDOFF-01).
#
# Modes:
#   (default)    : full inventory JSON with `briefing` field
#   --gc-cheap   : session-start AUTO-SWEEP (DEC-79) + sweep-report JSON with
#                  `briefing` field (empty when nothing swept -> silent line);
#                  ~100ms budget, defers remainder if exceeded
#   --gc         : destructive sweep + JSONL audit; emits summary JSON
#
# DEC-79 (2026-06-12): --gc-cheap performs the sweep instead of nagging. See
# run.sh header for the full rationale (nag never acted on; SWEEP-08 +
# KEEP-pin rails proven by archive-retention).
#
# Spec: SPECIFICATION.md § SWEEP-01 (as amended by DEC-79), SWEEP-07, SWEEP-08; DEC-52, DEC-54, DEC-79
# Substrate: claude-template/oracles/archive-retention/ (RETENTION-01..06)

param(
    [switch]$gc,
    [switch]$gcCheap
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# Sync .NET CWD with PowerShell's current location (CSI-01 smoke lesson).
try { [Environment]::CurrentDirectory = (Get-Location).Path } catch { }

# Accept --gc / --gc-cheap as positional argv tokens.
$mode = "briefing"
if ($gc) { $mode = "gc" }
if ($gcCheap) { $mode = "gc-cheap" }
foreach ($a in $args) {
    switch ($a) {
        "--gc"        { $mode = "gc" }
        "--gc-cheap"  { $mode = "gc-cheap" }
        "--dry-run"   { $mode = "dry-run" }
        default {
            if ($a -is [string] -and $a.StartsWith("--")) {
                Write-Error "handoff-retention: unknown mode: $a"
                exit 1
            }
        }
    }
}

$handoffDir  = ".claude/handoff"
$summaryFile = Join-Path $handoffDir "_summary.jsonl"
$emptyOutput = '{"swept":[],"retained_keep_pinned":[],"retained_under_ttl":[],"threshold_days":30,"threshold_source":"default","briefing":""}'

function Emit-Empty-Briefing {
    Write-Output $emptyOutput
    exit 0
}

# ---- Locate handoff dir ----
if (-not (Test-Path $handoffDir)) {
    if ($mode -eq "briefing") { Emit-Empty-Briefing }
    if ($mode -eq "gc" -or $mode -eq "dry-run") {
        Write-Output '{"swept":0,"before":0,"after":0,"threshold":"P30D","thresholdSource":"default","summarized":0,"note":"no handoff dir"}'
    }
    if ($mode -eq "gc-cheap") {
        # Nothing to sweep -> empty briefing (silent line)
        Write-Output '{"swept":0,"before":0,"after":0,"threshold":"P30D","thresholdSource":"default","summarized":0,"briefing":""}'
    }
    exit 0
}

# ---- Threshold resolution ----
$thresholdDays    = 30
$thresholdSource  = "default"
$thresholdDisplay = "P30D"

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
    if ($null -ne $cfg -and $cfg.PSObject.Properties.Name -contains "handoffRetention" -and $null -ne $cfg.handoffRetention) {
        if ($cfg.handoffRetention.PSObject.Properties.Name -contains "threshold") {
            $raw = $cfg.handoffRetention.threshold
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

# JSON-escape a string. Returns $null if value is null/empty.
function ConvertTo-JsonString {
    param([string]$Value)
    if ($null -eq $Value -or $Value -eq "") { return $null }
    $s = $Value
    $s = $s -replace '\\', '\\'
    $s = $s -replace '"', '\"'
    $s = $s -replace "`r", ''
    $s = $s -replace "`n", ' '
    $s = $s -replace "`t", ' '
    return $s
}

# Read first non-empty line of a file (cap at 200 chars after trim).
function Get-BriefFirstLine {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return $null }
    try {
        $line = Get-Content -Path $Path -ErrorAction SilentlyContinue | Where-Object { $_.Trim().Length -gt 0 } | Select-Object -First 1
        if ($null -eq $line) { return $null }
        $clean = $line.Trim()
        if ($clean.Length -gt 200) { $clean = $clean.Substring(0, 200) }
        return $clean
    } catch {
        return $null
    }
}

# Append a JSON line atomically; verify by re-reading last line.
function Append-Summary-Line {
    param([string]$Line)
    try {
        if (-not (Test-Path $handoffDir)) {
            New-Item -ItemType Directory -Path $handoffDir -Force -ErrorAction Stop | Out-Null
        }
        $utf8NoBom = New-Object System.Text.UTF8Encoding $false
        $parentResolved = (Resolve-Path -Path (Split-Path -Parent $summaryFile) -ErrorAction Stop).Path
        $absSummary = Join-Path $parentResolved (Split-Path -Leaf $summaryFile)
        [System.IO.File]::AppendAllText($absSummary, ($Line + "`n"), $utf8NoBom)
        # Verify
        $allLines = [System.IO.File]::ReadAllLines($absSummary)
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
    param([string]$FilePath, [string]$SweptAt, [int]$AgeDays)
    $firstLine = Get-BriefFirstLine -Path $FilePath
    $jFirst = ConvertTo-JsonString -Value $firstLine
    $jPath  = ConvertTo-JsonString -Value $FilePath
    $firstJson = if ($null -ne $jFirst) { '"' + $jFirst + '"' } else { 'null' }
    return ('{"file":"' + $jPath + '","swept_at":"' + $SweptAt + '","age_days":' + $AgeDays + ',"brief_first_line":' + $firstJson + '}')
}

# =============================================================================
# Mode dispatch
# =============================================================================
# --gc-cheap (DEC-79): falls through to the sweep block below alongside --gc,
# with a ~100ms time budget and a sweep-report briefing field.

# Collect handoff files matching the pattern (sorted alphabetically).
$allFiles = @()
try {
    $allFiles = @(Get-ChildItem -Path $handoffDir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^handoff-.+\.md$' } |
        Sort-Object -Property Name)
} catch { $allFiles = @() }

$beforeFiles = $allFiles.Count

if ($mode -eq "briefing" -and $beforeFiles -eq 0) {
    Emit-Empty-Briefing
}

# Classify
$sweptArr      = New-Object System.Collections.ArrayList
$keepPinnedArr = New-Object System.Collections.ArrayList
$underTtlArr   = New-Object System.Collections.ArrayList
$staleCount    = 0
$pinnedCount   = 0
$underCount    = 0

foreach ($f in $allFiles) {
    $relPath = ".claude/handoff/" + $f.Name
    $jPath = ConvertTo-JsonString -Value $relPath

    # KEEP-pin sibling
    if (Test-Path ($f.FullName + ".KEEP")) {
        [void]$keepPinnedArr.Add('"' + $jPath + '"')
        $pinnedCount++
        continue
    }

    $mtime = $f.LastWriteTimeUtc
    $ageDays = [int][Math]::Floor(($now - $mtime).TotalDays)

    if ($mtime -gt $cutoff) {
        $entry = '{"file":"' + $jPath + '","age_days":' + $ageDays + '}'
        [void]$underTtlArr.Add($entry)
        $underCount++
    } else {
        $firstLine = Get-BriefFirstLine -Path $f.FullName
        $jFirst = ConvertTo-JsonString -Value $firstLine
        $firstJson = if ($null -ne $jFirst) { '"' + $jFirst + '"' } else { 'null' }
        $entry = '{"file":"' + $jPath + '","swept_at":null,"age_days":' + $ageDays + ',"brief_first_line":' + $firstJson + '}'
        [void]$sweptArr.Add($entry)
        $staleCount++
    }
}

# =============================================================================
# --gc / --gc-cheap: actually sweep stale candidates (DEC-79: cheap mode
#            sweeps too, under a ~100ms budget, and reports via `briefing`)
# --dry-run: identical candidate computation, ZERO mutation (SWEEPER-05) --
#            no JSONL append, no Remove-Item; reports the would-sweep set.
# =============================================================================
if ($mode -eq "gc" -or $mode -eq "gc-cheap" -or $mode -eq "dry-run") {
    $sweepCount = 0
    $summarized = 0
    $sweptFiles = New-Object System.Collections.ArrayList
    $budgetExceeded = $false

    # Budget mirrors run.sh (1000ms — calibrated for MSYS bash subprocess
    # cost; PS per-file ops are much cheaper so this rarely fires here).
    $budgetMs = 0
    if ($mode -eq "gc-cheap") { $budgetMs = 1000 }
    # ULDF_HR_GC_BUDGET_MS: test seam (DEC-250), parity with the sh twin.
    if ($mode -eq "gc-cheap" -and $env:ULDF_HR_GC_BUDGET_MS -match '^[0-9]+$') { $budgetMs = [int]$env:ULDF_HR_GC_BUDGET_MS }
    $sweepWatch = [System.Diagnostics.Stopwatch]::StartNew()
    $workedAny = $false   # minimum-progress guarantee (DEFER-064 rule 3, DEC-250)

    foreach ($f in $allFiles) {
        # --gc-cheap budget: defer the remainder to the next session-start
        # rather than stalling the briefing (mirrors archive-retention) -- but
        # NEVER before one unit of real work, so a prefix of KEEP-pinned or
        # too-young files cannot starve the sweep (DEC-250).
        if ($budgetMs -gt 0 -and $workedAny -and $sweepWatch.ElapsedMilliseconds -gt $budgetMs) {
            $budgetExceeded = $true
            break
        }

        # KEEP-pin
        if (Test-Path ($f.FullName + ".KEEP")) { continue }

        $mtime = $f.LastWriteTimeUtc
        if ($mtime -gt $cutoff) { continue }

        $ageDays = [int][Math]::Floor(($now - $mtime).TotalDays)
        $relPath = ".claude/handoff/" + $f.Name

        # Past every cheap filter: a real sweep candidate, so the budget may bind.
        $workedAny = $true

        if ($mode -eq "dry-run") {
            $sweepCount++
            [void]$sweptFiles.Add($f.Name)
            continue
        }

        # Build summary line BEFORE delete (SWEEP-08 invariant)
        $line = Build-SummaryLine -FilePath $relPath -SweptAt $nowIso -AgeDays $ageDays
        if (-not (Append-Summary-Line -Line $line)) {
            Write-Warning "handoff-retention: summary write failed for $relPath; preserved"
            continue
        }
        $summarized++

        try {
            Remove-Item -Path $f.FullName -Force -ErrorAction Stop
            $sweepCount++
            [void]$sweptFiles.Add($f.Name)
        } catch {
            Write-Warning "handoff-retention: Remove-Item failed for $relPath; summary line was already appended"
        }
    }

    if ($mode -eq "dry-run") {
        $parts = @(
            "`"dryRun`":true",
            "`"wouldSweep`":$sweepCount",
            "`"before`":$beforeFiles",
            "`"after`":$beforeFiles",
            "`"threshold`":`"$thresholdDisplay`"",
            "`"thresholdSource`":`"$thresholdSource`""
        )
        if ($sweptFiles.Count -gt 0) {
            $csv = ($sweptFiles -join ",")
            $jCsv = ConvertTo-JsonString -Value $csv
            $parts += "`"wouldSweepFiles`":`"$jCsv`""
        }
        Write-Output ("{" + ($parts -join ",") + "}")
        exit 0
    }

    $afterFiles = $beforeFiles - $sweepCount

    $parts = @(
        "`"swept`":$sweepCount",
        "`"before`":$beforeFiles",
        "`"after`":$afterFiles",
        "`"threshold`":`"$thresholdDisplay`"",
        "`"thresholdSource`":`"$thresholdSource`"",
        "`"summarized`":$summarized"
    )
    if ($budgetExceeded) {
        $parts += "`"budgetExceeded`":true"
    }
    if ($sweptFiles.Count -gt 0) {
        $csv = ($sweptFiles -join ",")
        $jCsv = ConvertTo-JsonString -Value $csv
        $parts += "`"sweptFiles`":`"$jCsv`""
    }
    if ($mode -eq "gc-cheap") {
        # DEC-79 sweep-report briefing: silent (empty) when nothing swept.
        $briefingText = ""
        if ($sweepCount -gt 0) {
            $noun = if ($sweepCount -eq 1) { "brief" } else { "briefs" }
            $briefingText = "swept $sweepCount expired handoff $noun (>${thresholdDays}d, KEEP-pinned exempt) -- audit: $summaryFile"
            if ($budgetExceeded) {
                $briefingText += "; budget hit, remainder next session"
            }
        }
        $jBrief = ConvertTo-JsonString -Value $briefingText
        if ($null -eq $jBrief) { $jBrief = "" }
        $parts += "`"briefing`":`"$jBrief`""
    }
    Write-Output ("{" + ($parts -join ",") + "}")
    exit 0
}

# =============================================================================
# Default mode: emit full inventory JSON with briefing field
# =============================================================================

$briefing = ""
if ($staleCount -gt 0) {
    if ($staleCount -eq 1) {
        $briefing = "$staleCount brief older than ${thresholdDays}d, run /0-uldf-oracle handoff-retention --gc to sweep"
    } else {
        $briefing = "$staleCount briefs older than ${thresholdDays}d, run /0-uldf-oracle handoff-retention --gc to sweep"
    }
}
$jBriefing = ConvertTo-JsonString -Value $briefing
if ($null -eq $jBriefing) { $jBriefing = "" }

$sweptJson    = ($sweptArr      -join ",")
$keepJson     = ($keepPinnedArr -join ",")
$underJson    = ($underTtlArr   -join ",")

Write-Output ('{"swept":[' + $sweptJson + '],"retained_keep_pinned":[' + $keepJson + '],"retained_under_ttl":[' + $underJson + '],"threshold_days":' + $thresholdDays + ',"threshold_source":"' + $thresholdSource + '","briefing":"' + $jBriefing + '"}')
