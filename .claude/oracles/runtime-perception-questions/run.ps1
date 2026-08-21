# runtime-perception-questions oracle (Windows) -- ARIA-09 (Leg-D substrate)
#
# Answers: "in this session, did the agent ask a human to run-and-report a
# runtime-perception question that an AOR surface should have answered itself?"
# (the Human-as-I/O-Relay anti-pattern).
#
# Reads the probe-candidate marker log the Leg-C reflex appends to:
#   .claude/session-state/aria-probe-candidates.jsonl  (append-only JSONL)
# Output JSON: { count, questions[], scoped, idSource, briefing }
#   (briefing "" suppresses the line)
# NO-DATA / empty: missing or empty log -> count 0, questions [], briefing "".
# Read-only + idempotent. ASCII-only source (PW-005 lineage).
#
# SESSION SCOPING -- an unlabelled record is UNKNOWN, never MINE (ARIA-25,
# DEC-356; brief DEFER-179). This oracle asserts "candidates logged THIS
# session" and used to measure "null-or-this-session candidates", so any record
# whose writer could not name itself matched EVERY session. Measured on
# SessionHelm's 18-record log, that is every record since 2026-07-03: four
# autopilot finalizes over nine days were handed other sessions' relay events
# labelled as their own, correctly declined to promote them each time, and the
# mechanism promoted nothing. A null record is now OUT of scope whenever this
# session HAS an identity; when it has none, no filter runs and the output says
# so (scoped:false) instead of passing an unscoped population off as scoped.
# The identity comes from lib/aria-session-id.ps1 -- the SAME ladder the writer
# uses -- and is compared as an ALIAS SET, not one string (DEC-337).
# The legacy null backlog is descoped, not deleted: -All (or ARIA_PROBE_SCOPE=all)
# reports the whole log.

$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.Encoding]::UTF8

$log = ".claude/session-state/aria-probe-candidates.jsonl"

$scopeMode = if ($env:ARIA_PROBE_SCOPE) { $env:ARIA_PROBE_SCOPE } else { 'session' }
foreach ($a in $args) { if ("$a" -eq '--all' -or "$a" -eq '-All') { $scopeMode = 'all' } }

# ---- Resolve this session's alias set via the shared ladder ----------------
$idSource = 'none'
$aliases = @()
$libCandidates = @(
    (Join-Path $PSScriptRoot '../../scripts/lib/aria-session-id.ps1'),
    (Join-Path $PSScriptRoot '../../../claude-template/scripts/lib/aria-session-id.ps1'),
    (Join-Path $env:USERPROFILE '.claude/scripts/lib/aria-session-id.ps1')
)
foreach ($cand in $libCandidates) {
    if (Test-Path -LiteralPath $cand) { . $cand; break }
}
if (Get-Command Get-AriaSessionId -ErrorAction SilentlyContinue) {
    $idSource = [string](Get-AriaSessionId).Source
    $aliases = @(Get-AriaSessionIdAliases)
} else {
    # Lib absent: degrade to exactly the pre-fix identity (env only) and SAY SO,
    # rather than inline a second ladder that could drift from the writer's.
    if ($env:CLAUDE_SESSION_ID) {
        $aliases = @([string]$env:CLAUDE_SESSION_ID)
        $idSource = 'env (aria-session-id lib not found)'
    } else {
        $idSource = 'none (aria-session-id lib not found)'
    }
}
if ($scopeMode -eq 'all') { $aliases = @(); $idSource = 'unscoped (--all)' }
$scoped = ($aliases.Count -gt 0)

function Write-Empty {
    $s = if ($scoped) { 'true' } else { 'false' }
    [Console]::Out.Write('{"count":0,"questions":[],"scoped":' + $s +
        ',"idSource":' + ($idSource | ConvertTo-Json -Compress) + ',"briefing":""}')
    exit 0
}

if (-not (Test-Path -LiteralPath $log)) { Write-Empty }
$raw = Get-Content -LiteralPath $log -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
if ([string]::IsNullOrWhiteSpace($raw)) { Write-Empty }

$cats = @('navigation', 'errors', 'async', 'other')
$qs = New-Object System.Collections.ArrayList
$skippedUnlabelled = 0

foreach ($line in ($raw -split "`n")) {
    $t = $line.Trim()
    if ($t -eq '') { continue }
    try { $d = $t | ConvertFrom-Json } catch { continue }
    if ($null -eq $d) { continue }
    # Session scoping (ARIA-25): when this session has an identity, surface ONLY
    # records this session wrote. An unlabelled record is UNKNOWN, not mine --
    # counting it as mine is the DEFER-179 defect.
    if ($scoped) {
        $rid = $null
        if ($d.PSObject.Properties.Name -contains 'sessionId') { $rid = $d.sessionId }
        if ($null -eq $rid -or "$rid" -eq '') { $skippedUnlabelled++; continue }
        if ($aliases -notcontains [string]$rid) { continue }
    }
    $q = $null
    if ($d.PSObject.Properties.Name -contains 'question') { $q = $d.question }
    if (-not ($q -is [string]) -or [string]::IsNullOrEmpty($q)) { continue }
    $cat = 'other'
    if (($d.PSObject.Properties.Name -contains 'category') -and ($cats -contains $d.category)) { $cat = $d.category }
    $rec = [ordered]@{ question = $q; category = $cat; aria_could_answer = $false }
    if ($d.PSObject.Properties.Name -contains 'aria_could_answer') { $rec.aria_could_answer = [bool]$d.aria_could_answer }
    if (($d.PSObject.Properties.Name -contains 'capability') -and ($d.capability -is [string]) -and $d.capability) { $rec.capability = $d.capability }
    if (($d.PSObject.Properties.Name -contains 'suggested_endpoint') -and ($d.suggested_endpoint -is [string]) -and $d.suggested_endpoint) { $rec.suggested_endpoint = $d.suggested_endpoint }
    if ($d.PSObject.Properties.Name -contains 'surface_present') { $rec.surface_present = [bool]$d.surface_present }
    [void]$qs.Add($rec)
}

$count = $qs.Count
if ($count -eq 0) { Write-Empty }

$answerable = 0
foreach ($r in $qs) { if ($r.aria_could_answer) { $answerable++ } }
$briefing = "runtime-perception: $count human-relay probe candidate(s) logged ($answerable AOR-answerable) -- surfaced at /0-uldf-finalize Phase 11.5"
if (-not $scoped) {
    # Never pass an unscoped population off as "this session's" (ARIA-25).
    $briefing += " [WHOLE LOG -- not scoped to this session: $idSource]"
}
if ($briefing.Length -gt 200) { $briefing = $briefing.Substring(0, 197) + '...' }

$out = [ordered]@{ count = $count; questions = @($qs); scoped = $scoped
                   idSource = $idSource; briefing = $briefing }
if ($skippedUnlabelled -gt 0) { $out.skippedUnlabelled = $skippedUnlabelled }
[Console]::Out.Write(($out | ConvertTo-Json -Compress -Depth 6))
exit 0
