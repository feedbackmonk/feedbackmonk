# dispatchable-sessions oracle (Windows PowerShell)
# Answers: what live sibling sessions can THIS session dispatch work to right now?
#
# Reads .claude/collaboration/active-sessions.json. (Legacy ltads/sessions/active-sessions.json retired, Arc 1 DEC-198 -- dead per DEC-124.)
# Emits a JSON object with:
#   - count   : integer, number of live dispatchable peers
#   - peers   : array of {sessionId, sessionRole, role, workDir, claudeShellPid, dispatchable, spawnedAt, siblingGroup?}
#   - briefing: human-readable one-line summary for the session-start ORACLE BRIEFING
#
# Filter: status=='active' AND dispatchable==true AND claudeShellPid!=null AND PID alive.
# Legacy entries (no registryVersion or registryVersion=1) silently drop -- they predate dispatch.
# Strategy: always-fresh. Read-only on the registry (no mutation; stale-cleanup is a separate path).
#
# SELF-EXCLUSION (DISC-CSI-22): a session's OWN registry entry is not a sibling.
# The briefing path drops any entry whose id matches ULDF_SELF_SESSION_ID
# (exported by the session-start hook, which registered that id moments earlier)
# or, failing that, CLAUDE_SESSION_ID (present in spawned workers' env). When
# neither is set, no exclusion is possible and the caller's own entry MAY appear
# as a peer -- callers that know their id should set ULDF_SELF_SESSION_ID.
# Trigger incident: 2026-07-07 session interactive-20260707T162816Z-457288
# was briefed its own entry as a live sibling.
#
# PROJECT-SCOPED BY DESIGN: reads ONLY the current project's registry (there is no
# machine-global session registry; each project owns its own). It deliberately does
# NOT surface sessions from OTHER projects -- auto-surfacing would leak unrelated
# work into every briefing and invite mis-targeting. A cross-project session you
# spawned (spawn-claude-session -WorkDir <other>) is reached EXPLICITLY via
# `/0-uldf-dispatch --project=<path>`; completion notifies back via CSI-17. Model:
# FOUNDATIONS/CSI_DESIGN.md  4.5b; docs/planning/cross-project-session-dispatch-20260625.md.
#
# Modes (CSI-05 added --gc, --gc-cheap; RESUME-03 added --duplicate-of):
#   (default)           : read-only briefing path described above
#   --gc-cheap          : session-start hygiene sweep, ~1000ms budget (DEC-79-class
#                         calibration; was 100ms — self-consumed by Get-Process probes),
#                         defers honestly if exceeded
#   --gc                : on-demand hygiene sweep, no time budget, prints {swept,before,after,threshold,thresholdSource}
#   --driver-of=<arc>   : CSI-07 live driver-claim query
#   --duplicate-of=<id> : RESUME-03 project-scoped duplicate-self-check. Answers:
#                         is ANOTHER live session currently holding identity <id>
#                         in THIS project? Reads ONLY the project registry (never
#                         a machine-wide process scan -- DEFER-002 trigger incident:
#                         a worker misidentified an unrelated project's session as
#                         its twin via Get-Process). duplicate==true requires:
#                         active entry with id==<id> AND claudeShellPid alive AND
#                         the pid is NOT in the caller's own ancestor chain AND
#                         the entry's workDir matches the current workDir.
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
$driverOfArc = ""
$duplicateOfId = ""
if ($gc) { $mode = "gc" }
if ($gcCheap) { $mode = "gc-cheap" }
foreach ($a in $args) {
    switch -Wildcard ($a) {
        "--gc"          { $mode = "gc" }
        "--gc-cheap"    { $mode = "gc-cheap" }
        "--dry-run"     { $mode = "dry-run" }
        "--driver-of=*" {
            # CSI-07 query interface: who (if anyone) live-drives arc <arc>?
            $mode = "driver-of"
            $driverOfArc = ([string]$a).Substring(12)
            if (-not $driverOfArc) {
                Write-Error "dispatchable-sessions: --driver-of requires an arc id"
                exit 1
            }
        }
        "--duplicate-of=*" {
            # RESUME-03 query interface: does another live session hold identity <id> here?
            $mode = "duplicate-of"
            $duplicateOfId = ([string]$a).Substring(15)
            if (-not $duplicateOfId) {
                Write-Error "dispatchable-sessions: --duplicate-of requires a session id"
                exit 1
            }
        }
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
# (legacy ltads/sessions/active-sessions.json fallback retired -- dead per DEC-124/Arc 1 DEC-198;
# spawn + the CSI hooks have written .claude/collaboration/active-sessions.json since DISPATCH-01)
if (Test-Path ".claude/collaboration/active-sessions.json") {
    $registry = Join-Path $_dsPwd ".claude/collaboration/active-sessions.json"
} elseif (Test-Path ".claude/collaboration") {
    # DISC-CSI-27: the collaboration dir exists, so this project DOES keep a
    # registry -- "the file isn't there" is more likely the publish-rename
    # window than a genuine absence, and concluding "no live siblings" from it
    # is a false-death signal. One short re-probe. Projects with no
    # collaboration dir at all never pay this.
    Start-Sleep -Milliseconds 60
    if (Test-Path ".claude/collaboration/active-sessions.json") {
        $registry = Join-Path $_dsPwd ".claude/collaboration/active-sessions.json"
    }
}

if (-not $registry) {
    if ($mode -eq "briefing") { Emit-Empty }
    if ($mode -eq "gc" -or $mode -eq "dry-run") {
        Write-Output '{"swept":0,"before":0,"after":0,"threshold":"P1D","thresholdSource":"default","note":"no registry"}'
    }
    if ($mode -eq "driver-of") {
        Write-Output ('{"arc":"' + $driverOfArc + '","driver":null,"note":"no registry"}')
    }
    if ($mode -eq "duplicate-of") {
        # Gracefully absent: no registry means no project-scoped evidence of a
        # duplicate. duplicate:false -- a resume proceeds (never stand down on
        # absent evidence; DEFER-002).
        Write-Output ('{"sessionId":"' + $duplicateOfId + '","duplicate":false,"holder":null,"note":"no registry"}')
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

# ---- Identity-aware liveness (SWEEP-10 / DEC-252) ----------------------------
# A PID is not an identity; the OS reuses it, and a recycled id pins a DEAD
# session in `active` forever. See run.sh for the measured trigger (two entries
# held by RuntimeBroker and svchost, both reading ALIVE). This file carries its
# own inline probe from CSI-05, so rather than duplicate the identity logic --
# where a silent divergence between the copies is precisely the proxy-referent
# hazard OVALID warns about -- source the shared lib and delegate.
#
# The graceful fallback is itself the hazard (QUIESCE-08 W4): a broken load path
# installs a no-op and every harness stays green with the fix never loaded. So
# the chosen path is REPORTABLE and a smoke asserts it.
$script:UldfDsPidIdentity = "fallback"
foreach ($cand in @(
    (Join-Path $PSScriptRoot "..\..\scripts\lib\pid-liveness.ps1"),
    (Join-Path $PSScriptRoot "..\..\..\claude-template\scripts\lib\pid-liveness.ps1"),
    (Join-Path $env:USERPROFILE ".claude\scripts\lib\pid-liveness.ps1")
)) {
    if (Test-Path $cand) { try { . $cand; $script:UldfDsPidIdentity = "lib" } catch { } ; break }
}

function Test-PidAliveAs {
    param([int]$ProcId, [string]$RecordedAt, [string]$NameLike)
    if ($script:UldfDsPidIdentity -eq "lib" -and (Get-Command Test-UldfPidAliveAs -ErrorAction SilentlyContinue)) {
        return [bool](Test-UldfPidAliveAs $ProcId $RecordedAt $NameLike)
    }
    return (Test-PidAlive -ProcId $ProcId)   # pre-DEC-252 behaviour, identity ignored
}

if ($env:ULDF_DS_REPORT_PID_IDENTITY -eq "1") { Write-Output $script:UldfDsPidIdentity; exit 0 }

# ---- UTF-8-no-BOM read helper (handles BOM) ----
# DISC-CSI-27: reads RETRY across the publish window. A tmp->registry publish
# rename makes the destination report as absent (or throw ERROR_ACCESS_DENIED)
# to a concurrent reader for a short interval. An unretried read returns $null
# there, and every mode above turns $null into "no sessions" -- i.e. LIVE peers
# reported as MISSING FROM REGISTRY. That false-death signal is what invites a
# duplicate spawn onto work that is still running, so the retry is load-bearing,
# not cosmetic.
function Convert-DsNormalizeDates {
    param($Node)
    # TWIN-01 / DEFER-101 (DEC-263). pwsh 6+'s ConvertFrom-Json coerces
    # ISO-8601-shaped strings to [DateTime]; powershell.exe 5.1 leaves them as
    # [string] (DISC-ARC-01). The SACT-06 resourceClaim leg compares
    # `[string]$s.resourceClaim.boundUntil` lexically against an ISO "now", and
    # [string] on a [DateTime] renders it in the CURRENT CULTURE
    # ("01/01/2099 00:00:00") -- which sorts BELOW every ISO date, so under
    # pwsh every live claim was dropped from peers[]. It also protects the
    # EMITTED schema: the claim object is passed through to ConvertTo-Json
    # verbatim, so a surviving [DateTime] changes the oracle's own output shape
    # (SACT-06 specifies boundUntil as an ISO string).
    if ($null -eq $Node) { return $null }
    if ($Node -is [datetime]) {
        if ($Node.Kind -eq [System.DateTimeKind]::Local) { $Node = $Node.ToUniversalTime() }
        if ($Node.Kind -eq [System.DateTimeKind]::Utc) {
            if ($Node.Millisecond -ne 0) { return $Node.ToString("yyyy-MM-ddTHH:mm:ss.fffZ") }
            return $Node.ToString("yyyy-MM-ddTHH:mm:ssZ")
        }
        if ($Node.Millisecond -ne 0) { return $Node.ToString("yyyy-MM-ddTHH:mm:ss.fff") }
        return $Node.ToString("yyyy-MM-ddTHH:mm:ss")
    }
    if ($Node -is [System.Array]) {
        for ($i = 0; $i -lt $Node.Count; $i++) { $Node[$i] = Convert-DsNormalizeDates -Node $Node[$i] }
        return , $Node
    }
    if ($Node -is [System.Management.Automation.PSCustomObject]) {
        foreach ($p in $Node.PSObject.Properties) { $p.Value = Convert-DsNormalizeDates -Node $p.Value }
        return $Node
    }
    return $Node
}

function Read-RegistryJson {
    param([string]$Path)
    $delays = @(20, 60, 150)
    for ($i = 0; $i -le $delays.Count; $i++) {
        try {
            $bytes = [System.IO.File]::ReadAllBytes($Path)
            if ($bytes.Length -eq 0) { return $null }
            if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
                $jsonText = [System.Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
            } else {
                $jsonText = [System.Text.Encoding]::UTF8.GetString($bytes)
            }
            if ([string]::IsNullOrWhiteSpace($jsonText)) { return $null }
            return (Convert-DsNormalizeDates -Node ($jsonText | ConvertFrom-Json -ErrorAction Stop))
        } catch {
            # A genuinely absent registry is not worth retrying; only the
            # transient publish-window states are.
            if (-not (Test-Path -LiteralPath $Path) -and $i -eq 0) {
                Start-Sleep -Milliseconds $delays[0]
                if (-not (Test-Path -LiteralPath $Path)) { return $null }
            }
            if ($i -lt $delays.Count) { Start-Sleep -Milliseconds $delays[$i] }
        }
    }
    return $null
}

$data = Read-RegistryJson -Path $registry
if ($null -eq $data) {
    if ($mode -eq "briefing") { Emit-Empty }
    if ($mode -eq "gc" -or $mode -eq "dry-run") {
        Write-Output '{"swept":0,"before":0,"after":0,"threshold":"P1D","thresholdSource":"default","note":"unparseable registry"}'
    }
    if ($mode -eq "driver-of") {
        Write-Output ('{"arc":"' + $driverOfArc + '","driver":null,"note":"unparseable registry"}')
    }
    if ($mode -eq "duplicate-of") {
        Write-Output ('{"sessionId":"' + $duplicateOfId + '","duplicate":false,"holder":null,"note":"unparseable registry"}')
    }
    exit 0
}

$sessions = @()
if ($data.PSObject.Properties.Name -contains "sessions") {
    $sessions = @($data.sessions)
}

# =============================================================================
# --driver-of=<arc> (CSI-07 query interface)
# =============================================================================
# Returns the LIVE driver claim for the given arc, or driver:null. A claim is
# live when its owning entry is status==active, its boundUntil (if any) has
# not passed, and the entry's claudeShellPid (if any) is alive. Read-only.
# Output (frozen; byte-parity with run.sh): {"arc":"<arc>","driver":null}
#   or {"arc":"<arc>","driver":{"sessionId","arc","consentScope","boundUntil",
#       "entryId","claudeShellPid"}}
if ($mode -eq "driver-of") {
    $dofNow = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    $cand = $null
    foreach ($s in $sessions) {
        if ($null -eq $s) { continue }
        if ([string]$s.status -ne "active") { continue }
        $drv = $s.driver
        if ($null -eq $drv) { continue }
        if ([string]$drv.arc -ne $driverOfArc) { continue }
        $bu = [string]$drv.boundUntil
        if ($bu -and ($bu -lt $dofNow)) { continue }
        $cand = $s
        break
    }
    if ($null -eq $cand) {
        Write-Output ('{"arc":"' + $driverOfArc + '","driver":null}')
        exit 0
    }
    $dofPid = $cand.claudeShellPid
    # SWEEP-10 / DEFER-088: identity-aware -- a recycled pid must not hold the arc.
    if ($dofPid -and ([string]$dofPid -match '^[0-9]+$') -and -not (Test-PidAliveAs -ProcId ([int]$dofPid) -RecordedAt ([string]$cand.claudeShellPidWrittenAt))) {
        Write-Output ('{"arc":"' + $driverOfArc + '","driver":null,"note":"driver pid dead (stale claim)"}')
        exit 0
    }
    $drvOut = $cand.driver
    $result = [ordered]@{
        arc    = $driverOfArc
        driver = [ordered]@{
            sessionId      = if ($drvOut.sessionId) { [string]$drvOut.sessionId } else { [string]$cand.sessionId }
            arc            = [string]$drvOut.arc
            consentScope   = if ($null -ne $drvOut.consentScope) { [string]$drvOut.consentScope } else { $null }
            boundUntil     = if ($null -ne $drvOut.boundUntil) { [string]$drvOut.boundUntil } else { $null }
            entryId        = [string]$cand.id
            claudeShellPid = if ($null -ne $dofPid) { $dofPid } else { $null }
        }
    }
    Write-Output (($result | ConvertTo-Json -Compress -Depth 5))
    exit 0
}

# =============================================================================
# --duplicate-of=<sessionId> (RESUME-03 project-scoped duplicate-self-check)
# =============================================================================
# Answers: is ANOTHER live session currently holding identity <sessionId> in
# THIS project? Project-scoped by construction: only the project registry is
# consulted -- NEVER a machine-wide process scan, which spans every Claude
# session on the machine and misidentifies unrelated projects' sessions as
# twins (DEFER-002 trigger incident).
#
# Output (schema-stable across shells):
#   {"sessionId":"<id>","duplicate":<bool>,"holder":null|{entryId,claudeShellPid,
#    workDir,workDirMatch,sessionRole,spawnedAt,isSelf},"note":"<reason>"}
#
# duplicate==true iff: an ACTIVE registry entry with id==<sessionId> exists,
# its claudeShellPid is ALIVE, the pid is NOT in the calling process's own
# ancestor chain (self-exclusion: the registry entry a resuming session wrote
# for itself is not its own twin), AND the entry's workDir matches the current
# workDir (normalized). A live holder in a different workDir reports the
# holder but duplicate:false (not a same-project duplicate -- worktree
# siblings and registry drift are surfaced, not acted on).
if ($mode -eq "duplicate-of") {
    function Get-DupNormPath {
        param([string]$P)
        if ([string]::IsNullOrEmpty($P)) { return "" }
        # Resolve 8.3 short names (CARBON~1) to long form when the path exists,
        # so a short-form writer and a long-form reader still compare equal.
        try {
            $resolved = (Get-Item -LiteralPath $P -ErrorAction SilentlyContinue).FullName
            if ($resolved) { $P = $resolved }
        } catch { }
        return $P.Replace('/', '\').TrimEnd('\').ToLowerInvariant()
    }

    # Collect this process's ancestor PIDs (bounded walk, cycle-guarded). The
    # caller's claude.exe AND its host shell are both ancestors of this oracle
    # process, so whichever of the two the registry recorded is covered.
    $dupAncestors = @{}
    $dupCur = $PID
    for ($dupHop = 0; $dupHop -lt 30 -and $dupCur -gt 4; $dupHop++) {
        if ($dupAncestors.ContainsKey($dupCur)) { break }
        $dupAncestors[$dupCur] = $true
        $dupParent = $null
        try {
            $dupProc = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$dupCur" -ErrorAction SilentlyContinue
            if ($dupProc -and $dupProc.ParentProcessId) { $dupParent = [int]$dupProc.ParentProcessId }
        } catch { }
        if (-not $dupParent) { break }
        $dupCur = $dupParent
    }

    $dupEntry = $null
    foreach ($s in $sessions) {
        if ($null -eq $s) { continue }
        if ([string]$s.id -ne $duplicateOfId) { continue }
        if ([string]$s.status -ne "active") { continue }
        $dupEntry = $s
        break
    }

    if ($null -eq $dupEntry) {
        Write-Output ('{"sessionId":"' + $duplicateOfId + '","duplicate":false,"holder":null,"note":"no active registry entry for this identity"}')
        exit 0
    }

    $dupPid = 0
    if ($null -ne $dupEntry.claudeShellPid -and ([string]$dupEntry.claudeShellPid) -match '^[0-9]+$') {
        $dupPid = [int]$dupEntry.claudeShellPid
    }
    if ($dupPid -le 0) {
        Write-Output ('{"sessionId":"' + $duplicateOfId + '","duplicate":false,"holder":null,"note":"entry has no pid (cannot be a live duplicate)"}')
        exit 0
    }
    # SWEEP-10 / DEFER-088: identity-aware -- a recycled pid cannot fabricate a
    # live duplicate holder; absent anchor keeps the existence-only verdict.
    if (-not (Test-PidAliveAs -ProcId $dupPid -RecordedAt ([string]$dupEntry.claudeShellPidWrittenAt))) {
        Write-Output ('{"sessionId":"' + $duplicateOfId + '","duplicate":false,"holder":null,"note":"holder pid dead (stale entry; csi-05 sweeps it)"}')
        exit 0
    }

    $dupIsSelf = $dupAncestors.ContainsKey($dupPid)
    $dupEntryWorkDir = if ($dupEntry.workDir) { [string]$dupEntry.workDir } else { "" }
    $dupWorkDirMatch = ((Get-DupNormPath -P $dupEntryWorkDir) -eq (Get-DupNormPath -P $_dsPwd)) -and ($dupEntryWorkDir -ne "")

    $dupVerdict = (-not $dupIsSelf) -and $dupWorkDirMatch
    $dupNote = if ($dupIsSelf) {
        "identity held by the calling session itself"
    } elseif (-not $dupWorkDirMatch) {
        "live holder in a different workDir (not a same-project duplicate)"
    } else {
        "another live session holds this identity in this workDir"
    }

    $dupResult = [ordered]@{
        sessionId = $duplicateOfId
        duplicate = $dupVerdict
        holder    = [ordered]@{
            entryId        = [string]$dupEntry.id
            claudeShellPid = $dupPid
            workDir        = $dupEntryWorkDir
            workDirMatch   = $dupWorkDirMatch
            sessionRole    = if ($dupEntry.sessionRole) { [string]$dupEntry.sessionRole } else { "" }
            spawnedAt      = if ($dupEntry.spawnedAt) { [string]$dupEntry.spawnedAt } else { "" }
            isSelf         = $dupIsSelf
        }
        note      = $dupNote
    }
    Write-Output (($dupResult | ConvertTo-Json -Compress -Depth 5))
    exit 0
}

# =============================================================================
# Mode dispatch
# =============================================================================
if ($mode -eq "gc" -or $mode -eq "gc-cheap" -or $mode -eq "dry-run") {
    # -------------------------------------------------------------------------
    # CSI-05 hygiene sweep
    # --dry-run: identical candidate computation (registry read, liveness probe,
    # age filter), ZERO mutation (SWEEPER-05) -- exits before lock acquisition.
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

    # 1000ms (was 100ms): the per-entry liveness probe loop is self-consuming on
    # Windows — each Get-Process probe + the candidate parse could exceed 100ms
    # before the first sweep fired, so the would-sweep set was abandoned as "over
    # budget" and validate T7 flaked. Calibrated to the same value CLAUDE-C set
    # for the handoff-retention sweep (DEC-79). Deferral stays honest: a genuinely
    # over-budget scan still sets $budgetExceeded -> budgetExceeded:true (gc mode).
    # DEFER-064 parity with run.sh: ULDF_DS_GC_BUDGET_MS test seam (non-numeric
    # falls back), and a minimum-progress guarantee — the budget is consulted
    # only after at least one probe has run, so setup cost can defer the tail
    # of the scan but never starve the whole sweep (validate T8 locks this).
    $budgetMs = 1000
    if ($env:ULDF_DS_GC_BUDGET_MS -match '^[0-9]+$') { $budgetMs = [int]$env:ULDF_DS_GC_BUDGET_MS }
    if ($mode -eq "gc" -or $mode -eq "dry-run") { $budgetMs = 0 }
    $startTicks = [System.Diagnostics.Stopwatch]::StartNew()

    $before = $sessions.Count

    # Identify candidates -> apply liveness + age filter -> mark for sweep.
    $sweepIndices = New-Object System.Collections.ArrayList
    $sweepIds     = New-Object System.Collections.ArrayList
    $budgetExceeded = $false
    $probedAny      = $false   # minimum-progress guarantee (DEFER-064)

    for ($i = 0; $i -lt $sessions.Count; $i++) {
        if ($budgetMs -gt 0 -and $probedAny -and $startTicks.ElapsedMilliseconds -gt $budgetMs) {
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

        # Live PIDs are NEVER swept regardless of age -- and "live" is now
        # identity-aware (SWEEP-10 / DEFER-088): a pid the OS recycled onto an
        # unrelated process is positive evidence of death, so the entry becomes
        # sweepable (the self-healing leg). Anchor is claudeShellPidWrittenAt,
        # never spawnedAt; absent anchor degrades to the existence-only verdict.
        $probedAny = $true
        if (Test-PidAliveAs -ProcId $pidVal -RecordedAt ([string]$s.claudeShellPidWrittenAt)) { continue }

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

    # ---- --dry-run: report the would-sweep set and exit BEFORE the lock -----
    if ($mode -eq "dry-run") {
        $parts = @(
            "`"dryRun`":true",
            "`"wouldSweep`":$sweptCount",
            "`"before`":$before",
            "`"after`":$before",
            "`"threshold`":`"$thresholdDisplay`"",
            "`"thresholdSource`":`"$thresholdSource`""
        )
        if ($sweepIds.Count -gt 0) {
            $idsCsv = ($sweepIds -join ",")
            $parts += "`"wouldSweepIds`":`"$idsCsv`""
        }
        Write-Output ("{" + ($parts -join ",") + "}")
        exit 0
    }

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
                # Identity-aware re-validate (same verdict as the scan above).
                if (Test-PidAliveAs -ProcId $pv -RecordedAt ([string]$entry.claudeShellPidWrittenAt)) { continue }
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
        # session's project, flip the topmost arc in ltads/arc-state.json to
        # CONCLUDED (concludedBy.via: csi-05-gc-sweep) via the ARC-02 lib
        # (ARC-03 migration). Cross-workDir reconciliation forbidden per Phase
        # 1.6 plan -- shared-repo state is reconciled by SHARED-CSI-04 paths,
        # not here. Legacy prose-only projects: no flip (graceful; ltads-state
        # `legacy` verdict + ARC-11 converter are the migration path).
        #
        # Graceful absence: missing lib, missing arc-state.json, or
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

                    $arcStatePath = Join-Path $thisProjectRoot "ltads/arc-state.json"
                    if (-not (Test-Path $arcStatePath)) { continue }

                    # Verify the arc record is about THIS swept session (not a
                    # different session that happens to share the project).
                    # Arc-owner id via the ARC-02 lib -- the most-recent
                    # checkpoints[].by of the topmost arc (DEC-44; JSON
                    # successor of the prose Mid-arc Checkpoint token read).
                    if (-not (Get-Command Get-ArcStateArcOwnerId -ErrorAction SilentlyContinue)) {
                        $astCandidates = @(
                            (Join-Path $PSScriptRoot "../../scripts/lib/arc-state.ps1"),
                            (Join-Path $env:USERPROFILE ".claude/scripts/lib/arc-state.ps1")
                        )
                        foreach ($astCand in $astCandidates) {
                            if (Test-Path $astCand) { . $astCand; break }
                        }
                    }
                    if (-not (Get-Command Get-ArcStateArcOwnerId -ErrorAction SilentlyContinue)) { continue }
                    $csSessionId = Get-ArcStateArcOwnerId -Path $arcStatePath
                    if ([string]::IsNullOrEmpty($csSessionId)) { continue }
                    if ($csSessionId -ne $entryId) { continue }

                    try {
                        Invoke-CsiFlipArcConcluded -ArcStatePath $arcStatePath -SessionId $entryId -ConcludedBy "csi-05-gc-sweep" | Out-Null
                    } catch {
                        # Graceful absence
                    }
                }
            }
        }
    }

    $after = $before - $sweptCount

    # =========================================================================
    # DISC-PRO-13: closed[]-entry size audit (Arc 3 stream 3)
    # =========================================================================
    # Mirror of run.sh: bounds individual closed[] entries at ~4 KB serialized.
    # Oversized entries are truncated to a bounded summary (small scalar fields
    # kept, bulk dropped, truncated:true + originalBytes recorded). SWEEP-08:
    # one JSONL audit line per truncated entry is appended to
    # _registry-audit.jsonl and verified BEFORE the rewrite; on audit failure
    # the rewrite is skipped. Runs in gc AND gc-cheap.
    $closedTruncated = 0
    $closedBoundBytes = 4096
    $registryAuditLog = Join-Path (Split-Path -Parent $registry) "_registry-audit.jsonl"
    if (Test-Path $registry) {
        try {
            $ctData = Read-RegistryJson -Path $registry
            $oversized = New-Object System.Collections.ArrayList
            if ($null -ne $ctData -and $ctData.PSObject.Properties.Name -contains "closed" -and $ctData.closed) {
                $ctClosed = @($ctData.closed)
                for ($ci = 0; $ci -lt $ctClosed.Count; $ci++) {
                    $ce = $ctClosed[$ci]
                    if ($null -eq $ce) { continue }
                    $ceJson = $ce | ConvertTo-Json -Compress -Depth 10
                    $ceBytes = [System.Text.Encoding]::UTF8.GetByteCount($ceJson)
                    if ($ceBytes -gt $closedBoundBytes) {
                        $ceId = ""
                        if ($ce.PSObject.Properties.Name -contains 'id' -and $ce.id) { $ceId = [string]$ce.id }
                        [void]$oversized.Add(@{ index = $ci; id = $ceId; bytes = $ceBytes })
                    }
                }
            }

            if ($oversized.Count -gt 0) {
                # SWEEP-08: audit line per oversized entry BEFORE the rewrite.
                $auditOk = $true
                $utf8NoBomCt = New-Object System.Text.UTF8Encoding $false
                foreach ($ov in $oversized) {
                    $idEsc = ($ov.id -replace '\\', '\\') -replace '"', '\"'
                    $auditLine = '{"ts":"' + $nowIso + '","kind":"closed-entry-truncated","id":"' + $idEsc + '","index":' + $ov.index + ',"originalBytes":' + $ov.bytes + ',"boundBytes":' + $closedBoundBytes + '}'
                    try {
                        [System.IO.File]::AppendAllText($registryAuditLog, $auditLine + "`n", $utf8NoBomCt)
                        $auditTail = [System.IO.File]::ReadAllLines($registryAuditLog)
                        if ($auditTail.Length -eq 0 -or $auditTail[$auditTail.Length - 1] -ne $auditLine) {
                            $auditOk = $false
                            break
                        }
                    } catch {
                        $auditOk = $false
                        break
                    }
                }

                if (-not $auditOk) {
                    Write-Warning "dispatchable-sessions: _registry-audit.jsonl write failed; closed[] truncation skipped (SWEEP-08)"
                } else {
                    # Truncating rewrite under the same mkdir-lock contract.
                    $ctLock = "$registry.lock"
                    $ctLockOk = $false
                    foreach ($ctAttempt in 1..4) {
                        try {
                            New-Item -ItemType Directory -Path $ctLock -ErrorAction Stop | Out-Null
                            $ctLockOk = $true
                            break
                        } catch {
                            switch ($ctAttempt) {
                                1 { Start-Sleep -Milliseconds 50 }
                                2 { Start-Sleep -Milliseconds 200 }
                                3 { Start-Sleep -Milliseconds 800 }
                            }
                        }
                    }
                    if (-not $ctLockOk) {
                        Write-Warning "dispatchable-sessions: lock contention; closed[] truncation deferred"
                    } else {
                        try {
                            # Re-read inside the lock (TOCTOU).
                            $ctData2 = Read-RegistryJson -Path $registry
                            if ($null -ne $ctData2 -and $ctData2.PSObject.Properties.Name -contains "closed" -and $ctData2.closed) {
                                $keepFields = @("id", "spawnedAt", "status", "sweptAt", "closedAt", "closedBy", "role", "workDir")
                                $newClosed = New-Object System.Collections.ArrayList
                                $didTruncate = 0
                                foreach ($ce in @($ctData2.closed)) {
                                    if ($null -ne $ce) {
                                        $ceJson = $ce | ConvertTo-Json -Compress -Depth 10
                                        $ceBytes = [System.Text.Encoding]::UTF8.GetByteCount($ceJson)
                                        if ($ceBytes -gt $closedBoundBytes) {
                                            $t = [ordered]@{}
                                            foreach ($k in $keepFields) {
                                                if ($ce.PSObject.Properties.Name -contains $k -and $null -ne $ce.$k) {
                                                    $v = $ce.$k
                                                    if ($v -is [string] -and $v.Length -gt 256) { $v = $v.Substring(0, 256) }
                                                    $t[$k] = $v
                                                }
                                            }
                                            $t["truncated"] = $true
                                            $t["originalBytes"] = $ceBytes
                                            $t["truncatedAt"] = $nowIso
                                            [void]$newClosed.Add([pscustomobject]$t)
                                            $didTruncate++
                                            continue
                                        }
                                    }
                                    [void]$newClosed.Add($ce)
                                }
                                if ($didTruncate -gt 0) {
                                    $ctData2.closed = $newClosed.ToArray()
                                    if ($ctData2.PSObject.Properties.Name -contains "lastUpdated") {
                                        $ctData2.lastUpdated = $nowIso
                                    } else {
                                        $ctData2 | Add-Member -NotePropertyName lastUpdated -NotePropertyValue $nowIso -Force
                                    }
                                    $ctJson = $ctData2 | ConvertTo-Json -Depth 10
                                    $ctTmp = "$registry.trunc.tmp.$PID"
                                    [System.IO.File]::WriteAllText($ctTmp, $ctJson, $utf8NoBomCt)
                                    Move-Item -Path $ctTmp -Destination $registry -Force
                                    $closedTruncated = $didTruncate
                                }
                            }
                        } catch {
                            Remove-Item "$registry.trunc.tmp.$PID" -Force -ErrorAction SilentlyContinue
                        } finally {
                            Remove-Item $ctLock -Force -ErrorAction SilentlyContinue
                        }
                    }
                }
            }
        } catch {
            # Graceful absence — size audit is best-effort
        }
    }

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
        # CSI-16: the cache now lives in per-session files under
        # .claude/session-state/sessions/; sharedRepos is workdir-derived (not
        # identity), so ANY session's cache is valid — take the newest. Legacy
        # shared this-session.json remains the fallback for unmigrated trees.
        $stateFile = ""
        $perSessionDir = ".claude/session-state/sessions"
        if (Test-Path $perSessionDir) {
            $srCand = Get-ChildItem -Path $perSessionDir -Filter '*.json' -File -ErrorAction SilentlyContinue |
                Sort-Object LastWriteTime -Descending |
                Where-Object { (Get-Content $_.FullName -Raw -ErrorAction SilentlyContinue) -match '"sharedRepos"' } |
                Select-Object -First 1
            if ($srCand) { $stateFile = $srCand.FullName }
        }
        if (-not $stateFile) { $stateFile = ".claude/session-state/this-session.json" }
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
                # Identity-aware (SWEEP-10 / DEFER-088): same verdict as the local sweep.
                if (Test-PidAliveAs -ProcId $pidVal -RecordedAt ([string]$entry.claudeShellPidWrittenAt)) { continue }

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
                        # Re-check liveness before actually expiring (identity-aware)
                        $pv = 0
                        try { $pv = [int]$orig.claudeShellPid } catch { $pv = 0 }
                        if ($pv -gt 0 -and (Test-PidAliveAs -ProcId $pv -RecordedAt ([string]$orig.claudeShellPidWrittenAt))) {
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
        if ($closedTruncated -gt 0) { $parts += "`"closedTruncated`":$closedTruncated" }
        Write-Output ("{" + ($parts -join ",") + "}")
    }
    exit 0
}

# =============================================================================
# Default mode: briefing path
# =============================================================================

if ($sessions.Count -eq 0) { Emit-Empty }

# ---- Step 0: resolve the calling session's own id (DISC-CSI-22) ----
# Precedence: ULDF_SELF_SESSION_ID (hook-set, authoritative) > CLAUDE_SESSION_ID
# (spawned-session env). Empty => no exclusion.
$selfSessionId = ""
if ($env:ULDF_SELF_SESSION_ID) { $selfSessionId = [string]$env:ULDF_SELF_SESSION_ID }
elseif ($env:CLAUDE_SESSION_ID) { $selfSessionId = [string]$env:CLAUDE_SESSION_ID }

# ---- Filter + liveness check ----
$peers = @()
foreach ($s in $sessions) {
    if (-not $s) { continue }
    if ($s.status -ne "active") { continue }
    if (-not $s.dispatchable) { continue }
    if ($null -eq $s.claudeShellPid) { continue }
    if ($selfSessionId -and ([string]$s.id -eq $selfSessionId)) { continue }

    $pidVal = 0
    try { $pidVal = [int]$s.claudeShellPid } catch { continue }
    if ($pidVal -le 0) { continue }

    # SWEEP-10 (DEC-252): identity-aware, so a pid the OS has RECYCLED onto an
    # unrelated process cannot keep a dead session visible as a live peer. The
    # anchor is claudeShellPidWrittenAt (stamped WITH the pid), never spawnedAt.
    # TWIN-01: the bash twin does exactly this at its Step 2.
    if (-not (Test-PidAliveAs -ProcId $pidVal -RecordedAt ([string]$s.claudeShellPidWrittenAt))) { continue }

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
    # SACT-06 (DEC-240): additively include intent + unexpired resourceClaim.
    if ($s.PSObject.Properties.Name -contains 'intent' -and $s.intent) {
        $peerObj.intent = [string]$s.intent
    }
    if ($s.PSObject.Properties.Name -contains 'resourceClaim' -and $null -ne $s.resourceClaim) {
        $nowIso = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        $bu = ''
        if ($s.resourceClaim.PSObject.Properties.Name -contains 'boundUntil' -and $null -ne $s.resourceClaim.boundUntil) { $bu = [string]$s.resourceClaim.boundUntil }
        if ((-not $bu) -or ($bu -ge $nowIso)) {
            $peerObj.resourceClaim = $s.resourceClaim
        }
    }
    $peers += $peerObj
}

if ($peers.Count -eq 0) { Emit-Empty }

# ---- Build briefing line ----
# SACT-06: the label answers WHO AND WHAT — intent appended when present
# (truncated at 40); a fixture without intents renders the v1 label unchanged.
$labels = $peers | ForEach-Object {
    if ($_.Contains('intent') -and $_.intent) {
        $i = [string]$_.intent
        if ($i.Length -gt 40) { $i = $i.Substring(0, 40) + "..." }
        "$($_.sessionId) ($($_.sessionRole): $i)"
    } else {
        "$($_.sessionId) ($($_.sessionRole))"
    }
}
$briefing = "$($peers.Count) live sibling(s): " + ($labels -join ", ")
$claimFrags = @($peers | Where-Object { $_.Contains('resourceClaim') } | ForEach-Object {
    $res = ''; $kind = ''
    if ($_.resourceClaim.PSObject.Properties.Name -contains 'resource') { $res = [string]$_.resourceClaim.resource }
    if ($_.resourceClaim.PSObject.Properties.Name -contains 'kind') { $kind = [string]$_.resourceClaim.kind }
    "$($_.sessionId):$res($kind)"
})
if ($claimFrags.Count -gt 0) {
    $briefing += " | resource claims: " + ($claimFrags -join ", ")
}

# ---- Emit (compressed JSON, single line) ----
$result = [ordered]@{
    count    = $peers.Count
    peers    = @($peers)
    briefing = $briefing
}

$result | ConvertTo-Json -Compress -Depth 5
