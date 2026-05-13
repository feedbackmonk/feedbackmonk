# ltads-state oracle (Windows PowerShell)
# Formalized from the state detection originally embedded in session-start.ps1.
# Reports the LTADS state: none / permanent / temporary / legacy / incomplete_temp / broken

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$LtadsPath = "ltads"

if (-not (Test-Path $LtadsPath)) {
    $result = [ordered]@{
        state = "none"
        has_ltads_dir = $false
        is_tracked = $false
        config_exists = $false
        is_temporary = $false
        session_id = $null
        session_status = $null
        summary = "No LTADS on this project"
    }
    $result | ConvertTo-Json -Compress -Depth 3
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

# Read current-session.md
$sessionStatus = ""
$sessionId = ""
$currentSessionPath = Join-Path $LtadsPath "sessions/current-session.md"
if (Test-Path $currentSessionPath) {
    try {
        $sessionContent = Get-Content $currentSessionPath -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($sessionContent -match '(?m)^\s*(?:##\s+|-\s+\*\*|\*\*)Status(?:\*\*)?\s*:\s*([A-Z_]+)') {
            $sessionStatus = $Matches[1].ToUpper()
        }
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

# Classify
$state = "broken"
$summary = ""
if (-not $configExists) {
    if ($isTracked) {
        $state = "legacy"
        $summary = "Legacy permanent LTADS (no config.json, tracked in git). Run /0-uldf-ltads-admin init to upgrade."
    } else {
        $state = "incomplete_temp"
        $summary = "Incomplete temporary state (no config.json, untracked). Safe to delete ltads/ manually."
    }
} elseif ($isTemporary) {
    $state = "temporary"
    $summary = "Temporary LTADS"
    if ($sessionId) { $summary += ", session $sessionId" }
    if ($sessionStatus) { $summary += " ($sessionStatus)" }
} else {
    $state = "permanent"
    $summary = "Permanent LTADS"
    if ($sessionId) { $summary += ", session $sessionId" }
    if ($sessionStatus) { $summary += " ($sessionStatus)" }
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
    session_id = $sidOut
    session_status = $ssOut
    summary = $summary
}

$result | ConvertTo-Json -Compress -Depth 3
