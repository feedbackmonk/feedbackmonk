# pid-orphan-detector oracle self-test (Windows PowerShell)
#
# Phase 1: validate the read-only briefing path.
# Phase 2: validate --gc and --gc-cheap sweep semantics in a sandbox.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()

$OracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$global:Pass = 0
$global:Fail = 0
function Pass-Test { param([string]$Msg) Write-Host "PASS: $Msg"; $global:Pass++ }
function Fail-Test { param([string]$Msg) Write-Host "FAIL: $Msg" -ForegroundColor Red; $global:Fail++ }

function Get-LiveBashPid {
    try {
        $p = Get-Process -Name bash -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($p) { return $p.Id }
    } catch { }
    return $PID  # this PowerShell's own PID -- alive by definition
}

# =============================================================================
# Phase 1 -- briefing path
# =============================================================================
$out = & powershell.exe -NoProfile -File (Join-Path $OracleDir "run.ps1") 2>&1
$outStr = ($out -join "`n")
try {
    $parsed = $outStr | ConvertFrom-Json -ErrorAction Stop
    Pass-Test "Phase1: briefing emits valid JSON"
} catch {
    Fail-Test "Phase1: briefing output is not valid JSON ($outStr)"
}
foreach ($f in @('swept','alive','malformed','briefing')) {
    if ($outStr -match "`"$f`"") {
        Pass-Test "Phase1: briefing has field '$f'"
    } else {
        Fail-Test "Phase1: briefing missing field '$f'"
    }
}

# =============================================================================
# Phase 2 -- sandbox sweep semantics
# =============================================================================
$sandbox  = Join-Path $env:TEMP ("pidoracle-" + [guid]::NewGuid().ToString())
$exec     = Join-Path $sandbox "ltads\execution"
$oracleDst = Join-Path $sandbox ".claude\oracles\pid-orphan-detector"
$libDst   = Join-Path $sandbox ".claude\scripts\lib"
New-Item -ItemType Directory -Path $exec -Force | Out-Null
New-Item -ItemType Directory -Path $oracleDst -Force | Out-Null
New-Item -ItemType Directory -Path $libDst -Force | Out-Null

Copy-Item (Join-Path $OracleDir "run.ps1")     (Join-Path $oracleDst "run.ps1")
Copy-Item (Join-Path $OracleDir "oracle.json") (Join-Path $oracleDst "oracle.json")

$libCandidates = @(
    (Join-Path $OracleDir "..\..\scripts\lib\pid-liveness.ps1"),
    (Join-Path $OracleDir "..\..\..\claude-template\scripts\lib\pid-liveness.ps1")
)
foreach ($cand in $libCandidates) {
    if (Test-Path $cand) {
        Copy-Item $cand (Join-Path $libDst "pid-liveness.ps1")
        break
    }
}

$alivePid = Get-LiveBashPid
$deadPid  = 999999

Set-Content -LiteralPath (Join-Path $exec "worker-shell-20260508-100000-001.pid") -Value "$alivePid" -Encoding ASCII
Set-Content -LiteralPath (Join-Path $exec "worker-shell-20260101-100000-002.pid") -Value "$deadPid" -Encoding ASCII
Set-Content -LiteralPath (Join-Path $exec "worker-shell-20260101-100000-bogus.pid") -Value "garbage" -Encoding ASCII

Push-Location $sandbox
try {
    # T1: default mode lists, does not delete
    $defaultOut = (& powershell.exe -NoProfile -File ".claude\oracles\pid-orphan-detector\run.ps1" 2>&1) -join "`n"
    Write-Host "[default]: $defaultOut"
    if ($defaultOut -match "`"referenced_pid`":$alivePid")    { Pass-Test "T1: alive PID surfaces in alive[]" } else { Fail-Test "T1: alive PID missing ($defaultOut)" }
    if ($defaultOut -match "`"referenced_pid`":$deadPid")     { Pass-Test "T1: dead PID surfaces in swept[]" } else { Fail-Test "T1: dead PID missing ($defaultOut)" }
    if ($defaultOut -match "worker-shell-20260101-100000-bogus") { Pass-Test "T1: malformed file surfaces in malformed[]" } else { Fail-Test "T1: malformed missing" }
    if (Test-Path (Join-Path $exec "worker-shell-20260101-100000-002.pid")) { Pass-Test "T1: default mode does NOT delete dead-PID file" } else { Fail-Test "T1: default mode deleted dead-PID file" }

    # T2: --gc deletes only dead PIDs
    $gcOut = (& powershell.exe -NoProfile -File ".claude\oracles\pid-orphan-detector\run.ps1" "--gc" 2>&1) -join "`n"
    Write-Host "[--gc]: $gcOut"
    if (Test-Path (Join-Path $exec "worker-shell-20260508-100000-001.pid"))   { Pass-Test "T2: alive-PID file preserved by --gc" } else { Fail-Test "T2: alive-PID file deleted" }
    if (-not (Test-Path (Join-Path $exec "worker-shell-20260101-100000-002.pid"))) { Pass-Test "T2: dead-PID file deleted by --gc" } else { Fail-Test "T2: dead-PID file NOT deleted" }

    # T3: malformed file preserved
    if (Test-Path (Join-Path $exec "worker-shell-20260101-100000-bogus.pid")) { Pass-Test "T3: malformed .pid preserved (failure-open)" } else { Fail-Test "T3: malformed .pid was deleted" }

    # T5: _pid-summary.jsonl
    $summary = Join-Path $exec "_pid-summary.jsonl"
    if (Test-Path $summary) {
        $lines = (Get-Content -LiteralPath $summary -ErrorAction Stop) | Where-Object { $_ -ne "" }
        if (@($lines).Count -eq 1) { Pass-Test "T5: _pid-summary.jsonl has 1 line" } else { Fail-Test "T5: $($lines.Count) lines, expected 1" }
        $line = $lines | Select-Object -First 1
        try {
            $obj = $line | ConvertFrom-Json -ErrorAction Stop
            Pass-Test "T5: summary line is valid JSON"
            foreach ($f in @('pid_file','referenced_pid','liveness_at_sweep','mtime','sweptAt')) {
                if ($obj.PSObject.Properties.Name -contains $f) {
                    # OK
                } else {
                    Fail-Test "T5: summary missing field '$f'"
                }
            }
            if ($obj.referenced_pid -eq $deadPid) { Pass-Test "T5: summary references dead PID $deadPid" } else { Fail-Test "T5: summary referenced_pid mismatch" }
            if ($obj.liveness_at_sweep -eq $false) { Pass-Test "T5: summary liveness_at_sweep=false" } else { Fail-Test "T5: summary liveness_at_sweep mismatch" }
        } catch {
            Fail-Test "T5: summary line not valid JSON ($line)"
        }
    } else {
        Fail-Test "T5: _pid-summary.jsonl was not created"
    }

    # T4: idempotence
    $gcOut2 = (& powershell.exe -NoProfile -File ".claude\oracles\pid-orphan-detector\run.ps1" "--gc" 2>&1) -join "`n"
    if ($gcOut2 -match '"swept":\[\]') { Pass-Test "T4: idempotence (second --gc swept[] empty)" } else { Fail-Test "T4: second --gc not idempotent ($gcOut2)" }

    # T6: --gc-cheap silent + performs sweep
    Set-Content -LiteralPath (Join-Path $exec "worker-shell-20260102-100000-003.pid") -Value "$deadPid" -Encoding ASCII
    $cheapOut = (& powershell.exe -NoProfile -File ".claude\oracles\pid-orphan-detector\run.ps1" "--gc-cheap" 2>&1) -join "`n"
    if ([string]::IsNullOrWhiteSpace($cheapOut)) { Pass-Test "T6: --gc-cheap silent on success" } else { Fail-Test "T6: --gc-cheap emitted output: $cheapOut" }
    if (-not (Test-Path (Join-Path $exec "worker-shell-20260102-100000-003.pid"))) { Pass-Test "T6: --gc-cheap performed the sweep" } else { Fail-Test "T6: --gc-cheap did not delete dead-PID file" }
} finally {
    Pop-Location
}

# T7: empty exec dir
$emptySandbox = Join-Path $env:TEMP ("pidoracle-empty-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path (Join-Path $emptySandbox "ltads\execution") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $emptySandbox ".claude\oracles\pid-orphan-detector") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $emptySandbox ".claude\scripts\lib") -Force | Out-Null
Copy-Item (Join-Path $OracleDir "run.ps1")     (Join-Path $emptySandbox ".claude\oracles\pid-orphan-detector\run.ps1")
Copy-Item (Join-Path $OracleDir "oracle.json") (Join-Path $emptySandbox ".claude\oracles\pid-orphan-detector\oracle.json")
foreach ($cand in $libCandidates) {
    if (Test-Path $cand) {
        Copy-Item $cand (Join-Path $emptySandbox ".claude\scripts\lib\pid-liveness.ps1")
        break
    }
}

Push-Location $emptySandbox
try {
    $emptyOut = (& powershell.exe -NoProfile -File ".claude\oracles\pid-orphan-detector\run.ps1" 2>&1) -join "`n"
    if ($emptyOut -match '"briefing":""') { Pass-Test "T7: empty exec -> empty briefing" } else { Fail-Test "T7: empty exec briefing not empty ($emptyOut)" }
    if ($emptyOut -match '"swept":\[\]') { Pass-Test "T7: empty exec -> swept[] empty" } else { Fail-Test "T7: empty exec swept[] non-empty ($emptyOut)" }
} finally {
    Pop-Location
    Remove-Item -Recurse -Force -LiteralPath $emptySandbox -ErrorAction SilentlyContinue
}
Remove-Item -Recurse -Force -LiteralPath $sandbox -ErrorAction SilentlyContinue

Write-Host "----"
Write-Host "Total: PASS=$($global:Pass)  FAIL=$($global:Fail)"
if ($global:Fail -gt 0) { exit 1 }
exit 0
