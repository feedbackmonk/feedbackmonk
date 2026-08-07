# probandurgy-footprint oracle (Windows PowerShell)
# Verification Oracle: validates per-project manifest of declared Probandurgy
# mechanisms against actual build-config gates.
#
# Thin wrapper: all logic lives in run.py for cross-platform parity with run.sh.
# Output: single JSON object matching FOOTPRINT-04 frozen output schema.
#
# Spec: SPECIFICATION.md FOOTPRINT-04; DEC-62/63; DISC-FOOTPRINT-01

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$PyImpl = Join-Path $ScriptDir "run.py"

# Probe-verified Python interpreter selection. Microsoft Store python stub
# returns 0 from Get-Command but errors on real use -- exec a no-op import
# first to confirm the interpreter actually runs.
$Python = $null
foreach ($cand in @('python3', 'python', 'py')) {
    $cmd = Get-Command $cand -ErrorAction SilentlyContinue
    if ($null -eq $cmd) { continue }
    try {
        if ($cand -eq 'py') {
            & $cand -3 -c "import sys" 2>$null | Out-Null
        } else {
            & $cand -c "import sys" 2>$null | Out-Null
        }
        if ($LASTEXITCODE -eq 0) {
            $Python = $cand
            break
        }
    } catch {
        continue
    }
}

if (-not $Python) {
    # Fail-soft: emit graceful-absent shape with a manifest-malformed violation
    # naming the missing dependency so Phase 11 can surface it without blocking.
    Write-Output '{"schemaVersion":"1","manifest_present":false,"mechanisms_declared":[],"violations":[{"mechanism":null,"kind":"manifest-malformed","evidence":"python interpreter","remediation":"Install Python 3 (python3, python, or py -3 on PATH). The probandurgy-footprint oracle uses Python for cross-platform JSON parsing parity with run.sh."}],"advisory":true}'
    exit 0
}

if ($Python -eq 'py') {
    & py -3 $PyImpl @args
} else {
    & $Python $PyImpl @args
}
exit $LASTEXITCODE
