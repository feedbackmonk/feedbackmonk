# aria-status oracle (Windows PowerShell)
# Answers: what is the ARIA instrumentation status of this project?
#
# Output schema and briefing-line forms documented in run.sh (cross-platform parity).

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$DefaultEndpoint = "http://127.0.0.1:14550/aria/health"
$AriaConfig = ".claude/aria.json"
$AriaManifest = ".claude/aria-manifest.json"   # AOR capabilities disk companion (ARIA-17)
$TelemetryLog = ".claude/aria-telemetry.jsonl"
$ProbeTimeoutSec = 0.3

# ---- Surface detection (delegate to ui-surface-detector) ----
$surfaceKind = "none"
$surfaceOracleDir = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "..\ui-surface-detector"
$surfaceRun = Join-Path $surfaceOracleDir "run.ps1"
if (Test-Path $surfaceRun) {
    try {
        $surfaceOut = & powershell -NoProfile -ExecutionPolicy Bypass -File $surfaceRun 2>$null
        if ($surfaceOut) {
            [string]$so = if ($surfaceOut -is [array]) { $surfaceOut -join "" } else { "$surfaceOut" }
            $parsed = $so.Trim() | ConvertFrom-Json -ErrorAction SilentlyContinue
            if ($parsed -and $parsed.surface_kind) { $surfaceKind = $parsed.surface_kind }
        }
    } catch {}
}

$surfacePresent = -not (@("none","cli-tool") -contains $surfaceKind)

# ---- No-surface: emit empty briefing ----
if (-not $surfacePresent) {
    $emptyResult = [ordered]@{
        surface_present = $false
        exposure_mechanism = "none"
        endpoint_reachable = $false
        foundation_layer = [ordered]@{ errors = $false; async = $false; navigation = $false }
        briefing = ""
    }
    Write-Output ($emptyResult | ConvertTo-Json -Compress -Depth 5)
    exit 0
}

# ---- Detect aria.json config ----
$hasConfig = $false
$endpointUrl = $DefaultEndpoint
$exposureMechanism = "http"
if (Test-Path $AriaConfig) {
    $hasConfig = $true
    try {
        $cfgRaw = Get-Content $AriaConfig -Raw -Encoding UTF8 -ErrorAction Stop
        $cfg = $cfgRaw | ConvertFrom-Json -ErrorAction Stop
        if ($cfg.endpoint_url) { $endpointUrl = [string]$cfg.endpoint_url }
        if ($cfg.exposure_mechanism) { $exposureMechanism = [string]$cfg.exposure_mechanism }
    } catch {}
}

# ---- Probe endpoint ----
$endpointReachable = $false
$ariaHealthBody = $null
try {
    # Invoke-WebRequest -TimeoutSec accepts integer seconds in Windows PowerShell 5.x;
    # use [int][Math]::Ceiling to ensure compatibility, but prefer fractional via System.Net.WebClient
    $req = [System.Net.HttpWebRequest]::Create($endpointUrl)
    $req.Method = "GET"
    $req.Timeout = [int]($ProbeTimeoutSec * 1000)  # ms
    $req.ReadWriteTimeout = [int]($ProbeTimeoutSec * 1000)
    $req.Accept = "application/json"
    $req.AllowAutoRedirect = $false
    $resp = $req.GetResponse()
    try {
        $stream = $resp.GetResponseStream()
        $reader = New-Object System.IO.StreamReader($stream)
        $ariaHealthBody = $reader.ReadToEnd()
        $reader.Close()
    } finally {
        $resp.Close()
    }
    if ($ariaHealthBody) { $endpointReachable = $true }
} catch {
    $endpointReachable = $false
}

# ---- Parse aria_health response ----
$flErrors = $false
$flAsync = $false
$flNav = $false
$recentSuccessAt = $null

if ($endpointReachable) {
    try {
        $parsed = $ariaHealthBody | ConvertFrom-Json -ErrorAction Stop
        if (-not $parsed._meta) {
            # Contract violation
            $endpointReachable = $false
        } else {
            switch ($parsed.oracleStatus) {
                "healthy" {
                    $flErrors = $true; $flAsync = $true; $flNav = $true
                    $recentSuccessAt = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture)
                }
                "degraded" {
                    $flErrors = $true; $flAsync = $true; $flNav = $true
                    $degraded = @()
                    if ($parsed._meta.degradedCategories) { $degraded = @($parsed._meta.degradedCategories) }
                    if ($degraded -contains "errors") { $flErrors = $false }
                    if ($degraded -contains "async") { $flAsync = $false }
                    if ($degraded -contains "navigation") { $flNav = $false }
                    $recentSuccessAt = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture)
                }
                default {
                    # Unknown oracleStatus
                    $endpointReachable = $false
                }
            }
        }
    } catch {
        $endpointReachable = $false
    }
}

