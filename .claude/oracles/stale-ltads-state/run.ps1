# stale-ltads-state oracle (Windows)
#
# CSI-14 (Phase 1.6): emit a [stale-ltads-state] briefing line when the
# topmost arc in ltads/arc-state.json has status ACTIVE/PAUSED but the
# matching active-sessions.json entry is closed/expired/missing/PID-dead.
#
# Output: single-line JSON (always-fresh; ~60ms budget).
# Gracefully absent: when state is consistent, briefing field is empty so the
# session-start hook emits no line.
#
# Field reading (ARC-03 migration, DEC-199) -- byte-identical verdicts with
# run.sh:
#   Status + correlation id come from the ARC-02 arc-state lib
#   (scripts/lib/arc-state.ps1 -- Get-ArcStateStatus / Get-ArcStateArcOwnerId).
#   Correlation id = most-recent checkpoints[].by of the topmost arc (DEC-44).
#   The arc `id` (e.g. A042) is never a registry key, so it is NOT used. No
#   recoverable id -> degrade to consistent (stale:false). Legacy prose-only
#   projects (no arc-state.json) degrade to consistent -- the ltads-state
#   oracle's `legacy` verdict surfaces the migration.

$ErrorActionPreference = 'SilentlyContinue'

$arcState = "ltads/arc-state.json"
$registry = ".claude/collaboration/active-sessions.json"

# ---- Dot-source the ARC-02 arc-state lib ------------------------------------
# Probe order mirrors pid-orphan-detector's lib resolution.
$astLibCandidates = @(
    (Join-Path $PSScriptRoot "../../scripts/lib/arc-state.ps1"),
    (Join-Path $PSScriptRoot "../../../claude-template/scripts/lib/arc-state.ps1"),
    (Join-Path $env:USERPROFILE ".claude/scripts/lib/arc-state.ps1")
)
foreach ($cand in $astLibCandidates) {
    if (Test-Path $cand) { . $cand; break }
}

# ---- SWEEP-10 / DEFER-095: identity-aware liveness via lib/pid-liveness.ps1 --
# A recycled owner pid defended a stale ACTIVE arc forever. Identity-refused ->
# dead -> registry-pid-dead-state-active. Anchor-only, no name glob (DEC-257);
# absent anchor or lib unavailable -> byte-identical existence-only verdict.
# Path is REPORTABLE (QUIESCE-08 W4). Dot-sourced at SCRIPT scope (sourcing
# inside a function scopes the lib functions to the function).
$slsPidIdentity = 'fallback'
foreach ($slsPlCand in @(
    (Join-Path $PSScriptRoot "../../scripts/lib/pid-liveness.ps1"),
    (Join-Path $PSScriptRoot "../../../claude-template/scripts/lib/pid-liveness.ps1"),
    (Join-Path $env:USERPROFILE ".claude/scripts/lib/pid-liveness.ps1")
)) {
    if (Test-Path -LiteralPath $slsPlCand) {
        try {
            if (-not (Test-Path variable:global:_UldfPidLivenessLoaded)) { $global:_UldfPidLivenessLoaded = $false }
            . $slsPlCand
            if (Get-Command Test-UldfPidAliveAs -ErrorAction SilentlyContinue) { $slsPidIdentity = 'lib' }
        } catch { }
        break
    }
}
if ($env:ULDF_SLS_REPORT_PID_IDENTITY -eq '1') {
    Write-Output $slsPidIdentity
    exit 0
}

function Emit-Json($obj) {
    $json = $obj | ConvertTo-Json -Compress -Depth 5
    Write-Output $json
    exit 0
}

function Emit-Consistent($statusValue, $sessionId) {
    $obj = [ordered]@{
        stale   = $false
        details = [ordered]@{
            current_session_status = $statusValue
            current_session_id     = $sessionId
            registry_status        = "active"
            registry_pid_alive     = $null
            inconsistency_kind     = "none"
        }
        briefing = ""
    }
    Emit-Json $obj
}

# ---- Graceful absence: no arc record (incl. legacy prose-only) --------------
if (-not (Test-Path $arcState)) {
    Emit-Consistent $null $null
}

# ---- Read topmost arc via the ARC-02 lib ------------------------------------
# If the lib could not be sourced, degrade to consistent rather than emit a
# stale verdict we cannot verify. Malformed documents yield $null getters.
if (-not (Get-Command Get-ArcStateStatus -ErrorAction SilentlyContinue)) {
    Write-Warning "stale-ltads-state: arc-state.ps1 not found; degrading to consistent"
    Emit-Consistent $null $null
}

$statusValue = Get-ArcStateStatus -Path $arcState

# Only ACTIVE/PAUSED warrant the inconsistency check (IN_PROGRESS was prose-era
# vocabulary; the schema normalizes it to ACTIVE at migration).
if ($statusValue -notin @('ACTIVE', 'PAUSED')) {
    Emit-Consistent $statusValue $null
}

$sessionId = Get-ArcStateArcOwnerId -Path $arcState

