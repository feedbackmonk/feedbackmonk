# handoff-retention oracle self-test (Windows PowerShell)
#
# Same test plan as validate.sh:
#   T1. --gc deletes briefs older than threshold (no .KEEP file).
#   T2. --gc does NOT delete briefs younger than threshold.
#   T3. Sibling <file>.KEEP exempts brief from sweep regardless of age.
#   T4. --gc is idempotent: re-running on post-sweep dir sweeps zero.
#   T5. --gc emits JSON summary with all expected fields.
#   T6. .claude/config.json handoffRetention.threshold honored (numeric and PnD forms).
#   T7. --gc-cheap is silent on success.
#   T8. _summary.jsonl receives one valid JSON line per swept brief BEFORE delete (SWEEP-08).
#   T9. Malformed config falls back to default 30 days with threshold_source=default.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path

$pass = 0
$fail = 0
function Test-Pass { param([string]$m) Write-Host "PASS: $m"; $script:pass++ }
function Test-Fail { param([string]$m) Write-Host "FAIL: $m" -ForegroundColor Red; $script:fail++ }

# =============================================================================
# Phase 1 — briefing path against the real handoff dir
# =============================================================================

$output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir "run.ps1") 2>&1
$outputStr = ($output | Out-String).Trim()

try {
    $parsed = $outputStr | ConvertFrom-Json -ErrorAction Stop
    foreach ($f in @("swept","retained_keep_pinned","retained_under_ttl","threshold_days","threshold_source","briefing")) {
        if (-not ($parsed.PSObject.Properties.Name -contains $f)) {
            Test-Fail "briefing: missing schema field '$f'"
        }
    }
    if ($parsed.threshold_days -eq 30) {
        Test-Pass "briefing: threshold_days=30 (default)"
    } else {
        Test-Fail "briefing: threshold_days != 30 (got $($parsed.threshold_days))"
    }
} catch {
    Test-Fail "briefing: output is not valid JSON: $outputStr"
}

# =============================================================================
# Phase 2 — sweep semantics in a sandbox
# =============================================================================

$sandbox = Join-Path $env:TEMP ("handoff-ret-" + (Get-Random))
New-Item -ItemType Directory -Path $sandbox -Force | Out-Null

