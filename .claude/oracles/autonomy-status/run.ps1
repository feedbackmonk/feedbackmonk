# autonomy-status oracle (Windows PowerShell)
# Resolves the autonomy cascade and emits JSON.
#
# Cascade order (first non-skip-non-empty wins):
#   1. Session override (caller-supplied via --session-override=<level>)
#   2. ltads/arc-state.json topmost-arc autonomyOverride field (skip if CONCLUDED/PAUSED)
#   3. .claude/session-state/task-arc-autonomy.json (skip if expired or grantor PID dead)
#   4. ltads/config.json autonomy.default (may not hold autopilot/supervised)
#   5. ~/.claude/machine-autonomy.json (the AUTODEF-02 machine default)
#   6. Default: collaborative
#
# AUTODEF (AUTODEF-01..04, DEC-189): steps 4 and 5 resolve TOGETHER — project
# config CAPS the machine default downward (more consultative wins). Autopilot
# submodes (incl. `director`) are parsed and emitted as `submode`; at `director`
# the `spec` domain carries a submode-definitional clamp FLOOR of `ask-major`.
#
# AUTODOM (AUTODOM-01..05, DEC-172): the resolved level seeds a six-domain
# consultation vector (spec/plan/delegate/decide/quality/commit) from a frozen
# level->domain default matrix. Explicit overrides (session store, then persisted
# config) may only TIGHTEN a domain; a loosening override is CLAMPED to the level
# default and reported. Byte-parity with run.sh is a hard requirement (DISC-AUTO-01).

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

# Set by whichever cascade step resolves; consumed by Emit-Json/Resolve-Domains.
$Script:Submode = ''      # autopilot submode token, '' otherwise
$Script:CappedFrom = ''   # the machine-default designation a project config capped away

# Split a designation (`autopilot:director`, `supervised`, ...) into base level
# and (autopilot only) submode. Returns @{ Level = ''; Submode = '' }; Level is
# empty when the token is not a recognized level. Single parse point: a submode
# the resolver cannot read is a silent downgrade (DISC-AUTO-01).
function Get-Designation {
    param([string]$Raw)
    $r = ("$Raw" -replace '[\s]', '').ToLower()
    $base = $r; $sub = ''
    $i = $r.IndexOf(':')
    if ($i -ge 0) { $base = $r.Substring(0, $i); $sub = $r.Substring($i + 1) }
    $lvl = ''
    if ($base -in @("autopilot","supervised","collaborative","controlled","manual")) { $lvl = $base }
    $submode = ''
    if ($lvl -eq 'autopilot' -and $sub -in @("continuous","phase","session","task","director")) { $submode = $sub }
    return @{ Level = $lvl; Submode = $submode }
}

# Ladder position, least -> most consultative (AUTODEF-03 "more consultative wins").
function Get-LevelRank {
    param([string]$Level)
    switch ($Level) {
        "autopilot"     { return 1 }
        "supervised"    { return 2 }
        "collaborative" { return 3 }
        "controlled"    { return 4 }
        "manual"        { return 5 }
        default         { return 0 }
    }
}

# ---------------------------------------------------------------------------
# AUTODOM: domain vector (AUTODOM-01..04)
# ---------------------------------------------------------------------------

# Canonical domain order. Every emitted `domains` object carries all six keys.
$DomainOrder = @("spec", "plan", "delegate", "decide", "quality", "commit")

# AUTODOM-04 — option orderings, least-consultative -> most-consultative.
$DomainOptions = @{
    spec     = @("auto", "critical-only", "ask-major", "ask-all")
    plan     = @("auto", "summarize", "approve", "step-by-step")
    delegate = @("auto", "summarize", "approve-roles", "approve-prompts")
    decide   = @("auto-all", "ask-critical", "ask-major", "ask-all")
    quality  = @("auto", "report", "approve")
    commit   = @("auto", "approve-message", "approve-diff")
}

# AUTODOM-03 — the FROZEN level->domain default matrix, in $DomainOrder.
# `commit` is `auto` at collaborative and above ON PURPOSE (DEC-116 form 5:
# finalize IS the commit consent surface; a default of approve-message there
# would manufacture the consent gate COMMS-GATE-02 bans).
$LevelDefaults = @{
    autopilot     = @("auto", "auto", "auto", "auto-all", "auto", "auto")
    supervised    = @("critical-only", "summarize", "summarize", "ask-critical", "report", "auto")
    collaborative = @("ask-major", "approve", "approve-roles", "ask-major", "report", "auto")
    controlled    = @("ask-all", "approve", "approve-prompts", "ask-all", "approve", "approve-message")
    manual        = @("ask-all", "step-by-step", "approve-prompts", "ask-all", "approve", "approve-diff")
}

