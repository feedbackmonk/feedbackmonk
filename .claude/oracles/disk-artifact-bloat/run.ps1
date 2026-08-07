# disk-artifact-bloat oracle (Windows)
#
# Cheap every-session tripwire for regenerable build-artifact disk bloat.
# Reads the free space of the dev drive (drive of CWD by default, or the
# configured root) and emits a [disk-artifact-bloat] briefing line ONLY when
# free space crosses the warn/alert threshold, or when free space has dropped
# by >= driftGb since the last sweep baseline. All-clear -> empty briefing
# (session-start suppresses the line). Graceful absence: undeterminable free
# space -> free_gb null + empty briefing.
#
# Output: single-line JSON matching the FROZEN schema in oracle.json.
# READ-ONLY. Never mutates state. The heavy enumeration + cleanup + baseline
# write live in scripts/disk-artifact-sweep.ps1.

$ErrorActionPreference = 'SilentlyContinue'

# ---- Defaults --------------------------------------------------------------
$WarnGb  = 80
$AlertGb = 40
$DriftGb = 50
$Root    = $null   # null -> use drive of CWD

function Read-JsonConfig([string]$path) {
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    try {
        $raw = Get-Content -LiteralPath $path -Raw -Encoding UTF8 -ErrorAction Stop
        if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
        return ($raw | ConvertFrom-Json -ErrorAction Stop)
    } catch { return $null }
}

function Apply-Config($cfg) {
    if ($null -eq $cfg) { return }
    $d = $cfg.diskArtifactBloat
    if ($null -eq $d) { return }
    if ($d.PSObject.Properties.Name -contains 'root'    -and $d.root)    { $script:Root    = [string]$d.root }
    if ($d.PSObject.Properties.Name -contains 'warnGb'  -and $null -ne $d.warnGb)  { try { $script:WarnGb  = [double]$d.warnGb }  catch {} }
    if ($d.PSObject.Properties.Name -contains 'alertGb' -and $null -ne $d.alertGb) { try { $script:AlertGb = [double]$d.alertGb } catch {} }
    if ($d.PSObject.Properties.Name -contains 'driftGb' -and $null -ne $d.driftGb) { try { $script:DriftGb = [double]$d.driftGb } catch {} }
}

# Priority (lowest first; later overrides earlier): global config -> project
# config -> env vars.
Apply-Config (Read-JsonConfig (Join-Path $env:USERPROFILE ".claude/config.json"))
Apply-Config (Read-JsonConfig ".claude/config.json")

if ($env:ULDF_BLOAT_ROOT)     { $Root    = $env:ULDF_BLOAT_ROOT }
if ($env:ULDF_BLOAT_WARN_GB)  { try { $WarnGb  = [double]$env:ULDF_BLOAT_WARN_GB }  catch {} }
if ($env:ULDF_BLOAT_ALERT_GB) { try { $AlertGb = [double]$env:ULDF_BLOAT_ALERT_GB } catch {} }
if ($env:ULDF_BLOAT_DRIFT_GB) { try { $DriftGb = [double]$env:ULDF_BLOAT_DRIFT_GB } catch {} }

# ---- Resolve the path whose drive we check ---------------------------------
$probePath = if ($Root) { $Root } else { (Get-Location).Path }

function Emit-Json($obj) {
    Write-Output ($obj | ConvertTo-Json -Compress -Depth 5)
    exit 0
}

function Emit-Empty([string]$drive, $warn, $alert) {
    Emit-Json ([ordered]@{
        tripped           = $false
        level             = "ok"
        drive             = $drive
        free_gb           = $null
        total_gb          = $null
        warn_gb           = $warn
        alert_gb          = $alert
        drift_gb          = $null
        baseline_age_days = $null
        briefing          = ""
    })
}

# ---- Read free space via the drive's root --------------------------------
$driveLabel = $null
$freeBytes  = $null
$totalBytes = $null

