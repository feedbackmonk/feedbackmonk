# dispatchable-sessions oracle (Windows PowerShell)
# Answers: what live sibling sessions can THIS session dispatch work to right now?
#
# Reads .claude/collaboration/active-sessions.json (with ltads/sessions/active-sessions.json
# as legacy fallback) and emits a JSON object with:
#   - count   : integer, number of live dispatchable peers
#   - peers   : array of {sessionId, sessionRole, role, workDir, claudeShellPid, dispatchable, spawnedAt, siblingGroup?}
#   - briefing: human-readable one-line summary for the session-start ORACLE BRIEFING
#
# Filter: status=='active' AND dispatchable==true AND claudeShellPid!=null AND PID alive.
# Legacy entries (no registryVersion or registryVersion=1) silently drop -- they predate dispatch.
# Strategy: always-fresh. Read-only on the registry (no mutation; stale-cleanup is a separate path).
#
# Modes (CSI-05 added --gc, --gc-cheap):
#   (default)    : read-only briefing path described above
#   --gc-cheap   : session-start hygiene sweep, ~100ms budget, defers if exceeded
#   --gc         : on-demand hygiene sweep, no time budget, prints {swept,before,after,threshold,thresholdSource}
#
# Sweep criteria (--gc / --gc-cheap):
#   status=='active' AND claudeShellPid!=null AND PID dead AND spawnedAt older than threshold (default 24h).
#   Action: flip status to 'expired' + sweptAt timestamp; move entry from sessions[] to closed[].
#   Threshold: .claude/config.json csi.registryHygieneThreshold (numeric hours OR ISO-8601 PnH/PnD), default 24.
# CSI-05 closes DISC-PRO-05's REGISTRY-GC-01 follow-up.

