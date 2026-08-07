# cloud-sync-coordination-hazard oracle self-test (Windows / PowerShell)
# Twin of validate.sh -- same legs, same intent. See validate.sh for the notes.

$ErrorActionPreference = 'Continue'
$ORACLE_DIR = Split-Path -Parent $MyInvocation.MyCommand.Path

$script:PASS = 0; $script:FAIL = 0
function ok($m)  { Write-Output "PASS: $m"; $script:PASS++ }
function bad($m) { Write-Output "FAIL: $m"; $script:FAIL++ }

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("csch-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $sandbox -Force | Out-Null

function Run-In([string]$dir) {
    Push-Location $dir
    try { return (& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $ORACLE_DIR "run.ps1") 2>$null | Out-String).Trim() }
    finally { Pop-Location }
}
function Mk-Root([string]$p) {
    New-Item -ItemType Directory -Path (Join-Path $p ".claude/collaboration") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $p ".claude/session-state") -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $p ".claude/collaboration/active-sessions.json") -Value '{"sessions":[]}' -Encoding UTF8
}

try {
    # T1 + T2: clean root
    $clean = Join-Path $sandbox "plainlocal\project"
    Mk-Root $clean
    $out = Run-In $clean
    try { $null = $out | ConvertFrom-Json; ok "T1: output is valid JSON" } catch { bad "T1: not valid JSON: $out" }
    $missing = @()
    foreach ($f in @("hosted","provider","root","matched_segment","at_risk_paths","briefing")) {
        if ($out -notmatch "`"$f`"") { $missing += $f }
    }
    if ($missing.Count -eq 0) { ok "T1: frozen schema fields present" } else { bad "T1: missing fields: $($missing -join ',')" }
    if ($out -match '"hosted":false') { ok "T2: clean root -> hosted=false" } else { bad "T2: clean root hosted!=false ($out)" }
    if ($out -match '"briefing":""') { ok "T2: clean root -> empty briefing (line suppressed)" } else { bad "T2: non-empty briefing on clean root ($out)" }

    # T3: synthetic hosted root
    $hosted = Join-Path $sandbox "Users\x\OneDrive\Developer\project"
    Mk-Root $hosted
    $out = Run-In $hosted
    if ($out -match '"hosted":true') { ok "T3: OneDrive segment -> hosted=true" } else { bad "T3: not detected ($out)" }
    if ($out -match '"provider":"OneDrive"') { ok "T3: provider reported" } else { bad "T3: provider missing ($out)" }
    if ($out -match '"briefing":""') { bad "T3: hosted root emitted an EMPTY briefing" } else { ok "T3: hosted root emits a non-empty briefing" }

    # T4: segment-awareness (false-positive guard)
    $near = Join-Path $sandbox "Developer\OneDriveTools\project"
    Mk-Root $near
    $out = Run-In $near
    if ($out -match '"hosted":false') { ok "T4: 'OneDriveTools' does NOT trip (no false positive)" } else { bad "T4: FALSE POSITIVE ($out)" }

    # T5: configurable provider list
    $custom = Join-Path $sandbox "Users\x\MegaSync\project"
    Mk-Root $custom
    $env:ULDF_CLOUD_SYNC_PROVIDERS = "MegaSync"
    $out = Run-In $custom
    $env:ULDF_CLOUD_SYNC_PROVIDERS = $null
    if ($out -match '"hosted":true') { ok "T5: ULDF_CLOUD_SYNC_PROVIDERS extends detection" } else { bad "T5: custom provider not honored ($out)" }
    $out = Run-In $custom
    if ($out -match '"hosted":false') { ok "T5: unlisted provider does not trip by default" } else { bad "T5: MegaSync tripped unconfigured ($out)" }

    # T6: at_risk_paths reflects reality
    $bare = Join-Path $sandbox "Users\x\OneDrive\bare"
    New-Item -ItemType Directory -Path $bare -Force | Out-Null
    $out = Run-In $bare
    if ($out -match '"at_risk_paths":\[\]') { ok "T6: no coordination store -> at_risk_paths empty" } else { bad "T6: expected empty at_risk_paths ($out)" }
    $out = Run-In $hosted
    if ($out -match 'active-sessions.json') { ok "T6: existing coordination surfaces are named" } else { bad "T6: surfaces not named ($out)" }

    # T7: graceful absence -- must still emit valid schema, never fabricate.
    $out = Run-In $sandbox
    if ($out -match '"hosted":') { ok "T7: still emits valid schema outside a git repo" } else { bad "T7: malformed output ($out)" }
} finally {
    $env:ULDF_CLOUD_SYNC_PROVIDERS = $null
    Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}

Write-Output "----"
Write-Output "Total: PASS=$($script:PASS)  FAIL=$($script:FAIL)"
if ($script:FAIL -gt 0) { exit 1 }
exit 0
