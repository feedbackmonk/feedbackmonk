# overseer-wake oracle (Windows PowerShell) -- CSO Phase 1, component C1 (the cheap mechanic).
#
# Byte-compatible verdict contract with run.sh (CSO domain schema):
#   {wake:bool, signals:[{detectorId,severity,sessions:[...],summary}], summary, briefing}
#
# Question: is there anything in the airspace worth waking the LLM Overseer for?
#
# This is the cost defense of CSO (CSO-09 / Q-CSO-01): a PURE-SHELL deterministic
# aggregator over the four already-deterministic detectors. It does the routine
# watching at ~0 LLM tokens so the expensive LLM Overseer is woken ONLY when this
# oracle raises wake:true. NO LLM is in this path.
#
# READ-ONLY by contract: never writes. Aggregates four sources, each gracefully
# absent (a missing source is NO-DATA -- omitted -- NEVER a fabricated all-clear;
# wake fires only on positive evidence):
#   (d) stall          : active registry entries whose claudeShellPid is DEAD
#   (a) concurrent-mut : the sibling concurrent-mutation oracle's external_mutation
#   (b) shared-foreign : another live session's sharedClaim on a shared-repo file
#   (c) touches-screen : a PODS touches.json path claimed by >=2 distinct sessions
#
# briefing is empty when wake=false (the session-start hook then emits no line).

$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# Sync .NET CWD with PowerShell's current location.
try { [Environment]::CurrentDirectory = (Get-Location).Path } catch { }

$_owPwd   = (Get-Location).Path
$_thisDir = $PSScriptRoot

