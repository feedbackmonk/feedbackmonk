# validate.ps1 -- self-test for the overseer-wake oracle (CSO Phase 1, C1).
# PowerShell parity mirror of validate.sh. Two layers: (1) schema-field presence
# on the real tree; (2) fixture-driven wake/clear behavior in disposable sandbox
# projects. The oracle resolves paths from the current location, so each sandbox
# run sets the process working directory to the sandbox (Push-Location) -- the
# PS analogue of validate.sh's `( cd "$1" && bash "$RUN" )`.

$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()

$OracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Run       = Join-Path $OracleDir "run.ps1"

$script:pass = 0
$script:fail = 0
function Ok([string]$m) { Write-Host "PASS: $m"; $script:pass++ }
function No([string]$m) { Write-Host "FAIL: $m"; $script:fail++ }

# ---- Run the oracle with a given directory as cwd; return stdout string -------
function Invoke-Oracle([string]$Dir) {
    Push-Location $Dir
    try {
        $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Run 2>$null
        if ($null -eq $out) { return "" }
        if ($out -is [array]) { return ($out -join "") }
        return [string]$out
    } finally {
        Pop-Location
    }
}

# ---- Parse helpers (graceful: $null object on bad JSON) ----------------------
function Get-Obj([string]$json) {
    if ([string]::IsNullOrWhiteSpace($json)) { return $null }
    try { return ($json | ConvertFrom-Json -ErrorAction Stop) } catch { return $null }
}

