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