# ---- esc(): mirror run.sh esc -- backslash first, then doublequote ----------
function Esc([string]$s) {
    if ($null -eq $s) { return "" }
    return $s.Replace('\', '\\').Replace('"', '\"')
}

# ---- BOM-safe JSON read (handles BOM; returns $null on any fault) -----------
function Convert-OwNormalizeDates {
    param($Node)
    # TWIN-01 / DEFER-101 (DEC-263). pwsh 6+'s ConvertFrom-Json coerces
    # ISO-8601-shaped strings to [DateTime]; powershell.exe 5.1 leaves them as
    # [string] (DISC-ARC-01). The shared-claim leg below compares
    # `[string]$claim.boundUntil` lexically against an ISO "now", and [string]
    # on a [DateTime] renders it in the CURRENT CULTURE
    # ("01/01/2099 00:00:00") -- which sorts BELOW every ISO date, so under
    # pwsh every live claim read as expired and the shared-foreign-claim
    # detector never fired. Normalize at the parse boundary
    # (Convert-AstNormalizeDates precedent, scripts/lib/arc-state.ps1).
    if ($null -eq $Node) { return $null }
    if ($Node -is [datetime]) {
        if ($Node.Kind -eq [System.DateTimeKind]::Local) { $Node = $Node.ToUniversalTime() }
        if ($Node.Kind -eq [System.DateTimeKind]::Utc) {
            if ($Node.Millisecond -ne 0) { return $Node.ToString("yyyy-MM-ddTHH:mm:ss.fffZ", [System.Globalization.CultureInfo]::InvariantCulture) }
            return $Node.ToString("yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture)
        }
        if ($Node.Millisecond -ne 0) { return $Node.ToString("yyyy-MM-ddTHH:mm:ss.fff", [System.Globalization.CultureInfo]::InvariantCulture) }
        return $Node.ToString("yyyy-MM-ddTHH:mm:ss", [System.Globalization.CultureInfo]::InvariantCulture)
    }
    if ($Node -is [System.Array]) {
        for ($i = 0; $i -lt $Node.Count; $i++) { $Node[$i] = Convert-OwNormalizeDates -Node $Node[$i] }
        return , $Node
    }
    if ($Node -is [System.Management.Automation.PSCustomObject]) {
        foreach ($p in $Node.PSObject.Properties) { $p.Value = Convert-OwNormalizeDates -Node $p.Value }
        return $Node
    }
    return $Node
}

function Read-Json {
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
        return (Convert-OwNormalizeDates -Node ($jsonText | ConvertFrom-Json -ErrorAction Stop))
    } catch {
        return $null
    }
}

# ---- PID liveness (mirror run.sh pid_alive: empty/non-numeric/<=0 -> dead) ---
function Test-PidAlive {
    param($ProcId)
    if ($null -eq $ProcId) { return $false }
    $ps = [string]$ProcId
    if ($ps -notmatch '^[0-9]+$') { return $false }
    $pi = 0
    try { $pi = [int]$ps } catch { return $false }
    if ($pi -le 0) { return $false }
    try {
        $proc = Get-Process -Id $pi -ErrorAction SilentlyContinue
        return $null -ne $proc
    } catch {
        return $false
    }
}

# ---- SWEEP-10 / DEFER-095: identity-aware liveness via lib/pid-liveness.ps1 --
# A recycled registry pid made a DEAD session read alive: never stall-flagged,
# its shared claims honoured, its touches claims live. Refuse on POSITIVE
# evidence only -- absent anchor or lib unavailable is byte-identical to the
# existence-only probe (a stall detector must never page the Overseer about a
# LIVE session on unreadable evidence). Anchor-only, no name glob (DEC-257).
# Path REPORTABLE (QUIESCE-08 W4). Dot-sourced at SCRIPT scope.
$owPidIdentity = 'fallback'
foreach ($owPlCand in @(
    (Join-Path $_thisDir "../../scripts/lib/pid-liveness.ps1"),
    (Join-Path $_thisDir "../../../claude-template/scripts/lib/pid-liveness.ps1"),
    (Join-Path $env:USERPROFILE ".claude/scripts/lib/pid-liveness.ps1")
)) {
    if ($owPlCand -and (Test-Path -LiteralPath $owPlCand)) {
        try {
            if (-not (Test-Path variable:global:_UldfPidLivenessLoaded)) { $global:_UldfPidLivenessLoaded = $false }
            . $owPlCand
            if (Get-Command Test-UldfPidAliveAs -ErrorAction SilentlyContinue) { $owPidIdentity = 'lib' }
        } catch { }
        break
    }
}
if ($env:ULDF_OW_REPORT_PID_IDENTITY -eq '1') {
    Write-Output $owPidIdentity
    exit 0
}

function Test-OwPidAliveAs {
    # Test-OwPidAliveAs <pid> <anchor> -- identity-aware when the lib resolved.
    param($ProcId, [string]$Anchor = '')
    if ($script:owPidIdentity -eq 'lib') {
        return [bool](Test-UldfPidAliveAs $ProcId $Anchor)
    }
    return (Test-PidAlive -ProcId $ProcId)
}

# ---- Invoke a sibling oracle .ps1 as a child process, BOUNDED by a timeout ----
# Mirror run.sh `bounded <secs> bash <child>`: a slow child cannot drag this
# aggregator past the fast-lane budget on the session-start briefing path. On
# timeout the child is killed and "" is returned -> the source degrades to
# NO-DATA (graceful, mark_absent-equivalent), never blocks. Uses a real
# [Diagnostics.Process] with WaitForExit(ms) (reliable on Windows PowerShell 5.1)
# and an async stdout read to avoid a full-pipe deadlock on the child's output.
function Invoke-SiblingPs1 {
    param([string]$Path, [double]$TimeoutSec = 5)
    $proc = $null
    try {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName               = "powershell.exe"
        $psi.Arguments              = "-NoProfile -ExecutionPolicy Bypass -File `"$Path`""
        $psi.UseShellExecute        = $false
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError  = $true
        $psi.CreateNoWindow         = $true
        # Inherit this aggregator's cwd so the child resolves paths identically.
        $psi.WorkingDirectory       = (Get-Location).Path
        $proc = [System.Diagnostics.Process]::Start($psi)
        # Read stdout asynchronously so a large/blocking child cannot deadlock us.
        $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
        $timeoutMs = [int]([Math]::Max(0, $TimeoutSec * 1000))
        if (-not $proc.WaitForExit($timeoutMs)) {
            try { $proc.Kill() } catch { }
            return ""   # timed out -> NO-DATA
        }
        $out = $stdoutTask.GetAwaiter().GetResult()
        if ($null -eq $out) { return "" }
        return [string]$out
    } catch {
        return ""
    } finally {
        if ($proc) { try { $proc.Dispose() } catch { } }
    }
}

# ---- Locate the registry file (first-match wins) ----------------------------
# (legacy ltads/sessions/active-sessions.json fallback retired -- dead per DEC-124/Arc 1 DEC-198)
$registry = $null
if (Test-Path ".claude/collaboration/active-sessions.json") {
    $registry = Join-Path $_owPwd ".claude/collaboration/active-sessions.json"
}

# ---- Accumulators -----------------------------------------------------------
$signals      = New-Object System.Collections.Generic.List[string]   # JSON object strings
$detectorIds  = New-Object System.Collections.Generic.List[string]   # for the briefing bracket
$sourcesSeen   = New-Object System.Collections.Generic.List[string]
$sourcesAbsent = New-Object System.Collections.Generic.List[string]

function Add-Signal {
    param([string]$DetectorId, [string]$Severity, [string]$SessionsJson, [string]$Summary)
    $entry = '{"detectorId":"' + (Esc $DetectorId) + '","severity":"' + (Esc $Severity) +
             '","sessions":' + $SessionsJson + ',"summary":"' + (Esc $Summary) + '"}'
    $signals.Add($entry) | Out-Null
    $detectorIds.Add($DetectorId) | Out-Null
}

# =============================================================================
# (d) stall: active registry entries with a dead claudeShellPid
# =============================================================================
if ($registry) {
    $sourcesSeen.Add("stall-monitor") | Out-Null
    $data = Read-Json -Path $registry
    $sessions = @()
    if ($null -ne $data -and ($data.PSObject.Properties.Name -contains "sessions")) {
        $sessions = @($data.sessions)
    }
    foreach ($s in $sessions) {
        if ($null -eq $s) { continue }
        if ([string]$s.status -ne "active") { continue }
        if ($null -eq $s.claudeShellPid) { continue }
        $sid = if ($s.id) { [string]$s.id } else { "" }
        if (-not $sid) { continue }
        $shellPid = $s.claudeShellPid
        if (([string]$shellPid) -notmatch '^[0-9]+$') { continue }
        $role = ""
        if ($s.sessionRole) { $role = [string]$s.sessionRole }
        elseif ($s.role)    { $role = [string]$s.role }
        if (-not $role) { $role = "worker" }
        $shellWa = ""
        if ($s.PSObject.Properties.Name -contains 'claudeShellPidWrittenAt' -and $null -ne $s.claudeShellPidWrittenAt) {
            $shellWa = [string]$s.claudeShellPidWrittenAt
        }
        if (-not (Test-OwPidAliveAs $shellPid $shellWa)) {
            Add-Signal "stall-monitor" "critical" ('["' + (Esc $sid) + '"]') `
                "active $role $sid claudeShellPid $shellPid is dead (terminal/stall)"
        }
    }
} else {
    $sourcesAbsent.Add("stall-monitor") | Out-Null
}

