# pid-orphan-detector oracle (Windows PowerShell)
# Answers: are there worker-shell-*.pid files referencing PIDs that are no longer alive?
#
# Liveness-based -- NO TTL per DEC-54. Three-leg defense per DEC-55.
#
# Modes:
#   (default)    : briefing path -- emit {swept[], alive[], malformed[], briefing} (read-only)
#   --gc-cheap   : session-start hygiene sweep, ~100ms budget, defers if exceeded
#   --gc         : on-demand full sweep, no time budget
#
# SWEEP-08 invariant: pre-delete JSONL append to ltads/execution/_pid-summary.jsonl.

param(
    [switch]$gc,
    [switch]$gcCheap
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

try { [Environment]::CurrentDirectory = (Get-Location).Path } catch { }

# Accept --gc / --gc-cheap as positional argv tokens.
$mode = "briefing"
if ($gc) { $mode = "gc" }
if ($gcCheap) { $mode = "gc-cheap" }
foreach ($a in $args) {
    switch ($a) {
        "--gc"        { $mode = "gc" }
        "--gc-cheap"  { $mode = "gc-cheap" }
        default {
            if ($a -is [string] -and $a.StartsWith("--")) {
                Write-Error "pid-orphan-detector: unknown mode: $a"
                exit 1
            }
        }
    }
}

$execDir     = "ltads/execution"
$summaryFile = Join-Path $execDir "_pid-summary.jsonl"
$emptyOutput = '{"swept":[],"alive":[],"malformed":[],"briefing":""}'

function Emit-Empty {
    Write-Output $emptyOutput
    exit 0
}

# ---- Source the shared liveness helper -----------------------------------------
$thisDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$libCandidates = @(
    (Join-Path $thisDir "..\..\scripts\lib\pid-liveness.ps1"),
    (Join-Path $thisDir "..\..\..\claude-template\scripts\lib\pid-liveness.ps1"),
    (Join-Path $env:USERPROFILE ".claude\scripts\lib\pid-liveness.ps1")
)
$libLoaded = $false
foreach ($cand in $libCandidates) {
    if ($cand -and (Test-Path $cand)) {
        try {
            . $cand
            $libLoaded = $true
            break
        } catch { }
    }
}
# Defensive inline fallback if helper unavailable.
if (-not (Get-Command -Name Test-UldfPidAlive -CommandType Function -ErrorAction SilentlyContinue)) {
    function Test-UldfPidAlive {
        param($PidValue)
        if ($null -eq $PidValue) { return $false }
        $p = 0
        if (-not [int]::TryParse([string]$PidValue, [ref]$p)) { return $false }
        if ($p -le 0) { return $false }
        try { return ($null -ne (Get-Process -Id $p -ErrorAction SilentlyContinue)) } catch { return $false }
    }
}

# ---- Locate the exec dir; absent => graceful nothing-to-do ---------------------
if (-not (Test-Path $execDir)) {
    if ($mode -eq "briefing" -or $mode -eq "gc") { Emit-Empty }
    # --gc-cheap silent on graceful absence
    exit 0
}

# ---- JSON string escape (path / brief field) -----------------------------------
function ConvertTo-JsonStringEscaped {
    param([string]$Text)
    if ($null -eq $Text) { return "" }
    $s = $Text
    $s = $s -replace '\\', '\\\\'
    $s = $s -replace '"',  '\"'
    $s = $s -replace "`r", ""
    $s = $s -replace "`n", " "
    return $s
}

function Get-IsoMtime {
    param([string]$Path)
    try {
        $ut = (Get-Item -LiteralPath $Path -ErrorAction Stop).LastWriteTimeUtc
        return $ut.ToString('yyyy-MM-ddTHH:mm:ssZ')
    } catch {
        return ""
    }
}

# Atomic-append + verify. Returns $true on success, $false on failure.
# Uses UTF-8 NO BOM ([System.Text.UTF8Encoding]::new($false)) so JSONL parsers
# (jq, python json) don't trip on the BOM PowerShell's Add-Content -Encoding UTF8
# emits as the first byte on file creation.
function Append-Summary {
    param([string]$Line)
    try {
        if (-not (Test-Path $execDir)) {
            New-Item -ItemType Directory -Path $execDir -Force | Out-Null
        }
        $encNoBom = [System.Text.UTF8Encoding]::new($false)
        $fullPath = (Resolve-Path -LiteralPath $execDir -ErrorAction Stop).Path
        $fullPath = Join-Path $fullPath "_pid-summary.jsonl"
        [System.IO.File]::AppendAllText($fullPath, $Line + "`n", $encNoBom)
        # Read-back verify last line via raw file read (preserves BOM-absence).
        $allText = [System.IO.File]::ReadAllText($fullPath, $encNoBom)
        $lines = $allText -split "`r?`n" | Where-Object { $_ -ne "" }
        $tail = $lines | Select-Object -Last 1
        return ($tail -eq $Line)
    } catch {
        return $false
    }
}

# Enumerate target .pid files.
function Get-PidFiles {
    $files = @()
    try {
        $files += Get-ChildItem -Path $execDir -Filter "worker-shell-*.pid" -File -ErrorAction SilentlyContinue
    } catch { }
    $legacy = Join-Path $execDir "worker-shell.pid"
    if (Test-Path $legacy) {
        try { $files += Get-Item -LiteralPath $legacy -ErrorAction SilentlyContinue } catch { }
    }
    return $files
}

$nowIso = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')

$swept     = @()  # array of pscustomobject {pid_file, referenced_pid, mtime}
$aliveList = @()  # array of pscustomobject {pid_file, referenced_pid}
$malformed = @()  # array of strings (paths)

$budgetMs = 0
# Cheap-mode budget: 500ms cap (parity with run.sh). Get-Process per-call cost
# is lower in PowerShell native than in Git Bash via powershell.exe spawn, but
# the budget stays at 500ms for cross-platform parity and to absorb cold-start
# variability on slow disks / antivirus. Worker-shell .pid populations are
# typically <5 entries; budget absorbs the realistic worst case.
if ($mode -eq "gc-cheap") { $budgetMs = 500 }
$startTicks = [System.Diagnostics.Stopwatch]::GetTimestamp()
$ticksPerMs = [System.Diagnostics.Stopwatch]::Frequency / 1000.0
$budgetExceeded = $false

foreach ($f in (Get-PidFiles)) {
    if (-not $f) { continue }
    $relPath = ($f.FullName) -replace '\\', '/'
    # Trim leading absolute-prefix back to repo-relative if possible.
    $cwdNorm = ((Get-Location).Path) -replace '\\', '/'
    if ($relPath.StartsWith($cwdNorm + '/')) {
        $relPath = $relPath.Substring($cwdNorm.Length + 1)
    }

    if ($budgetMs -gt 0) {
        $elapsedMs = ([System.Diagnostics.Stopwatch]::GetTimestamp() - $startTicks) / $ticksPerMs
        if ($elapsedMs -gt $budgetMs) { $budgetExceeded = $true; break }
    }

    $rawContent = ""
    try {
        $rawContent = (Get-Content -LiteralPath $f.FullName -TotalCount 1 -ErrorAction Stop).Trim()
    } catch {
        $malformed += $relPath
        continue
    }
    $rawContent = $rawContent -replace '\s', ''

    if ($rawContent -notmatch '^[0-9]+$') {
        $malformed += $relPath
        continue
    }
    $pidVal = 0
    if (-not [int]::TryParse($rawContent, [ref]$pidVal) -or $pidVal -le 0) {
        $malformed += $relPath
        continue
    }

    if (Test-UldfPidAlive $pidVal) {
        $aliveList += [pscustomobject]@{
            pid_file       = $relPath
            referenced_pid = $pidVal
        }
    } else {
        $mt = Get-IsoMtime -Path $f.FullName
        $swept += [pscustomobject]@{
            pid_file       = $relPath
            referenced_pid = $pidVal
            mtime          = $mt
        }
    }
}

# ---- Sweep modes: pre-delete summary append + delete ---------------------------
if ($mode -eq "gc" -or $mode -eq "gc-cheap") {
    $newSwept = @()
    foreach ($e in $swept) {
        $jp = ConvertTo-JsonStringEscaped $e.pid_file
        $jm = ConvertTo-JsonStringEscaped $e.mtime
        if ([string]::IsNullOrWhiteSpace($e.mtime)) {
            $line = "{`"pid_file`":`"$jp`",`"referenced_pid`":$($e.referenced_pid),`"liveness_at_sweep`":false,`"mtime`":null,`"sweptAt`":`"$nowIso`"}"
        } else {
            $line = "{`"pid_file`":`"$jp`",`"referenced_pid`":$($e.referenced_pid),`"liveness_at_sweep`":false,`"mtime`":`"$jm`",`"sweptAt`":`"$nowIso`"}"
        }
        if (-not (Append-Summary -Line $line)) {
            [Console]::Error.WriteLine("pid-orphan-detector: summary write failed for $($e.pid_file); preserved")
            continue
        }
        try {
            Remove-Item -LiteralPath $e.pid_file -Force -ErrorAction Stop
            $newSwept += $e
        } catch {
            [Console]::Error.WriteLine("pid-orphan-detector: delete failed for $($e.pid_file) (summary already appended)")
        }
    }
    $swept = $newSwept

    if ($mode -eq "gc-cheap") { exit 0 }
}

# ---- Emit JSON output (briefing OR --gc summary; same shape) -------------------
function Emit-SweptArray {
    param($items)
    $parts = @()
    foreach ($e in $items) {
        $jp = ConvertTo-JsonStringEscaped $e.pid_file
        $jm = ConvertTo-JsonStringEscaped $e.mtime
        if ([string]::IsNullOrWhiteSpace($e.mtime)) {
            $parts += "{`"pid_file`":`"$jp`",`"referenced_pid`":$($e.referenced_pid),`"liveness_at_sweep`":false,`"mtime`":null}"
        } else {
            $parts += "{`"pid_file`":`"$jp`",`"referenced_pid`":$($e.referenced_pid),`"liveness_at_sweep`":false,`"mtime`":`"$jm`"}"
        }
    }
    return ($parts -join ',')
}
function Emit-AliveArray {
    param($items)
    $parts = @()
    foreach ($e in $items) {
        $jp = ConvertTo-JsonStringEscaped $e.pid_file
        $parts += "{`"pid_file`":`"$jp`",`"referenced_pid`":$($e.referenced_pid)}"
    }
    return ($parts -join ',')
}
function Emit-MalformedArray {
    param($items)
    $parts = @()
    foreach ($p in $items) {
        $jp = ConvertTo-JsonStringEscaped $p
        $parts += "`"$jp`""
    }
    return ($parts -join ',')
}

$sweptCount = @($swept).Count
if ($sweptCount -gt 0) {
    $briefing = "[pid-orphans] $sweptCount stale worker-shell PIDs, run /0-uldf-oracle pid-orphan-detector --gc to clean"
} else {
    $briefing = ""
}

$out = "{`"swept`":[" + (Emit-SweptArray -items $swept) + "],"
$out += "`"alive`":[" + (Emit-AliveArray -items $aliveList) + "],"
$out += "`"malformed`":[" + (Emit-MalformedArray -items $malformed) + "],"
$out += "`"briefing`":`"" + (ConvertTo-JsonStringEscaped $briefing) + "`""
if ($budgetExceeded -and $mode -eq "gc") {
    $out += ",`"budgetExceeded`":true"
}
$out += "}"
Write-Output $out
exit 0
