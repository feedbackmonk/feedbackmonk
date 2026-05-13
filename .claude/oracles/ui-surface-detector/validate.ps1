# ui-surface-detector oracle self-test (Windows PowerShell)

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

foreach ($field in @("surface_kind","confidence","evidence")) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

$validKinds = @("tauri-desktop","electron-desktop","web-spa","react-native","flutter","mobile-native","backend-service","cli-tool","none")
if ($validKinds -notcontains $parsed.surface_kind) {
    Write-Error "FAIL: surface_kind='$($parsed.surface_kind)' is not a valid value"
    exit 1
}

$validConf = @("high","medium","low")
if ($validConf -notcontains $parsed.confidence) {
    Write-Error "FAIL: confidence='$($parsed.confidence)' is not a valid value"
    exit 1
}

Write-Host "PASS: ui-surface-detector oracle validates (surface_kind=$($parsed.surface_kind), confidence=$($parsed.confidence))"
exit 0