# =============================================================================
# (a) concurrent-mutation: reuse the sibling CSI-10 oracle's verdict
# =============================================================================
$cmRun = $null
foreach ($cand in @(
    (Join-Path $_owPwd ".claude/oracles/concurrent-mutation/run.ps1"),
    (Join-Path $_thisDir "../concurrent-mutation/run.ps1")
)) {
    if ($cand -and (Test-Path -LiteralPath $cand)) { $cmRun = $cand; break }
}
if ($cmRun) {
    $sourcesSeen.Add("concurrent-mutation") | Out-Null
    $cmOut = Invoke-SiblingPs1 -Path $cmRun -TimeoutSec 1.5
    if ($cmOut) {
        $cmObj = $null
        try { $cmObj = $cmOut | ConvertFrom-Json -ErrorAction Stop } catch { $cmObj = $null }
        if ($null -ne $cmObj) {
            $ext = $false
            if ($cmObj.PSObject.Properties.Name -contains 'external_mutation') {
                $ext = [bool]$cmObj.external_mutation
            }
            if ($ext) {
                $cmSum = ""
                if ($cmObj.PSObject.Properties.Name -contains 'summary' -and $cmObj.summary) {
                    $cmSum = [string]$cmObj.summary
                }
                if (-not $cmSum) { $cmSum = "external LTADS mutation detected" }
                Add-Signal "concurrent-mutation" "warn" "[]" $cmSum
            }
        }
    }
} else {
    $sourcesAbsent.Add("concurrent-mutation") | Out-Null
}

