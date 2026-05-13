# autonomy-status oracle (Windows PowerShell)
# Resolves the four-step autonomy cascade and emits JSON.
#
# Cascade order (first non-skip-non-empty wins):
#   1. Session override (caller-supplied via --session-override=<level>)
#   2. ltads/sessions/current-session.md Autonomy Override (skip if Status: CONCLUDED)
#   3. .claude/session-state/task-arc-autonomy.json (skip if expired or grantor PID dead)
#   4. ltads/config.json autonomy.default (cap at collaborative)
#   5. Default: collaborative

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$SessionOverride = ""
foreach ($arg in $args) {
    if ($arg -like "--session-override=*") {
        $SessionOverride = $arg.Substring("--session-override=".Length)
    }
}

function Test-PidAlive {
    param([int]$ProcessId)
    if ($ProcessId -le 0) { return $false }
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    return $null -ne $proc
}

function Emit-Json {
    param(
        [string]$Level,
        [string]$Source,
        [string]$ArcId = "",
        [string]$ExpiresAt = "",
        [string]$Detail
    )

    $arcJson = if ($ArcId) { "`"$([System.Web.HttpUtility]::JavaScriptStringEncode($ArcId))`"" } else { "null" }
    $expiresJson = if ($ExpiresAt) { "`"$([System.Web.HttpUtility]::JavaScriptStringEncode($ExpiresAt))`"" } else { "null" }
    $detailJson = [System.Web.HttpUtility]::JavaScriptStringEncode($Detail)

    $briefing = ""
    if ($Level -ne "collaborative") {
        if ($ArcId -and $ExpiresAt) {
            $briefing = '{"level":"' + $Level + '","source":"' + $Source + '","arc_id":"' + $ArcId + '","expires_at":"' + $ExpiresAt + '"}'
        } else {
            $briefing = '{"level":"' + $Level + '","source":"' + $Source + '"}'
        }
    }
    $briefingJson = [System.Web.HttpUtility]::JavaScriptStringEncode($briefing)

    Write-Output "{`"level`":`"$Level`",`"source`":`"$Source`",`"arc_id`":$arcJson,`"expires_at`":$expiresJson,`"source_detail`":`"$detailJson`",`"briefing`":`"$briefingJson`"}"
}

# Load System.Web for HttpUtility (PowerShell 5.1 default — already available; no-op on PS 7+)
Add-Type -AssemblyName System.Web -ErrorAction SilentlyContinue

# ---------------------------------------------------------------------------
# Step 1: Session override
# ---------------------------------------------------------------------------

if ($SessionOverride) {
    if ($SessionOverride -in @("autopilot","supervised","collaborative","controlled","manual")) {
        Emit-Json -Level $SessionOverride -Source "session-override" -Detail "Session override passed via --session-override flag"
        exit 0
    }
}

# ---------------------------------------------------------------------------
# Step 2: LTADS current-session.md
# ---------------------------------------------------------------------------

$LtadsFile = "ltads/sessions/current-session.md"
if (Test-Path $LtadsFile) {
    try {
        $content = Get-Content $LtadsFile -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        $statusMatch = [regex]::Match($content, '\*\*Status\*\*\s*:\s*([A-Z_]+)')
        $status = if ($statusMatch.Success) { $statusMatch.Groups[1].Value.ToUpper() } else { "" }

        if ($status -ne "CONCLUDED" -and $status -ne "PAUSED") {
            $overrideMatch = [regex]::Match($content, '(?m)^\*\*Autonomy Override\*\*\s*:\s*([a-zA-Z]+)')
            if ($overrideMatch.Success) {
                $level = $overrideMatch.Groups[1].Value.ToLower()
                if ($level -in @("autopilot","supervised","collaborative","controlled","manual")) {
                    Emit-Json -Level $level -Source "ltads-session" -Detail "ltads/sessions/current-session.md Autonomy Override line"
                    exit 0
                }
            }
        }
    } catch { }
}

# ---------------------------------------------------------------------------
# Step 3: .claude/session-state/task-arc-autonomy.json
# ---------------------------------------------------------------------------

$ArcFile = ".claude/session-state/task-arc-autonomy.json"
if (Test-Path $ArcFile) {
    try {
        $arc = Get-Content $ArcFile -Raw -Encoding UTF8 | ConvertFrom-Json -ErrorAction SilentlyContinue
        if ($arc -and $arc.level -in @("autopilot","supervised","collaborative","controlled","manual")) {
            $expired = $false
            if ($arc.expires_at) {
                try {
                    $expiresUtc = [DateTime]::Parse($arc.expires_at, $null, [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal)
                    if ([DateTime]::UtcNow -gt $expiresUtc) { $expired = $true }
                } catch { }
            }

            $dead = $false
            if ($arc.grantor_pid) {
                if (-not (Test-PidAlive -ProcessId $arc.grantor_pid)) { $dead = $true }
            }

            if (-not $expired -and -not $dead) {
                $arcId = if ($arc.arc_id) { "$($arc.arc_id)" } else { "" }
                $expires = if ($arc.expires_at) { "$($arc.expires_at)" } else { "" }
                Emit-Json -Level $arc.level -Source "task-arc-autonomy" -ArcId $arcId -ExpiresAt $expires -Detail ".claude/session-state/task-arc-autonomy.json (TTL valid, grantor alive)"
                exit 0
            }
        }
    } catch { }
}

# ---------------------------------------------------------------------------
# Step 4: ltads/config.json
# ---------------------------------------------------------------------------

$ConfigFile = "ltads/config.json"
if (Test-Path $ConfigFile) {
    try {
        $cfg = Get-Content $ConfigFile -Raw -Encoding UTF8 | ConvertFrom-Json -ErrorAction SilentlyContinue
        if ($cfg.autonomy -and $cfg.autonomy.default) {
            $cfgLevel = "$($cfg.autonomy.default)".ToLower()
            switch ($cfgLevel) {
                { $_ -in @("collaborative","controlled","manual") } {
                    Emit-Json -Level $cfgLevel -Source "config" -Detail "ltads/config.json autonomy.default"
                    exit 0
                }
                { $_ -in @("autopilot","supervised") } {
                    Emit-Json -Level "collaborative" -Source "config" -Detail "ltads/config.json autonomy.default is '$cfgLevel' (CAPPED to collaborative per cascade rule)"
                    exit 0
                }
            }
        }
    } catch { }
}

# ---------------------------------------------------------------------------
# Step 5: Default
# ---------------------------------------------------------------------------

Emit-Json -Level "collaborative" -Source "default" -Detail "No override / LTADS / arc-autonomy / config - falling through to documented default"
