# Project-State Oracle: background-job-status
#
# Answers, deterministically: of the jobs launched via the tracked-background-run
# wrapper (scripts/jobs/Invoke-TrackedRun.ps1), which are running / done /
# failed / stalled -- and which finished but have NOT been acknowledged yet?
#
# This is the program that replaces agent inference. The recurring bug it kills:
# an agent backgrounds a build/E2E/sync via the Bash tool, then either waits
# forever on a completion notification that never fires (detached children) or
# fails to notice the work already finished. The wrapper writes a terminal
# sentinel JSON in a `finally{}` regardless of detached children; this oracle
# reads those sentinels and reports the truth.
#
# Modes (first positional arg):
#   (none) | all   -> every known job, plus a `briefing` enforcement field
#   latest         -> the most-recently-started job only
#   <job_id>       -> that specific job
#
# Enforcement surface: because oracle.json declares
# consultation.typical_sessions_using = "every" AND this run emits a non-empty
# `briefing` field when any job is still PENDING (running/stalled) or
# COMPLETED-BUT-UNACKNOWLEDGED, the session-start hook surfaces a
# [background-job-status] line automatically -- a finished job cannot be
# silently missed at the next turn/session. When nothing is pending/unacked the
# `briefing` field is "" (suppresses the line; graceful absence).
#
# Acknowledge path: `run.ps1 --ack <job_id>` (or `--ack all`) marks jobs handled
# so they drop off the enforcement surface. History stays in the sentinel.
#
# Output: a single JSON object to stdout per oracle.json schema.

$ErrorActionPreference = "Continue"
try { [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new() } catch { }
try { $OutputEncoding = [System.Text.UTF8Encoding]::new() } catch { }
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# Stall threshold: if a RUNNING job's heartbeat is older than this, it is
# flagged `stalled` (the wrapper likely died, taking its heartbeat with it).
# Default = 4x a 15s heartbeat + slack. Override with $env:GC_JOB_STALL_SECONDS.
$stallSeconds = 90
if ($env:GC_JOB_STALL_SECONDS -and ($env:GC_JOB_STALL_SECONDS -as [int])) {
    $stallSeconds = [int]$env:GC_JOB_STALL_SECONDS
}

# Resolve repo root: this oracle lives at
# .claude/oracles/background-job-status/run.ps1 -> ../../../
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $oracleDir "..\..\..")).Path
$jobsDir = Join-Path $repoRoot ".claude\session-state\jobs"

function Emit($obj) {
    $obj | ConvertTo-Json -Compress -Depth 8
}

# Parse args: optional --ack <id|all>, otherwise a selector.
$selector = "all"
$ackTarget = $null
if ($args.Count -ge 1) {
    if ($args[0] -eq "--ack") {
        $ackTarget = if ($args.Count -ge 2) { [string]$args[1] } else { "all" }
    } else {
        $selector = [string]$args[0]
    }
}

# Graceful absence: no jobs dir yet -> empty answer (never an error).
if (-not (Test-Path $jobsDir)) {
    Emit ([ordered]@{
        ok = $true
        jobs = @()
        count = 0
        pending = @()
        unacknowledged = @()
        briefing = ""
        summary = "No tracked jobs (jobs dir does not exist yet)."
        stall_seconds = $stallSeconds
    })
    exit 0
}

$sentinelFiles = Get-ChildItem -Path $jobsDir -Filter "*.json" -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notlike "*.tmp" -and $_.Name -notlike "*.hb.tmp" }

function Read-Sentinel($file) {
    try {
        $raw = Get-Content -LiteralPath $file.FullName -Raw -Encoding UTF8 -ErrorAction Stop
        return ($raw | ConvertFrom-Json -ErrorAction Stop)
    } catch { return $null }
}

function Age-Seconds($iso) {
    if ([string]::IsNullOrWhiteSpace($iso)) { return $null }
    try {
        $t = [datetime]::Parse($iso, [System.Globalization.CultureInfo]::InvariantCulture, [System.Globalization.DateTimeStyles]::AdjustToUniversal -bor [System.Globalization.DateTimeStyles]::AssumeUniversal)
        return [int]([math]::Round(((Get-Date).ToUniversalTime() - $t).TotalSeconds))
    } catch { return $null }
}

# --- Acknowledge path -------------------------------------------------------
if ($null -ne $ackTarget) {
    $acked = @()
    foreach ($f in $sentinelFiles) {
        $s = Read-Sentinel $f
        if ($null -eq $s) { continue }
        if ($ackTarget -eq "all" -or $s.job_id -eq $ackTarget) {
            # Only meaningful to ack a finished job, but allow ack on any.
            try {
                $s | Add-Member -NotePropertyName acknowledged -NotePropertyValue $true -Force
                $json = $s | ConvertTo-Json -Depth 8
                $tmp = "$($f.FullName).ack.tmp"
                [System.IO.File]::WriteAllText($tmp, $json, $utf8NoBom)
                Move-Item -LiteralPath $tmp -Destination $f.FullName -Force
                $acked += [string]$s.job_id
            } catch { }
        }
    }
    Emit ([ordered]@{
        ok = $true
        acknowledged = $acked
        count = $acked.Count
        briefing = ""
        summary = "Acknowledged $($acked.Count) job(s): $($acked -join ', ')"
    })
    exit 0
}

