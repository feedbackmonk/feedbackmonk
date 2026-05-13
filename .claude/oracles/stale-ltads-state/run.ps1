# stale-ltads-state oracle (Windows)
#
# CSI-14 (Phase 1.6): emit a [stale-ltads-state] briefing line when
# ltads/sessions/current-session.md Status is ACTIVE/PAUSED/IN_PROGRESS but
# the matching active-sessions.json entry is closed/expired/missing/PID-dead.
#
# Output: single-line JSON (always-fresh; ~60ms budget).
# Gracefully absent: when state is consistent, briefing field is empty so the
# session-start hook emits no line.

$ErrorActionPreference = 'SilentlyContinue'

$csMd     = "ltads/sessions/current-session.md"
$registry = ".claude/collaboration/active-sessions.json"

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

# ---- Graceful absence: no LTADS file ---------------------------------------
if (-not (Test-Path $csMd)) {
    Emit-Consistent $null $null
}

# ---- Read current-session.md -----------------------------------------------
$content = $null
try {
    $content = Get-Content $csMd -Raw -Encoding UTF8 -ErrorAction Stop
} catch {
    Emit-Consistent $null $null
}
if ([string]::IsNullOrEmpty($content)) {
    Emit-Consistent $null $null
}

$statusValue = $null
$sessionId   = $null
foreach ($line in ($content -split "`r?`n")) {
    if (-not $statusValue -and $line -match '^Status:\s*(\S+)') {
        $statusValue = $Matches[1].Trim()
    }
    if (-not $sessionId -and $line -match '^Session:\s*(\S+)') {
        $sessionId = $Matches[1].Trim()
    }
    if ($statusValue -and $sessionId) { break }
}

# Only ACTIVE/PAUSED/IN_PROGRESS warrant inconsistency check.
if ($statusValue -notin @('ACTIVE', 'PAUSED', 'IN_PROGRESS')) {
    Emit-Consistent $statusValue $sessionId
}

# Status is ACTIVE/PAUSED/IN_PROGRESS but no Session: id -> can't lookup.
if ([string]::IsNullOrEmpty($sessionId)) {
    Emit-Consistent $statusValue $sessionId
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
        briefing = "current-session.md Status: $statusValue (session $sessionId) but active-sessions.json missing"
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

if ($reg.PSObject.Properties.Name -contains 'sessions' -and $reg.sessions) {
    foreach ($s in @($reg.sessions)) {
        if ($null -ne $s -and $s.id -eq $sessionId) {
            $regStatus = "active"
            if ($s.PSObject.Properties.Name -contains 'claudeShellPid' -and $null -ne $s.claudeShellPid) {
                try { $regPid = [int]$s.claudeShellPid } catch { $regPid = $null }
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
                $proc = Get-Process -Id $regPid -ErrorAction SilentlyContinue
                if ($null -ne $proc) {
                    $pidAlive = $true
                } else {
                    $pidAlive = $false
                    $inconsistencyKind = "registry-pid-dead-state-active"
                    $briefing = "current-session.md Status: $statusValue (session $sessionId) but registered PID is dead -- next GC sweep will reconcile, or run /0-uldf-finalize manually"
                }
            } catch {
                $pidAlive = $null
            }
        }
    }
    "closed" {
        $inconsistencyKind = "registry-closed-state-active"
        $briefing = "current-session.md Status: $statusValue (session $sessionId) but registry shows entry as CLOSED -- run /0-uldf-finalize or /0-uldf-ltads-stop to reconcile"
    }
    "expired" {
        $inconsistencyKind = "registry-expired-state-active"
        $briefing = "current-session.md Status: $statusValue (session $sessionId) but registry shows entry as EXPIRED (CSI-05 GC swept it) -- state should have been auto-flipped by CSI-13"
    }
    "missing" {
        $inconsistencyKind = "registry-missing-state-active"
        $briefing = "current-session.md Status: $statusValue (session $sessionId) but no matching registry entry -- session never registered or registry was reset"
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