try {
    if (-not (Test-Path -LiteralPath $probePath)) { Emit-Empty $null $WarnGb $AlertGb }
    $item = Get-Item -LiteralPath $probePath -Force -ErrorAction Stop
    $qualifier = (Split-Path -Qualifier $item.FullName -ErrorAction SilentlyContinue)  # e.g. "S:"
    if ($qualifier) {
        $driveLetter = $qualifier.TrimEnd(':')
        $driveLabel  = "$driveLetter`:"
        $psd = Get-PSDrive -Name $driveLetter -ErrorAction SilentlyContinue
        if ($psd -and $null -ne $psd.Free) {
            $freeBytes  = [double]$psd.Free
            if ($null -ne $psd.Used) { $totalBytes = [double]$psd.Free + [double]$psd.Used }
        }
    }
} catch { }

if ($null -eq $freeBytes) { Emit-Empty $driveLabel $WarnGb $AlertGb }

$gib     = 1073741824.0
$freeGb  = [math]::Round($freeBytes / $gib, 1)
$totalGb = if ($null -ne $totalBytes) { [math]::Round($totalBytes / $gib, 1) } else { $null }

# ---- Drift vs the last sweep baseline --------------------------------------
$driftGbActual   = $null
$baselineAgeDays = $null
$baselinePath = Join-Path $env:USERPROFILE ".claude/session-state/disk-artifact-baseline.json"
$baseline = Read-JsonConfig $baselinePath
if ($null -ne $baseline) {
    try {
        $sameDrive = $true
        if ($baseline.PSObject.Properties.Name -contains 'drive' -and $baseline.drive -and $driveLabel) {
            $sameDrive = ([string]$baseline.drive -ieq $driveLabel)
        }
        if ($sameDrive -and ($baseline.PSObject.Properties.Name -contains 'free_bytes') -and ($null -ne $baseline.free_bytes)) {
            $drop = ([double]$baseline.free_bytes - $freeBytes) / $gib
            $driftGbActual = [math]::Round($drop, 1)
            if ($baseline.PSObject.Properties.Name -contains 'scanned_at' -and $baseline.scanned_at) {
                try {
                    $bt = [DateTimeOffset]::Parse([string]$baseline.scanned_at).UtcDateTime
                    $baselineAgeDays = [int][math]::Floor(([DateTime]::UtcNow - $bt).TotalDays)
                } catch { }
            }
        }
    } catch { }
}

# ---- Classify + compose briefing -------------------------------------------
$level    = "ok"
$briefing = ""

$driftClause = ""
if ($null -ne $driftGbActual -and $driftGbActual -ge $DriftGb) {
    $ageTxt = if ($null -ne $baselineAgeDays) { "$baselineAgeDays" + "d ago" } else { "since last sweep" }
    $driftClause = "; free dropped $driftGbActual GB $ageTxt"
}

if ($freeGb -lt $AlertGb) {
    $level    = "alert"
    $briefing = "ALERT $driveLabel $freeGb GB free (alert < $AlertGb GB)$driftClause -- run ~/.claude/scripts/disk-artifact-sweep.ps1 to reclaim regenerable artifacts"
} elseif ($freeGb -lt $WarnGb) {
    $level    = "warn"
    $briefing = "$driveLabel $freeGb GB free (warn < $WarnGb GB)$driftClause -- run ~/.claude/scripts/disk-artifact-sweep.ps1 (dry-run) to inspect"
} elseif ($driftClause -ne "") {
    # Above thresholds but a large drift since last sweep -> rapid-drift tripwire.
    $level    = "warn"
    $ageTxt = if ($null -ne $baselineAgeDays) { "$baselineAgeDays" + "d ago" } else { "since last sweep" }
    $briefing = "$driveLabel free dropped $driftGbActual GB $ageTxt ($freeGb GB free) -- run ~/.claude/scripts/disk-artifact-sweep.ps1 -Scan"
}

Emit-Json ([ordered]@{
    tripped           = ($briefing -ne "")
    level             = $level
    drive             = $driveLabel
    free_gb           = $freeGb
    total_gb          = $totalGb
    warn_gb           = $WarnGb
    alert_gb          = $AlertGb
    drift_gb          = $driftGbActual
    baseline_age_days = $baselineAgeDays
    briefing          = $briefing
})