# ---- query_count_24h from telemetry log (v1+v2 capture contract, ARIA-13) ----
# NO-DATA semantics (DISC-ARIA-09 / DEC-81): a missing log file is NO-DATA, never zero;
# a 24h window with zero heartbeats AND zero query events is NO-DATA for the window.
# Counting rules: v2 lines ("schemaVersion" present) count when event=="query";
# heartbeats (event=="telemetry-armed") tracked separately; probe:true lines excluded;
# v1 legacy lines (no "schemaVersion", {tool,timestamp,...}) count as query events.
$queryCount24h = $null
$heartbeatCount24h = 0
$telemetryState = "no-data"
if (Test-Path $TelemetryLog) {
    try {
        $cutoff = (Get-Date).ToUniversalTime().AddHours(-24).ToString("yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture)
        $count = 0
        Get-Content $TelemetryLog -Encoding UTF8 -ErrorAction SilentlyContinue | ForEach-Object {
            $line = $_
            $ts = $null
            if ($line -match '"ts"\s*:\s*"([^"]+)"') { $ts = $Matches[1] }
            elseif ($line -match '"timestamp"\s*:\s*"([^"]+)"') { $ts = $Matches[1] }
            if (-not $ts -or $ts -lt $cutoff) { return }
            if ($line -match '"probe"\s*:\s*true') { return }
            if ($line -match '"event"\s*:\s*"telemetry-armed"') { $heartbeatCount24h++; return }
            if ($line -match '"event"\s*:\s*"query"') { $count++; return }
            if ($line -notmatch '"schemaVersion"' -and $line -match '"timestamp"') { $count++ }
        }
        if ($count -gt 0 -or $heartbeatCount24h -gt 0) {
            $telemetryState = "measured"
            $queryCount24h = $count
        }
        # else: zero heartbeats AND zero queries in window -> NO-DATA, not zero
    } catch {}
}

# ---- Compose briefing ----
$briefing = ""
if ($endpointReachable) {
    if ($flErrors -and $flAsync -and $flNav) {
        # NO-DATA is never presented as qph=0 (DISC-ARIA-09).
        if ($telemetryState -eq "measured") {
            $qph = 0
            if ($queryCount24h -ne $null -and $queryCount24h -gt 0) { $qph = [int][Math]::Floor($queryCount24h / 24) }
            $briefing = "ARIA: errors+async+navigation healthy (qph=$qph)"
        } else {
            $briefing = "ARIA: errors+async+navigation healthy (qph=no-data; telemetry meter not armed, see ARIA-13)"
        }
    } else {
        $healthyCats = @()
        $degradedCats = @()
        foreach ($pair in @(@{n="navigation";v=$flNav}, @{n="errors";v=$flErrors}, @{n="async";v=$flAsync})) {
            if ($pair.v) { $healthyCats += $pair.n } else { $degradedCats += $pair.n }
        }
        if ($healthyCats.Count -gt 0 -and $degradedCats.Count -gt 0) {
            $briefing = "ARIA: " + ($healthyCats -join "+") + " healthy; " + ($degradedCats -join "|") + " UNREACHABLE -- see /0-uldf-oracle aria-status"
        } elseif ($degradedCats.Count -gt 0) {
            $briefing = "ARIA: " + ($degradedCats -join "|") + " UNREACHABLE -- see /0-uldf-oracle aria-status"
        } else {
            $briefing = "ARIA: status unknown -- see /0-uldf-oracle aria-status"
        }
    }
} elseif ($hasConfig) {
    $briefing = "ARIA: configured but endpoint unreachable at $endpointUrl"
} else {
    $briefing = "ARIA: UI/runtime surface detected; no ARIA instrumentation. /0-uldf-ldis-plan can scaffold."
}

if ($briefing.Length -gt 200) { $briefing = $briefing.Substring(0, 197) + "..." }

# ---- AOR capabilities disk companion (ARIA-17) ----
# Value-free projection of the runtime manifest (AOR_CONTRACTS.md §4) committed by
# an AOR-conformant app. NO-DATA: absent/malformed -> field omitted, never an
# empty-capabilities claim. Source of truth is the runtime manifest()/GET /aor/capabilities.
$capabilities = $null
if ((Test-Path $AriaManifest) -and ((Get-Item $AriaManifest).Length -gt 0)) {
    try {
        $cap = (Get-Content -Raw -LiteralPath $AriaManifest) | ConvertFrom-Json -ErrorAction Stop
        if ($cap -and $cap.PSObject.Properties.Name -contains "verbs") { $capabilities = $cap }
    } catch {}
}

# ---- Emit JSON ----
$result = [ordered]@{
    surface_present = $true
    exposure_mechanism = $exposureMechanism
    endpoint_reachable = $endpointReachable
    endpoint_url = $endpointUrl
    foundation_layer = [ordered]@{ errors = $flErrors; async = $flAsync; navigation = $flNav }
}
if ($recentSuccessAt) { $result.recent_success_at = $recentSuccessAt }
if ($telemetryState -eq "measured" -and $queryCount24h -ne $null) { $result.query_count_24h = $queryCount24h }
$result.telemetry_state = $telemetryState
if ($capabilities -ne $null) {
    $result.capabilities = $capabilities
    # Operability Ladder surface (OPER-05, DEC-133) -- additive top-level convenience
    # projection of the capabilities' optional operabilityTier / switchboard fields
    # (AOR_CONTRACTS.md Appendix A). NO-DATA: omitted when undeclared -- never
    # inferred, never defaulted to "T0".
    $tier = $capabilities.operabilityTier
    if ($tier -is [string] -and @("T0","T1","T2","T3") -contains $tier) {
        $result.operability_tier = $tier
    }
    $sb = $capabilities.switchboard
    if ($sb -is [System.Array] -and ($sb.Count -eq 0 -or -not ($sb | Where-Object { -not ($_ -is [string] -and $_.StartsWith("aor.")) }))) {
        if ($sb.Count -gt 0) { $result.switchboard = $sb }
    }
}
$result.briefing = $briefing

Write-Output ($result | ConvertTo-Json -Compress -Depth 8)
exit 0