# No recoverable owner id -> can't correlate; degrade to consistent.
if ([string]::IsNullOrEmpty($sessionId)) {
    Emit-Consistent $statusValue $null
}

# ---- Registry missing -> stale (registry-missing-state-active) -------------
if (-not (Test-Path $registry)) {
    $obj = [ordered]@{
        stale   = $true
        details = [ordered]@{
            current_session_status = $statusValue
            current_session_id     = $sessionId
            registry_status        = "missing"
            registry_pid_alive     = $null
            inconsistency_kind     = "registry-missing-state-active"
        }
        briefing = "arc-state.json topmost arc: $statusValue (arc owner $sessionId) but active-sessions.json missing"
    }
    Emit-Json $obj
}

# ---- Find matching registry entry ------------------------------------------
$reg = $null
try {
    $rawText = $null
    $bytes = [System.IO.File]::ReadAllBytes($registry)
    if ($bytes.Length -gt 0) {
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            $rawText = [System.Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
        } else {
            $rawText = [System.Text.Encoding]::UTF8.GetString($bytes)
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($rawText)) {
        $reg = $rawText | ConvertFrom-Json -ErrorAction Stop
    }
} catch {
    Emit-Consistent $statusValue $sessionId
}
if ($null -eq $reg) {
    Emit-Consistent $statusValue $sessionId
}

$regStatus = "missing"
$regPid    = $null
$regAnchor = ''

if ($reg.PSObject.Properties.Name -contains 'sessions' -and $reg.sessions) {
    foreach ($s in @($reg.sessions)) {
        if ($null -ne $s -and $s.id -eq $sessionId) {
            $regStatus = "active"
            if ($s.PSObject.Properties.Name -contains 'claudeShellPid' -and $null -ne $s.claudeShellPid) {
                try { $regPid = [int]$s.claudeShellPid } catch { $regPid = $null }
                # DEFER-095: the identity anchor for the pid probe.
                if ($s.PSObject.Properties.Name -contains 'claudeShellPidWrittenAt' -and $null -ne $s.claudeShellPidWrittenAt) {
                    $regAnchor = [string]$s.claudeShellPidWrittenAt
                }
            }
            break
        }
    }
}

if ($regStatus -eq "missing" -and $reg.PSObject.Properties.Name -contains 'closed' -and $reg.closed) {
    foreach ($s in @($reg.closed)) {
        if ($null -ne $s -and $s.id -eq $sessionId) {
            if ($s.PSObject.Properties.Name -contains 'status' -and $s.status) {
                $regStatus = [string]$s.status
            } else {
                $regStatus = "closed"
            }
            break
        }
    }
}

# ---- Inconsistency classification ------------------------------------------
$inconsistencyKind = "none"
$pidAlive          = $null
$briefing          = ""

switch ($regStatus) {
    "active" {
        if ($regPid -gt 0) {
            try {
                # DEFER-095: identity-aware -- a recycled pid (started after the
                # anchor) reads dead; absent anchor/lib -> existence-only.
                $procAlive = $false
                if ($slsPidIdentity -eq 'lib') {
                    $procAlive = [bool](Test-UldfPidAliveAs $regPid $regAnchor)
                } else {
                    $procAlive = ($null -ne (Get-Process -Id $regPid -ErrorAction SilentlyContinue))
                }
                if ($procAlive) {
                    $pidAlive = $true
                } else {
                    $pidAlive = $false
                    $inconsistencyKind = "registry-pid-dead-state-active"
                    $briefing = "arc-state.json topmost arc: $statusValue (arc owner $sessionId) but that session's PID is dead -- next GC sweep will reconcile, or run /0-uldf-finalize manually"
                }
            } catch {
                $pidAlive = $null
            }
        }
    }
    "closed" {
        $inconsistencyKind = "registry-closed-state-active"
        $briefing = "arc-state.json topmost arc: $statusValue (arc owner $sessionId) but registry shows that session as CLOSED -- run /0-uldf-finalize --complete-arc to reconcile"
    }
    "expired" {
        $inconsistencyKind = "registry-expired-state-active"
        $briefing = "arc-state.json topmost arc: $statusValue (arc owner $sessionId) but registry shows that session as EXPIRED (CSI-05 GC swept it) -- state should have been auto-flipped by CSI-13"
    }
    "missing" {
        $inconsistencyKind = "registry-missing-state-active"
        $briefing = "arc-state.json topmost arc: $statusValue (arc owner $sessionId) but no matching registry entry -- that session never registered or the registry was reset"
    }
}

if ($inconsistencyKind -eq "none") {
    Emit-Consistent $statusValue $sessionId
}

$obj = [ordered]@{
    stale   = $true
    details = [ordered]@{
        current_session_status = $statusValue
        current_session_id     = $sessionId
        registry_status        = $regStatus
        registry_pid_alive     = $pidAlive
        inconsistency_kind     = $inconsistencyKind
    }
    briefing = $briefing
}
Emit-Json $obj
