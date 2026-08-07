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
#   T9. --gc-cheap never starves: every invocation sweeps >=1 candidate, so a
#       backlog drains to zero across repeated session-starts (DEFER-019).
#  T10. Time-invariance: the whole sandbox phase is green under a clock shifted
#       +1 year, so no cell can rot across its own threshold (DEFER-072).
#  T11. The ULDF_FAKE_NOW seam rejects non-numeric values (falls back to the
#       real clock) -- the seam moves a delete cutoff and must not fail open.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$selfPath  = $MyInvocation.MyCommand.Path

# =============================================================================
# Fixture clock (DEFER-072)
# =============================================================================
# Fixture ages are computed from `now` at runtime, NEVER written as calendar
# literals. The properties under test are *older than threshold* and *younger
# than threshold* -- both relative by definition; a literal date additionally
# encodes the authoring day, which is not part of the contract. Fixture
# collab-20260420-130000 was "~10 days" old when authored (2026-04-30), crossed
# the P90D default on 2026-07-19, and left this harness red for 15 days before
# an unrelated finalize tripped over it.
#
# ULDF_FAKE_NOW (epoch seconds) shifts this validator's notion of `now`; run.ps1
# honors the same seam, so validator and oracle shift together. Faking one alone
# would only measure the skew between them. T10 uses it; T11 guards it.
$epochBase = [DateTime]::new(1970, 1, 1, 0, 0, 0, [DateTimeKind]::Utc)
$nowEpochFixture = $null
if ($env:ULDF_FAKE_NOW -match '^[0-9]+$') { $nowEpochFixture = [int64]$env:ULDF_FAKE_NOW }
if ($null -eq $nowEpochFixture) {
    $nowEpochFixture = [int64]([DateTime]::UtcNow - $epochBase).TotalSeconds
}

# collab-YYYYMMDD-HHMMSS basename for (now - $DaysAgo days).
# $Hms is a fixed time-of-day used purely as a GROUP DISCRIMINATOR, so a cell
# can match its own fixtures without a date literal (T9). It is not part of the
# age: it leaves each fixture's true age within +/-1 day of the requested
# offset, and every margin here is >= 5 days, so that slack is immaterial.
function Get-FixtureName {
    param([int]$DaysAgo, [string]$Hms)
    $dt = $epochBase.AddSeconds([double]($nowEpochFixture - ([int64]$DaysAgo * 86400)))
    return "collab-" + $dt.ToString('yyyyMMdd') + "-" + $Hms
}

$pass = 0
$fail = 0
function Test-Pass { param([string]$m) Write-Host "PASS: $m"; $script:pass++ }
function Test-Fail { param([string]$m) Write-Host "FAIL: $m" -ForegroundColor Red; $script:fail++ }

# =============================================================================
# Phase 1 — briefing path against the real archived dir
# =============================================================================

