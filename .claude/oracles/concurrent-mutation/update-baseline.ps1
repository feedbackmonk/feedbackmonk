# concurrent-mutation baseline writer (Windows PowerShell) -- CSI-10, Phase 3.
#
# PowerShell sibling of update-baseline.sh: captures / refreshes this session's
# known-good snapshot of the tracked LTADS path set into
# .claude/session-state/this-session.json under csi10Baseline (read-merge-write,
# preserving all other top-level keys). Fail-open silent no-op on every fault.
#
# Usage:
#   update-baseline.ps1 [-Root <dir>] [-SessionId <id>] [-SessionStart <iso>]

param(
    [string]$Root = "",
    [string]$SessionId = "",
    [string]$SessionStart = ""
)

$ErrorActionPreference = 'SilentlyContinue'

if (-not $Root) { $Root = (Get-Location).Path }
try { Set-Location -LiteralPath $Root } catch { exit 0 }

$baselineFile = ".claude/session-state/this-session.json"
$trackedFiles = @(
    "ltads/arc-state.json",
    "ltads/arc-state.archive.json",
    "ltads/sessions/session-history.md",
    "ltads/sessions/blockers.md"
)
$trackedDirs = @("ltads/execution")
$epoch0 = [datetime]::new(1970, 1, 1, 0, 0, 0, [System.DateTimeKind]::Utc)

# ---- Resolve sessionId ------------------------------------------------------
$mySid = $SessionId
if (-not $mySid) {
    $idLibCandidates = @(
        (Join-Path $PSScriptRoot "../../scripts/lib/session-identity.ps1"),
        (Join-Path $PSScriptRoot "../../../claude-template/scripts/lib/session-identity.ps1"),
        (Join-Path $env:USERPROFILE ".claude/scripts/lib/session-identity.ps1")
    )
    foreach ($cand in $idLibCandidates) {
        if (Test-Path $cand) { . $cand; break }
    }
    if (Get-Command Get-CsiIdentity -ErrorAction SilentlyContinue) {
        try { $mySid = [string](Get-CsiIdentity -ProjectRoot $Root).SessionId } catch { $mySid = "" }
    }
    if (-not $mySid -and $env:CLAUDE_SESSION_ID) { $mySid = $env:CLAUDE_SESSION_ID }
}
if (-not $mySid) { $mySid = "unknown" }

# ---- Collect tracked files + mtimes ----------------------------------------
$mtimes = [ordered]@{}
$fileList = New-Object System.Collections.Generic.List[string]
foreach ($f in $trackedFiles) {
    if (Test-Path -LiteralPath $f -PathType Leaf) { $fileList.Add($f) | Out-Null }
}
foreach ($d in $trackedDirs) {
    if (Test-Path -LiteralPath $d -PathType Container) {
        Get-ChildItem -LiteralPath $d -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object {
            $rel = (Resolve-Path -LiteralPath $_.FullName -Relative).TrimStart('.\').Replace('\', '/')
            $fileList.Add($rel) | Out-Null
        }
    }
}
foreach ($f in $fileList) {
    try {
        $m = [int64](((Get-Item -LiteralPath $f).LastWriteTimeUtc) - $epoch0).TotalSeconds
        $mtimes[$f] = $m
    } catch { }
}

# ---- headSha + timestamps --------------------------------------------------
$headSha = ""
try {
    if (Get-Command git -ErrorAction SilentlyContinue) {
        & git rev-parse --is-inside-work-tree 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) {
            $h = (& git rev-parse HEAD 2>$null)
            if ($h -and ($h.Trim() -match '^[0-9a-fA-F]+$')) { $headSha = $h.Trim() }
        }
    }
} catch { $headSha = "" }
$nowIso = [datetime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")

# ---- Determine sessionStart -------------------------------------------------
$start = $SessionStart
if (-not $start -and (Test-Path -LiteralPath $baselineFile)) {
    try {
        $raw = [System.IO.File]::ReadAllText($baselineFile, [System.Text.UTF8Encoding]::new($false))
        if (-not [string]::IsNullOrWhiteSpace($raw)) {
            $doc = $raw | ConvertFrom-Json -ErrorAction Stop
            if ($doc.PSObject.Properties.Name -contains 'csi10Baseline') {
                $pb = $doc.csi10Baseline
                $prevSid = if ($pb.PSObject.Properties.Name -contains 'sessionId') { [string]$pb.sessionId } else { "" }
                $prevStart = if ($pb.PSObject.Properties.Name -contains 'sessionStart') { [string]$pb.sessionStart } else { "" }
                if ($prevSid -eq $mySid -and $prevStart) { $start = $prevStart }
            }
        }
    } catch { }
}
if (-not $start) { $start = $nowIso }

# ---- Compose csi10Baseline + merge-write -----------------------------------
$b = [ordered]@{
    sessionId    = $mySid
    sessionStart = $start
    capturedAt   = $nowIso
    headSha      = $headSha
    mtimes       = $mtimes
}

$dir = Split-Path -Parent $baselineFile
if ($dir -and -not (Test-Path -LiteralPath $dir)) {
    try { New-Item -ItemType Directory -Path $dir -Force | Out-Null } catch { exit 0 }
}

# Read-merge: preserve all existing top-level keys.
$mergedDoc = [ordered]@{}
if (Test-Path -LiteralPath $baselineFile) {
    try {
        $raw = [System.IO.File]::ReadAllText($baselineFile, [System.Text.UTF8Encoding]::new($false))
        if (-not [string]::IsNullOrWhiteSpace($raw)) {
            $existing = $raw | ConvertFrom-Json -ErrorAction Stop
            foreach ($p in $existing.PSObject.Properties) {
                if ($p.Name -ne 'csi10Baseline') { $mergedDoc[$p.Name] = $p.Value }
            }
        }
    } catch { $mergedDoc = [ordered]@{} }
}
$mergedDoc['csi10Baseline'] = $b

$tmp = "$baselineFile.csi10.tmp.$PID"
try {
    # -InputObject (not pipe): piping an indexer-built OrderedDictionary makes
    # PS 5.1 enumerate DictionaryEntry items, serializing the type name instead
    # of the object. -InputObject serializes the dictionary whole.
    $json = ConvertTo-Json -InputObject $mergedDoc -Depth 8 -Compress
    [System.IO.File]::WriteAllText($tmp, $json, [System.Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $tmp -Destination $baselineFile -Force
} catch {
    try { Remove-Item -LiteralPath $tmp -Force } catch { }
}
exit 0
