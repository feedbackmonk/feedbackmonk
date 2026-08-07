# cloud-sync-coordination-hazard oracle (Windows / PowerShell)
#
# Twin of run.sh -- byte-equivalent JSON contract. See run.sh for the full
# rationale (DISC-CSI-25 / DEFER-016 / DEC-188): the mkdir lock in
# registry-write.* serializes processes on one machine's LOCAL filesystem view
# and has no leverage over a sync client's after-the-fact reconciliation, which
# can discard a write that was verified locally at write-time.
#
# Silence means "no recognized provider segment" -- never "verified safe".

$ErrorActionPreference = 'SilentlyContinue'

function ConvertTo-JsonString([string]$s) {
    if ($null -eq $s) { return '' }
    # NOTE the replacement strings. PowerShell's -replace does NOT process
    # backslash escapes in the replacement (only `$`), so a single-quoted
    # '\\\\' is FOUR literal backslashes, not two -- which double-escaped every
    # Windows path and made `root` parse back as S:\\SourceControlled\\ULDF.
    # Caught by diffing this twin's output against run.sh's rather than by
    # reading either in isolation. Pattern '\\' is the regex for one backslash;
    # replacement '\\' is the two literal characters JSON wants.
    return ($s -replace '\\', '\\' -replace '"', '\"')
}

function Emit($hosted, $provider, $root, $segment, $atRisk, $briefing) {
    $h = if ($hosted) { 'true' } else { 'false' }
    $p = if ($null -eq $provider) { 'null' } else { '"' + (ConvertTo-JsonString $provider) + '"' }
    $r = if ($null -eq $root) { 'null' } else { '"' + (ConvertTo-JsonString $root) + '"' }
    $s = if ($null -eq $segment) { 'null' } else { '"' + (ConvertTo-JsonString $segment) + '"' }
    Write-Output ('{"hosted":' + $h + ',"provider":' + $p + ',"root":' + $r + ',"matched_segment":' + $s + ',"at_risk_paths":[' + $atRisk + '],"briefing":"' + (ConvertTo-JsonString $briefing) + '"}')
}

# --- Resolve the project root -------------------------------------------------
$root = ""
try {
    $g = git rev-parse --show-toplevel 2>$null
    if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($g)) { $root = ([string]$g).Trim() }
} catch { }
if ([string]::IsNullOrWhiteSpace($root)) {
    try { $root = (Get-Location).Path } catch { $root = "" }
}
if ([string]::IsNullOrWhiteSpace($root)) {
    # Graceful absence -- say nothing rather than guess in either direction.
    Emit $false $null $null $null "" ""
    exit 0
}

# Prefer the native Windows form (a git-reported path may be POSIX-style).
$inspect = $root
try { $inspect = (Resolve-Path -LiteralPath $root -ErrorAction Stop).Path } catch { }

# --- Provider list (env > project config > defaults) ---------------------------
$providers = @()
if (-not [string]::IsNullOrWhiteSpace($env:ULDF_CLOUD_SYNC_PROVIDERS)) {
    $providers = $env:ULDF_CLOUD_SYNC_PROVIDERS -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ }
}
if ($providers.Count -eq 0) {
    $cfg = Join-Path $root ".claude/config.json"
    if (Test-Path -LiteralPath $cfg) {
        try {
            $o = Get-Content -LiteralPath $cfg -Raw | ConvertFrom-Json
            if ($o.cloudSyncHazard -and $o.cloudSyncHazard.providers) {
                $providers = @($o.cloudSyncHazard.providers | ForEach-Object { [string]$_ })
            }
        } catch { }
    }
}
if ($providers.Count -eq 0) {
    $providers = @("OneDrive","Dropbox","Google Drive","GoogleDrive","iCloud Drive","iCloudDrive")
}

# --- Match a provider segment -------------------------------------------------
# Segment-aware so a project merely NAMED "OneDriveTools" does not trip.
$norm = ($inspect -replace '\\','/').ToLowerInvariant()
$hosted = $false; $provider = $null; $segment = $null
foreach ($prov in $providers) {
    if ([string]::IsNullOrWhiteSpace($prov)) { continue }
    $pl = $prov.ToLowerInvariant()
    if (("/" + $norm + "/") -like ("*/" + $pl + "/*")) {
        $hosted = $true; $provider = $prov; $segment = $prov
        break
    }
}

if (-not $hosted) {
    Emit $false $null $inspect $null "" ""
    exit 0
}

# --- Which coordination surfaces actually exist under the hosted root? --------
$atRiskItems = @()
foreach ($rel in @(".claude/collaboration/active-sessions.json", ".claude/session-state", ".claude/collaboration")) {
    if (Test-Path -LiteralPath (Join-Path $root $rel)) {
        $atRiskItems += ('"' + (ConvertTo-JsonString $rel) + '"')
    }
}
$atRisk = ($atRiskItems -join ',')

$brief = "coordination store is inside $provider -- concurrent writes to active-sessions.json / touches.json / channels can be silently reconciled away below the lock (DISC-CSI-25). Prefer a local path for this project, or avoid concurrent PODS work here."
Emit $true $provider $inspect $segment $atRisk $brief
exit 0
