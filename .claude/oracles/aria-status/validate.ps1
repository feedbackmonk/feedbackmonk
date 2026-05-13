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

Write-Host "PASS: aria-status oracle validates (surface_present=$($parsed.surface_present), exposure_mechanism=$($parsed.exposure_mechanism), briefing_len=$($parsed.briefing.Length))"
exit 0
