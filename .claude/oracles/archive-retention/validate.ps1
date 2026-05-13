# archive-retention oracle self-test (Windows PowerShell)
#
# Same test plan as validate.sh:
#   T1. Sweep deletes dirs older than threshold (with no KEEP file).
#   T2. Sweep does NOT delete dirs younger than threshold.
#   T3. KEEP file exempts a dir from sweep regardless of age.
#   T4. Sweep is idempotent: re-running on post-sweep dir sweeps zero.
#   T5. --gc emits JSON summary with all expected fields.
#   T6. .claude/config.json archiveRetention.threshold is honored.
#   T7. --gc-cheap is silent on success and performs the sweep.
#   T8. _summary.jsonl receives one JSON line per swept dir BEFORE delete.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path

$pass = 0
$fail = 0
function Test-Pass { param([string]$m) Write-Host "PASS: $m"; $script:pass++ }
function Test-Fail { param([string]$m) Write-Host "FAIL: $m" -ForegroundColor Red; $script:fail++ }

# =============================================================================
# Phase 1 — briefing path against the real archived dir
# =============================================================================

$output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir "run.ps1") 2>&1
$outputStr = ($output | Out-String).Trim()

try {
    $parsed = $outputStr | ConvertFrom-Json -ErrorAction Stop
    foreach ($f in @("count","dirs","threshold","thresholdSource","summary")) {
        if (-not ($parsed.PSObject.Properties.Name -contains $f)) {
            Test-Fail "briefing: missing schema field '$f'"
        }
    }
    if ($parsed.count -is [int] -or $parsed.count -is [long] -or $parsed.count -is [double]) {
        Test-Pass "briefing: count=$($parsed.count)"
    } else {
        Test-Fail "briefing: 'count' is not numeric"
    }
} catch {
    Test-Fail "briefing: output is not valid JSON: $outputStr"
}

# =============================================================================
# Phase 2 — sweep semantics in a sandbox
# =============================================================================

$sandbox = Join-Path $env:TEMP ("retention-" + (Get-Random))
New-Item -ItemType Directory -Path $sandbox -Force | Out-Null