# =============================================================================
# (b) shared-foreign: another live session's sharedClaim on a shared-repo file
# =============================================================================
$wsrRun = $null
foreach ($cand in @(
    (Join-Path $_owPwd ".claude/oracles/workspace-shared-repos/run.ps1"),
    (Join-Path $_thisDir "../workspace-shared-repos/run.ps1")
)) {
    if ($cand -and (Test-Path -LiteralPath $cand)) { $wsrRun = $cand; break }
}
if ($wsrRun) {
    $sharedPaths = @()
    $wsrOut = Invoke-SiblingPs1 -Path $wsrRun -TimeoutSec 1.0
    if ($wsrOut) {
        $wsrObj = $null
        try { $wsrObj = $wsrOut | ConvertFrom-Json -ErrorAction Stop } catch { $wsrObj = $null }
        if ($null -ne $wsrObj -and ($wsrObj.PSObject.Properties.Name -contains 'repos') -and $wsrObj.repos) {
            foreach ($r in @($wsrObj.repos)) {
                if ($null -ne $r -and $r.path) { $sharedPaths += [string]$r.path }
            }
        }
    }
    if ($sharedPaths.Count -gt 0) {
        $sourcesSeen.Add("shared-foreign-claim") | Out-Null
        $mySid = if ($env:CLAUDE_SESSION_ID) { [string]$env:CLAUDE_SESSION_ID } else { "" }
        $nowIso = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture)
        foreach ($repo in $sharedPaths) {
            if ([string]::IsNullOrEmpty($repo)) { continue }
            $sreg = Join-Path $repo ".claude/collaboration/active-sessions.json"
            if (-not (Test-Path -LiteralPath $sreg)) { continue }
            $sdata = Read-Json -Path $sreg
            if ($null -eq $sdata) { continue }
            $ssessions = @()
            if ($sdata.PSObject.Properties.Name -contains 'sessions') { $ssessions = @($sdata.sessions) }
            $rname = Split-Path -Leaf $repo
            foreach ($e in $ssessions) {
                if ($null -eq $e) { continue }
                $eid = if ($e.id) { [string]$e.id } else { "" }
                if ($eid -eq $mySid) { continue }
                if ([string]$e.status -ne "active") { continue }
                $claim = $e.sharedClaim
                if ($null -eq $claim) { continue }
                $files = @()
                if ($claim.PSObject.Properties.Name -contains 'files' -and $claim.files) { $files = @($claim.files) }
                if ($files.Count -le 0) { continue }
                $bu = ""
                if ($claim.PSObject.Properties.Name -contains 'boundUntil' -and $claim.boundUntil) { $bu = [string]$claim.boundUntil }
                if ($bu -and ($bu -lt $nowIso)) { continue }
                # Liveness: claim liveness is its entry's PID liveness (CSI-07).
                # A null pid skips the liveness gate (mirrors run.sh).
                # Identity-aware per DEFER-095.
                $epid = $e.claudeShellPid
                if ($null -ne $epid -and ([string]$epid) -ne "") {
                    $ewa = ""
                    if ($e.PSObject.Properties.Name -contains 'claudeShellPidWrittenAt' -and $null -ne $e.claudeShellPidWrittenAt) {
                        $ewa = [string]$e.claudeShellPidWrittenAt
                    }
                    if (-not (Test-OwPidAliveAs $epid $ewa)) { continue }
                }
                $nfiles = $files.Count
                Add-Signal "shared-foreign-claim" "warn" ('["' + (Esc $eid) + '"]') `
                    "$eid holds a live claim on $nfiles file(s) in shared repo $rname"
            }
        }
    } else {
        $sourcesAbsent.Add("shared-foreign-claim") | Out-Null
    }
} else {
    $sourcesAbsent.Add("shared-foreign-claim") | Out-Null
}

# =============================================================================
# (c) touches pre-screen: a touches.json path claimed by >=2 distinct sessions
# =============================================================================
# Liveness predicate (DEC-215, DEFER-039 phantom fix — mirror run.sh): a claim
# is only as live as its CLAIMANT. Live iff (1) the claiming collab dir's
# workers/<id>/shell.pid names a live PID, or (2) an active registry entry
# whose JSON mentions this collab id has id==claimant and a live PID. Probes
# cached per (dir,claimant).
$script:_tcLiveCache = @{}
function Test-TcClaimantAlive {
    param([string]$CollabDir, [string]$ClaimantId)
    $key = "$CollabDir|$ClaimantId"
    if ($script:_tcLiveCache.ContainsKey($key)) { return $script:_tcLiveCache[$key] }
    $alive = $false
    # (1) worker shell.pid inside the claiming collab dir
    $pidFile = Join-Path (Join-Path (Join-Path $CollabDir "workers") $ClaimantId) "shell.pid"
    if (Test-Path -LiteralPath $pidFile) {
        try {
            $pRaw = (Get-Content -LiteralPath $pidFile -Raw -ErrorAction Stop) -replace '[^0-9]', ''
            if ($pRaw -and (Test-PidAlive -ProcId $pRaw)) { $alive = $true }
        } catch { }
    }
    # (2) active registry entry mentioning this collab id with id==claimant
    if (-not $alive -and $registry -and (Test-Path -LiteralPath $registry)) {
        $rdata = Read-Json -Path $registry
        if ($null -ne $rdata -and ($rdata.PSObject.Properties.Name -contains 'sessions')) {
            $cbase = Split-Path -Leaf $CollabDir
            foreach ($e in @($rdata.sessions)) {
                if ($null -eq $e) { continue }
                if ([string]$e.status -ne "active") { continue }
                $eid = if ($e.id) { [string]$e.id } else { "" }
                if ($eid -ne $ClaimantId) { continue }
                $ejson = ""
                try { $ejson = ($e | ConvertTo-Json -Depth 10 -Compress) } catch { $ejson = "" }
                if ($ejson -notlike "*$cbase*") { continue }
                $epid = $null
                $ewa = ""
                if ($e.PSObject.Properties.Name -contains 'claudeShellPid' -and $null -ne $e.claudeShellPid) {
                    $epid = $e.claudeShellPid
                    # Anchor rides only the claudeShellPid form (DEFER-095).
                    if ($e.PSObject.Properties.Name -contains 'claudeShellPidWrittenAt' -and $null -ne $e.claudeShellPidWrittenAt) {
                        $ewa = [string]$e.claudeShellPidWrittenAt
                    }
                }
                elseif ($e.PSObject.Properties.Name -contains 'pid' -and $null -ne $e.pid) { $epid = $e.pid }
                if ($null -ne $epid -and (Test-OwPidAliveAs $epid $ewa)) { $alive = $true; break }
            }
        }
    }
    $script:_tcLiveCache[$key] = $alive
    return $alive
}

$touchesFiles = @()
try {
    $touchesFiles = @(Get-ChildItem -Path ".claude/collaboration/*/file-tracking/touches.json" -File -ErrorAction SilentlyContinue)
} catch { $touchesFiles = @() }
if ($touchesFiles.Count -gt 0) {
    $sourcesSeen.Add("touches-conflict") | Out-Null
    foreach ($tf in $touchesFiles) {
        # collab dir = parent of file-tracking/
        $tcCollabDir = Split-Path -Parent (Split-Path -Parent $tf.FullName)
        $tdata = Read-Json -Path $tf.FullName
        if ($null -eq $tdata) { continue }
        if (-not ($tdata.PSObject.Properties.Name -contains 'files') -or $null -eq $tdata.files) { continue }
        $filesObj = $tdata.files
        foreach ($prop in $filesObj.PSObject.Properties) {
            $tpath = $prop.Name
            $rec = $prop.Value
            if ($null -eq $rec) { continue }
            $cl = @()
            $agents = $null
            if ($rec.PSObject.Properties.Name -contains 'agents' -and $rec.agents) { $agents = @($rec.agents) }
            if ($agents -and $agents.Count -gt 0) {
                $cl = $agents
            } elseif ($rec.PSObject.Properties.Name -contains 'touches' -and $rec.touches) {
                foreach ($t in @($rec.touches)) {
                    if ($null -ne $t -and $t.sessionId) { $cl += [string]$t.sessionId }
                }
            }
            # sorted unique non-empty
            $clSorted = @($cl | Where-Object { $_ } | ForEach-Object { [string]$_ } | Sort-Object -Unique)
            # DEC-215: keep only LIVE claimants; >=2 live claimants to signal.
            $clLive = @($clSorted | Where-Object { Test-TcClaimantAlive -CollabDir $tcCollabDir -ClaimantId $_ })
            if ($clLive.Count -ge 2) {
                $csv = ($clLive -join ",")
                $sessJson = "[" + (($clLive | ForEach-Object { '"' + (Esc $_) + '"' }) -join ",") + "]"
                Add-Signal "touches-conflict" "warn" $sessJson `
                    "$tpath claimed by >=2 live sessions ($csv) -- needs Overseer conflict pass"
            }
        }
    }
} else {
    $sourcesAbsent.Add("touches-conflict") | Out-Null
}

