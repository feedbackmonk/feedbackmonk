# aria-status oracle self-test (Windows PowerShell)

$ErrorActionPreference = "Stop"
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path

try {
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir "run.ps1") 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Error "FAIL: run.ps1 exited non-zero"
        exit 1
    }
} catch {
    Write-Error "FAIL: run.ps1 threw: $_"
    exit 1
}

[string]$outputStr = if ($output -is [array]) { $output -join "" } else { "$output" }
$outputStr = $outputStr.Trim()

try {
    $parsed = $outputStr | ConvertFrom-Json
} catch {
    Write-Error "FAIL: output is not valid JSON: $_"
    Write-Error "Output: $outputStr"
    exit 1
}

foreach ($field in @("surface_present","exposure_mechanism","endpoint_reachable","foundation_layer","briefing")) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

foreach ($flField in @("errors","async","navigation")) {
    if (-not ($parsed.foundation_layer.PSObject.Properties.Name -contains $flField)) {
        Write-Error "FAIL: foundation_layer missing '$flField'"
        exit 1
    }
}

$validMech = @("tauri-ipc","http","websocket","file","none")
if ($validMech -notcontains $parsed.exposure_mechanism) {
    Write-Error "FAIL: exposure_mechanism='$($parsed.exposure_mechanism)' not in enum"
    exit 1
}

if ($parsed.briefing.Length -gt 200) {
    Write-Error "FAIL: briefing length $($parsed.briefing.Length) exceeds 200-char cap"
    exit 1
}

if ((-not $parsed.surface_present) -and $parsed.briefing -ne "") {
    Write-Error "FAIL: surface_present=false but briefing is non-empty: '$($parsed.briefing)'"
    exit 1
}

# AOR capabilities (ARIA-17) is OPTIONAL (NO-DATA when the disk companion is absent),
# but when present it must carry a "verbs" key (never an empty-capabilities claim).
if ($parsed.PSObject.Properties.Name -contains "capabilities") {
    if (-not ($parsed.capabilities.PSObject.Properties.Name -contains "verbs")) {
        Write-Error "FAIL: capabilities present but missing 'verbs' (empty-capabilities claim forbidden)"
        exit 1
    }
}

# Operability Ladder fields (OPER-05, DEC-133) are OPTIONAL and additive: valid
# T0..T3 token, aor.*-prefixed switchboard entries, and both require capabilities
# (they are projections of it -- a tier with no capabilities is a fabrication).
if ($parsed.PSObject.Properties.Name -contains "operability_tier") {
    if (@("T0","T1","T2","T3") -notcontains $parsed.operability_tier) {
        Write-Error "FAIL: operability_tier='$($parsed.operability_tier)' not in T0..T3"
        exit 1
    }
    if (-not ($parsed.PSObject.Properties.Name -contains "capabilities")) {
        Write-Error "FAIL: operability_tier present without capabilities (projection without source)"
        exit 1
    }
}
if ($parsed.PSObject.Properties.Name -contains "switchboard") {
    if (-not ($parsed.PSObject.Properties.Name -contains "capabilities")) {
        Write-Error "FAIL: switchboard present without capabilities (projection without source)"
        exit 1
    }
    $bad = @($parsed.switchboard | Where-Object { -not ($_ -is [string] -and $_.StartsWith("aor.")) })
    if ($bad.Count -gt 0) {
        Write-Error "FAIL: switchboard contains non-aor.* entries: $($bad -join ', ')"
        exit 1
    }
}

Write-Host "PASS: aria-status oracle validates (surface_present=$($parsed.surface_present), exposure_mechanism=$($parsed.exposure_mechanism), briefing_len=$($parsed.briefing.Length))"
exit 0
