# dispatchable-sessions oracle self-test (Windows PowerShell)
#
# Phase 1: validate the read-only briefing path against the real registry.
# Phase 2: validate --gc and --gc-cheap sweep semantics in a sandbox:
#   T1. Sweep flips dead-PID + old-spawnedAt entries to status=expired and moves them to closed[].
#   T2. Sweep does NOT touch live-PID entries (regardless of age).
#   T3. Sweep does NOT touch dead-PID entries that are younger than threshold (age guard).
#   T4. Sweep is idempotent: re-running on the post-sweep registry sweeps zero more.
#   T5. On-demand --gc emits a JSON summary {swept,before,after,threshold,thresholdSource}.
#   T6. .claude/config.json csi.registryHygieneThreshold is honored.
#   T7. --gc-cheap is silent on success and performs the sweep.
#   T8. Minimum-progress guarantee: even under a starved budget (ULDF_DS_GC_BUDGET_MS=1)
#       the FIRST candidate is probed and swept (DEFER-064).
#   T9. Bounded tail deferral: budget exhaustion after the first probe defers the
#       TAIL only; already-collected candidates are still swept.
#   T10. Convergence: repeated cheap passes drain the deferred backlog to zero
#       (DISC-ORA-05 rule; rides the T9 fixture).
# Phase 3: validate --duplicate-of semantics (RESUME-03) in the same sandbox:
#   D1. No active entry for the identity -> duplicate:false.
#   D2. Live non-self holder, same workDir -> duplicate:true.
#   D3. Dead-PID holder -> duplicate:false (stale).
#   D4. Holder pid in the caller's own ancestor chain -> duplicate:false, isSelf:true.
#   D5. Live non-self holder, DIFFERENT workDir -> duplicate:false (cross-project guard).

$ErrorActionPreference = "Stop"
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path

$pass = 0
$fail = 0
function Mark-Pass($msg) { Write-Host "PASS: $msg"; $script:pass++ }
function Mark-Fail($msg) { Write-Host "FAIL: $msg" -ForegroundColor Red; $script:fail++ }

# =============================================================================
# Phase 1 -- briefing path against the real registry
# =============================================================================

try {
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir "run.ps1") 2>&1
    if ($LASTEXITCODE -ne 0) {
        Mark-Fail "briefing: run.ps1 exited non-zero"
        exit 1
    }
} catch {
    Mark-Fail "briefing: run.ps1 threw: $_"
    exit 1
}

[string]$outputStr = if ($output -is [array]) { $output -join "" } else { "$output" }
$outputStr = $outputStr.Trim()

try { $parsed = $outputStr | ConvertFrom-Json } catch {
    Mark-Fail "briefing: output is not valid JSON: $_"
    Write-Host "Output: $outputStr"
    exit 1
}

foreach ($field in @("count","peers","briefing")) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Mark-Fail "briefing: missing schema field '$field'"
    }
}
if ($parsed.count -isnot [int] -and $parsed.count -isnot [long]) {
    Mark-Fail "briefing: 'count' is not an integer"
} elseif ($parsed.count -lt 0) {
    Mark-Fail "briefing: 'count' is negative ($($parsed.count))"
} else {
    Mark-Pass "briefing: count=$($parsed.count)"
}

if ($parsed.count -eq 0) {
    if (-not $parsed.briefing.StartsWith("No live siblings")) {
        Mark-Fail "briefing: count=0 but briefing not 'No live siblings...'"
    }
}
if ($parsed.count -gt 0) {
    if ($parsed.briefing -notmatch "^\d+ live sibling") {
        Mark-Fail "briefing: count>0 but briefing missing '<N> live sibling' header"
    }
    if ($parsed.peers.Count -ne $parsed.count) {
        Mark-Fail "briefing: peers.Count != count"
    }
}

# =============================================================================
# Phase 2 -- --gc / --gc-cheap sweep semantics in a sandbox
# =============================================================================

