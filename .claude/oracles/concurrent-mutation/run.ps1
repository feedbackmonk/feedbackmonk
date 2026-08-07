# concurrent-mutation oracle (Windows PowerShell) -- CSI-10, CSI Phase 3.
#
# Verification Oracle (kind: "verification", ORACULURGY_DESIGN.md Part 11).
# Byte-compatible verdict contract with run.sh (CSI-10 domain schema):
#   {external_mutation, mutations:[{path,source,since,by_session}],
#    baseline_age_seconds, summary, briefing}
#
# READ-ONLY by contract: this script NEVER writes. The per-session baseline is
# persisted by the sibling update-baseline.ps1. Failure-open on every input
# fault (absent/foreign/unparseable baseline -> external_mutation:false,
# baseline_age_seconds:null, empty briefing).

$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$baselineFile = ".claude/session-state/this-session.json"
$trackedFiles = @(
    "ltads/arc-state.json",
    "ltads/arc-state.archive.json",
    "ltads/sessions/session-history.md",
    "ltads/sessions/blockers.md"
)
$trackedDirs = @("ltads/execution")
$maxMut = 50
$epoch0 = [datetime]::new(1970, 1, 1, 0, 0, 0, [System.DateTimeKind]::Utc)

function Emit-Absent($summary) {
    $obj = [ordered]@{
        external_mutation    = $false
        mutations            = @()
        baseline_age_seconds = $null
        summary              = [string]$summary
        briefing             = ""
    }
    Write-Output ($obj | ConvertTo-Json -Compress -Depth 6)
    exit 0
}

# ---- Resolve own sessionId (env-first, CSI-16) ------------------------------
$mySid = ""
$idLibCandidates = @(
    (Join-Path $PSScriptRoot "../../scripts/lib/session-identity.ps1"),
    (Join-Path $PSScriptRoot "../../../claude-template/scripts/lib/session-identity.ps1"),
    (Join-Path $env:USERPROFILE ".claude/scripts/lib/session-identity.ps1")
)
foreach ($cand in $idLibCandidates) {
    if (Test-Path $cand) { . $cand; break }
}
if (Get-Command Get-CsiIdentity -ErrorAction SilentlyContinue) {
    try { $mySid = [string](Get-CsiIdentity -ProjectRoot (Get-Location).Path).SessionId } catch { $mySid = "" }
}
if (-not $mySid -and $env:CLAUDE_SESSION_ID) { $mySid = $env:CLAUDE_SESSION_ID }

# ---- Graceful absence: no baseline file ------------------------------------
if (-not (Test-Path -LiteralPath $baselineFile)) {
    Emit-Absent "no baseline (this-session.json absent)"
}

# ---- Parse baseline ---------------------------------------------------------
$baseline = $null
try {
    $raw = [System.IO.File]::ReadAllText($baselineFile, [System.Text.UTF8Encoding]::new($false))
    if (-not [string]::IsNullOrWhiteSpace($raw)) {
        $doc = $raw | ConvertFrom-Json -ErrorAction Stop
        if ($doc.PSObject.Properties.Name -contains 'csi10Baseline') {
            $baseline = $doc.csi10Baseline
        }
    }
} catch {
    Emit-Absent "baseline unparseable"
}
if ($null -eq $baseline) {
    Emit-Absent "no csi10 baseline recorded for this workdir"
}

$bSid   = if ($baseline.PSObject.Properties.Name -contains 'sessionId')   { [string]$baseline.sessionId }   else { "" }
$bStart = if ($baseline.PSObject.Properties.Name -contains 'sessionStart') { [string]$baseline.sessionStart } else { "" }
$bHead  = if ($baseline.PSObject.Properties.Name -contains 'headSha')     { [string]$baseline.headSha }     else { "" }

# Per-session keying: a baseline authored by a different session is not mine.
if ($bSid -and $mySid -and ($bSid -ne $mySid)) {
    Emit-Absent "baseline belongs to another session ($bSid)"
}

# ---- baseline_age_seconds --------------------------------------------------
$ageJson = $null
if ($bStart) {
    try {
        $startDt = [datetime]::Parse($bStart, [System.Globalization.CultureInfo]::InvariantCulture, ([System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal))
        $age = [int64](([datetime]::UtcNow) - $startDt).TotalSeconds
        if ($age -lt 0) { $age = 0 }
        $ageJson = [int64]$age
    } catch { $ageJson = $null }
}

$mutations = New-Object System.Collections.Generic.List[object]
$mtimeHits = 0
$gitlogHits = 0

function Add-Mut($mutPath, $source, $since) {
    if ($mutations.Count -ge $maxMut) { return }
    $mutations.Add([ordered]@{
        path       = [string]$mutPath
        source     = [string]$source
        since      = [string]$since
        by_session = $null
    }) | Out-Null
}

# ---- mtime leg --------------------------------------------------------------
if ($baseline.PSObject.Properties.Name -contains 'mtimes' -and $baseline.mtimes) {
    foreach ($prop in $baseline.mtimes.PSObject.Properties) {
        $bpath = $prop.Name
        $bmt = $null
        try { $bmt = [int64]$prop.Value } catch { continue }
        if (-not (Test-Path -LiteralPath $bpath -PathType Leaf)) { continue }
        try {
            $cur = [int64](((Get-Item -LiteralPath $bpath).LastWriteTimeUtc) - $epoch0).TotalSeconds
        } catch { continue }
        if ($cur -gt $bmt) {
            $since = $epoch0.AddSeconds($cur).ToString("yyyy-MM-ddTHH:mm:ssZ")
            if (-not $since) { $since = $bStart }
            Add-Mut $bpath "mtime" $since
            $mtimeHits++
        }
    }
}

# ---- git-log leg ------------------------------------------------------------
$gitAvailable = $false
try {
    if (Get-Command git -ErrorAction SilentlyContinue) {
        & git rev-parse --is-inside-work-tree 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) { $gitAvailable = $true }
    }
} catch { $gitAvailable = $false }

if ($gitAvailable -and $bHead) {
    $curHead = (& git rev-parse HEAD 2>$null)
    if ($curHead -and ($curHead.Trim() -ne $bHead)) {
        $range = "$bHead..HEAD"
        $pathArgs = @("--") + $trackedFiles + $trackedDirs
        $latest = (& git log -1 --format=%cI $range @pathArgs 2>$null)
        if ($latest) { $latest = $latest.Trim() } else { $latest = $bStart }
        $changed = & git diff --name-only $range @pathArgs 2>$null
        if ($changed) {
            foreach ($cpath in $changed) {
                if ([string]::IsNullOrWhiteSpace($cpath)) { continue }
                Add-Mut $cpath.Trim() "git-log" $latest
                $gitlogHits++
            }
        }
    }
}

# ---- Compose verdict --------------------------------------------------------
$anchor = if ($bStart) { $bStart } else { "baseline" }
if ($mutations.Count -gt 0) {
    $ext = $true
    $summary = "$($mutations.Count) external LTADS mutation(s) since $anchor"
    $briefing = "$($mutations.Count) external LTADS-state mutation(s) since $anchor (mtime: $mtimeHits, git-log: $gitlogHits) -- another session may be editing this arc; reconcile before committing"
} else {
    $ext = $false
    $summary = "no external LTADS mutation since $anchor"
    $briefing = ""
}

$result = [ordered]@{
    external_mutation    = $ext
    mutations            = [object[]]$mutations.ToArray()
    baseline_age_seconds = $ageJson
    summary              = $summary
    briefing             = $briefing
}
Write-Output ($result | ConvertTo-Json -Compress -Depth 6)
exit 0