function Get-DomainRank {
    param([string]$Domain, [string]$Option)
    $opts = $DomainOptions[$Domain]
    if ($null -eq $opts) { return -1 }
    for ($i = 0; $i -lt $opts.Count; $i++) {
        if ($opts[$i] -eq $Option) { return $i }
    }
    return -1
}

# AUTODOM-01 — the two stores. Session store overlays the persisted one, per
# domain. Both optional; absence is the common case.
function Get-DomainsBlock {
    param([string]$Path)
    $result = @{}
    if (-not (Test-Path $Path)) { return $result }
    try {
        $raw = Get-Content $Path -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if (-not $raw) { return $result }
        # Regex-extract the flat `"domains": { ... }` object (mirrors run.sh's
        # sed extraction exactly, so a malformed file degrades identically in
        # both shells rather than one honoring what the other drops).
        $flat = $raw -replace "`r|`n", ""
        $m = [regex]::Match($flat, '"domains"\s*:\s*\{([^}]*)\}')
        if (-not $m.Success) { return $result }
        $block = $m.Groups[1].Value
        foreach ($d in $DomainOrder) {
            $dm = [regex]::Match($block, "`"$d`"\s*:\s*`"([^`"]*)`"")
            if ($dm.Success -and $dm.Groups[1].Value) { $result[$d] = $dm.Groups[1].Value }
        }
    } catch { }
    return $result
}

$DomainOverridesRaw = @{}
foreach ($kv in (Get-DomainsBlock "ltads/config.json").GetEnumerator())                        { $DomainOverridesRaw[$kv.Key] = $kv.Value }
foreach ($kv in (Get-DomainsBlock ".claude/session-state/autonomy-domains.json").GetEnumerator()) { $DomainOverridesRaw[$kv.Key] = $kv.Value }

function Resolve-Domains {
    param([string]$Level)

    $defaults = $LevelDefaults[$Level]
    if ($null -eq $defaults) { $defaults = $LevelDefaults["collaborative"] }
    # AUTODEF-01 — `autopilot:director` is `autopilot` with ONE pin: the `spec`
    # domain floor is `ask-major`. Applied as the level default so the existing
    # tighten-only machinery clamps a `spec=auto` override and reports it.
    if ($Level -eq 'autopilot' -and $Script:Submode -eq 'director') {
        $defaults = @("ask-major") + $defaults[1..($defaults.Count - 1)]
    }

    $domPairs = @(); $ovrNames = @(); $ovrPairs = @()
    $clampObjs = @(); $clampBrief = @(); $invalidObjs = @(); $invalidBrief = @()

    for ($i = 0; $i -lt $DomainOrder.Count; $i++) {
        $d = $DomainOrder[$i]
        $def = $defaults[$i]
        $final = $def
        $req = $DomainOverridesRaw[$d]
        if ($req) {
            $reqRank = Get-DomainRank -Domain $d -Option $req
            $defRank = Get-DomainRank -Domain $d -Option $def
            if ($reqRank -lt 0) {
                # Unrecognized option: ignored (level default stands) and REPORTED.
                $invalidObjs  += ('{"domain":"' + $d + '","requested":"' + [System.Web.HttpUtility]::JavaScriptStringEncode($req) + '"}')
                $invalidBrief += ('"' + $d + '=' + $req + '"')
            } elseif ($reqRank -lt $defRank) {
                # AUTODOM-04 tighten-only: loosening override clamped, reported.
                $clampObjs  += ('{"domain":"' + $d + '","requested":"' + $req + '","clamped_to":"' + $def + '"}')
                # NB: no <, >, & or ' in briefing text — JavaScriptStringEncode
                # \u-escapes those and would break byte parity with run.sh.
                $clampBrief += ('"' + $d + ': ' + $req + ' clamped to ' + $def + '"')
            } else {
                $final = $req
                $ovrNames += ('"' + $d + '"')
                $ovrPairs += ('"' + $d + '":"' + $req + '"')
            }
        }
        $domPairs += ('"' + $d + '":"' + $final + '"')
    }

    return [pscustomobject]@{
        DomJson      = ($domPairs -join ',')
        OvrNames     = ($ovrNames -join ',')
        OvrPairs     = ($ovrPairs -join ',')
        ClampJson    = ($clampObjs -join ',')
        ClampBrief   = ($clampBrief -join ',')
        InvalidJson  = ($invalidObjs -join ',')
        InvalidBrief = ($invalidBrief -join ',')
        Active       = (($ovrNames.Count + $clampObjs.Count + $invalidObjs.Count) -gt 0)
    }
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

    $dom = Resolve-Domains -Level $Level

    # AUTODOM-05 — briefing also fires when a domain override is ACTIVE at
    # collaborative, so an active override is unavoidably in context at session
    # start (session-start prints this string verbatim as `[autonomy] ...`).
    # AUTODEF-04 — when the machine default governs (or a project config capped
    # it away), the briefing is emitted UNCONDITIONALLY: an ambient standing
    # grant must be unmissable in every session it governs.
    $forceBrief = ($Source -eq 'machine-default' -or $Source -eq 'config-cap')

    $briefing = ""
    if (($Level -ne "collaborative") -or $dom.Active -or $forceBrief) {
        $base = '"level":"' + $Level + '","source":"' + $Source + '"'
        if ($Script:Submode)    { $base = $base + ',"submode":"' + $Script:Submode + '"' }
        if ($Script:CappedFrom) { $base = $base + ',"capped_from":"' + [System.Web.HttpUtility]::JavaScriptStringEncode($Script:CappedFrom) + '"' }
        if ($ArcId -and $ExpiresAt) {
            $base = $base + ',"arc_id":"' + $ArcId + '","expires_at":"' + $ExpiresAt + '"'
        }
        $domBrief = ""
        if ($dom.OvrPairs)     { $domBrief += ',"domains":{' + $dom.OvrPairs + '}' }
        if ($dom.ClampBrief)   { $domBrief += ',"clamped":[' + $dom.ClampBrief + ']' }
        if ($dom.InvalidBrief) { $domBrief += ',"invalid":[' + $dom.InvalidBrief + ']' }
        $briefing = '{' + $base + $domBrief + '}'
    }
    $briefingJson = [System.Web.HttpUtility]::JavaScriptStringEncode($briefing)

    $submodeJson = if ($Script:Submode) { "`"$($Script:Submode)`"" } else { "null" }

    Write-Output "{`"level`":`"$Level`",`"submode`":$submodeJson,`"source`":`"$Source`",`"arc_id`":$arcJson,`"expires_at`":$expiresJson,`"source_detail`":`"$detailJson`",`"briefing`":`"$briefingJson`",`"domains`":{$($dom.DomJson)},`"domain_overrides`":[$($dom.OvrNames)],`"domain_clamped`":[$($dom.ClampJson)],`"domain_invalid`":[$($dom.InvalidJson)]}"
}

# Load System.Web for HttpUtility (PowerShell 5.1 default — already available; no-op on PS 7+)
Add-Type -AssemblyName System.Web -ErrorAction SilentlyContinue

# ---------------------------------------------------------------------------
# Step 1: Session override
# ---------------------------------------------------------------------------

if ($SessionOverride) {
    $d = Get-Designation $SessionOverride
    if ($d.Level) {
        $Script:Submode = $d.Submode
        Emit-Json -Level $d.Level -Source "session-override" -Detail "Session override passed via --session-override flag"
        exit 0
    }
}

# ---------------------------------------------------------------------------
# Step 2: arc-state.json autonomyOverride (topmost arc; ARC-03 / DEC-199)
# ---------------------------------------------------------------------------
# The session-scoped override lives on the topmost arc as `autonomyOverride`
# (scrubbed mechanically at the terminus flip). Malformed/absent -> leg
# skipped (graceful). Legacy prose-only projects carry no arc-state.json.

$ArcStateFile = "ltads/arc-state.json"
if (Test-Path $ArcStateFile) {
    try {
        $arcDoc = Get-Content $ArcStateFile -Raw -Encoding UTF8 -ErrorAction SilentlyContinue | ConvertFrom-Json
        $arcs = @($arcDoc.arcs)
        $status = ""
        $override = ""
        if ($arcs.Count -gt 0 -and $null -ne $arcs[0]) {
            if ($arcs[0].status) { $status = [string]$arcs[0].status }
            if ($arcs[0].PSObject.Properties['autonomyOverride'] -and $arcs[0].autonomyOverride) {
                $override = [string]$arcs[0].autonomyOverride
            }
        }
        if ($status -ne "CONCLUDED" -and $status -ne "PAUSED" -and $override) {
            # The field may carry a submode (`autopilot:director`); parsing is
            # centralized so sh and ps1 cannot diverge on submode handling.
            $d = Get-Designation $override
            if ($d.Level) {
                $Script:Submode = $d.Submode
                Emit-Json -Level $d.Level -Source "ltads-session" -Detail "ltads/arc-state.json topmost-arc autonomyOverride field"
                exit 0
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
        $arcD = if ($arc) { Get-Designation "$($arc.level)" } else { @{ Level = ''; Submode = '' } }
        if ($arcD.Level) {
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
                $Script:Submode = $arcD.Submode
                Emit-Json -Level $arcD.Level -Source "task-arc-autonomy" -ArcId $arcId -ExpiresAt $expires -Detail ".claude/session-state/task-arc-autonomy.json (TTL valid, grantor alive)"
                exit 0
            }
        }
    } catch { }
}

# ---------------------------------------------------------------------------
# Steps 4+5: ltads/config.json AND ~/.claude/machine-autonomy.json
#
# Resolved TOGETHER (AUTODEF-03): project config CAPS the machine default
# downward — resolved level is the MORE consultative of the two. Project config
# still may not hold autopilot/supervised; durable loosening lives on exactly
# one auditable, revocable, user-word-only surface.
# ---------------------------------------------------------------------------

$CfgLevel = ""; $CfgLegacy = ""
$ConfigFile = "ltads/config.json"
if (Test-Path $ConfigFile) {
    try {
        $cfg = Get-Content $ConfigFile -Raw -Encoding UTF8 | ConvertFrom-Json -ErrorAction SilentlyContinue
        if ($cfg.autonomy -and $cfg.autonomy.default) {
            $cfgRaw = "$($cfg.autonomy.default)".ToLower()
            # Parse through the shared designation parser, NOT a bare-token list: a
            # legacy config value carrying a submode (`autopilot:phase`) must still
            # be recognized and neutralized. Matching only bare tokens made such a
            # value fall through as if the file said nothing -- which, once a machine
            # default exists, silently ESCALATES a project that had explicitly asked
            # for a BOUNDED autopilot into the ambient continuous-class one.
            $cfgD = Get-Designation $cfgRaw
            if ($cfgD.Level -in @("collaborative","controlled","manual")) { $CfgLevel = $cfgD.Level }
            elseif ($cfgD.Level -in @("autopilot","supervised")) {
                # Legacy value: still neutralized to collaborative, advisory retargeted.
                # The full designation (submode included) is echoed back in the advisory.
                $CfgLevel = "collaborative"; $CfgLegacy = $cfgRaw
            }
        }
    } catch { }
}

# AUTODEF-02 — the machine default. ULDF_MACHINE_AUTONOMY_FILE is a TEST SEAM
# (smokes point it at a sandbox); production always resolves ~/.claude/.
$MachineFile = $env:ULDF_MACHINE_AUTONOMY_FILE
if (-not $MachineFile) {
    $homeDir = $HOME; if (-not $homeDir) { $homeDir = $env:USERPROFILE }
    $MachineFile = Join-Path $homeDir ".claude/machine-autonomy.json"
}
$MachLevel = ""; $MachSub = ""; $MachRaw = ""
if (Test-Path $MachineFile) {
    # Malformed or absent => skipped SILENTLY (graceful absence): a machine-global
    # consent-bearing file must never break every session's briefing.
    try {
        $mj = Get-Content $MachineFile -Raw -Encoding UTF8 | ConvertFrom-Json -ErrorAction SilentlyContinue
        if ($mj -and $mj.level) {
            $MachRaw = "$($mj.level)"
            $md = Get-Designation $MachRaw
            $MachLevel = $md.Level; $MachSub = $md.Submode
        }
    } catch { }
}

if ($CfgLevel -and $MachLevel) {
    if ((Get-LevelRank $CfgLevel) -gt (Get-LevelRank $MachLevel)) {
        $Script:CappedFrom = $MachRaw
        $detail = "ltads/config.json autonomy.default $CfgLevel CAPS the machine default $MachRaw (more consultative wins)"
        if ($CfgLegacy) { $detail = $detail + "; config value is legacy $CfgLegacy, neutralized to collaborative - run /0-uldf-autonomy-set collaborative --persist to clean up, or grant durable autopilot with /0-uldf-autonomy-set autopilot:director --machine-default" }
        Emit-Json -Level $CfgLevel -Source "config-cap" -Detail $detail
        exit 0
    }
    $Script:Submode = $MachSub
    Emit-Json -Level $MachLevel -Source "machine-default" -Detail "machine default $MachRaw from ~/.claude/machine-autonomy.json (project config $CfgLevel is not more consultative)"
    exit 0
}

if ($CfgLevel) {
    if ($CfgLegacy) {
        Emit-Json -Level $CfgLevel -Source "config" -Detail "ltads/config.json autonomy.default is $CfgLegacy (CAPPED to collaborative - project config cannot hold autopilot/supervised; durable autopilot lives on the machine default, /0-uldf-autonomy-set autopilot:director --machine-default)"
        exit 0
    }
    Emit-Json -Level $CfgLevel -Source "config" -Detail "ltads/config.json autonomy.default"
    exit 0
}

if ($MachLevel) {
    $Script:Submode = $MachSub
    Emit-Json -Level $MachLevel -Source "machine-default" -Detail "machine default $MachRaw from ~/.claude/machine-autonomy.json (machine-global standing grant, AUTODEF-02)"
    exit 0
}

# ---------------------------------------------------------------------------
# Step 6: Default
# ---------------------------------------------------------------------------

Emit-Json -Level "collaborative" -Source "default" -Detail "No override / LTADS / arc-autonomy / config - falling through to documented default"
