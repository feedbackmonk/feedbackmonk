# pods-state oracle (Windows) -- PowerShell mirror of run.sh.
#
# Scrutiny 05 ADD-3: "is a PODS session active, who is on the roster, what is
# each worker's status, any open alerts/messages-to-LEAD/decisions?" in one
# deterministic call. Output is byte-parity JSON with run.sh (frozen schema in
# README.md). READ-ONLY; gracefully absent -> {"active": false, ...}.
#
# PARSING PARITY CONTRACT: behavior-locked to monitor-pods parse conventions
# (heading '## CLAUDE-X | Role', tolerant '**Status**:'/'**Status:**' forms,
# upper-cased values, completion synonyms COMPLETE|COMPLETED|DONE|FINISHED,
# spawned = shell.pid OR status != PENDING, To: LEAD/ALL matching). A change
# to the monitor's parser changes this oracle in the same commit.
#
# NOTE: ASCII-only string literals (template convention).

$ErrorActionPreference = 'SilentlyContinue'

$registry = '.claude/collaboration/active-sessions.json'
$now = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')

function Esc([string]$s) {
    if ($null -eq $s) { return '' }
    return ($s -replace '\\', '\\' -replace '"', '\"')
}

function Emit-Inactive {
    Write-Output ('{"active":false,"podsSession":null,"sessionDir":null,"agents":[],"counts":{"total":0,"complete":0,"blocked":0,"unspawned":0},"allComplete":false,"open":{"alerts":[],"messagesToLead":[],"decisions":[]},"monitor":{"pidFileExists":false,"pid":null,"live":false},"generatedAt":"' + $now + '"}')
    exit 0
}