# =============================================================================
# (e) resource-contention: overlapping live resource claims, or a live claim
#     coexisting with a foreign RUNNING tracked job (CSO-21, § SACT; DEC-269)
# =============================================================================
# Mirror of run.sh leg (e) -- see that file for the full rationale and the
# declared bounds (null-owner jobs excluded, no cmd->resource classifier,
# unclaimed jobs out of scope, `warn`/nudge ceiling because pausing the second
# claimant would arbitrate the resource to the first).
# Two engine-specific notes:
#   * claims are read through Read-Json, so `boundUntil` is already normalized
#     to an ISO string -- without that, pwsh 6+ would coerce it to [DateTime]
#     and the lexical compare would read every live claim as expired (DEC-263).
#   * session ids are sorted with an ORDINAL comparer, not Sort-Object (which is
#     culture-aware and case-insensitive), so the ordering matches `sort -u` in
#     the twin byte-for-byte.
function Sort-RcOrdinal {
    param([string[]]$Values)
    $list = New-Object System.Collections.Generic.List[string]
    foreach ($v in @($Values)) { if ($v -and -not $list.Contains([string]$v)) { $list.Add([string]$v) | Out-Null } }
    $arr = $list.ToArray()
    [System.Array]::Sort($arr, [System.StringComparer]::Ordinal)
    return , $arr
}