function New-Sandbox {
    $d = Join-Path $env:TEMP ("cso-ow-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $d -Force | Out-Null
    return $d
}
function Write-Registry([string]$Dir, [string]$Json) {
    $col = Join-Path $Dir ".claude/collaboration"
    New-Item -ItemType Directory -Path $col -Force | Out-Null
    # ASCII/UTF8 no-BOM write (registry is plain JSON).
    [System.IO.File]::WriteAllText((Join-Path $col "active-sessions.json"), $Json, (New-Object System.Text.UTF8Encoding($false)))
}

# ---- Layer 1: real-tree schema presence -------------------------------------
$out = Invoke-Oracle (Get-Location).Path
$obj = Get-Obj $out
if ($null -ne $obj) { Ok "L1.1 emits valid JSON on the real tree" } else { No "L1.1 invalid JSON: $out" }
foreach ($f in @("wake", "signals", "summary", "briefing")) {
    if ($null -ne $obj -and ($obj.PSObject.Properties.Name -contains $f)) {
        Ok "L1.2 schema field '$f' present"
    } else {
        No "L1.2 missing field '$f'"
    }
}

# ---- Layer 2a: dead-PID active entry -> wake=true, stall-monitor ------------
$SB = New-Sandbox
Write-Registry $SB '{"registryVersion":2,"sessions":[{"id":"worker-dead","status":"active","claudeShellPid":999999,"sessionRole":"orchestrated-worker"}],"closed":[]}'
$O = Invoke-Oracle $SB
$obj = Get-Obj $O
if ($null -ne $obj -and ([string]$obj.wake) -eq "True") { Ok "L2a.1 dead-PID active entry -> wake=true" } else { No "L2a.1 expected wake=true, got: $O" }
if ($O -match '"detectorId":"stall-monitor"') { Ok "L2a.2 emits a stall-monitor signal" } else { No "L2a.2 no stall-monitor signal: $O" }
if ($O -match 'worker-dead') { Ok "L2a.3 signal names the dead session" } else { No "L2a.3 dead session not named: $O" }
$B = if ($null -ne $obj) { [string]$obj.briefing } else { "" }
if (-not [string]::IsNullOrEmpty($B)) { Ok "L2a.4 briefing non-empty when wake=true" } else { No "L2a.4 briefing empty when wake=true" }
Remove-Item -Recurse -Force $SB -ErrorAction SilentlyContinue

# ---- Layer 2b: clean registry -> wake=false, briefing empty -----------------
$SB = New-Sandbox
Write-Registry $SB '{"registryVersion":2,"sessions":[],"closed":[]}'
$O = Invoke-Oracle $SB
$obj = Get-Obj $O
if ($null -ne $obj -and ([string]$obj.wake) -eq "False") { Ok "L2b.1 empty registry -> wake=false" } else { No "L2b.1 expected wake=false, got: $O" }
$B = if ($null -ne $obj) { [string]$obj.briefing } else { "" }
if ([string]::IsNullOrEmpty($B)) { Ok "L2b.2 briefing empty when wake=false" } else { No "L2b.2 briefing should be empty: $B" }
# never a false-clear CLAIM: summary must mention sources observed or NO-DATA, not "all clear"
$S = if ($null -ne $obj) { [string]$obj.summary } else { "" }
if ($S -match '(?i)no airspace signals|NO-DATA') { Ok "L2b.3 honest no-signal summary (no fabricated all-clear)" } else { No "L2b.3 summary suspicious: $S" }
Remove-Item -Recurse -Force $SB -ErrorAction SilentlyContinue

# ---- Layer 2c: live-PID active entry -> NOT a stall ------------------------
# Use this validate.ps1 process's own Windows PID ($PID) -- the oracle's probe is
# Get-Process (Windows PID namespace), so $PID is guaranteed alive there.
$SB = New-Sandbox
Write-Registry $SB ('{"registryVersion":2,"sessions":[{"id":"worker-live","status":"active","claudeShellPid":' + $PID + ',"sessionRole":"orchestrated-worker"}],"closed":[]}')
$O = Invoke-Oracle $SB
if ($O -match '"detectorId":"stall-monitor"') { No "L2c.1 live PID wrongly flagged as stall: $O" } else { Ok "L2c.1 live-PID active entry is NOT a stall" }
Remove-Item -Recurse -Force $SB -ErrorAction SilentlyContinue

# ---- Layer 2d: touches.json multi-claimant screen, liveness-gated -----------
# DEC-215 (DEFER-039 phantom fix): claimants only count when LIVE — backed by
# the collab dir's workers/<id>/shell.pid or an active registry entry. This
# process's own $PID serves as the live PID (Get-Process namespace).
function New-TouchesSandbox {
    param([string]$PidA, [string]$PidB)
    $sb = New-Sandbox
    $ft = Join-Path $sb ".claude/collaboration/collab-test/file-tracking"
    New-Item -ItemType Directory -Path $ft -Force | Out-Null
    $touches = '{"schemaVersion":"1.0","files":{"src/main.ts":{"agents":["CLAUDE-A","CLAUDE-B"],"action":"MODIFIED"},"src/solo.ts":{"agents":["CLAUDE-A"]}}}'
    [System.IO.File]::WriteAllText((Join-Path $ft "touches.json"), $touches, (New-Object System.Text.UTF8Encoding($false)))
    foreach ($pair in @(@("CLAUDE-A", $PidA), @("CLAUDE-B", $PidB))) {
        if ($pair[1]) {
            $wd = Join-Path $sb (".claude/collaboration/collab-test/workers/" + $pair[0])
            New-Item -ItemType Directory -Path $wd -Force | Out-Null
            [System.IO.File]::WriteAllText((Join-Path $wd "shell.pid"), [string]$pair[1], (New-Object System.Text.UTF8Encoding($false)))
        }
    }
    Write-Registry $sb '{"registryVersion":2,"sessions":[],"closed":[]}'
    return $sb
}

# Dead-claimants shape (no liveness evidence) -> NO signal (the Feb-dir phantom)
$SB = New-TouchesSandbox -PidA "" -PidB ""
$O = Invoke-Oracle $SB
if ($O -match '"detectorId":"touches-conflict"') { No "L2d.0 dead claimants wrongly signalled (phantom): $O" } else { Ok "L2d.0 claimants without liveness evidence -> NO signal (DEC-215)" }
Remove-Item -Recurse -Force $SB -ErrorAction SilentlyContinue

# Two LIVE claimants -> signal fires (detection preserved)
$SB = New-TouchesSandbox -PidA ([string]$PID) -PidB ([string]$PID)
$O = Invoke-Oracle $SB
if ($O -match '"detectorId":"touches-conflict"') { Ok "L2d.1 multi-claimant touches path (live claimants) -> touches-conflict signal" } else { No "L2d.1 no touches-conflict signal: $O" }
if ($O -match 'src/main\.ts') { Ok "L2d.2 names the contested path" } else { No "L2d.2 contested path not named: $O" }
if ($O -match 'src/solo\.ts') { No "L2d.3 single-claimant path wrongly flagged" } else { Ok "L2d.3 single-claimant path NOT flagged" }
Remove-Item -Recurse -Force $SB -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "================================================================"
Write-Host "overseer-wake validate (ps): $script:pass passed, $script:fail failed"
Write-Host "================================================================"
if ($script:fail -eq 0) { exit 0 } else { exit 1 }
