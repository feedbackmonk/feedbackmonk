# ltads-state oracle (Windows PowerShell)
# Formalized from the state detection originally embedded in session-start.ps1.
# Reports the LTADS state: none / permanent / temporary / legacy / incomplete_temp / broken
#
# ARC-01/ARC-06 (DEC-199): the oracle is also the arc-state.json VALIDATOR --
# additive output fields `arc_state` (valid|invalid|legacy|absent|unvalidated),
# `arc_mode` (FRESH|CONTINUATION per the ARC-06 two-fact rule), `topmost_arc`
# ({id, status, boundConsentExpired}) and `size_warning` (ARC-01 cap-approach
# WARN). When arc-state.json is valid, session_id/session_status come from the
# topmost arc; otherwise the legacy prose read below still serves them, and a
# prose-only project reports `arc_state: "legacy"` (migration pending, ARC-11).
#
# Arc-state fields are read via the single-writer lib's getters
# (scripts/lib/arc-state.ps1, ARC-02) -- never hand-rolled. The prose parser
# lib retired per ARC-04: on a `legacy` project session_status is unavailable
# (empty) -- the `legacy` verdict itself (file presence) is the load-bearing
# signal, surfacing the ARC-11 converter offer.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$LtadsPath = "ltads"

# ---- Dot-source the arc-state lib -------------------------------------------
$astLibCandidates = @(
    (Join-Path $PSScriptRoot "../../scripts/lib/arc-state.ps1"),
    (Join-Path $PSScriptRoot "../../../claude-template/scripts/lib/arc-state.ps1"),
    (Join-Path $env:USERPROFILE ".claude/scripts/lib/arc-state.ps1")
)
foreach ($cand in $astLibCandidates) {
    if (Test-Path $cand) { . $cand; break }
}

if (-not (Test-Path $LtadsPath)) {
    $result = [ordered]@{
        state = "none"
        has_ltads_dir = $false
        is_tracked = $false
        config_exists = $false
        is_temporary = $false
        cleanup_candidate = $false
        session_id = $null
        session_status = $null
        summary = "No LTADS on this project"
        arc_state = "absent"
        arc_mode = "FRESH"
        topmost_arc = $null
        size_warning = $false
    }
    $result | ConvertTo-Json -Compress -Depth 4
    exit 0
}

# Read config.json
$configExists = $false
$isTemporary = $false
$configPath = Join-Path $LtadsPath "config.json"
if (Test-Path $configPath) {
    $configExists = $true
    try {
        $configContent = Get-Content $configPath -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($configContent -match '"temporary"\s*:\s*true') {
            $isTemporary = $true
        }
    } catch {}
}

# ---- arc-state.json validation + topmost-arc report (ARC-01/ARC-06) --------
$arcState = "absent"
$arcMode = "FRESH"
$topmostArc = $null
$sizeWarning = $false
$arcStatePath = Join-Path $LtadsPath "arc-state.json"
$currentSessionPath = Join-Path $LtadsPath "sessions/current-session.md"
if (Test-Path $arcStatePath) {
    if (-not (Get-Command Test-ArcStateValid -ErrorAction SilentlyContinue)) {
        # NO-DATA, never false-green (and never a false "invalid").
        Write-Warning "ltads-state: arc-state.ps1 lib unavailable; arc-state.json cannot be validated"
        $arcState = "unvalidated"
        $arcMode = $null
    } else {
        $arcState = Test-ArcStateValid -Path $arcStatePath
        if ($arcState -eq "valid") {
            $arcMode = Get-ArcStateArcMode -Path $arcStatePath
            $topJson = Get-ArcStateTopmost -Path $arcStatePath
            if ($topJson) {
                try {
                    $top = $topJson | ConvertFrom-Json -ErrorAction Stop
                    $bcExpired = $null
                    if ($top.PSObject.Properties['boundConsent']) { $bcExpired = $top.boundConsent.expired }
                    $topmostArc = [ordered]@{
                        id = $top.id
                        status = $top.status
                        boundConsentExpired = $bcExpired
                    }
                } catch {}
            }
        } else {
            $arcMode = $null   # malformed: never guess a session mode (ARC-06)
        }
        # ARC-01 size-cap approach WARN (>80% of cap; breach is lib-rotated).
        $cap = 4096
        if ($env:ULDF_ARC_STATE_SIZE_CAP) {
            $capParsed = 0
            if ([int]::TryParse($env:ULDF_ARC_STATE_SIZE_CAP, [ref]$capParsed)) { $cap = $capParsed }
        }
        try {
            $sz = (Get-Item -LiteralPath $arcStatePath -ErrorAction Stop).Length
            if ($sz -gt [math]::Floor($cap * 0.8)) { $sizeWarning = $true }
        } catch {}
    }
} elseif (Test-Path $currentSessionPath) {
    # Prose-only arc record: the ARC-11 migration-pending surfacing trigger.
    $arcState = "legacy"
    $arcMode = $null
}

