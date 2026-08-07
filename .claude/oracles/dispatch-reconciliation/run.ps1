# dispatch-reconciliation -- "did those unconfirmed dispatches ever land?"
#
# DISPATCH-12 (DEC-230). Verification Oracle. PowerShell twin of run.sh
# (TWIN-01: byte-identical verdicts over shared fixtures).
#
# The gap it closes (GitCellar, 2026-08-02): dispatch-log.jsonl durably records
# every attempt, but nothing ever answered whether an UNCONFIRMED one eventually
# landed -- so the result stayed permanent NO-DATA and the only way to settle it
# was to hand-read .claude/session-state/turn-state/*.json, which is exactly
# what a live session had to do mid-incident.
#
# ---------------------------------------------------------------------------
# THE VERDICT VOCABULARY IS DELIBERATELY WEAKER THAN "CONFIRMED".
# ---------------------------------------------------------------------------
# The originating brief proposed upgrading these to CONFIRMED-LATE. That
# overclaims and was not adopted. TSTATE holds only the target's LATEST
# transition, so a stamp newer than the dispatch proves the target took *a* turn
# afterwards -- never that the turn was OURS.
#
# FAIL is a four-way conjunction, and the `still live` leg is what keeps this
# lane self-clearing.
#
# Exit codes:  0 pass | 1 stranded (actionable) | 2 no-data

param(
    [string]$WorkDir = (Get-Location).Path
)

$ErrorActionPreference = 'Continue'

$lookbackHours = 24
if ($env:ULDF_DISPATCH_RECONCILE_HOURS) {
    $parsed = 0
    if ([int]::TryParse($env:ULDF_DISPATCH_RECONCILE_HOURS, [ref]$parsed) -and $parsed -gt 0) {
        $lookbackHours = $parsed
    }
}

$stateDir = Join-Path $WorkDir ".claude/session-state"
$log      = Join-Path $stateDir "dispatch-log.jsonl"
$turnDir  = Join-Path $stateDir "turn-state"
$registry = Join-Path $WorkDir ".claude/collaboration/active-sessions.json"

$contract = "ACTIVITY-AFTER-DISPATCH means the target took SOME turn after the dispatch -- it is consistent with consumption, never proof of it. turn-state keeps only the latest transition."

function ConvertTo-DrJsonString {
    param([string]$s)
    if ($null -eq $s) { return "" }
    return ($s -replace '\\', '\\' -replace '"', '\"')
}

$script:logPresent = "false"
$script:scanned = 0
$script:nActivity = 0
$script:nNoAct = 0
$script:nNoData = 0
$script:nStranded = 0
$script:entriesJson = ""

function Write-DrResult {
    param([string]$Status, [int]$Code)
    Write-Output ('{"status":"' + $Status +
        '","details":{"log_present":' + $script:logPresent +
        ',"entries_scanned":' + $script:scanned +
        ',"lookback_hours":' + $lookbackHours +
        ',"activity_after_dispatch":' + $script:nActivity +
        ',"no_activity_since":' + $script:nNoAct +
        ',"no_data":' + $script:nNoData +
        ',"stranded_actionable":' + $script:nStranded +
        ',"contract":"' + (ConvertTo-DrJsonString $contract) +
        '"},"entries":[' + $script:entriesJson + ']}')
    exit $Code
}

if (-not (Test-Path -LiteralPath $log)) {
    Write-DrResult -Status "no-data" -Code 2
}
$script:logPresent = "true"

$epochBase = [datetime]::new(1970, 1, 1, 0, 0, 0, [DateTimeKind]::Utc)
$nowEpoch = [int64][Math]::Floor(((Get-Date).ToUniversalTime() - $epochBase).TotalSeconds)
$cutoff = $nowEpoch - ($lookbackHours * 3600)

# ISO8601 -> epoch by explicit field arithmetic rather than ConvertFrom-Json /
# [datetime]::Parse: powershell.exe 5.1 and pwsh 7 diverge on ISO-date coercion
# (DISC-ARC-01), and this must agree with the bash twin exactly.
function ConvertFrom-DrIso {
    param([string]$Iso)
    if ($Iso -notmatch '^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})Z?$') { return [int64]0 }
    $dt = [datetime]::new([int]$Matches[1], [int]$Matches[2], [int]$Matches[3],
                          [int]$Matches[4], [int]$Matches[5], [int]$Matches[6],
                          [DateTimeKind]::Utc)
    return [int64][Math]::Floor(($dt - $epochBase).TotalSeconds)
}