$rcJobsDir = Join-Path $_owPwd ".claude/session-state/jobs"
if ($registry) {
    $sourcesSeen.Add("resource-contention") | Out-Null
    $rcNow = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture)

    # --- live resource claims -> resource => claimant ids, and a kind per resource
    $rcClaims = @{}    # resource -> List[string] of claimant session ids
    $rcKinds  = @{}    # resource -> first non-empty kind seen
    $rcData = Read-Json -Path $registry
    $rcSessions = @()
    if ($null -ne $rcData -and ($rcData.PSObject.Properties.Name -contains 'sessions')) {
        $rcSessions = @($rcData.sessions)
    }
    foreach ($e in $rcSessions) {
        if ($null -eq $e) { continue }
        if ([string]$e.status -ne "active") { continue }
        $claim = $e.resourceClaim
        if ($null -eq $claim) { continue }
        $res = ""
        if ($claim.PSObject.Properties.Name -contains 'resource' -and $claim.resource) { $res = [string]$claim.resource }
        if (-not $res) { continue }
        $eid = if ($e.id) { [string]$e.id } else { "" }
        if (-not $eid) { continue }
        $bu = ""
        if ($claim.PSObject.Properties.Name -contains 'boundUntil' -and $claim.boundUntil) { $bu = [string]$claim.boundUntil }
        if ($bu -and ($bu -lt $rcNow)) { continue }
        # Liveness: the claim is as live as its owning entry (CSI-07);
        # identity-aware per DEFER-095. A null pid skips the gate (mirrors run.sh).
        $epid = $e.claudeShellPid
        if ($null -ne $epid -and ([string]$epid) -ne "") {
            $ewa = ""
            if ($e.PSObject.Properties.Name -contains 'claudeShellPidWrittenAt' -and $null -ne $e.claudeShellPidWrittenAt) {
                $ewa = [string]$e.claudeShellPidWrittenAt
            }
            if (-not (Test-OwPidAliveAs $epid $ewa)) { continue }
        }
        if (-not $rcClaims.ContainsKey($res)) { $rcClaims[$res] = New-Object System.Collections.Generic.List[string] }
        if (-not $rcClaims[$res].Contains($eid)) { $rcClaims[$res].Add($eid) | Out-Null }
        $ckind = ""
        if ($claim.PSObject.Properties.Name -contains 'kind' -and $claim.kind) { $ckind = [string]$claim.kind }
        if ($ckind -and -not $rcKinds.ContainsKey($res)) { $rcKinds[$res] = $ckind }
    }

    # --- running, non-stalled, session-attributed tracked jobs: owner => job ids
    # No counterpart to run.sh's `mark_absent "resource-contention:jobs"`, and that
    # is NOT a TWIN gap: the bash twin needs a portable `date` to build the
    # staleness cutoff and marks the job half NO-DATA when it cannot get one,
    # whereas Get-Date cannot fail. There is no reachable branch to mirror.
    $rcJobOwners = @{}     # owner sessionId -> List[string] of job ids
    if (Test-Path -LiteralPath $rcJobsDir) {
        $rcStall = 90
        if ($env:GC_JOB_STALL_SECONDS -and ($env:GC_JOB_STALL_SECONDS -as [int])) { $rcStall = [int]$env:GC_JOB_STALL_SECONDS }
        $rcCutoff = (Get-Date).ToUniversalTime().AddSeconds(-1 * $rcStall).ToString("yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture)
        $rcJobFiles = @()
        try { $rcJobFiles = @(Get-ChildItem -Path (Join-Path $rcJobsDir "*.json") -File -ErrorAction SilentlyContinue) } catch { $rcJobFiles = @() }
        foreach ($jf in $rcJobFiles) {
            $j = Read-Json -Path $jf.FullName
            if ($null -eq $j) { continue }
            if ([string]$j.status -ne "running") { continue }
            $owner = ""
            if ($j.PSObject.Properties.Name -contains 'session_id' -and $null -ne $j.session_id) { $owner = [string]$j.session_id }
            if (-not $owner) { continue }
            $hb = ""
            if ($j.PSObject.Properties.Name -contains 'last_heartbeat_at' -and $j.last_heartbeat_at) { $hb = [string]$j.last_heartbeat_at }
            if ($hb -and ($hb -lt $rcCutoff)) { continue }
            $jid = ""
            if ($j.PSObject.Properties.Name -contains 'job_id' -and $null -ne $j.job_id) { $jid = [string]$j.job_id }
            if (-not $rcJobOwners.ContainsKey($owner)) { $rcJobOwners[$owner] = New-Object System.Collections.Generic.List[string] }
            if (-not $rcJobOwners[$owner].Contains($jid)) { $rcJobOwners[$owner].Add($jid) | Out-Null }
        }
    }

    # --- compose: one signal per contended resource (CSO-05 grouping)
    foreach ($res in (Sort-RcOrdinal -Values @($rcClaims.Keys))) {
        $claimants = Sort-RcOrdinal -Values @($rcClaims[$res])
        $kindTag = ""
        if ($rcKinds.ContainsKey($res)) { $kindTag = " [" + $rcKinds[$res] + "]" }
        if ($claimants.Count -ge 2) {
            $csv = ($claimants -join ",")
            $sj = "[" + (($claimants | ForEach-Object { '"' + (Esc $_) + '"' }) -join ",") + "]"
            Add-Signal "resource-contention" "warn" $sj `
                "$res claimed by $($claimants.Count) live sessions ($csv)$kindTag -- overlapping resource claims"
        } elseif ($claimants.Count -eq 1 -and $rcJobOwners.Count -gt 0) {
            $owner = [string]$claimants[0]
            $fOwners = @($rcJobOwners.Keys | Where-Object { [string]$_ -ne $owner })
            if ($fOwners.Count -gt 0) {
                $fJobs = New-Object System.Collections.Generic.List[string]
                foreach ($fo in $fOwners) { foreach ($jid in $rcJobOwners[$fo]) { $fJobs.Add([string]$jid) | Out-Null } }
                $fJobsCsv = ((Sort-RcOrdinal -Values $fJobs.ToArray()) -join ",")
                $allSess = Sort-RcOrdinal -Values (@($owner) + @($fOwners | ForEach-Object { [string]$_ }))
                $sj = "[" + (($allSess | ForEach-Object { '"' + (Esc $_) + '"' }) -join ",") + "]"
                Add-Signal "resource-contention" "warn" $sj `
                    "$owner holds a live claim on $res$kindTag while other session(s) run tracked job(s) $fJobsCsv -- verify they do not contend"
            }
        }
    }
} else {
    $sourcesAbsent.Add("resource-contention") | Out-Null
}

# =============================================================================
# Compose verdict
# =============================================================================
$sigCount = $signals.Count
if ($sigCount -gt 0) {
    $wake = "true"
    $summary = "$sigCount airspace signal(s) -- wake Overseer"
    # bracket: detectorIds sorted, uniq -c, formatted id(count), space-joined
    $bracket = ($detectorIds | Group-Object | Sort-Object Name | ForEach-Object { "$($_.Name)($($_.Count))" }) -join " "
    $briefing = "overseer-wake: $sigCount airspace signal(s) [$bracket] -- run the Overseer (perceive -> advise) or /0-uldf-oracle overseer-wake"
} else {
    $wake = "false"
    $seenTrim = ($sourcesSeen -join " ")
    if ($seenTrim) {
        $summary = "no airspace signals (sources observed: $seenTrim)"
    } else {
        $summary = "no detector sources available (NO-DATA -- not an all-clear)"
    }
    $briefing = ""
}

$signalsJoined = ($signals -join ",")
$out = '{"wake":' + $wake + ',"signals":[' + $signalsJoined + '],"summary":"' + (Esc $summary) + '","briefing":"' + (Esc $briefing) + '"}'
[Console]::Out.Write($out + "`n")
exit 0
