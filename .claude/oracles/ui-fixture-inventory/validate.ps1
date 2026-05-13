# Self-test for ui-fixture-inventory oracle (Windows PowerShell).
# See validate.sh for scenario definitions.

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Oracle = Join-Path $ScriptDir "run.ps1"

if (-not (Test-Path $Oracle)) { Write-Error "FATAL: oracle run.ps1 missing"; exit 1 }

$Pass = 0; $Fail = 0
function Ok($msg)  { Write-Host "PASS: $msg"; $script:Pass++ }
function Bad($msg) { Write-Host "FAIL: $msg" -ForegroundColor Red; $script:Fail++ }

$Sandbox = Join-Path $env:TEMP "fixture-inv-validate-$([System.Guid]::NewGuid().ToString('N').Substring(0,8))"
New-Item -ItemType Directory -Force -Path $Sandbox | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Sandbox ".claude\oracles\ui-surface-detector") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Sandbox ".claude\oracles\ui-fixture-inventory") | Out-Null

try {
    Copy-Item $Oracle (Join-Path $Sandbox ".claude\oracles\ui-fixture-inventory\run.ps1")

    # Stub surface oracle
    $stubPath = Join-Path $Sandbox ".claude\oracles\ui-surface-detector\run.ps1"
    @'
$sk = if ($env:SURFACE_KIND_STUB) { $env:SURFACE_KIND_STUB } else { "none" }
Write-Output ('{"surface_kind":"' + $sk + '","confidence":"high","evidence":["stub"]}')
'@ | Set-Content -Path $stubPath -Encoding UTF8

    function Run-Oracle($surface) {
        Push-Location $Sandbox
        try {
            $env:SURFACE_KIND_STUB = $surface
            $out = & powershell -NoProfile -File ".\.claude\oracles\ui-fixture-inventory\run.ps1"
            return $out
        } finally {
            Pop-Location
            $env:SURFACE_KIND_STUB = $null
        }
    }

    # Scenario 1: no surface
    $out = Run-Oracle "none"
    if ($out -match '"has_fixtures":false' -and $out -match '"briefing":""') {
        Ok "Scenario 1: no-surface emits empty briefing"
    } else { Bad "Scenario 1: got $out" }

    # Scenario 2: surface, no fixtures
    $out = Run-Oracle "tauri-desktop"
    if ($out -match '"has_fixtures":false' -and $out -match "scaffold") {
        Ok "Scenario 2: surface-no-fixtures emits scaffold briefing"
    } else { Bad "Scenario 2: got $out" }

    # Scenario 3: surface + fixtures
    New-Item -ItemType Directory -Force -Path (Join-Path $Sandbox "tests\fixtures") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $Sandbox "tests\smoke") | Out-Null
    "// fixture stub" | Set-Content -Path (Join-Path $Sandbox "tests\fixtures\example-smoke.ts")
    "// smoke stub"   | Set-Content -Path (Join-Path $Sandbox "tests\smoke\example.spec.ts")

    $out = Run-Oracle "tauri-desktop"
    if ($out -match '"has_fixtures":true' -and $out -match '"briefing":""') {
        Ok "Scenario 3: surface-with-fixtures emits empty briefing"
    } else { Bad "Scenario 3: got $out" }

} finally {
    Remove-Item -Recurse -Force $Sandbox -ErrorAction SilentlyContinue
}

Write-Host "---"
Write-Host "Passed: $Pass / $($Pass + $Fail)"
if ($Fail -eq 0) { exit 0 } else { exit 1 }