# Read current-session.md (legacy prose read; superseded by arc-state when valid)
$sessionStatus = ""
$sessionId = ""
if ($arcState -eq "valid" -and $null -ne $topmostArc) {
    $sessionId = [string]$topmostArc.id
    $sessionStatus = [string]$topmostArc.status
} elseif (Test-Path $currentSessionPath) {
    try {
        # Legacy prose project (ARC-04: parser lib retired): session_status is
        # deliberately unavailable -- the `legacy` arc_state verdict is the
        # load-bearing signal; the ARC-11 converter is the migration path.
        $sessionContent = Get-Content $currentSessionPath -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($sessionContent -match '(?m)^\s*-?\s*\*\*ID\*\*\s*:\s*([^\r\n]+)') {
            $sessionId = $Matches[1].Trim()
        }
    } catch {}
}

# Git tracking
$isTracked = $false
try {
    $trackedFiles = git ls-files "$LtadsPath/" 2>$null
    if ($LASTEXITCODE -eq 0 -and $trackedFiles) {
        $isTracked = $true
    }
} catch {}

# Spec exhaustion (LTADS-GC-01): parse the "True Progress: X/Y" header of
# spec-progress.md. Exhausted iff X == Y and Y > 0. Missing/unparseable -> not
# exhausted (conservative).
$specExhausted = $false
$specProgressPath = Join-Path $LtadsPath "execution/spec-progress.md"
if (Test-Path $specProgressPath) {
    try {
        $spContent = Get-Content $specProgressPath -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($spContent -match 'True Progress\*{0,2}\s*:?\s*(\d+)/(\d+)') {
            $tpDone = [int]$Matches[1]
            $tpTotal = [int]$Matches[2]
            if ($tpTotal -gt 0 -and $tpDone -eq $tpTotal) { $specExhausted = $true }
        }
    } catch {}
}

# Classify
$state = "broken"
$summary = ""
$cleanupCandidate = $false
if (-not $configExists) {
    if ($isTracked) {
        $state = "legacy"
        $summary = "Legacy permanent LTADS (no config.json, tracked in git). Run /0-uldf-ltads-admin init to upgrade."
    } else {
        $state = "incomplete_temp"
        # Orphan from failed auto-init: always a cleanup candidate; the shared
        # cleanup snippet ALWAYS prompts for this state (never auto-deletes).
        $cleanupCandidate = $true
        $summary = "Incomplete temporary state (no config.json, untracked). Cleanup candidate - offer ~/.claude/segments/-ltads/_temp-ltads-cleanup.md (always prompts for orphans)."
    }
} elseif ($isTemporary) {
    $state = "temporary"
    $summary = "Temporary LTADS"
    if ($sessionId) { $summary += ", session $sessionId" }
    if ($sessionStatus) { $summary += " ($sessionStatus)" }
    if ($specExhausted) {
        # LTADS-GC-01: temporary + spec exhausted -> the ephemeral ltads/ is
        # eligible for cleanup regardless of whether any end-command ever ran
        # (fixes the leak-by-construction, skill-corpus scrutiny 04 ADD-2).
        $cleanupCandidate = $true
        $summary += " - spec exhausted; temporary ltads/ eligible for cleanup (run ~/.claude/segments/-ltads/_temp-ltads-cleanup.md)"
    }
} else {
    $state = "permanent"
    $summary = "Permanent LTADS"
    if ($sessionId) { $summary += ", session $sessionId" }
    if ($sessionStatus) { $summary += " ($sessionStatus)" }
}

# Arc-record verdict annotations (ARC-01/ARC-06/ARC-11)
switch ($arcState) {
    "legacy"      { $summary += "; prose arc record (legacy) - migration pending (ARC-11: convert at next arc start)" }
    "invalid"     { $summary += "; arc-state.json MALFORMED - writers refuse until fixed/restored" }
    "unvalidated" { $summary += "; arc-state.json present but unvalidatable (lib missing) - NO-DATA" }
    "valid"       { $summary += "; arc-state $arcMode" }
}
if ($sizeWarning) {
    $summary += "; arc-state near size cap (concluded-arc rotation on next lib write)"
}

$sidOut = $null
if ($sessionId) { $sidOut = $sessionId }
$ssOut = $null
if ($sessionStatus) { $ssOut = $sessionStatus }

$result = [ordered]@{
    state = $state
    has_ltads_dir = $true
    is_tracked = $isTracked
    config_exists = $configExists
    is_temporary = $isTemporary
    cleanup_candidate = $cleanupCandidate
    session_id = $sidOut
    session_status = $ssOut
    summary = $summary
    arc_state = $arcState
    arc_mode = $arcMode
    topmost_arc = $topmostArc
    size_warning = $sizeWarning
}

$result | ConvertTo-Json -Compress -Depth 4
