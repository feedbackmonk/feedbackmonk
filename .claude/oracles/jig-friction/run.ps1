# jig-friction oracle (Windows) -- JIG-04, DEC-142. Parity with run.sh.
#
# The "manufactured boredom" signal: a deterministic detector over the EXISTING
# command-usage telemetry that spots GRINDING (N structurally-similar named
# invocations in one session) and surfaces each cluster as an advisory,
# evidence-carrying briefing line. ADVISORY ONLY -- never blocks. See run.sh
# for the full doctrine header (substrate, scope resolution, threshold bias).
#
# Output: single-line JSON, the FROZEN contract (GUIDE 6.1):
#   {"status":"ok|signal","signals":[{"pattern","count","evidence","suggestedArchetype"}],"briefing"}
#   INVARIANT: status:"ok" => briefing == "" (quiet path; empty briefing is
#   suppressed by the session-start fan-out).
# Graceful absence: missing registry / any internal failure -> quiet JSON, exit 0.
# PW-005: explicit UTF-8 output; ASCII-only strings in this file.

$ErrorActionPreference = 'SilentlyContinue'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }

$Dir = ''; $Session = ''; $MinRepeats = ''; $WindowMinutes = ''
for ($i = 0; $i -lt $args.Count; $i++) {
    $a = [string]$args[$i]
    switch -Regex ($a) {
        '^--dir=(.*)'             { $Dir = $Matches[1]; break }
        '^--dir$'                 { $i++; $Dir = [string]$args[$i]; break }
        '^--session=(.*)'         { $Session = $Matches[1]; break }
        '^--session$'             { $i++; $Session = [string]$args[$i]; break }
        '^--min-repeats=(.*)'     { $MinRepeats = $Matches[1]; break }
        '^--min-repeats$'         { $i++; $MinRepeats = [string]$args[$i]; break }
        '^--window-minutes=(.*)'  { $WindowMinutes = $Matches[1]; break }
        '^--window-minutes$'      { $i++; $WindowMinutes = [string]$args[$i]; break }
        default { }
    }
}

function Emit-Quiet {
    [Console]::Out.WriteLine('{"status":"ok","signals":[],"briefing":""}')
    exit 0
}

function Json-Str([string]$s) {
    # Encode a single string as a JSON scalar (quoted, escaped).
    if ($null -eq $s) { $s = '' }
    return ($s | ConvertTo-Json -Compress)
}

# ConvertFrom-Json (PS 7+) coerces ISO-8601 `at` strings to [datetime]; PS 5.1
# leaves them as [string]. Normalize BOTH forms so timestamps stay the raw ISO
# string (bash parity: evidence carries the verbatim `at`) and window math works.
# ZONE (DEC-302 / LEDGER-ZONE-02) -- this oracle measures its window RELATIVE to
# the ledger's own max row, which is why DEFER-139 called it the only zone-correct
# consumer. That holds only over a UNIFORMLY-zoned ledger: from DEC-302 `at` is
# UTC-with-`Z` and older rows are BARE (legacy LOCAL), so one file carries both
# across the transition and a ledger-relative window would span a full offset at
# the boundary. Get-AtDate resolves BOTH forms to one UTC instant.
# PER-FILE COPY ON PURPOSE (DEC-279/DEC-251). InvariantCulture on every parse and
# format: a custom format with no provider uses the CURRENT CULTURE'S CALENDAR
# (DEFER-122), which round-trips and is therefore silently wrong.
$jfInv = [System.Globalization.CultureInfo]::InvariantCulture

# Verbatim-ish evidence string: preserves the row's own marker (bash parity --
# evidence carries the `at` as the file spells it).
function Get-AtStr($r) {
    if ($r.at -is [datetime]) {
        if ($r.at.Kind -eq [System.DateTimeKind]::Utc) { return $r.at.ToString("yyyy-MM-dd\THH:mm:ss\Z", $jfInv) }
        return $r.at.ToString('yyyy-MM-dd\THH:mm:ss', $jfInv)
    }
    return [string]$r.at
}

