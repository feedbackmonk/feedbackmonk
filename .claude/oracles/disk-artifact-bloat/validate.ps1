# disk-artifact-bloat oracle self-test (Windows)
#
# Parity with validate.sh. The oracle reads a LIVE drive's free space, so tests
# force thresholds (env overrides) against the real free value and craft a
# sandbox baseline for the drift path:
#
#   T1. ok      -> warn/alert below real free -> level=ok, briefing=""
#   T2. warn    -> warn above real free        -> level=warn, briefing has "(warn"
#   T3. alert   -> alert above real free        -> level=alert, briefing has "ALERT"
#   T4. absence -> ROOT = nonexistent path      -> free_gb=null, briefing=""
#   T5. drift   -> sandbox USERPROFILE baseline -> drift_gb!=null, "free dropped"

$ErrorActionPreference = 'Continue'
$OracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Run = Join-Path $OracleDir "run.ps1"

$Pass = 0; $Fail = 0
function Pass($m) { Write-Host "PASS: $m"; $script:Pass++ }
function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; $script:Fail++ }

$SCHEMA = @('tripped','level','drive','free_gb','total_gb','warn_gb','alert_gb','drift_gb','baseline_age_days','briefing')

$Sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("dab-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $Sandbox -Force | Out-Null

function Run-Oracle([hashtable]$envOverrides) {
    $saved = @{}
    foreach ($k in $envOverrides.Keys) {
        $saved[$k] = [Environment]::GetEnvironmentVariable($k)
        [Environment]::SetEnvironmentVariable($k, $envOverrides[$k])
    }
    $prevLoc = Get-Location
    try {
        Set-Location $Sandbox
        $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $Run 2>&1 | Out-String
        return $out.Trim()
    } finally {
        Set-Location $prevLoc
        foreach ($k in $envOverrides.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
    }
}

function Parse($out, $label) {
    try { return ($out | ConvertFrom-Json -ErrorAction Stop) }
    catch { Fail "${label}: invalid JSON: $out"; return $null }
}
function Assert-Schema($obj, $label) {
    if ($null -eq $obj) { return $false }
    foreach ($f in $SCHEMA) {
        if ($obj.PSObject.Properties.Name -notcontains $f) { Fail "${label}: missing schema field '$f'"; return $false }
    }
    return $true
}

# ---- T1. ok ----------------------------------------------------------------
$o = Run-Oracle @{ ULDF_BLOAT_WARN_GB='1'; ULDF_BLOAT_ALERT_GB='1' }
$j = Parse $o T1; Assert-Schema $j T1 | Out-Null
if ($j -and $j.level -eq 'ok' -and [string]::IsNullOrEmpty($j.briefing)) { Pass 'T1: ok -> level=ok briefing=""' }
else { Fail "T1: expected ok/empty; got level=$($j.level) br='$($j.briefing)'" }

# ---- T2. warn --------------------------------------------------------------
$o = Run-Oracle @{ ULDF_BLOAT_WARN_GB='999999'; ULDF_BLOAT_ALERT_GB='1' }
$j = Parse $o T2; Assert-Schema $j T2 | Out-Null
if ($j -and $j.level -eq 'warn' -and $j.briefing -match '\(warn') { Pass 'T2: warn trip' }
else { Fail "T2: expected warn + '(warn'; got level=$($j.level) br='$($j.briefing)'" }

# ---- T3. alert -------------------------------------------------------------
$o = Run-Oracle @{ ULDF_BLOAT_ALERT_GB='999999' }
$j = Parse $o T3; Assert-Schema $j T3 | Out-Null
if ($j -and $j.level -eq 'alert' -and $j.briefing -match 'ALERT') { Pass 'T3: alert trip' }
else { Fail "T3: expected alert + 'ALERT'; got level=$($j.level) br='$($j.briefing)'" }

# ---- T4. graceful absence --------------------------------------------------
$o = Run-Oracle @{ ULDF_BLOAT_ROOT=(Join-Path $Sandbox 'does-not-exist-xyz'); ULDF_BLOAT_WARN_GB='999999' }
$j = Parse $o T4; Assert-Schema $j T4 | Out-Null
if ($j -and $null -eq $j.free_gb -and [string]::IsNullOrEmpty($j.briefing)) { Pass 'T4: absence -> free_gb=null briefing=""' }
else { Fail "T4: expected free_gb null + empty briefing; got free_gb=$($j.free_gb) br='$($j.briefing)'" }

# ---- T5. drift (sandbox USERPROFILE, baseline = live free + 100 GiB) -------
$fakeHome = Join-Path $Sandbox 'home'
New-Item -ItemType Directory -Path (Join-Path $fakeHome '.claude/session-state') -Force | Out-Null
# Probe the sandbox drive's current free; prior free = current + 100 GiB.
$qual = (Split-Path -Qualifier $Sandbox).TrimEnd(':')
$cur = (Get-PSDrive -Name $qual).Free
$prior = [int64]$cur + 100 * 1073741824
# No 'drive' field -> same-drive check defaults true.
$bl = '{"scanned_at":"2026-06-01T00:00:00Z","free_bytes":' + $prior + ',"total_artifact_bytes":0}'
Set-Content -Path (Join-Path $fakeHome '.claude/session-state/disk-artifact-baseline.json') -Value $bl -Encoding UTF8
$o = Run-Oracle @{ USERPROFILE=$fakeHome }
$j = Parse $o T5; Assert-Schema $j T5 | Out-Null
if ($j -and $null -ne $j.drift_gb -and $j.briefing -match 'free dropped') { Pass "T5: drift -> drift_gb=$($j.drift_gb), 'free dropped' clause" }
else { Fail "T5: expected non-null drift_gb + 'free dropped'; got drift_gb=$($j.drift_gb) out=$o" }

# ---- cleanup + summary -----------------------------------------------------
Remove-Item -Recurse -Force $Sandbox -ErrorAction SilentlyContinue
Write-Host ""
Write-Host "================================================================"
Write-Host "  disk-artifact-bloat validate: $Pass PASS / $Fail FAIL"
Write-Host "================================================================"
if ($Fail -gt 0) { exit 1 }
exit 0
