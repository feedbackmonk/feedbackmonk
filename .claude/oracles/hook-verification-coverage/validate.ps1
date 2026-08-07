# Self-test for hook-verification-coverage (Windows PowerShell).
# Mirrors validate.sh scenarios 1-5.
$ErrorActionPreference = 'SilentlyContinue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$run = Join-Path $here 'run.ps1'
$fails = 0
function Check([string]$label, [string]$expect, [string]$output) {
    if ($output -like "*$expect*") {
        Write-Output "PASS: $label"
    } else {
        Write-Output "FAIL: $label -- expected '$expect' in: $output"
        $script:fails++
    }
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("hvc-validate-" + [System.Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path (Join-Path $tmp 'claude-template/hooks/tests') -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $tmp 'claude-template/scripts/csi-tests') -Force | Out-Null
try {
    $settings = Join-Path $tmp 'claude-template/settings.json'

    # 1. new uncovered hook -> fail
    Set-Content -LiteralPath $settings -Value '{"hooks":{"X":[{"command":"pwsh hooks/brand-new-gate.ps1"}]}}'
    $env:HVC_ROOT = $tmp
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String
    Check 'new uncovered hook fails' '"status":"fail"' $out
    Check 'new uncovered hook named' 'brand-new-gate' $out

    # 2. covered via tests/ file -> pass
    New-Item -ItemType File -Path (Join-Path $tmp 'claude-template/hooks/tests/brand-new-gate.test.ps1') -Force | Out-Null
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String
    Check 'tests/-file coverage passes' '"status":"pass"' $out

    # 3. covered via smoke reference -> pass
    Remove-Item (Join-Path $tmp 'claude-template/hooks/tests/brand-new-gate.test.ps1') -Force
    Set-Content -LiteralPath (Join-Path $tmp 'claude-template/scripts/csi-tests/gate-smoke.sh') -Value 'exercises brand-new-gate.sh end to end'
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String
    Check 'smoke-reference coverage passes' '"status":"pass"' $out

    # 4. baseline hook uncovered -> warn
    Set-Content -LiteralPath $settings -Value '{"hooks":{"X":[{"command":"pwsh hooks/pre-compact.ps1"}]}}'
    Remove-Item (Join-Path $tmp 'claude-template/scripts/csi-tests/gate-smoke.sh') -Force
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String
    Check 'baseline hook warns' '"status":"warn"' $out

    # 5. no settings -> graceful absence
    Remove-Item $settings -Force
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String
    Check 'graceful absence' '"applicable":false' $out
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    Remove-Item Env:HVC_ROOT -ErrorAction SilentlyContinue
}

if ($fails -eq 0) {
    Write-Output 'hook-verification-coverage validate: ALL PASS'
    exit 0
} else {
    Write-Output "hook-verification-coverage validate: $fails FAILURE(S)"
    exit 1
}