# ONE comparable instant (UTC) for both zone forms. Throws on unparseable input,
# which the callers below already catch and fail open on.
function Get-AtDate($r) {
    if ($r.at -is [datetime]) {
        # pwsh coerced it at ConvertFrom-Json; Kind records which form the file
        # carried -- Utc for `...Z`, Unspecified for bare (measured, both engines).
        if ($r.at.Kind -eq [System.DateTimeKind]::Utc) { return $r.at }
        if ($r.at.Kind -eq [System.DateTimeKind]::Local) { return $r.at.ToUniversalTime() }
        return [DateTime]::SpecifyKind($r.at, [System.DateTimeKind]::Local).ToUniversalTime()
    }
    $s = [string]$r.at
    if ($s.EndsWith('Z')) {
        return [datetime]::ParseExact($s, "yyyy-MM-dd\THH:mm:ss\Z", $jfInv,
               ([System.Globalization.DateTimeStyles]::AdjustToUniversal -bor `
                [System.Globalization.DateTimeStyles]::AssumeUniversal))
    }
    return [DateTime]::SpecifyKind(
        [datetime]::ParseExact($s, 'yyyy-MM-dd\THH:mm:ss', $jfInv),
        [System.DateTimeKind]::Local).ToUniversalTime()
}

# Sort key that never throws -- an unparseable row sorts oldest rather than
# aborting the whole scope/ordering pass.
function Get-AtSortKey($r) { try { return (Get-AtDate $r) } catch { return [DateTime]::MinValue } }

# ---- config (CLI > .claude/config.json jigFriction.* > default) --------------
if (Test-Path '.claude/config.json') {
    try {
        $cfg = Get-Content '.claude/config.json' -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($MinRepeats -eq '' -and $null -ne $cfg.jigFriction -and $null -ne $cfg.jigFriction.minRepeats) {
            $MinRepeats = [string]$cfg.jigFriction.minRepeats
        }
        if ($WindowMinutes -eq '' -and $null -ne $cfg.jigFriction -and $null -ne $cfg.jigFriction.windowMinutes) {
            $WindowMinutes = [string]$cfg.jigFriction.windowMinutes
        }
    } catch { }
}
if ($MinRepeats -notmatch '^[0-9]+$') { $MinRepeats = '5' }
if ($WindowMinutes -notmatch '^[0-9]+$') { $WindowMinutes = '0' }
$MinRepeats = [int]$MinRepeats
$WindowMinutes = [int]$WindowMinutes

# ---- data dir resolution (live registry primary) ----------------------------
if ($Dir -eq '') {
    $live = Join-Path $HOME '.claude/command-usage'
    if ((Test-Path $live) -and (Get-ChildItem -Path $live -Filter '*.jsonl' -ErrorAction SilentlyContinue)) {
        $Dir = $live
    } elseif ((Test-Path './claude-usage') -and (Get-ChildItem -Path './claude-usage' -Filter '*.jsonl' -ErrorAction SilentlyContinue)) {
        $Dir = './claude-usage'
    }
}
$files = @()
if ($Dir -ne '') { $files = @(Get-ChildItem -Path $Dir -Filter '*.jsonl' -ErrorAction SilentlyContinue) }
if ($files.Count -eq 0) {
    [Console]::Error.WriteLine('jig-friction: NO-DATA -- no *.jsonl registry (looked in --dir / ~/.claude/command-usage / ./claude-usage)')
    Emit-Quiet
}

# ---- read + parse (skip malformed; strip CR) --------------------------------
$recs = New-Object System.Collections.ArrayList
foreach ($f in $files) {
    $lines = Get-Content -Path $f.FullName -Encoding UTF8 -ErrorAction SilentlyContinue
    foreach ($line in $lines) {
        $t = ($line -replace "`r", '').Trim()
        if ($t -eq '') { continue }
        try { $o = $t | ConvertFrom-Json } catch { continue }
        if ($null -eq $o) { continue }
        if ($o -isnot [psobject]) { continue }
        if ($null -eq $o.cmd -or $o.cmd -isnot [string]) { continue }
        if ($null -eq $o.session -or $o.session -isnot [string]) { continue }
        [void]$recs.Add($o)
    }
}
if ($recs.Count -eq 0) { Emit-Quiet }

# ---- scope session (first hit wins) -----------------------------------------
$envSession = [string]$env:CLAUDE_SESSION_ID
if ($Session -ne '') {
    $scope = $Session
} elseif ($envSession -ne '' -and ($recs | Where-Object { $_.session -eq $envSession })) {
    $scope = $envSession
} else {
    # Sort on the canonical instant, not the raw string: a mixed-zone ledger
    # would otherwise pick the scope session by lexical accident (DEC-302).
    $scope = ($recs | Sort-Object { Get-AtSortKey $_ } | Select-Object -Last 1).session
}
$scoped = @($recs | Where-Object { $_.session -eq $scope })

# ---- optional windowMinutes tightening (fail-open on parse error) -----------
if ($WindowMinutes -gt 0 -and $scoped.Count -gt 0) {
    try {
        $times = @()
        foreach ($r in $scoped) { $times += (Get-AtDate $r) }
        $maxT = ($times | Sort-Object | Select-Object -Last 1)
        $cut = $maxT.AddMinutes(-1 * $WindowMinutes)
        $scoped = @($scoped | Where-Object {
            try { (Get-AtDate $_) -ge $cut } catch { $true }
        })
    } catch { }
}

# ---- helpers -----------------------------------------------------------------
function Get-Archetype([string]$c) {
    $l = $c.ToLower()
    if ($l -match 'screenshot|snapshot|navigate|browser|click|_type|playwright|scroll|hover|press_key') { return 'scenario-replayer' }
    elseif ($l -match 'log|grep|tail|distill') { return 'log-distiller' }
    elseif ($l -match 'diff|compare') { return 'diff-summarizer' }
    elseif ($l -match 'harvest|scrape|collect') { return 'corpus-harvester' }
    elseif ($l -match 'state') { return 'state-fabricator' }
    elseif ($l -match 'reset|teardown|provision|setup|restart|clean|env') { return 'environment-resetter' }
    elseif ($l -match 'generat|fabricat|mock|sample|seed|fixture|synth') { return 'fabricator' }
    else { return 'scenario-replayer' }
}
function Get-Sig($r) {
    $type = if ($r.type) { [string]$r.type } else { '?' }
    $arg1 = if ($r.arg1) { [string]$r.arg1 } else { '' }
    $flags = ''
    if ($r.flags) { $flags = (@($r.flags) -join ',') }
    $s = "$type|$($r.cmd)|$arg1|$flags"
    return ($s.ToLower() -replace '[0-9]+', 'N')
}
function Get-Estr($r) {
    $s = "$(Get-AtStr $r) $([string]$r.cmd)"
    if ($r.arg1 -and [string]$r.arg1 -ne '') { $s += " $([string]$r.arg1)" }
    if ($r.flags -and (@($r.flags).Count) -gt 0) { $s += " [$(@($r.flags) -join ',')]" }
    if ($s.Length -gt 200) { $s = $s.Substring(0, 197) + '...' }
    return $s
}

# ---- cluster by shape signature ---------------------------------------------
foreach ($r in $scoped) { $r | Add-Member -NotePropertyName _sig -NotePropertyValue (Get-Sig $r) -Force }
$groups = $scoped | Group-Object _sig | Where-Object { $_.Count -ge $MinRepeats }

$signalObjs = @()
foreach ($g in $groups) {
    $sorted = @($g.Group | Sort-Object { Get-AtSortKey $_ })
    $rep = $sorted[0]
    $pattern = ($rep._sig -replace '"', '') -replace '[|]+$', ''
    $ev = @()
    $take = [Math]::Min(20, $sorted.Count)
    for ($k = 0; $k -lt $take; $k++) { $ev += (Get-Estr $sorted[$k]) }
    if ($sorted.Count -gt 20) { $ev += ('...(+' + ($sorted.Count - 20) + ' more)') }
    $signalObjs += [pscustomobject]@{
        pattern = $pattern
        count   = $g.Count
        evidence = $ev
        suggestedArchetype = (Get-Archetype ([string]$rep.cmd))
    }
}
# sort by count desc
$signalObjs = @($signalObjs | Sort-Object -Property count -Descending)

# ---- assemble output (manual JSON => guaranteed array semantics) -------------
if ($signalObjs.Count -eq 0) {
    [Console]::Out.WriteLine('{"status":"ok","signals":[],"briefing":""}')
    exit 0
}

$sigJsonParts = @()
foreach ($s in $signalObjs) {
    $evParts = @()
    foreach ($e in @($s.evidence)) { $evParts += (Json-Str $e) }
    $evJson = '[' + ($evParts -join ',') + ']'
    $sigJsonParts += ('{"pattern":' + (Json-Str $s.pattern) + ',"count":' + $s.count +
        ',"evidence":' + $evJson + ',"suggestedArchetype":' + (Json-Str $s.suggestedArchetype) + '}')
}
$signalsJson = '[' + ($sigJsonParts -join ',') + ']'

$top = $signalObjs[0]
$brief = ('' + $signalObjs.Count + ' repetition signal(s) this session -- top: ' +
    $top.count + 'x ' + $top.pattern +
    ' (consider a ' + $top.suggestedArchetype + ' jig; see JIG_CATALOG.md)') -replace '"', ''

$out = '{"status":"signal","signals":' + $signalsJson + ',"briefing":' + (Json-Str $brief) + '}'
[Console]::Out.WriteLine($out)
exit 0
