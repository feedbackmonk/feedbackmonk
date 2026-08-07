# machine-quiescence oracle self-test (Windows PowerShell)
#
# Twin of validate.sh: verifies the oracle runs, emits schema-conforming JSON,
# and that its exit code agrees with its own `verdict` field.
#
# Full behavioural coverage (verdict classes, falsifiability cells, twin parity)
# lives in scripts/smoke-tests/machine-quiescence-smoke.sh.

$ErrorActionPreference = "Continue"
$OracleDir = $PSScriptRoot
$Pass = 0; $Fail = 0
function Ok($m)  { Write-Output "PASS: $m"; $script:Pass++ }
function Bad($m) { Write-Error  "FAIL: $m"; $script:Fail++ }

# --- 1. Runs, and its exit code matches its verdict -------------------------
$out = & (Join-Path $OracleDir "run.ps1") 2>&1 | Out-String
$rc = $LASTEXITCODE
if ($rc -notin @(0, 1, 2, 3)) { Bad "run.ps1 exited $rc (expected 0|1|2|3)" }

$verdict = ""
if ($out -match '"verdict":"([a-z]+)"') { $verdict = $matches[1] }
$expect = switch ($verdict) { "quiet" { 0 } "noisy" { 1 } "unknown" { 2 } "blocking" { 3 } default { -1 } }
if ($expect -ge 0 -and $rc -eq $expect) {
    Ok "exit code $rc agrees with verdict '$verdict'"
} else {
    Bad "exit code $rc does not agree with verdict '$verdict'"
}

# --- 2. Valid JSON carrying every required field ----------------------------
try {
    $d = $out | ConvertFrom-Json
    $req = @('schemaVersion','verdict','checkedAt','requiredPorts','timingSensitive',
             'counts','residue','blockingReasons','degraded','summary')
    $missing = @($req | Where-Object { -not ($d.PSObject.Properties.Name -contains $_) })
    if ($missing.Count -gt 0) { throw "missing fields: $($missing -join ', ')" }
    if ($d.schemaVersion -ne 1) { throw "schemaVersion must be 1" }
    if ($d.verdict -notin @('quiet','noisy','blocking','unknown')) { throw "bad verdict" }
    $n = $d.counts.devPortListeners + $d.counts.agentSessions + $d.counts.buildProcesses
    if ($n -ne @($d.residue).Count) { throw "counts ($n) disagree with residue length ($(@($d.residue).Count))" }
    # Fail-closed invariant: a declared degradation must never read as quiet.
    if (@($d.degraded).Count -gt 0 -and $d.verdict -ne 'unknown') { throw "degraded but verdict is not unknown" }
    if (@($d.blockingReasons).Count -gt 0 -and $d.verdict -notin @('blocking','unknown')) { throw "blocking reasons but verdict is not blocking" }
    Ok "output is schema-conforming JSON, counts agree with residue, fail-closed invariant holds"
} catch {
    Bad "schema/invariant check failed: $_"
}

# --- 3. Fail-closed on an unreadable probe ---------------------------------
$prev = $env:ULDF_QUIESCE_FIXTURE
$env:ULDF_QUIESCE_FIXTURE = Join-Path $OracleDir "__no_such_fixture__"
$out2 = & (Join-Path $OracleDir "run.ps1") 2>&1 | Out-String
$rc2 = $LASTEXITCODE
if ($null -eq $prev) { Remove-Item Env:\ULDF_QUIESCE_FIXTURE -ErrorAction SilentlyContinue } else { $env:ULDF_QUIESCE_FIXTURE = $prev }
if ($rc2 -eq 2 -and $out2 -match '"verdict":"unknown"') {
    Ok "an unreadable probe yields unknown/exit 2 - never quiet"
} else {
    Bad "unreadable probe yielded exit $rc2"
}

# --- 4. Never actuates ------------------------------------------------------
$hits = Select-String -Path (Join-Path $OracleDir "run.sh"), (Join-Path $OracleDir "run.ps1"), (Join-Path $OracleDir "probe.ps1") `
                      -Pattern 'Stop-Process|taskkill|pkill|kill -' -ErrorAction SilentlyContinue |
        Where-Object { $_.Line -notmatch '^\s*#' }
if ($hits) {
    Bad "a process-termination verb appears in the oracle - it must only report"
} else {
    Ok "no process-termination verb present (never actuates)"
}

Write-Output "machine-quiescence validate: $Pass passed, $Fail failed"
if ($Fail -gt 0) { exit 1 }
exit 0