param(
    [switch]$gc,
    [switch]$gcCheap
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# Sync .NET CWD with PowerShell's current location -- defends against the
# WriteAllText/ReadAllBytes class of CWD bug surfaced during CSI-01 smoke.
try { [Environment]::CurrentDirectory = (Get-Location).Path } catch { }

# Accept --gc / --gc-cheap as positional argv tokens (PowerShell's param block
# turns -gc into a switch, but bash-style "--gc" arrives in $args).
$mode = "briefing"
if ($gc) { $mode = "gc" }
if ($gcCheap) { $mode = "gc-cheap" }
foreach ($a in $args) {
    switch ($a) {
        "--gc"        { $mode = "gc" }
        "--gc-cheap"  { $mode = "gc-cheap" }
        default {
            if ($a -is [string] -and $a.StartsWith("--")) {
                Write-Error "dispatchable-sessions: unknown mode: $a"
                exit 1
            }
        }
    }
}

function Emit-Empty {
    Write-Output '{"count":0,"peers":[],"briefing":"No live siblings. /0-uldf-dispatch unavailable."}'
    exit 0
}

# ---- Locate the registry file (first-match wins) ----
# HYGIENE-04: registry-write helpers consumed below validate path-is-absolute.
# Resolve via (Get-Location).Path so the path is absolute regardless of caller cwd.
$_dsPwd = (Get-Location).Path
$registry = $null
if (Test-Path ".claude/collaboration/active-sessions.json") {
    $registry = Join-Path $_dsPwd ".claude/collaboration/active-sessions.json"
} elseif (Test-Path "ltads/sessions/active-sessions.json") {
    $registry = Join-Path $_dsPwd "ltads/sessions/active-sessions.json"
}

if (-not $registry) {
    if ($mode -eq "briefing") { Emit-Empty }
    if ($mode -eq "gc") {
        Write-Output '{"swept":0,"before":0,"after":0,"threshold":"P1D","thresholdSource":"default","note":"no registry"}'
    }
    exit 0
}

# ---- Liveness probe ----
function Test-PidAlive {
    param([int]$ProcId)
    if ($ProcId -le 0) { return $false }
    try {
        $proc = Get-Process -Id $ProcId -ErrorAction SilentlyContinue
        return $null -ne $proc
    } catch {
        return $false
    }
}

# ---- UTF-8-no-BOM read helper (handles BOM) ----
function Read-RegistryJson {
    param([string]$Path)
    try {
        $bytes = [System.IO.File]::ReadAllBytes($Path)
        if ($bytes.Length -eq 0) { return $null }
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            $jsonText = [System.Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
        } else {
            $jsonText = [System.Text.Encoding]::UTF8.GetString($bytes)
        }
        if ([string]::IsNullOrWhiteSpace($jsonText)) { return $null }
        return $jsonText | ConvertFrom-Json -ErrorAction Stop
    } catch {
        return $null
    }
}

$data = Read-RegistryJson -Path $registry
if ($null -eq $data) {
    if ($mode -eq "briefing") { Emit-Empty }
    if ($mode -eq "gc") {
        Write-Output '{"swept":0,"before":0,"after":0,"threshold":"P1D","thresholdSource":"default","note":"unparseable registry"}'
    }
    exit 0
}

$sessions = @()
if ($data.PSObject.Properties.Name -contains "sessions") {
    $sessions = @($data.sessions)
}

# =============================================================================
# Mode dispatch
# =============================================================================
if ($mode -eq "gc" -or $mode -eq "gc-cheap") {
    # -------------------------------------------------------------------------
    # CSI-05 hygiene sweep
    # -------------------------------------------------------------------------

    # ---- Read threshold from .claude/config.json -----------------------------
    $thresholdHours   = 24
    $thresholdSource  = "default"
    $thresholdDisplay = "P1D"

    if (Test-Path ".claude/config.json") {
        $cfg = Read-RegistryJson -Path ".claude/config.json"
        if ($null -ne $cfg -and $cfg.PSObject.Properties.Name -contains "csi" -and $null -ne $cfg.csi) {
            if ($cfg.csi.PSObject.Properties.Name -contains "registryHygieneThreshold") {
                $raw = $cfg.csi.registryHygieneThreshold
                if ($null -ne $raw) {
                    if ($raw -is [int] -or $raw -is [long] -or $raw -is [double]) {
                        $thresholdHours = [int]$raw
                        $thresholdSource = "config"
                        $thresholdDisplay = "PT${thresholdHours}H"
                    } elseif ($raw -is [string]) {
                        $rs = $raw.Trim()
                        if ($rs -match '^[0-9]+$') {
                            $thresholdHours = [int]$rs
                            $thresholdSource = "config"
                            $thresholdDisplay = "PT${thresholdHours}H"
                        } elseif ($rs -match '^P([0-9]+)H$') {
                            $thresholdHours = [int]$Matches[1]
                            $thresholdSource = "config"
                            $thresholdDisplay = $rs
                        } elseif ($rs -match '^P([0-9]+)D$') {
                            $thresholdHours = [int]$Matches[1] * 24
                            $thresholdSource = "config"
                            $thresholdDisplay = $rs
                        }
                    }
                }
            }
        }
    }

    $now       = (Get-Date).ToUniversalTime()
    $cutoff    = $now.AddHours(-1 * $thresholdHours)
    $nowIso    = $now.ToString('yyyy-MM-ddTHH:mm:ssZ')

    $budgetMs = 100
    if ($mode -eq "gc") { $budgetMs = 0 }
    $startTicks = [System.Diagnostics.Stopwatch]::StartNew()

    $before = $sessions.Count

    # Identify candidates -> apply liveness + age filter -> mark for sweep.
    $sweepIndices = New-Object System.Collections.ArrayList
    $sweepIds     = New-Object System.Collections.ArrayList
    $budgetExceeded = $false

    for ($i = 0; $i -lt $sessions.Count; $i++) {
        if ($budgetMs -gt 0 -and $startTicks.ElapsedMilliseconds -gt $budgetMs) {
            $budgetExceeded = $true
            break
        }

        $s = $sessions[$i]
        if ($null -eq $s) { continue }
        if ($s.status -ne "active") { continue }
        if ($null -eq $s.claudeShellPid) { continue }

        $pidVal = 0
        try { $pidVal = [int]$s.claudeShellPid } catch { continue }
        if ($pidVal -le 0) { continue }

        # Live PIDs are NEVER swept regardless of age.
        if (Test-PidAlive -ProcId $pidVal) { continue }

        # Age check: spawnedAt must be older than cutoff. Missing spawnedAt -> sweepable.
        if ($s.spawnedAt) {
            $spawned = $null
            try {
                $spawned = [System.DateTime]::Parse(
                    [string]$s.spawnedAt,
                    [System.Globalization.CultureInfo]::InvariantCulture,
                    [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal
                )
            } catch {
                $spawned = $null
            }
            if ($null -ne $spawned -and $spawned -gt $cutoff) {
                continue
            }
        }

        [void]$sweepIndices.Add($i)
        [void]$sweepIds.Add([string]$s.id)
    }

    $sweptCount = $sweepIndices.Count

    if ($sweptCount -gt 0) {
        # ---- Acquire lock + atomic write --------------------------------------
        $lockDir = "$registry.lock"
        $lockOk  = $false
        # 4 attempts spaced by 50ms / 200ms / 800ms (1050ms full retry budget,
        # mirrors registry-write.ps1 per DEC-22).
        $delays  = @(50, 200, 800)
        for ($attempt = 0; $attempt -lt 4 -and -not $lockOk; $attempt++) {
            try {
                New-Item -ItemType Directory -Path $lockDir -ErrorAction Stop | Out-Null
                $lockOk = $true
            } catch {
                if ($attempt -lt 3) { Start-Sleep -Milliseconds $delays[$attempt] }
            }
        }

        if (-not $lockOk) {
            if ($mode -eq "gc") {
                $note = '"note":"lock contention"'
                Write-Output "{`"swept`":0,`"before`":$before,`"after`":$before,`"threshold`":`"$thresholdDisplay`",`"thresholdSource`":`"$thresholdSource`",$note}"
            }
            exit 0
        }

        try {
            # Re-read inside the lock to avoid TOCTOU.
            $data2 = Read-RegistryJson -Path $registry
            if ($null -eq $data2) {
                Remove-Item $lockDir -Force -ErrorAction SilentlyContinue
                if ($mode -eq "gc") {
                    Write-Output "{`"swept`":0,`"before`":$before,`"after`":$before,`"threshold`":`"$thresholdDisplay`",`"thresholdSource`":`"$thresholdSource`",`"note`":`"reread failed`"}"
                }
                exit 0
            }
            if (-not ($data2.PSObject.Properties.Name -contains "sessions") -or $null -eq $data2.sessions) {
                $data2 | Add-Member -NotePropertyName sessions -NotePropertyValue @() -Force
            }
            if (-not ($data2.PSObject.Properties.Name -contains "closed") -or $null -eq $data2.closed) {
                $data2 | Add-Member -NotePropertyName closed -NotePropertyValue @() -Force
            }

            $reread = @($data2.sessions)

            # Verify the indices we computed still match (status=active, dead, old).
            # If the registry mutated since our scan, validate per-entry; tolerate.
            $validIndices = New-Object System.Collections.ArrayList
            foreach ($idx in $sweepIndices) {
                if ($idx -ge $reread.Count) { continue }
                $entry = $reread[$idx]
                if ($null -eq $entry) { continue }
                if ($entry.status -ne "active") { continue }
                if ($null -eq $entry.claudeShellPid) { continue }
                $pv = 0
                try { $pv = [int]$entry.claudeShellPid } catch { continue }
                if (Test-PidAlive -ProcId $pv) { continue }
                [void]$validIndices.Add($idx)
            }

            $newSessions = New-Object System.Collections.ArrayList
            $expired     = New-Object System.Collections.ArrayList
            for ($i = 0; $i -lt $reread.Count; $i++) {
                if ($validIndices.Contains($i)) {
                    $orig = $reread[$i]
                    $copy = [ordered]@{}
                    foreach ($prop in $orig.PSObject.Properties) {
                        $copy[$prop.Name] = $prop.Value
                    }
                    $copy["status"]   = "expired"
                    $copy["sweptAt"]  = $nowIso
                    [void]$expired.Add([pscustomobject]$copy)
                } else {
                    [void]$newSessions.Add($reread[$i])
                }
            }

            $data2.sessions = $newSessions.ToArray()
            $existingClosed = @($data2.closed)
            $data2.closed   = $existingClosed + $expired.ToArray()

            if ($data2.PSObject.Properties.Name -contains "lastUpdated") {
                $data2.lastUpdated = $nowIso
            } else {
                $data2 | Add-Member -NotePropertyName lastUpdated -NotePropertyValue $nowIso -Force
            }
            if ($data2.PSObject.Properties.Name -contains "lastPrunedAt") {
                $data2.lastPrunedAt = $nowIso
            } else {
                $data2 | Add-Member -NotePropertyName lastPrunedAt -NotePropertyValue $nowIso -Force
            }

            $newJson = $data2 | ConvertTo-Json -Depth 10
            $tmp = "$registry.tmp.$PID"
            try {
                $utf8NoBom = New-Object System.Text.UTF8Encoding $false
                [System.IO.File]::WriteAllText($tmp, $newJson, $utf8NoBom)
                Move-Item -Path $tmp -Destination $registry -Force
                $sweptCount = $validIndices.Count
            } catch {
                Remove-Item $tmp -Force -ErrorAction SilentlyContinue
                $sweptCount = 0
            }
        } finally {
            Remove-Item $lockDir -Force -ErrorAction SilentlyContinue
        }

        # =====================================================================
        # CSI-13: After registry close, reconcile local LTADS state.
        # =====================================================================
        # For each newly-expired entry whose workDir matches THIS GC-running
        # session's project, flip the matching ltads/sessions/current-session.md
        # to CONCLUDED (Concluded-By: csi-05-gc-sweep). Cross-workDir
        # reconciliation forbidden per Phase 1.6 plan -- shared-repo state
        # is reconciled by SHARED-CSI-04 paths, not here.
        #
        # Graceful absence: missing lib, missing current-session.md, or
        # sessionId mismatch all result in silent no-op (Probandurgy).
        if ($sweptCount -gt 0 -and $expired -and $expired.Count -gt 0) {
            $thisProjectRoot = (Get-Location).Path
            $libCandidates = @(
                (Join-Path $PSScriptRoot "../../scripts/lib/registry-write.ps1"),
                (Join-Path $env:USERPROFILE ".claude/scripts/lib/registry-write.ps1")
            )
            $csiLibPath = $null
            foreach ($cand in $libCandidates) {
                if (Test-Path $cand) { $csiLibPath = $cand; break }
            }
            if ($csiLibPath) {
                try {
                    . $csiLibPath
                } catch {
                    $csiLibPath = $null
                }
            }
            if ($csiLibPath) {
                foreach ($expEntry in $expired) {
                    if ($null -eq $expEntry) { continue }
                    $entryWorkDir = ""
                    if ($expEntry.PSObject.Properties.Name -contains 'workDir' -and $expEntry.workDir) {
                        $entryWorkDir = [string]$expEntry.workDir
                    }
                    if ([string]::IsNullOrWhiteSpace($entryWorkDir)) { continue }

                    # Normalize for comparison (Windows path separators vary).
                    $entryNorm   = $entryWorkDir -replace '\\','/' -replace '/+$',''
                    $projectNorm = $thisProjectRoot -replace '\\','/' -replace '/+$',''
                    if ($entryNorm -ne $projectNorm) { continue }

                    $entryId = ""
                    if ($expEntry.PSObject.Properties.Name -contains 'id' -and $expEntry.id) {
                        $entryId = [string]$expEntry.id
                    }
                    if ([string]::IsNullOrWhiteSpace($entryId)) { continue }

                    $csMd = Join-Path $thisProjectRoot "ltads/sessions/current-session.md"
                    if (-not (Test-Path $csMd)) { continue }

                    # Verify the current-session.md is about THIS swept session
                    # (not a different session that happens to share the project).
                    try {
                        $csContent = Get-Content $csMd -Raw -Encoding UTF8 -ErrorAction Stop
                    } catch { continue }
                    if (-not ($csContent -match '(?m)^Session:\s*(\S+)')) { continue }
                    $csSessionId = $Matches[1].Trim()
                    if ($csSessionId -ne $entryId) { continue }

                    try {
                        Invoke-CsiFlipCurrentSessionConcluded -CurrentSessionPath $csMd -SessionId $entryId -ConcludedBy "csi-05-gc-sweep" | Out-Null
                    } catch {
                        # Graceful absence
                    }
                }
            }
        }
    }

    $after = $before - $sweptCount

    # =========================================================================
    # SHARED-CSI-06: Cross-repo --gc-cheap extension
    # =========================================================================
    # In cheap mode, after the local sweep, iterate shared-repo registries and
    # apply the same staleness criteria (status="active" AND PID dead AND
    # spawnedAt older than threshold). Per-shared-repo budget <=50ms; cumulative
    # gate honors $budgetMs. Always exits 0; never blocks the briefing.
    if ($mode -eq "gc-cheap") {
        $sharedRepoPaths = @()

        # Prefer cached oracle output (set by SHARED-CSI-02 in session-start).
        $stateFile = ".claude/session-state/this-session.json"
        if (Test-Path $stateFile) {
            try {
                $stateRaw = Get-Content $stateFile -Raw -Encoding UTF8 -ErrorAction Stop
                if (-not [string]::IsNullOrWhiteSpace($stateRaw)) {
                    $stateObj = $stateRaw | ConvertFrom-Json -ErrorAction Stop
                    if ($stateObj -and ($stateObj.PSObject.Properties.Name -contains 'sharedRepos') -and $stateObj.sharedRepos) {
                        if ($stateObj.sharedRepos.PSObject.Properties.Name -contains 'repos' -and $stateObj.sharedRepos.repos) {
                            foreach ($r in @($stateObj.sharedRepos.repos)) {
                                if ($null -ne $r -and $r.path) {
                                    $sharedRepoPaths += [string]$r.path
                                }
                            }
                        }
                    }
                }
            } catch { }
        }

        # Fall back to invoking the discovery oracle if cache was missing.
        if ($sharedRepoPaths.Count -eq 0) {
            $candPaths = @(
                ".claude/oracles/workspace-shared-repos/run.ps1",
                "claude-template/oracles/workspace-shared-repos/run.ps1"
            )
            foreach ($cand in $candPaths) {
                if (Test-Path $cand) {
                    try {
                        $discRaw = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $cand 2>$null
                        if ($discRaw) {
                            $discStr = if ($discRaw -is [array]) { $discRaw -join "" } else { [string]$discRaw }
                            $discObj = $discStr | ConvertFrom-Json -ErrorAction Stop
                            if ($discObj.PSObject.Properties.Name -contains 'repos' -and $discObj.repos) {
                                foreach ($r in @($discObj.repos)) {
                                    if ($null -ne $r -and $r.path) {
                                        $sharedRepoPaths += [string]$r.path
                                    }
                                }
                            }
                        }
                    } catch { }
                    break
                }
            }
        }

        # Per-repo budget per spec SHARED-CSI-06. 200ms accommodates Get-Process
        # cold-start cost (~30-80ms) on slow Windows test environments while
        # still bounding total work. Production hot paths exit far under this.
        $sharedPerRepoBudgetMs = 200
        # Shared-loop cumulative budget is independent of the local cheap-mode
        # budget ($budgetMs=100 above) so local cost never starves shared sweeps.
        # 1000ms covers 5 repos at 200ms each.
        $sharedLoopBudgetMs = 1000
        $sharedLoopStart = [System.Diagnostics.Stopwatch]::StartNew()
        foreach ($sharedPath in $sharedRepoPaths) {
            if ([string]::IsNullOrEmpty($sharedPath)) { continue }

            # Cumulative shared-loop budget gate
            if ($sharedLoopStart.ElapsedMilliseconds -gt $sharedLoopBudgetMs) {
                $budgetExceeded = $true
                break
            }

            $sharedReg = Join-Path $sharedPath ".claude/collaboration/active-sessions.json"
            if (-not (Test-Path $sharedReg)) { continue }

            $sharedReg2 = Read-RegistryJson -Path $sharedReg
            if ($null -eq $sharedReg2) { continue }
            $sharedSessions = @()
            if ($sharedReg2.PSObject.Properties.Name -contains 'sessions') {
                $sharedSessions = @($sharedReg2.sessions)
            }
            if ($sharedSessions.Count -eq 0) { continue }

            $sharedSweepIndices = New-Object System.Collections.ArrayList
            $sharedRepoStart = [System.Diagnostics.Stopwatch]::StartNew()

            for ($si = 0; $si -lt $sharedSessions.Count; $si++) {
                if ($sharedRepoStart.ElapsedMilliseconds -gt $sharedPerRepoBudgetMs) { break }

                $entry = $sharedSessions[$si]
                if ($null -eq $entry) { continue }
                if ($entry.status -ne "active") { continue }
                if ($null -eq $entry.claudeShellPid) { continue }
                $pidVal = 0
                try { $pidVal = [int]$entry.claudeShellPid } catch { continue }
                if ($pidVal -le 0) { continue }
                if (Test-PidAlive -ProcId $pidVal) { continue }

                if ($entry.spawnedAt) {
                    try {
                        $spawned = [System.DateTime]::Parse(
                            [string]$entry.spawnedAt,
                            [System.Globalization.CultureInfo]::InvariantCulture,
                            [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal
                        )
                        if ($spawned -gt $cutoff) { continue }
                    } catch { }
                }

                [void]$sharedSweepIndices.Add($si)
            }

            if ($sharedSweepIndices.Count -eq 0) { continue }

            $sharedLock = "$sharedReg.lock"
            $sharedLockOk = $false
            for ($attempt = 0; $attempt -lt 4 -and -not $sharedLockOk; $attempt++) {
                try {
                    New-Item -ItemType Directory -Path $sharedLock -ErrorAction Stop | Out-Null
                    $sharedLockOk = $true
                } catch {
                    if ($attempt -lt 3) { Start-Sleep -Milliseconds $delays[$attempt] }
                }
            }
            if (-not $sharedLockOk) { continue }

            try {
                # Re-read inside lock (TOCTOU defense)
                $sharedRereadObj = Read-RegistryJson -Path $sharedReg
                if ($null -eq $sharedRereadObj) { continue }
                if (-not ($sharedRereadObj.PSObject.Properties.Name -contains 'sessions')) {
                    $sharedRereadObj | Add-Member -NotePropertyName sessions -NotePropertyValue @() -Force
                }
                if (-not ($sharedRereadObj.PSObject.Properties.Name -contains 'closed')) {
                    $sharedRereadObj | Add-Member -NotePropertyName closed -NotePropertyValue @() -Force
                }
                $sharedReread = @($sharedRereadObj.sessions)

                $sharedNew = New-Object System.Collections.ArrayList
                $sharedExpired = New-Object System.Collections.ArrayList
                for ($si = 0; $si -lt $sharedReread.Count; $si++) {
                    if ($sharedSweepIndices.Contains($si)) {
                        $orig = $sharedReread[$si]
                        if ($null -eq $orig) { continue }
                        # Re-check liveness before actually expiring
                        $pv = 0
                        try { $pv = [int]$orig.claudeShellPid } catch { $pv = 0 }
                        if ($pv -gt 0 -and (Test-PidAlive -ProcId $pv)) {
                            [void]$sharedNew.Add($orig)
                            continue
                        }
                        $copy = [ordered]@{}
                        foreach ($prop in $orig.PSObject.Properties) {
                            $copy[$prop.Name] = $prop.Value
                        }
                        $copy['status']  = 'expired'
                        $copy['sweptAt'] = $nowIso
                        [void]$sharedExpired.Add([pscustomobject]$copy)
                    } else {
                        [void]$sharedNew.Add($sharedReread[$si])
                    }
                }
                $sharedRereadObj.sessions = $sharedNew.ToArray()
                $existingSharedClosed = @($sharedRereadObj.closed)
                $sharedRereadObj.closed = $existingSharedClosed + $sharedExpired.ToArray()
                if ($sharedRereadObj.PSObject.Properties.Name -contains 'lastUpdated') {
                    $sharedRereadObj.lastUpdated = $nowIso
                } else {
                    $sharedRereadObj | Add-Member -NotePropertyName lastUpdated -NotePropertyValue $nowIso -Force
                }
                if ($sharedRereadObj.PSObject.Properties.Name -contains 'lastPrunedAt') {
                    $sharedRereadObj.lastPrunedAt = $nowIso
                } else {
                    $sharedRereadObj | Add-Member -NotePropertyName lastPrunedAt -NotePropertyValue $nowIso -Force
                }

                $sharedJson = $sharedRereadObj | ConvertTo-Json -Depth 10
                $sharedTmp = "$sharedReg.tmp.$PID"
                try {
                    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
                    [System.IO.File]::WriteAllText($sharedTmp, $sharedJson, $utf8NoBom)
                    Move-Item -Path $sharedTmp -Destination $sharedReg -Force
                } catch {
                    Remove-Item $sharedTmp -Force -ErrorAction SilentlyContinue
                }
            } finally {
                Remove-Item $sharedLock -Force -ErrorAction SilentlyContinue
            }
        }
    }

    if ($mode -eq "gc") {
        $idsCsv = ""
        if ($sweepIds.Count -gt 0) { $idsCsv = ($sweepIds -join ",") }
        $parts = @(
            "`"swept`":$sweptCount",
            "`"before`":$before",
            "`"after`":$after",
            "`"threshold`":`"$thresholdDisplay`"",
            "`"thresholdSource`":`"$thresholdSource`""
        )
        if ($budgetExceeded) { $parts += "`"budgetExceeded`":true" }
        if ($idsCsv) { $parts += "`"sweptIds`":`"$idsCsv`"" }
        Write-Output ("{" + ($parts -join ",") + "}")
    }
    exit 0
}

# =============================================================================
# Default mode: briefing path
# =============================================================================

if ($sessions.Count -eq 0) { Emit-Empty }

# ---- Filter + liveness check ----
$peers = @()
foreach ($s in $sessions) {
    if (-not $s) { continue }
    if ($s.status -ne "active") { continue }
    if (-not $s.dispatchable) { continue }
    if ($null -eq $s.claudeShellPid) { continue }

    $pidVal = 0
    try { $pidVal = [int]$s.claudeShellPid } catch { continue }
    if ($pidVal -le 0) { continue }

    if (-not (Test-PidAlive -ProcId $pidVal)) { continue }

    $peerObj = [ordered]@{
        sessionId      = if ($s.id) { [string]$s.id } else { "" }
        sessionRole    = if ($s.sessionRole) { [string]$s.sessionRole } else { "" }
        role           = if ($s.role) { [string]$s.role } else { "" }
        workDir        = if ($s.workDir) { [string]$s.workDir } else { "" }
        claudeShellPid = $pidVal
        dispatchable   = $true
        spawnedAt      = if ($s.spawnedAt) { [string]$s.spawnedAt } else { "" }
    }
    # WT-03: additively include siblingGroup when present (omit when absent;
    # frozen-schema additive contract for v1 consumers).
    if ($s.siblingGroup -and -not [string]::IsNullOrEmpty([string]$s.siblingGroup)) {
        $peerObj.siblingGroup = [string]$s.siblingGroup
    }
    $peers += $peerObj
}

if ($peers.Count -eq 0) { Emit-Empty }

# ---- Build briefing line ----
$labels = $peers | ForEach-Object { "$($_.sessionId) ($($_.sessionRole))" }
$briefing = "$($peers.Count) live sibling(s): " + ($labels -join ", ")

# ---- Emit (compressed JSON, single line) ----
$result = [ordered]@{
    count    = $peers.Count
    peers    = @($peers)
    briefing = $briefing
}

$result | ConvertTo-Json -Compress -Depth 5