function Get-DrTurnStamp {
    param([string]$Sid)
    $safe = ($Sid -replace '[^A-Za-z0-9._-]', '_')
    $f = Join-Path $turnDir "$safe.json"
    $out = @{ Epoch = [int64]0; At = "" }
    if (-not (Test-Path -LiteralPath $f)) { return $out }
    $line = Get-Content -LiteralPath $f -TotalCount 1 -ErrorAction SilentlyContinue
    if (-not $line) { return $out }
    if ($line -match '"atEpoch"\s*:\s*([0-9]+)') { $out.Epoch = [int64]$Matches[1] }
    if ($line -match '"at"\s*:\s*"([^"]*)"')     { $out.At = $Matches[1] }
    return $out
}

# Is the target still live? Registry entry active+dispatchable with a live PID.
function Test-DrTargetLive {
    param([string]$Sid)
    if (-not (Test-Path -LiteralPath $registry)) { return $false }
    $raw = Get-Content -LiteralPath $registry -Raw -ErrorAction SilentlyContinue
    if (-not $raw) { return $false }
    $entry = ($raw -split '\{') | Where-Object { $_ -like "*`"id`":`"$Sid`"*" } | Select-Object -First 1
    if (-not $entry) { return $false }
    if ($entry -notlike '*"status":"active"*') { return $false }
    if ($entry -notlike '*"dispatchable":true*') { return $false }
    if ($entry -notmatch '"claudeShellPid"\s*:\s*([0-9]+)') { return $false }
    $p = [int]$Matches[1]
    if ($p -le 0) { return $false }
    try {
        $proc = Get-Process -Id $p -ErrorAction SilentlyContinue
        return ($null -ne $proc)
    } catch { return $false }
}

foreach ($line in (Get-Content -LiteralPath $log -ErrorAction SilentlyContinue)) {
    if (-not $line) { continue }
    if ($line -notmatch '"outcome"\s*:\s*"([^"]*)"') { continue }
    $outcome = $Matches[1]
    if ($outcome -ne 'delivered-unconfirmed' -and $outcome -ne 'delivered-queued') { continue }

    $ts = ""
    if ($line -match '"ts"\s*:\s*"([^"]*)"') { $ts = $Matches[1] }
    $dEpoch = ConvertFrom-DrIso -Iso $ts
    if ($dEpoch -lt $cutoff) { continue }

    $target = ""
    if ($line -match '"target"\s*:\s*"([^"]*)"') { $target = $Matches[1] }
    $rid = ""
    if ($line -match '"resolvedId"\s*:\s*"([^"]*)"') { $rid = $Matches[1] }
    $reason = ""
    if ($line -match '"reason"\s*:\s*"([^"]*)"') { $reason = $Matches[1] }
    if (-not $rid) { $rid = $target }

    $script:scanned++

    $stamp = Get-DrTurnStamp -Sid $rid
    if ($stamp.Epoch -eq 0) {
        $verdict = "NO-DATA"; $script:nNoData++
    } elseif ($stamp.Epoch -gt $dEpoch) {
        $verdict = "ACTIVITY-AFTER-DISPATCH"; $script:nActivity++
    } else {
        $verdict = "NO-ACTIVITY-SINCE"; $script:nNoAct++
    }

    $live = Test-DrTargetLive -Sid $rid
    $liveStr = if ($live) { "true" } else { "false" }

    # The four-way conjunction. `delivered-queued` is excluded by construction:
    # a non-receipt from a mid-turn target is the EXPECTED result (DISPATCH-11),
    # not a symptom, and grading it would re-import the very conflation this
    # whole change removed.
    $actionable = "false"
    if ($reason -eq 'receipt-timeout' -and $verdict -eq 'NO-ACTIVITY-SINCE' -and $live) {
        $actionable = "true"
        $script:nStranded++
    }

    if ($script:entriesJson) { $script:entriesJson += "," }
    $script:entriesJson += '{"ts":"' + (ConvertTo-DrJsonString $ts) +
        '","target":"' + (ConvertTo-DrJsonString $target) +
        '","resolvedId":"' + (ConvertTo-DrJsonString $rid) +
        '","outcome":"' + (ConvertTo-DrJsonString $outcome) +
        '","reason":"' + (ConvertTo-DrJsonString $reason) +
        '","verdict":"' + $verdict +
        '","targetLive":' + $liveStr +
        ',"actionable":' + $actionable +
        ',"stampAt":"' + (ConvertTo-DrJsonString $stamp.At) + '"}'
}

if ($script:scanned -eq 0) { Write-DrResult -Status "pass" -Code 0 }
if ($script:nStranded -gt 0) { Write-DrResult -Status "fail" -Code 1 }
Write-DrResult -Status "pass" -Code 0