try {
    $archive = Join-Path $sandbox ".claude/collaboration/archived"
    $oracleSandboxDir = Join-Path $sandbox ".claude/oracles/archive-retention"
    New-Item -ItemType Directory -Path $archive -Force | Out-Null
    New-Item -ItemType Directory -Path $oracleSandboxDir -Force | Out-Null

    Copy-Item (Join-Path $oracleDir "run.ps1")     -Destination $oracleSandboxDir
    Copy-Item (Join-Path $oracleDir "oracle.json") -Destination $oracleSandboxDir

    # Build fixture: same shape as validate.sh
    $fixtures = @(
        "collab-20260101-100000",   # AGED, no KEEP -> sweepable (T1)
        "collab-20260201-120000",   # AGED, has KEEP -> kept (T3)
        "collab-20260420-130000",   # recent (~10 days) -> too-young (T2)
        "collab-bogus-name"         # unparseable -> never sweep
    )
    foreach ($d in $fixtures) {
        $dp = Join-Path $archive $d
        New-Item -ItemType Directory -Path (Join-Path $dp "workers/CLAUDE-A") -Force | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $dp "tasks") -Force | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $dp "channels") -Force | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $dp "file-tracking") -Force | Out-Null
        Set-Content -Path (Join-Path $dp "GUIDE.md") -Value "# Test session $d`n## Worker A notes" -Encoding utf8
        Set-Content -Path (Join-Path $dp "workers/CLAUDE-A/status.md") -Value "fixture-content" -Encoding utf8
        New-Item -ItemType File -Path (Join-Path $dp "tasks/task-1.md") -Force | Out-Null
        New-Item -ItemType File -Path (Join-Path $dp "tasks/task-2.md") -Force | Out-Null
    }

    # KEEP file on the second
    Set-Content -Path (Join-Path (Join-Path $archive "collab-20260201-120000") "KEEP") -Value "Keep because: T3 fixture" -Encoding utf8

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
        if ($gcJson.swept -eq 1) { Test-Pass "T1: --gc swept=1 (only collab-20260101-100000)" } else { Test-Fail "T1: --gc swept != 1 (got $($gcJson.swept))" }
        if ($gcJson.before -eq 3) { Test-Pass "T5: --gc before=3 (excludes bogus-name)" } else { Test-Fail "T5: --gc before != 3 (got $($gcJson.before))" }
        if ($gcJson.after -eq 2) { Test-Pass "T5: --gc after=2" } else { Test-Fail "T5: --gc after != 2 (got $($gcJson.after))" }
        if ($gcJson.summarized -eq 1) { Test-Pass "T5: --gc summarized=1" } else { Test-Fail "T5: --gc summarized != 1 (got $($gcJson.summarized))" }
    } catch {
        Test-Fail "T5: --gc output is not valid JSON: $gcOut"
    }

    if (Test-Path (Join-Path $archive "collab-20260420-130000")) { Test-Pass "T2: recent dir survived sweep" } else { Test-Fail "T2: recent dir was deleted" }
    if (Test-Path (Join-Path $archive "collab-20260201-120000")) { Test-Pass "T3: KEEP-pinned dir survived sweep" } else { Test-Fail "T3: KEEP-pinned dir was deleted" }
    if (-not (Test-Path (Join-Path $archive "collab-20260101-100000"))) { Test-Pass "T1: aged dir deleted" } else { Test-Fail "T1: aged dir was NOT deleted" }
    if (Test-Path (Join-Path $archive "collab-bogus-name")) { Test-Pass "Defensive: bogus-name dir preserved (failure-open)" } else { Test-Fail "Defensive: bogus-name dir was deleted" }

    # T8: _summary.jsonl
    $summaryFile = Join-Path $archive "_summary.jsonl"
    if (Test-Path $summaryFile) {
        $lines = @(Get-Content $summaryFile)
        if ($lines.Count -eq 1) { Test-Pass "T8: _summary.jsonl has exactly 1 line" } else { Test-Fail "T8: _summary.jsonl has $($lines.Count) lines, expected 1" }

        try {
            $summaryJson = $lines[0] | ConvertFrom-Json -ErrorAction Stop
            Test-Pass "T8: _summary.jsonl line is valid JSON"
            foreach ($f in @("sessionId","sweptAt","createdAt","ageDays","sizeBytes","workerCount","taskCount","criticVerdict","hasOverrideVeto","guideHeadline")) {
                if (-not ($summaryJson.PSObject.Properties.Name -contains $f)) {
                    Test-Fail "T8: _summary.jsonl missing field '$f'"
                }
            }
            if ($summaryJson.sessionId -eq "collab-20260101-100000") { Test-Pass "T8: _summary.jsonl sessionId matches" } else { Test-Fail "T8: _summary.jsonl sessionId mismatch: $($summaryJson.sessionId)" }
            if ($summaryJson.workerCount -eq 1) { Test-Pass "T8: _summary.jsonl workerCount=1" } else { Test-Fail "T8: _summary.jsonl workerCount=$($summaryJson.workerCount)" }
            if ($summaryJson.taskCount -eq 2) { Test-Pass "T8: _summary.jsonl taskCount=2" } else { Test-Fail "T8: _summary.jsonl taskCount=$($summaryJson.taskCount)" }
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

    # ---- T6: config.json threshold honored ----------------------------------
    Set-Content -Path (Join-Path $sandbox ".claude/config.json") -Value '{"archiveRetention":{"threshold":5}}' -Encoding utf8

    Push-Location $sandbox
    try {
        $gcOut3 = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --gc 2>&1) | Out-String
    } finally { Pop-Location }
    $gcOut3 = $gcOut3.Trim()
    Write-Host "[--gc with config 5d]: $gcOut3"
    try {
        $gcJson3 = $gcOut3 | ConvertFrom-Json -ErrorAction Stop
        if ($gcJson3.swept -eq 1) { Test-Pass "T6: 5d threshold sweeps the recent dir" } else { Test-Fail "T6: 5d threshold did not sweep (swept=$($gcJson3.swept))" }
        if ($gcJson3.thresholdSource -eq "config") { Test-Pass "T6: thresholdSource=config" } else { Test-Fail "T6: thresholdSource != config (got $($gcJson3.thresholdSource))" }
        if ($gcJson3.threshold -eq "P5D") { Test-Pass "T6: threshold=P5D" } else { Test-Fail "T6: threshold != P5D (got $($gcJson3.threshold))" }
    } catch {
        Test-Fail "T6: --gc with config output not JSON: $gcOut3"
    }

    $linesAfterT6 = @(Get-Content $summaryFile)
    if ($linesAfterT6.Count -eq 2) { Test-Pass "T8: _summary.jsonl has 2 lines after second sweep" } else { Test-Fail "T8: _summary.jsonl has $($linesAfterT6.Count) lines after second sweep, expected 2" }

    # ---- T7: --gc-cheap -----------------------------------------------------
    Remove-Item (Join-Path $sandbox ".claude/config.json") -Force -ErrorAction SilentlyContinue
    $newAged = Join-Path $archive "collab-20260102-100000"
    New-Item -ItemType Directory -Path (Join-Path $newAged "workers") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $newAged "tasks") -Force | Out-Null
    Set-Content -Path (Join-Path $newAged "GUIDE.md") -Value "# fixture" -Encoding utf8

    Push-Location $sandbox
    try {
        $cheapOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --gc-cheap 2>&1) | Out-String
    } finally { Pop-Location }
    $cheapOut = $cheapOut.Trim()
    if ([string]::IsNullOrWhiteSpace($cheapOut)) {
        Test-Pass "T7: --gc-cheap silent on success"
    } else {
        Test-Fail "T7: --gc-cheap emitted output (should be silent): $cheapOut"
    }
    if (-not (Test-Path $newAged)) { Test-Pass "T7: --gc-cheap performed the sweep" } else { Test-Fail "T7: --gc-cheap did not sweep aged dir" }

    $linesFinal = @(Get-Content $summaryFile)
    if ($linesFinal.Count -eq 3) { Test-Pass "T7: _summary.jsonl reached 3 lines (cumulative across 3 sweeps)" } else { Test-Fail "T7: _summary.jsonl has $($linesFinal.Count) lines after --gc-cheap, expected 3" }

} finally {
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

# =============================================================================
Write-Host "----"
Write-Host "Total: PASS=$pass  FAIL=$fail"
if ($fail -gt 0) { exit 1 }
exit 0