try {
    $handoff = Join-Path $sandbox ".claude/handoff"
    $oracleSandboxDir = Join-Path $sandbox ".claude/oracles/handoff-retention"
    New-Item -ItemType Directory -Path $handoff -Force | Out-Null
    New-Item -ItemType Directory -Path $oracleSandboxDir -Force | Out-Null

    Copy-Item (Join-Path $oracleDir "run.ps1")     -Destination $oracleSandboxDir
    Copy-Item (Join-Path $oracleDir "oracle.json") -Destination $oracleSandboxDir

    $now = (Get-Date).ToUniversalTime()

    # Build fixture briefs with explicit mtimes.
    function Make-AgedBrief {
        param([string]$DirPath, [string]$Name, [int]$DaysOld)
        $p = Join-Path $DirPath $Name
        Set-Content -Path $p -Value "# Aged handoff brief $DaysOld days`n`nRead first: docs/specs/SPECIFICATION.md" -Encoding utf8
        $stamp = $now.AddDays(-1 * $DaysOld)
        (Get-Item $p).LastWriteTimeUtc = $stamp
    }

    Make-AgedBrief -DirPath $handoff -Name "handoff-aged-A.md"      -DaysOld 60
    Make-AgedBrief -DirPath $handoff -Name "handoff-aged-B.md"      -DaysOld 45
    Make-AgedBrief -DirPath $handoff -Name "handoff-recent-A.md"    -DaysOld 5
    Make-AgedBrief -DirPath $handoff -Name "handoff-recent-B.md"    -DaysOld 1
    # Malformed: no .md extension
    Make-AgedBrief -DirPath $handoff -Name "handoff-no-extension"   -DaysOld 60

    # Pin aged-B
    Set-Content -Path (Join-Path $handoff "handoff-aged-B.md.KEEP") -Value "Keep because: T3 fixture" -Encoding utf8

    # ---- T1+T2+T3+T5+T8: --gc -----------------------------------------------
    Push-Location $sandbox
    try {
        $gcOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --gc 2>&1) | Out-String
    } finally { Pop-Location }
    $gcOut = $gcOut.Trim()
    Write-Host "[--gc summary]: $gcOut"

    try {
        $gcJson = $gcOut | ConvertFrom-Json -ErrorAction Stop
        foreach ($f in @("swept","before","after","threshold","thresholdSource","summarized")) {
            if (-not ($gcJson.PSObject.Properties.Name -contains $f)) {
                Test-Fail "T5: --gc summary missing field '$f'"
            }
        }
        if ($gcJson.swept -eq 1) { Test-Pass "T1: --gc swept=1 (only handoff-aged-A.md)" } else { Test-Fail "T1: --gc swept != 1 (got $($gcJson.swept))" }
        if ($gcJson.before -eq 4) { Test-Pass "T5: --gc before=4 (handoff-*.md only)" } else { Test-Fail "T5: --gc before != 4 (got $($gcJson.before))" }
        if ($gcJson.after -eq 3) { Test-Pass "T5: --gc after=3" } else { Test-Fail "T5: --gc after != 3 (got $($gcJson.after))" }
        if ($gcJson.summarized -eq 1) { Test-Pass "T5: --gc summarized=1" } else { Test-Fail "T5: --gc summarized != 1 (got $($gcJson.summarized))" }
    } catch {
        Test-Fail "T5: --gc output is not valid JSON: $gcOut"
    }

    if (Test-Path (Join-Path $handoff "handoff-recent-A.md")) { Test-Pass "T2: handoff-recent-A.md survived sweep" } else { Test-Fail "T2: handoff-recent-A.md was deleted" }
    if (Test-Path (Join-Path $handoff "handoff-recent-B.md")) { Test-Pass "T2: handoff-recent-B.md survived sweep" } else { Test-Fail "T2: handoff-recent-B.md was deleted" }
    if (Test-Path (Join-Path $handoff "handoff-aged-B.md")) { Test-Pass "T3: KEEP-pinned brief survived sweep" } else { Test-Fail "T3: KEEP-pinned brief was deleted" }
    if (-not (Test-Path (Join-Path $handoff "handoff-aged-A.md"))) { Test-Pass "T1: aged-A.md deleted" } else { Test-Fail "T1: aged-A.md was NOT deleted" }
    if (Test-Path (Join-Path $handoff "handoff-no-extension")) { Test-Pass "Defensive: handoff-no-extension preserved" } else { Test-Fail "Defensive: handoff-no-extension was deleted" }

    # T8: _summary.jsonl
    $summaryFile = Join-Path $handoff "_summary.jsonl"
    if (Test-Path $summaryFile) {
        $lines = @(Get-Content $summaryFile)
        if ($lines.Count -eq 1) { Test-Pass "T8: _summary.jsonl has exactly 1 line" } else { Test-Fail "T8: _summary.jsonl has $($lines.Count) lines, expected 1" }

        try {
            $summaryJson = $lines[0] | ConvertFrom-Json -ErrorAction Stop
            Test-Pass "T8: _summary.jsonl line is valid JSON"
            foreach ($f in @("file","swept_at","age_days","brief_first_line")) {
                if (-not ($summaryJson.PSObject.Properties.Name -contains $f)) {
                    Test-Fail "T8: _summary.jsonl missing field '$f'"
                }
            }
            if ($summaryJson.file -match 'handoff-aged-A.md') { Test-Pass "T8: _summary.jsonl file path matches" } else { Test-Fail "T8: _summary.jsonl file mismatch: $($summaryJson.file)" }
        } catch {
            Test-Fail "T8: _summary.jsonl line is not valid JSON: $($lines[0])"
        }
    } else {
        Test-Fail "T8: _summary.jsonl was not created"
    }

    # ---- T4: idempotence ----------------------------------------------------
    Push-Location $sandbox
    try {
        $gcOut2 = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --gc 2>&1) | Out-String
    } finally { Pop-Location }
    $gcOut2 = $gcOut2.Trim()
    Write-Host "[second --gc]: $gcOut2"
    try {
        $gcJson2 = $gcOut2 | ConvertFrom-Json -ErrorAction Stop
        if ($gcJson2.swept -eq 0) { Test-Pass "T4: idempotence (second --gc swept=0)" } else { Test-Fail "T4: idempotence violated (swept=$($gcJson2.swept))" }
    } catch {
        Test-Fail "T4: idempotence output not JSON: $gcOut2"
    }

    # ---- T6: config.json threshold honored (numeric form) ------------------
    Set-Content -Path (Join-Path $sandbox ".claude/config.json") -Value '{"handoffRetention":{"threshold":3}}' -Encoding utf8

    Push-Location $sandbox
    try {
        $gcOut3 = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --gc 2>&1) | Out-String
    } finally { Pop-Location }
    $gcOut3 = $gcOut3.Trim()
    Write-Host "[--gc with config 3d]: $gcOut3"
    try {
        $gcJson3 = $gcOut3 | ConvertFrom-Json -ErrorAction Stop
        if ($gcJson3.swept -eq 1) { Test-Pass "T6: 3d threshold sweeps recent-A" } else { Test-Fail "T6: 3d threshold did not sweep (swept=$($gcJson3.swept))" }
        if ($gcJson3.thresholdSource -eq "config") { Test-Pass "T6: thresholdSource=config" } else { Test-Fail "T6: thresholdSource != config (got $($gcJson3.thresholdSource))" }
        if ($gcJson3.threshold -eq "P3D") { Test-Pass "T6: threshold=P3D" } else { Test-Fail "T6: threshold != P3D (got $($gcJson3.threshold))" }
    } catch {
        Test-Fail "T6: --gc with config output not JSON: $gcOut3"
    }

    # T6 PnD form (default mode)
    Set-Content -Path (Join-Path $sandbox ".claude/config.json") -Value '{"handoffRetention":{"threshold":"P1D"}}' -Encoding utf8
    Push-Location $sandbox
    try {
        $defOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") 2>&1) | Out-String
    } finally { Pop-Location }
    $defOut = $defOut.Trim()
    Write-Host "[default with PnD config 1d]: $defOut"
    try {
        $defJson = $defOut | ConvertFrom-Json -ErrorAction Stop
        if ($defJson.threshold_days -eq 1) { Test-Pass "T6: PnD form parses (threshold_days=1)" } else { Test-Fail "T6: PnD form did not parse (got $($defJson.threshold_days))" }
        if ($defJson.threshold_source -eq "config") { Test-Pass "T6: PnD form sets source=config" } else { Test-Fail "T6: PnD form source mismatch" }
    } catch {
        Test-Fail "T6: PnD-form default output not JSON: $defOut"
    }

    # ---- T7: --gc-cheap silent ---------------------------------------------
    Push-Location $sandbox
    try {
        $cheapOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --gc-cheap 2>&1) | Out-String
    } finally { Pop-Location }
    $cheapOut = $cheapOut.Trim()
    if ([string]::IsNullOrWhiteSpace($cheapOut)) {
        Test-Pass "T7: --gc-cheap silent on success"
    } else {
        Test-Fail "T7: --gc-cheap emitted output: $cheapOut"
    }

    # ---- T9: malformed config falls back to default ------------------------
    Set-Content -Path (Join-Path $sandbox ".claude/config.json") -Value 'this is not json {{{' -Encoding utf8
    Push-Location $sandbox
    try {
        $badOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") 2>&1) | Out-String
    } finally { Pop-Location }
    $badOut = $badOut.Trim()
    Write-Host "[default with malformed config snippet]: $($badOut.Substring(0, [Math]::Min(200, $badOut.Length)))"
    try {
        $badJson = $badOut | ConvertFrom-Json -ErrorAction Stop
        if ($badJson.threshold_days -eq 30) { Test-Pass "T9: malformed config -> default 30 days" } else { Test-Fail "T9: malformed config did not fall back to 30d (got $($badJson.threshold_days))" }
        if ($badJson.threshold_source -eq "default") { Test-Pass "T9: malformed config -> threshold_source=default" } else { Test-Fail "T9: malformed config source mismatch" }
    } catch {
        Test-Fail "T9: malformed config output not JSON: $badOut"
    }

} finally {
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

# =============================================================================
Write-Host "----"
Write-Host "Total: PASS=$pass  FAIL=$fail"
if ($fail -gt 0) { exit 1 }
exit 0