# --- Build the job list -----------------------------------------------------
$jobs = @()
foreach ($f in $sentinelFiles) {
    $s = Read-Sentinel $f
    if ($null -eq $s) { continue }

    $status = [string]$s.status
    $hbAge = Age-Seconds $s.last_heartbeat_at
    $stalled = $false
    # A job is stalled only if it still claims to be running but its heartbeat
    # is stale (wrapper likely died). Done/failed jobs are never "stalled".
    if ($status -eq "running" -and $null -ne $hbAge -and $hbAge -gt $stallSeconds) {
        $stalled = $true
    }
    $acknowledged = $false
    if ($s.PSObject.Properties.Name -contains "acknowledged") { $acknowledged = [bool]$s.acknowledged }

    $done = ($status -eq "done" -or $status -eq "failed")

    # SACT-03: session attribution -- additive; null for pre-SACT sentinels.
    $sessionId = $null
    if ($s.PSObject.Properties.Name -contains "session_id" -and $s.session_id) { $sessionId = [string]$s.session_id }

    $jobs += [ordered]@{
        job_id              = [string]$s.job_id
        label               = [string]$s.label
        session_id          = $sessionId
        status              = $status
        done                = $done
        exit_code           = $s.exit_code
        stalled             = $stalled
        seconds_since_heartbeat = $hbAge
        started_at          = [string]$s.started_at
        ended_at            = if ($s.ended_at) { [string]$s.ended_at } else { $null }
        last_heartbeat_at   = [string]$s.last_heartbeat_at
        acknowledged        = $acknowledged
        log_path            = [string]$s.log_path
        cmd                 = [string]$s.cmd
    }
}

# Sort newest-first by started_at.
$jobs = @($jobs | Sort-Object -Property started_at -Descending)

# --- Selector filtering -----------------------------------------------------
$selected = $jobs
if ($selector -eq "latest") {
    $selected = @($jobs | Select-Object -First 1)
} elseif ($selector -ne "all") {
    $selected = @($jobs | Where-Object { $_.job_id -eq $selector })
    if ($selected.Count -eq 0) {
        Emit ([ordered]@{
            ok = $false
            job_id = $selector
            found = $false
            briefing = ""
            summary = "No tracked job with id '$selector'."
            stall_seconds = $stallSeconds
        })
        exit 0
    }
}

# --- Enforcement: pending + completed-but-unacknowledged --------------------
# Computed over ALL jobs (not just the selected subset) so the briefing line is
# stable regardless of selector.
$pending = @($jobs | Where-Object { $_.status -eq "running" })          # includes stalled (status still running)
$unacked = @($jobs | Where-Object { $_.done -and -not $_.acknowledged })

$briefing = ""
$parts = @()
if ($pending.Count -gt 0) {
    $p = $pending | ForEach-Object {
        $stallTag = if ($_.stalled) { " STALLED($($_.seconds_since_heartbeat)s)" } else { "" }
        $ownerTag = if ($_.session_id) { "@$($_.session_id)" } else { "" }
        "$($_.job_id)[$($_.label)]$ownerTag$stallTag"
    }
    $parts += "$($pending.Count) running: " + ($p -join ", ")
}
if ($unacked.Count -gt 0) {
    $u = $unacked | ForEach-Object {
        $verdict = if ($_.status -eq "failed") { "FAILED(rc=$($_.exit_code))" } else { "done" }
        "$($_.job_id)[$($_.label)]=$verdict"
    }
    $parts += "$($unacked.Count) finished-unacknowledged: " + ($u -join ", ")
}
if ($parts.Count -gt 0) {
    $joined = ($parts -join " | ")
    $briefing = "tracked jobs need attention -- $joined (status: .claude/oracles/background-job-status/run.ps1 <id>; ack: ... --ack <id>)"
}

$summary = "$($jobs.Count) tracked job(s); $($pending.Count) running, $($unacked.Count) finished-unacknowledged"

Emit ([ordered]@{
    ok = $true
    jobs = $selected
    count = $selected.Count
    total = $jobs.Count
    pending = @($pending | ForEach-Object { $_.job_id })
    unacknowledged = @($unacked | ForEach-Object { $_.job_id })
    stall_seconds = $stallSeconds
    briefing = $briefing
    summary = $summary
})
exit 0