# T10's inner run skips Phase 1: it reads the REAL repo (the dominant cost) and
# contains no fixtures to age, so it has nothing to say about time-invariance.
if ($env:ULDF_AR_INNER) {
    Write-Host "SKIP: Phase 1 (inner time-invariance run -- no fixtures on this path)"
} else {
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

    # Build fixture: same shape as validate.sh, every date computed from `now`.
    #   $fxAged   -- now-200d, no KEEP  -> sweepable at P90D           (T1)
    #   $fxKept   -- now-180d, has KEEP -> kept regardless of age      (T3)
    #   $fxRecent -- now-10d            -> too-young at P90D           (T2)
    #                                   -> AND sweepable at the P5D of (T6)
    #   $fxBogus  -- unparseable name   -> never swept (failure-open)
    # Offsets are chosen for margin, not precision: the nearest boundary is 5
    # days away, so the +/-1 day of time-of-day slack cannot flip any verdict.
    $fxAged   = Get-FixtureName -DaysAgo 200 -Hms "100000"
    $fxKept   = Get-FixtureName -DaysAgo 180 -Hms "120000"
    $fxRecent = Get-FixtureName -DaysAgo 10  -Hms "130000"
    $fxBogus  = "collab-bogus-name"
    $fxCheap  = Get-FixtureName -DaysAgo 190 -Hms "110000"   # built later, for T7
    $backlogHms = "190000"                                   # T9's group discriminator

    $fixtures = @($fxAged, $fxKept, $fxRecent, $fxBogus)
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
    Set-Content -Path (Join-Path (Join-Path $archive $fxKept) "KEEP") -Value "Keep because: T3 fixture" -Encoding utf8

    # ---- T1+T2+T3+T5+T8: --gc -----------------------------------------------
    if (-not $sandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run the sweeper in the CWD' }
    Push-Location -LiteralPath $sandbox -ErrorAction Stop
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
        if ($gcJson.swept -eq 1) { Test-Pass "T1: --gc swept=1 (only $fxAged)" } else { Test-Fail "T1: --gc swept != 1 (got $($gcJson.swept))" }
        if ($gcJson.before -eq 3) { Test-Pass "T5: --gc before=3 (excludes bogus-name)" } else { Test-Fail "T5: --gc before != 3 (got $($gcJson.before))" }
        if ($gcJson.after -eq 2) { Test-Pass "T5: --gc after=2" } else { Test-Fail "T5: --gc after != 2 (got $($gcJson.after))" }
        if ($gcJson.summarized -eq 1) { Test-Pass "T5: --gc summarized=1" } else { Test-Fail "T5: --gc summarized != 1 (got $($gcJson.summarized))" }
    } catch {
        Test-Fail "T5: --gc output is not valid JSON: $gcOut"
    }

    if (Test-Path (Join-Path $archive $fxRecent)) { Test-Pass "T2: recent dir survived sweep" } else { Test-Fail "T2: recent dir was deleted" }
    if (Test-Path (Join-Path $archive $fxKept)) { Test-Pass "T3: KEEP-pinned dir survived sweep" } else { Test-Fail "T3: KEEP-pinned dir was deleted" }
    if (-not (Test-Path (Join-Path $archive $fxAged))) { Test-Pass "T1: aged dir deleted" } else { Test-Fail "T1: aged dir was NOT deleted" }
    if (Test-Path (Join-Path $archive $fxBogus)) { Test-Pass "Defensive: bogus-name dir preserved (failure-open)" } else { Test-Fail "Defensive: bogus-name dir was deleted" }

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
            if ($summaryJson.sessionId -eq $fxAged) { Test-Pass "T8: _summary.jsonl sessionId matches" } else { Test-Fail "T8: _summary.jsonl sessionId mismatch: $($summaryJson.sessionId)" }
            if ($summaryJson.workerCount -eq 1) { Test-Pass "T8: _summary.jsonl workerCount=1" } else { Test-Fail "T8: _summary.jsonl workerCount=$($summaryJson.workerCount)" }
            if ($summaryJson.taskCount -eq 2) { Test-Pass "T8: _summary.jsonl taskCount=2" } else { Test-Fail "T8: _summary.jsonl taskCount=$($summaryJson.taskCount)" }
        } catch {
            Test-Fail "T8: _summary.jsonl line is not valid JSON: $($lines[0])"
        }
    } else {
        Test-Fail "T8: _summary.jsonl was not created"
    }

    # ---- T4: idempotence ----------------------------------------------------
    if (-not $sandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run the sweeper in the CWD' }
    Push-Location -LiteralPath $sandbox -ErrorAction Stop
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

    if (-not $sandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run the sweeper in the CWD' }
    Push-Location -LiteralPath $sandbox -ErrorAction Stop
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
    $newAged = Join-Path $archive $fxCheap
    New-Item -ItemType Directory -Path (Join-Path $newAged "workers") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $newAged "tasks") -Force | Out-Null
    Set-Content -Path (Join-Path $newAged "GUIDE.md") -Value "# fixture" -Encoding utf8

    if (-not $sandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run the sweeper in the CWD' }
    Push-Location -LiteralPath $sandbox -ErrorAction Stop
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

    # ---- T9: forward progress + backlog convergence (DEFER-019 regression) ---
    # The budget must never starve the sweep: each --gc-cheap invocation sweeps
    # at least one candidate, so a backlog drains to zero across repeated
    # session-starts. Regression guard for the class where setup cost consumed
    # the whole budget before the first candidate was examined and --gc-cheap
    # swept nothing, forever.
    # The backlog is identified by its own time-of-day discriminator
    # ($backlogHms), not by a date prefix: the old '^collab-2026010[0-9]-120000$'
    # regex was itself a calendar literal, so relative dates alone would have
    # left this cell counting zero dirs and passing vacuously.
    $backlog = 5
    foreach ($i in 1..$backlog) {
        $d = Join-Path $archive (Get-FixtureName -DaysAgo (300 + $i) -Hms $backlogHms)
        New-Item -ItemType Directory -Path (Join-Path $d "workers") -Force | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $d "tasks") -Force | Out-Null
        Set-Content -Path (Join-Path $d "GUIDE.md") -Value "# fixture" -Encoding utf8
    }
    $linesBefore = @(Get-Content $summaryFile).Count

    $remain = $backlog
    $passes = 0
    $stalled = $false
    while ($passes -lt $backlog) {
        $prev = $remain
        if (-not $sandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run the sweeper in the CWD' }
        Push-Location -LiteralPath $sandbox -ErrorAction Stop
        try {
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --gc-cheap *>$null
        } finally { Pop-Location }
        $passes++
        $remain = @(Get-ChildItem -Path $archive -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match ('^collab-[0-9]{8}-' + $backlogHms + '$') }).Count
        if ($remain -eq 0) { break }
        if ($remain -eq $prev) { $stalled = $true; break }
    }

    if (-not $stalled) {
        Test-Pass "T9: --gc-cheap makes forward progress every invocation (no budget starvation)"
    } else {
        Test-Fail "T9: --gc-cheap swept nothing on an invocation with $remain candidates pending (budget starvation)"
    }

    if ($remain -eq 0) {
        Test-Pass "T9: backlog of $backlog drained to zero in $passes --gc-cheap invocation(s)"
    } else {
        Test-Fail "T9: backlog did not converge -- $remain of $backlog dirs remain after $passes invocations"
    }

    # Audit-trail invariant: exactly one summary line per swept dir, none lost.
    $sweptLines = @(Get-Content $summaryFile).Count - $linesBefore
    if ($sweptLines -eq $backlog) {
        Test-Pass "T9: audit trail gained exactly $backlog lines (one per swept dir)"
    } else {
        Test-Fail "T9: audit trail gained $sweptLines lines for $backlog swept dirs (expected $backlog)"
    }

    # ---- T11: the ULDF_FAKE_NOW seam rejects non-numeric values -------------
    # The seam moves a delete cutoff, so it must fail SAFE (fall back to the
    # real clock) rather than fail open on garbage. A dedicated probe dir gives
    # the assertion teeth: now-10d is too-young under the real clock and 375d
    # old under a +1yr shift. Without it the comparison would be vacuous (the
    # post-T9 archive holds only a KEEP-pinned and an unparsable dir, whose
    # verdicts no clock can change). --dry-run mutates nothing.
    $fxSeam = Get-FixtureName -DaysAgo 10 -Hms "140000"
    $fxSeamPath = Join-Path $archive $fxSeam
    New-Item -ItemType Directory -Path (Join-Path $fxSeamPath "workers") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $fxSeamPath "tasks") -Force | Out-Null
    Set-Content -Path (Join-Path $fxSeamPath "GUIDE.md") -Value "# fixture" -Encoding utf8
    $seamFuture = $nowEpochFixture + 31536000

    $seamPrev = $env:ULDF_FAKE_NOW
    try {
        # Trailing garbage on an otherwise-valid epoch: the shape a lenient
        # parse (PowerShell's -as [int], bash arithmetic) would silently coerce.
        $env:ULDF_FAKE_NOW = "${seamFuture}x"
        if (-not $sandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run the sweeper in the CWD' }
        Push-Location -LiteralPath $sandbox -ErrorAction Stop
        try {
            $seamBogusOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --dry-run 2>&1) | Out-String
        } finally { Pop-Location }

        # Genuinely unset, not merely absent from this scope: under T10's inner
        # run this validator's OWN environment already carries ULDF_FAKE_NOW.
        $env:ULDF_FAKE_NOW = $null
        if (-not $sandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run the sweeper in the CWD' }
        Push-Location -LiteralPath $sandbox -ErrorAction Stop
        try {
            $seamUnsetOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --dry-run 2>&1) | Out-String
        } finally { Pop-Location }

        $env:ULDF_FAKE_NOW = [string]$seamFuture
        if (-not $sandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run the sweeper in the CWD' }
        Push-Location -LiteralPath $sandbox -ErrorAction Stop
        try {
            $seamNumOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleSandboxDir "run.ps1") --dry-run 2>&1) | Out-String
        } finally { Pop-Location }
    } finally {
        $env:ULDF_FAKE_NOW = $seamPrev
    }
    $seamBogusOut = $seamBogusOut.Trim()
    $seamUnsetOut = $seamUnsetOut.Trim()
    $seamNumOut   = $seamNumOut.Trim()

    if ($seamBogusOut -match '"clockSource":"fake"') {
        Test-Fail "T11: non-numeric ULDF_FAKE_NOW was honored (got: $seamBogusOut)"
    } else {
        Test-Pass "T11: non-numeric ULDF_FAKE_NOW ignored (no clockSource:fake)"
    }
    # Assert about the PROBE DIR specifically, never the total count: a preceding
    # cell that failed to sweep (e.g. T7/T9 under a starved --gc-cheap budget on
    # a loaded machine) leaves other aged dirs in the archive, and a count
    # assertion would report that as a seam defect.
    if ($seamBogusOut -match [regex]::Escape($fxSeam)) {
        Test-Fail "T11: non-numeric seam moved the cutoff -- probe dir became sweepable (got: $seamBogusOut)"
    } else {
        Test-Pass "T11: non-numeric seam left the probe dir too-young (failed safe)"
    }
    if ($seamBogusOut -eq $seamUnsetOut) {
        Test-Pass "T11: non-numeric seam is byte-identical to unset"
    } else {
        Test-Fail "T11: non-numeric seam diverged from unset (bogus: $seamBogusOut / unset: $seamUnsetOut)"
    }
    # The positive half -- without it T11 would pass just as well on a seam that
    # never works at all.
    if ($seamNumOut -match [regex]::Escape($fxSeam) -and $seamNumOut -match '"clockSource":"fake"') {
        Test-Pass "T11: numeric ULDF_FAKE_NOW IS honored (probe dir becomes sweepable, declares clockSource:fake)"
    } else {
        Test-Fail "T11: numeric ULDF_FAKE_NOW was not honored -- seam is dead (got: $seamNumOut)"
    }
    Remove-Item $fxSeamPath -Recurse -Force -ErrorAction SilentlyContinue

} finally {
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

# ---- T10: time-invariance (DEFER-072's decisive cell) -----------------------
# Re-run the sandbox phase with `now` shifted +1 year. A relative-date fixture
# is time-invariant by construction; a calendar literal is not, and that
# difference is exactly what must be proven rather than assumed -- the fixtures
# this cell guards sat red for 15 days precisely because nothing asserted it.
# ULDF_AR_INNER bounds the recursion to one level and skips Phase 1.
if (-not $env:ULDF_AR_INNER) {
    $futureEpoch = $nowEpochFixture + 31536000
    $prevFake  = $env:ULDF_FAKE_NOW
    $prevInner = $env:ULDF_AR_INNER
    try {
        $env:ULDF_AR_INNER = "1"
        $env:ULDF_FAKE_NOW = [string]$futureEpoch
        $innerOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $selfPath 2>&1) | Out-String
        $innerRc = $LASTEXITCODE
    } finally {
        $env:ULDF_AR_INNER = $prevInner
        $env:ULDF_FAKE_NOW = $prevFake
    }
    if ($innerRc -eq 0) {
        Test-Pass "T10: sandbox phase green under a +1yr faked clock (time-invariant)"
    } else {
        Test-Fail "T10: NOT time-invariant -- +1yr faked-clock run exited $innerRc"
        ($innerOut -split "`n") | Where-Object { $_ -match '^FAIL' } | ForEach-Object { Write-Host $_ }
    }
}

# =============================================================================
Write-Host "----"
Write-Host "Total: PASS=$pass  FAIL=$fail"
if ($fail -gt 0) { exit 1 }
exit 0