# ---- podsSession id extraction (DEFER-096) ----------------------------------
# Algorithm-identical to run.sh's awk extractor (TWIN-01); see that file for the
# full rationale. Summary: the CANONICAL shape is the STRING
# `"podsSession": "collab-..."`; the DICT shape `{"sessionId":...,"agents":[...]}`
# is tolerated ADDITIVELY and never narrowed away, because machine-local
# registries on other machines keep the dict form indefinitely. The id is read
# ONLY from inside the podsSession value -- the brace walk (which skips string
# literals so a brace in a workDir or role cannot unbalance it) is what stops a
# sessions[] entry's `sessionId`/`siblingGroup`/`podsWindow` from being reported
# as this project's active collab (validate.{sh,ps1} cell 7c). Preference
# `sessionId` then `id`, the alias pair csi_arc_conclude_eligibility accepts,
# with `id` taken only from before any `agents` key so an agent's own id cannot
# win. No JSON-parser dependency, matching the sh twin.
function Get-PodsSessionId([string]$raw) {
    if ([string]::IsNullOrEmpty($raw)) { return '' }
    $key = '"podsSession"'
    $pos = 0
    $rest = $null
    while ($true) {
        $n = $raw.IndexOf($key, $pos)
        if ($n -lt 0) { return '' }
        $cand = $raw.Substring($n + $key.Length)
        # A value string may legitimately contain the bare word podsSession --
        # require the KEY form (followed by a colon).
        if ([regex]::IsMatch($cand, '^[ \t\r\n]*:')) { $rest = $cand; break }
        $pos = $n + $key.Length
    }
    $rest = [regex]::Replace($rest, '^[ \t\r\n]*:[ \t\r\n]*', '')
    if ($rest.Length -eq 0) { return '' }
    $first = $rest[0]

    # --- canonical: string form ---
    if ($first -eq '"') {
        $m = [regex]::Match($rest, '^"([^"]*)"')
        if ($m.Success) { return $m.Groups[1].Value }
        return ''
    }
    # null / number / array -> no id (inactive), same as before.
    if ($first -ne '{') { return '' }

    # --- legacy: dict form. Isolate the object by brace depth. ---
    $L = $rest.Length; $depth = 0; $i = 0
    while ($i -lt $L) {
        $ch = $rest[$i]
        if ($ch -eq '"') {                    # skip a string literal wholesale
            $j = $i + 1
            while ($j -lt $L) {
                $cj = $rest[$j]
                if ($cj -eq '\') { $j = $j + 2; continue }
                if ($cj -eq '"') { break }
                $j = $j + 1
            }
            $i = $j + 1
            continue
        }
        if ($ch -eq '{') { $depth = $depth + 1 }
        elseif ($ch -eq '}') {
            $depth = $depth - 1
            if ($depth -eq 0) { $i = $i + 1; break }
        }
        $i = $i + 1
    }
    if ($i -gt $L) { $i = $L }
    $obj = $rest.Substring(0, $i)

    $m = [regex]::Match($obj, '"sessionId"[ \t\r\n]*:[ \t\r\n]*"([^"]*)"')
    if ($m.Success) { return $m.Groups[1].Value }
    $head = $obj
    $a = $obj.IndexOf('"agents"')
    if ($a -ge 0) { $head = $obj.Substring(0, $a) }
    $m = [regex]::Match($head, '"id"[ \t\r\n]*:[ \t\r\n]*"([^"]*)"')
    if ($m.Success) { return $m.Groups[1].Value }
    return ''
}

if (-not (Test-Path $registry)) { Emit-Inactive }
$podsId = ''
try {
    $regRaw = [System.IO.File]::ReadAllText($registry)
    $podsId = Get-PodsSessionId $regRaw
} catch { }
if (-not $podsId) { Emit-Inactive }

$sessionDir = ".claude/collaboration/$podsId"
if (-not (Test-Path $sessionDir -PathType Container)) { Emit-Inactive }
$statusFile = "$sessionDir/channels/status.md"
$workersDir = "$sessionDir/workers"

# ---- Per-agent parse (monitor-pods parse_snapshot parity + Progress) --------
$agents = New-Object System.Collections.ArrayList
if (Test-Path $statusFile) {
    $cur = $null
    $lines = @()
    try { $lines = [System.IO.File]::ReadAllLines($statusFile) } catch { $lines = @() }
    function Flush-Agent {
        param($a)
        if ($a) { [void]$script:agentsRef.Add($a) }
    }
    $script:agentsRef = $agents
    foreach ($ln in $lines) {
        # LEAD FIRST, AND THIS ORDERING IS NOW LOAD-BEARING (it was not before).
        # The widened id rule below matches '## LEAD | Coordinator', which the old
        # CLAUDE--anchored rule could not -- so the LEAD exclusion has to run first
        # or the coordinator joins the completable worker set and allComplete is
        # false forever. Same rule, same position, as monitor-pods (DISC-MON-09).
        if ($ln -match '^## LEAD([ \t]|\|)') { Flush-Agent $cur; $cur = $null; continue }
        # DISC-MON-09/DISC-MON-10 PARITY (DEC-306). This used to be
        # '^## (CLAUDE-[A-Z0-9_-]+) \|' while monitor-pods had already been widened
        # to any bounded id token -- so this oracle was BLIND to every PODS session
        # whose roster is not CLAUDE-*, which is every wave-6..9 roster. Measured
        # 2026-08-06 with a control ('## W9-B | r' -> no agents; '## CLAUDE-A | r'
        # -> one agent). This file's own README says the two parsers are
        # "behavior-locked ... and must not drift". They had. Keep them identical.
        if ($ln -match '^## ([A-Za-z][A-Za-z0-9_-]*) \|') {
            Flush-Agent $cur
            $id = $Matches[1]
            $role = ($ln -replace '^## [A-Za-z][A-Za-z0-9_-]* \| ', '').TrimEnd()
            $spawned = Test-Path (Join-Path $workersDir "$id/shell.pid")
            $cur = @{ id = $id; role = $role; status = ''; progress = ''; spawned = [bool]$spawned }
            continue
        }
        if ($cur -and $ln -match '^\*\*Status(\*\*:|:\*\*)') {
            $s = $ln -replace '.*\*\*Status(\*\*:|:\*\*)[ \t]*', ''
            $s = ($s -split '[ \t\r]')[0]
            $cur.status = $s.ToUpperInvariant()
            if ($cur.status -ne 'PENDING') { $cur.spawned = $true }
            continue
        }
        if ($cur -and $ln -match '^\*\*Progress(\*\*:|:\*\*)') {
            $s = $ln -replace '.*\*\*Progress(\*\*:|:\*\*)[ \t]*', ''
            $cur.progress = $s.TrimEnd()
            continue
        }
    }
    Flush-Agent $cur
}

# ---- Channel scan (monitor-pods scan_channel_file parity) --------------------
function Scan-Channel {
    param([string]$File, [string]$Prefix, [string]$Fire, [bool]$NeedTo)
    $out = New-Object System.Collections.ArrayList
    if (-not (Test-Path $File)) { return $out }
    $id = ''; $to = ''; $st = ''
    $flush = {
        if ($id) {
            if ($st.ToUpperInvariant() -eq $Fire) {
                $tu = $to.ToUpperInvariant()
                if ((-not $NeedTo) -or ($tu -match 'LEAD') -or ($tu -match '(^|[^A-Z])ALL([^A-Z]|$)')) {
                    [void]$out.Add($id)
                }
            }
        }
    }
    $lines = @()
    try { $lines = [System.IO.File]::ReadAllLines($File) } catch { return $out }
    foreach ($ln in $lines) {
        if ($ln -match ('^## (' + $Prefix + '-[A-Za-z0-9_-]+):')) {
            & $flush
            $id = $Matches[1]; $to = ''; $st = ''
            continue
        }
        if ($ln -match '^## ') { & $flush; $id = ''; continue }
        if ($id -and $ln -match '^\*\*To(\*\*:|:\*\*)') {
            $to = ($ln -replace '.*\*\*To(\*\*:|:\*\*)[ \t]*', '').TrimEnd()
            continue
        }
        if ($id -and $ln -match '^\*\*Status(\*\*:|:\*\*)') {
            $s = $ln -replace '.*\*\*Status(\*\*:|:\*\*)[ \t]*', ''
            $st = ($s -split '[ \t\r]')[0]
            continue
        }
    }
    & $flush
    return $out
}

function Ids-ToJson {
    param($Ids)
    $parts = @()
    foreach ($i in @($Ids)) { if ($i) { $parts += ('"' + (Esc $i) + '"') } }
    return ($parts -join ',')
}

# ---- Aggregate ---------------------------------------------------------------
$total = 0; $complete = 0; $blocked = 0; $unspawned = 0
$agentParts = @()
foreach ($a in $agents) {
    $total++
    $isComplete = $a.status -in @('COMPLETE', 'COMPLETED', 'DONE', 'FINISHED')
    if ($isComplete) { $complete++ }
    if ($a.status -eq 'BLOCKED') { $blocked++ }
    if (-not $a.spawned) { $unspawned++ }
    $stJson = if ($a.status) { '"' + (Esc $a.status) + '"' } else { 'null' }
    $pgJson = if ($a.progress) { '"' + (Esc $a.progress) + '"' } else { 'null' }
    $icJson = if ($isComplete) { 'true' } else { 'false' }
    $spJson = if ($a.spawned) { 'true' } else { 'false' }
    $agentParts += ('{"id":"' + (Esc $a.id) + '","role":"' + (Esc $a.role) + '","status":' + $stJson + ',"progress":' + $pgJson + ',"isComplete":' + $icJson + ',"spawned":' + $spJson + '}')
}
$allComplete = if ($total -gt 0 -and $complete -eq $total) { 'true' } else { 'false' }

$alertsJson = Ids-ToJson (Scan-Channel "$sessionDir/channels/alerts.md"    'ALERT' 'ACTIVE' $false)
$msgsJson   = Ids-ToJson (Scan-Channel "$sessionDir/channels/messages.md"  'MSG'   'OPEN'   $true)
$decsJson   = Ids-ToJson (Scan-Channel "$sessionDir/channels/decisions.md" 'DEC'   'OPEN'   $false)

# ---- Monitor visibility (DEFER-005): <session>/monitor.pid singleton record --
# live is best-effort process-existence (advisory; the authoritative duplicate
# defense is the monitor's own startup singleton check).
$monPidFile = "$sessionDir/monitor.pid"
$monExists = 'false'; $monPid = 'null'; $monLive = 'false'
if (Test-Path $monPidFile) {
    $monExists = 'true'
    $mpRaw = ''
    try { $mpRaw = ([System.IO.File]::ReadAllText($monPidFile)).Trim() } catch { }
    $mpVal = 0
    if ($mpRaw -match '^[0-9]+$' -and [int]::TryParse($mpRaw, [ref]$mpVal) -and $mpVal -gt 0) {
        $monPid = "$mpVal"
        if (Get-Process -Id $mpVal -ErrorAction SilentlyContinue) { $monLive = 'true' }
    }
}

Write-Output ('{"active":true,"podsSession":"' + (Esc $podsId) + '","sessionDir":"' + (Esc $sessionDir) + '","agents":[' + ($agentParts -join ',') + '],"counts":{"total":' + $total + ',"complete":' + $complete + ',"blocked":' + $blocked + ',"unspawned":' + $unspawned + '},"allComplete":' + $allComplete + ',"open":{"alerts":[' + $alertsJson + '],"messagesToLead":[' + $msgsJson + '],"decisions":[' + $decsJson + ']},"monitor":{"pidFileExists":' + $monExists + ',"pid":' + $monPid + ',"live":' + $monLive + '},"generatedAt":"' + $now + '"}')
exit 0