$sandbox = Join-Path $env:TEMP ("csi05-validate-" + [System.Guid]::NewGuid().ToString("N").Substring(0,8))
New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
try {
    New-Item -ItemType Directory -Path (Join-Path $sandbox ".claude/collaboration") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $sandbox ".claude/oracles/dispatchable-sessions") -Force | Out-Null

    Copy-Item (Join-Path $oracleDir "run.ps1")     (Join-Path $sandbox ".claude/oracles/dispatchable-sessions/run.ps1")     -Force
    Copy-Item (Join-Path $oracleDir "oracle.json") (Join-Path $sandbox ".claude/oracles/dispatchable-sessions/oracle.json") -Force

    $reg = Join-Path $sandbox ".claude/collaboration/active-sessions.json"

    # ---- Pick PIDs ---------------------------------------------------------
    $alivePid = $PID
    $deadPid  = 999999
    while (Get-Process -Id $deadPid -ErrorAction SilentlyContinue) { $deadPid++ }

    $now      = (Get-Date).ToUniversalTime()
    $oldIso    = $now.AddHours(-25).ToString("yyyy-MM-ddTHH:mm:ssZ")
    $recentIso = $now.AddSeconds(-60).ToString("yyyy-MM-ddTHH:mm:ssZ")
    $thirteenIso = $now.AddHours(-13).ToString("yyyy-MM-ddTHH:mm:ssZ")

    function Write-Fixture {
        param([string]$Path, [object[]]$Sessions)
        $obj = [pscustomobject]@{
            sessions   = $Sessions
            stale      = @()
            closed     = @()
            lastUpdated = $null
        }
        $json = $obj | ConvertTo-Json -Depth 10
        $utf8NoBom = New-Object System.Text.UTF8Encoding $false
        [System.IO.File]::WriteAllText($Path, $json, $utf8NoBom)
    }

    function Read-Reg {
        param([string]$Path)
        $bytes = [System.IO.File]::ReadAllBytes($Path)
        $jsonText = [System.Text.Encoding]::UTF8.GetString($bytes)
        return $jsonText | ConvertFrom-Json
    }

    function Run-Gc {
        param([string]$Mode)
        Push-Location $sandbox
        try {
            if ($Mode -eq "gc-cheap") {
                $out = & powershell -NoProfile -ExecutionPolicy Bypass -File ".claude/oracles/dispatchable-sessions/run.ps1" "--gc-cheap" 2>&1
            } else {
                $out = & powershell -NoProfile -ExecutionPolicy Bypass -File ".claude/oracles/dispatchable-sessions/run.ps1" "--gc" 2>&1
            }
        } finally {
            Pop-Location
        }
        if ($out -is [array]) { return ($out -join "").Trim() } else { return "$out".Trim() }
    }

    # ---- Build initial fixture ---------------------------------------------
    $sessions = @(
        [pscustomobject]@{ id="DEAD-OLD";    sessionRole="pods-worker"; claudeShellPid=$deadPid;  status="active"; dispatchable=$true; spawnedAt=$oldIso;    role="to-sweep" },
        [pscustomobject]@{ id="ALIVE-OLD";   sessionRole="pods-worker"; claudeShellPid=$alivePid; status="active"; dispatchable=$true; spawnedAt=$oldIso;    role="alive-guard" },
        [pscustomobject]@{ id="DEAD-RECENT"; sessionRole="pods-worker"; claudeShellPid=$deadPid;  status="active"; dispatchable=$true; spawnedAt=$recentIso; role="age-guard" },
        [pscustomobject]@{ id="ENDED";       sessionRole="pods-worker"; claudeShellPid=$deadPid;  status="ended";  dispatchable=$true; spawnedAt=$oldIso;    role="non-active" }
    )
    Write-Fixture -Path $reg -Sessions $sessions

    # ---- T1+T2+T3+T5: --gc -------------------------------------------------
    $gcOut = Run-Gc -Mode "gc"
    Write-Host "[--gc summary]: $gcOut"

    try { $gcSummary = $gcOut | ConvertFrom-Json } catch {
        Mark-Fail "T5: --gc summary not valid JSON: $gcOut"
        $gcSummary = $null
    }

    if ($gcSummary) {
        foreach ($f in @("swept","before","after","threshold","thresholdSource")) {
            if (-not ($gcSummary.PSObject.Properties.Name -contains $f)) {
                Mark-Fail "T5: --gc summary missing field '$f' (got: $gcOut)"
            }
        }
        if ($gcSummary.swept -eq 1) { Mark-Pass "T1: --gc swept=1 (only DEAD-OLD)" } else { Mark-Fail "T1: --gc swept != 1 (got: $gcOut)" }
        if ($gcSummary.before -eq 4) { Mark-Pass "T5: --gc before=4" } else { Mark-Fail "T5: --gc before != 4 (got: $gcOut)" }
        if ($gcSummary.after  -eq 3) { Mark-Pass "T5: --gc after=3"  } else { Mark-Fail "T5: --gc after != 3 (got: $gcOut)" }
    }

    $post = Read-Reg -Path $reg
    $postIds       = @($post.sessions | ForEach-Object { $_.id })
    $postClosedIds = @($post.closed   | ForEach-Object { $_.id })
    $postClosedSt  = @($post.closed   | ForEach-Object { $_.status })
    $postClosedSwept = @($post.closed | ForEach-Object { [bool]$_.sweptAt })

    if (-not ($postIds -contains "DEAD-OLD") -and ($postIds -contains "ALIVE-OLD") -and ($postIds -contains "DEAD-RECENT") -and ($postIds -contains "ENDED")) {
        Mark-Pass "T1+T2+T3: sessions[] retains ALIVE-OLD + DEAD-RECENT + ENDED, drops DEAD-OLD"
    } else {
        Mark-Fail "T1+T2+T3: sessions[] unexpected: $($postIds -join ',')"
    }
    if (($postClosedIds.Count -eq 1) -and ($postClosedIds -contains "DEAD-OLD")) {
        Mark-Pass "T1: closed[] received DEAD-OLD"
    } else {
        Mark-Fail "T1: closed[] missing DEAD-OLD: $($postClosedIds -join ',')"
    }
    if (($postClosedSt.Count -eq 1) -and ($postClosedSt[0] -eq "expired")) {
        Mark-Pass "T1: closed[].status == expired"
    } else {
        Mark-Fail "T1: closed[].status not expired: $($postClosedSt -join ',')"
    }
    if (($postClosedSwept.Count -eq 1) -and ($postClosedSwept[0] -eq $true)) {
        Mark-Pass "T1: closed[].sweptAt set"
    } else {
        Mark-Fail "T1: closed[].sweptAt missing"
    }

    # ---- T4: idempotence ---------------------------------------------------
    $gcOut2 = Run-Gc -Mode "gc"
    Write-Host "[second --gc]: $gcOut2"
    try { $sum2 = $gcOut2 | ConvertFrom-Json } catch { $sum2 = $null }
    if ($sum2 -and $sum2.swept -eq 0) {
        Mark-Pass "T4: idempotence (second --gc swept=0)"
    } else {
        Mark-Fail "T4: idempotence violated (got: $gcOut2)"
    }

    # ---- T6: config threshold ----------------------------------------------
    $cfgPath = Join-Path $sandbox ".claude/config.json"
    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($cfgPath, '{"csi":{"registryHygieneThreshold":12}}', $utf8NoBom)

    $sessions2 = @(
        [pscustomobject]@{ id="DEAD-13H"; sessionRole="pods-worker"; claudeShellPid=$deadPid; status="active"; dispatchable=$true; spawnedAt=$thirteenIso; role="threshold-test" }
    )
    Write-Fixture -Path $reg -Sessions $sessions2

    $gcOut3 = Run-Gc -Mode "gc"
    Write-Host "[--gc with config 12h]: $gcOut3"
    try { $sum3 = $gcOut3 | ConvertFrom-Json } catch { $sum3 = $null }
    if ($sum3 -and $sum3.swept -eq 1) {
        Mark-Pass "T6: config 12h threshold sweeps DEAD-13H"
    } else {
        Mark-Fail "T6: 13h entry NOT swept under 12h threshold (got: $gcOut3)"
    }
    if ($sum3 -and $sum3.thresholdSource -eq "config") {
        Mark-Pass "T6: thresholdSource=config"
    } else {
        Mark-Fail "T6: thresholdSource != config (got: $gcOut3)"
    }

    # ---- T7: --gc-cheap silent + sweeps ------------------------------------
    Remove-Item $cfgPath -Force -ErrorAction SilentlyContinue
    $sessions3 = @(
        [pscustomobject]@{ id="DEAD-OLD-C"; sessionRole="pods-worker"; claudeShellPid=$deadPid; status="active"; dispatchable=$true; spawnedAt=$oldIso; role="cheap-test" }
    )
    Write-Fixture -Path $reg -Sessions $sessions3

    $cheapOut = Run-Gc -Mode "gc-cheap"
    if ([string]::IsNullOrWhiteSpace($cheapOut)) {
        Mark-Pass "T7: --gc-cheap silent on success"
    } else {
        Mark-Fail "T7: --gc-cheap emitted output (should be silent): $cheapOut"
    }

    $postCheap = Read-Reg -Path $reg
    if (@($postCheap.sessions).Count -eq 0 -and @($postCheap.closed).Count -eq 1) {
        Mark-Pass "T7: --gc-cheap performed the sweep (active=0, closed=1)"
    } else {
        Mark-Fail "T7: --gc-cheap did not sweep correctly (active=$(@($postCheap.sessions).Count), closed=$(@($postCheap.closed).Count))"
    }

    # ---- T8: minimum-progress guarantee under a starved budget (DEFER-064) --
    # 1ms budget via the ULDF_DS_GC_BUDGET_MS seam: without the first-probe
    # exemption, setup cost defers the whole sweep before any probe runs and
    # this cell is RED. Post-fix the first candidate is always probed.
    $sessions4 = @(
        [pscustomobject]@{ id="DEAD-OLD-T8"; sessionRole="pods-worker"; claudeShellPid=$deadPid; status="active"; dispatchable=$true; spawnedAt=$oldIso; role="min-progress-test" }
    )
    Write-Fixture -Path $reg -Sessions $sessions4
    $savedBudget = $env:ULDF_DS_GC_BUDGET_MS
    try {
        $env:ULDF_DS_GC_BUDGET_MS = "1"
        $t8Out = Run-Gc -Mode "gc-cheap"
    } finally {
        $env:ULDF_DS_GC_BUDGET_MS = $savedBudget
    }
    if ([string]::IsNullOrWhiteSpace($t8Out)) {
        Mark-Pass "T8: starved-budget --gc-cheap still silent"
    } else {
        Mark-Fail "T8: starved-budget --gc-cheap emitted output: $t8Out"
    }
    $postT8 = Read-Reg -Path $reg
    if (@($postT8.sessions).Count -eq 0 -and @($postT8.closed).Count -eq 1) {
        Mark-Pass "T8: min-progress guarantee -- first candidate swept even at 1ms budget"
    } else {
        Mark-Fail "T8: starved budget starved the WHOLE sweep (active=$(@($postT8.sessions).Count), closed=$(@($postT8.closed).Count))"
    }

    # ---- T9: bounded tail deferral -------------------------------------------
    # Two dead+old entries at a 1ms budget: the first probe (>=1ms in a fresh
    # child process) exhausts the budget, so the head sweeps and the tail defers.
    $sessions5 = @(
        [pscustomobject]@{ id="DEAD-T9-A"; sessionRole="pods-worker"; claudeShellPid=$deadPid; status="active"; dispatchable=$true; spawnedAt=$oldIso; role="tail-deferral-a" },
        [pscustomobject]@{ id="DEAD-T9-B"; sessionRole="pods-worker"; claudeShellPid=$deadPid; status="active"; dispatchable=$true; spawnedAt=$oldIso; role="tail-deferral-b" }
    )
    Write-Fixture -Path $reg -Sessions $sessions5
    try {
        $env:ULDF_DS_GC_BUDGET_MS = "1"
        $null = Run-Gc -Mode "gc-cheap"
    } finally {
        $env:ULDF_DS_GC_BUDGET_MS = $savedBudget
    }
    $postT9 = Read-Reg -Path $reg
    if (@($postT9.sessions).Count -eq 1 -and @($postT9.closed).Count -eq 1) {
        Mark-Pass "T9: tail deferred, head swept (bounded deferral)"
    } else {
        Mark-Fail "T9: expected head swept + tail deferred (active=$(@($postT9.sessions).Count), closed=$(@($postT9.closed).Count))"
    }

    # ---- T10: convergence -- repeated cheap passes drain the deferred tail --
    # DISC-ORA-05's rule: test that a backlog drains to zero across repeated
    # invocations, not just that one item sweeps under a lucky budget.
    try {
        $env:ULDF_DS_GC_BUDGET_MS = "1"
        $null = Run-Gc -Mode "gc-cheap"
    } finally {
        $env:ULDF_DS_GC_BUDGET_MS = $savedBudget
    }
    $postT10 = Read-Reg -Path $reg
    if (@($postT10.sessions).Count -eq 0 -and @($postT10.closed).Count -eq 2) {
        Mark-Pass "T10: second cheap pass drains the deferred tail (convergence)"
    } else {
        Mark-Fail "T10: backlog did not converge to zero (active=$(@($postT10.sessions).Count), closed=$(@($postT10.closed).Count))"
    }

    # =========================================================================
    # Phase 3 -- --duplicate-of semantics (RESUME-03)
    # =========================================================================

    # Live NON-self PID for D2/D5: a hidden sleeper ($PID itself is an ancestor
    # of the spawned run.ps1, so it would read as isSelf). Cleaned up below.
    #
    # FIXTURE-LIFETIME CONTRACT (DEFER-064): the sleeper must outlive every cell
    # that embeds its PID in a fixture. At 120s the bash twin's single sleeper
    # expired mid-run (its powershell.exe-forking probes stretch the run past
    # 4 min) and D5/S1-S3 asserted "alive" about a genuinely dead PID. 1800s +
    # the Ensure-Sleeper re-check keep the same class out of this twin.
    function Start-SleeperProcess {
        try {
            return Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 1800' -PassThru -WindowStyle Hidden
        } catch { return $null }
    }
    function Ensure-Sleeper {
        param([object]$Sleeper, [string]$Label)
        if ($null -ne $Sleeper -and (Get-Process -Id $Sleeper.Id -ErrorAction SilentlyContinue)) { return $Sleeper }
        Write-Host "[harness] fixture sleeper not alive before $Label -- re-spawning (fixture-lifetime event, not an oracle verdict; DEFER-064)"
        return Start-SleeperProcess
    }
    $sleeper = Start-SleeperProcess

    $sandboxWd = (Get-Item $sandbox).FullName

    function Write-DupFixture {
        param([int]$HolderPid, [string]$HolderWorkDir)
        $sessions = @(
            [pscustomobject]@{ id="CLAUDE-DUP"; sessionRole="pods-worker"; claudeShellPid=$HolderPid; status="active"; dispatchable=$true; spawnedAt="2026-06-12T00:00:00Z"; workDir=$HolderWorkDir }
        )
        Write-Fixture -Path $reg -Sessions $sessions
    }

    function Run-Dup {
        param([string]$Id)
        Push-Location $sandbox
        try {
            $out = & powershell -NoProfile -ExecutionPolicy Bypass -File ".claude/oracles/dispatchable-sessions/run.ps1" "--duplicate-of=$Id" 2>&1
        } finally {
            Pop-Location
        }
        if ($out -is [array]) { return ($out -join "").Trim() } else { return "$out".Trim() }
    }

    try {
        if ($null -ne $sleeper) {
            # D1: no active entry for the queried identity
            Write-DupFixture -HolderPid $sleeper.Id -HolderWorkDir $sandboxWd
            $d1 = Run-Dup -Id "CLAUDE-NOPE"
            if ($d1 -match '"duplicate":false' -and $d1 -match 'no active registry entry') {
                Mark-Pass "D1: no entry -> duplicate:false"
            } else { Mark-Fail "D1: unexpected output: $d1" }

            # D2: live non-self holder in the SAME workDir -> duplicate:true
            $d2 = Run-Dup -Id "CLAUDE-DUP"
            if ($d2 -match '"duplicate":true' -and $d2 -match '"workDirMatch":true' -and $d2 -match '"isSelf":false') {
                Mark-Pass "D2: live non-self same-workDir holder -> duplicate:true"
            } else { Mark-Fail "D2: unexpected output: $d2" }

            # D5: live non-self holder in a DIFFERENT workDir -> duplicate:false
            $sleeper = Ensure-Sleeper -Sleeper $sleeper -Label "D5 fixture write"
            Write-DupFixture -HolderPid $sleeper.Id -HolderWorkDir "C:\some\other\project"
            $d5 = Run-Dup -Id "CLAUDE-DUP"
            if ($d5 -match '"duplicate":false' -and $d5 -match '"workDirMatch":false') {
                Mark-Pass "D5: different-workDir holder -> duplicate:false (cross-project guard)"
            } else { Mark-Fail "D5: unexpected output: $d5" }
        } else {
            Write-Host "SKIP: D1/D2/D5 (could not spawn sleeper process)"
        }

        # D3: dead-PID holder -> duplicate:false (stale)
        Write-DupFixture -HolderPid $deadPid -HolderWorkDir $sandboxWd
        $d3 = Run-Dup -Id "CLAUDE-DUP"
        if ($d3 -match '"duplicate":false' -and $d3 -match 'holder pid dead') {
            Mark-Pass "D3: dead holder -> duplicate:false (stale)"
        } else { Mark-Fail "D3: unexpected output: $d3" }

        # D4: holder pid in the caller's own ancestor chain ($PID of THIS
        # validate process is an ancestor of the spawned run.ps1)
        Write-DupFixture -HolderPid $PID -HolderWorkDir $sandboxWd
        $d4 = Run-Dup -Id "CLAUDE-DUP"
        if ($d4 -match '"duplicate":false' -and $d4 -match '"isSelf":true') {
            Mark-Pass "D4: self-held identity -> duplicate:false, isSelf:true"
        } else { Mark-Fail "D4: unexpected output: $d4" }
    } finally {
        if ($null -ne $sleeper) {
            Stop-Process -Id $sleeper.Id -Force -ErrorAction SilentlyContinue
        }
    }

    # =========================================================================
    # Phase 4 -- briefing-path self-exclusion (DISC-CSI-22)
    # =========================================================================
    # A session's own registry entry must not be reported to it as a live
    # sibling. Exclusion keys on ULDF_SELF_SESSION_ID > CLAUDE_SESSION_ID env.

    # Fresh sleeper for Phase 4 (1800s + Ensure-Sleeper; see the fixture-lifetime
    # contract note above).
    $sleeper4 = Start-SleeperProcess

    function Write-SelfFixture {
        param([int]$LivePid, [bool]$IncludePeer)
        $sessions = @(
            [pscustomobject]@{ id="SELF-ENTRY"; sessionRole="interactive"; claudeShellPid=$LivePid; status="active"; dispatchable=$true; spawnedAt="2026-07-07T00:00:00Z"; workDir="X"; registryVersion=2 }
        )
        if ($IncludePeer) {
            $sessions += [pscustomobject]@{ id="PEER-ENTRY"; sessionRole="pods-worker"; claudeShellPid=$LivePid; status="active"; dispatchable=$true; spawnedAt="2026-07-07T00:00:00Z"; workDir="X"; registryVersion=2 }
        }
        Write-Fixture -Path $reg -Sessions $sessions
    }

    function Run-BriefingEnv {
        param([string]$SelfId, [string]$ClaudeId)
        $savedSelf = $env:ULDF_SELF_SESSION_ID
        $savedClaude = $env:CLAUDE_SESSION_ID
        try {
            $env:ULDF_SELF_SESSION_ID = if ($SelfId) { $SelfId } else { $null }
            $env:CLAUDE_SESSION_ID = if ($ClaudeId) { $ClaudeId } else { $null }
            Push-Location $sandbox
            try {
                $out = & powershell -NoProfile -ExecutionPolicy Bypass -File ".claude/oracles/dispatchable-sessions/run.ps1" 2>&1
            } finally {
                Pop-Location
            }
            if ($out -is [array]) { return ($out -join "").Trim() } else { return "$out".Trim() }
        } finally {
            $env:ULDF_SELF_SESSION_ID = $savedSelf
            $env:CLAUDE_SESSION_ID = $savedClaude
        }
    }

    if ($null -ne $sleeper4) {
        try {
            Write-SelfFixture -LivePid $sleeper4.Id -IncludePeer $true

            # S1: ULDF_SELF_SESSION_ID excludes own entry, peer survives
            $s1 = Run-BriefingEnv -SelfId "SELF-ENTRY" -ClaudeId ""
            if ($s1 -match '"count":1' -and $s1 -match 'PEER-ENTRY' -and $s1 -notmatch 'SELF-ENTRY') {
                Mark-Pass "S1: ULDF_SELF_SESSION_ID excludes self, keeps peer"
            } else { Mark-Fail "S1: unexpected output: $s1" }

            # S2: no identity env -> both entries reported (legacy no-exclusion path)
            $s2 = Run-BriefingEnv -SelfId "" -ClaudeId ""
            if ($s2 -match '"count":2') {
                Mark-Pass "S2: no identity env -> no exclusion (count 2)"
            } else { Mark-Fail "S2: unexpected output: $s2" }

            # S3: CLAUDE_SESSION_ID fallback excludes own entry
            $s3 = Run-BriefingEnv -SelfId "" -ClaudeId "SELF-ENTRY"
            if ($s3 -match '"count":1' -and $s3 -notmatch 'SELF-ENTRY') {
                Mark-Pass "S3: CLAUDE_SESSION_ID fallback excludes self"
            } else { Mark-Fail "S3: unexpected output: $s3" }

            # S4: self is the ONLY live entry -> canonical empty output
            $sleeper4 = Ensure-Sleeper -Sleeper $sleeper4 -Label "S4 fixture write"
            Write-SelfFixture -LivePid $sleeper4.Id -IncludePeer $false
            $s4 = Run-BriefingEnv -SelfId "SELF-ENTRY" -ClaudeId ""
            if ($s4 -match '"count":0' -and $s4 -match 'No live siblings') {
                Mark-Pass "S4: self-only registry -> canonical empty output"
            } else { Mark-Fail "S4: unexpected output: $s4" }
        } finally {
            Stop-Process -Id $sleeper4.Id -Force -ErrorAction SilentlyContinue
        }
    } else {
        Write-Host "SKIP: Phase 4 (could not spawn sleeper process)"
    }

} finally {
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

# =============================================================================
# Summary
# =============================================================================
Write-Host "----"
Write-Host "Total: PASS=$pass  FAIL=$fail"
if ($fail -gt 0) { exit 1 }
exit 0
