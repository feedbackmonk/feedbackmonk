# runtime-perception-questions oracle (Windows) -- ARIA-09 (Leg-D substrate)
#
# Answers: "in this session, did the agent ask a human to run-and-report a
# runtime-perception question that an AOR surface should have answered itself?"
# (the Human-as-I/O-Relay anti-pattern).
#
# Reads the probe-candidate marker log the Leg-C reflex appends to:
#   .claude/session-state/aria-probe-candidates.jsonl  (append-only JSONL)
# Output JSON: { count, questions[], briefing }  (briefing "" suppresses the line)
# NO-DATA / empty: missing or empty log -> count 0, questions [], briefing "".
# Read-only + idempotent. ASCII-only source (PW-005 lineage).

$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.Encoding]::UTF8

$log = ".claude/session-state/aria-probe-candidates.jsonl"
$sess = $env:CLAUDE_SESSION_ID

function Write-Empty {
    [Console]::Out.Write('{"count":0,"questions":[],"briefing":""}')
    exit 0
}

if (-not (Test-Path -LiteralPath $log)) { Write-Empty }
$raw = Get-Content -LiteralPath $log -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
if ([string]::IsNullOrWhiteSpace($raw)) { Write-Empty }

$cats = @('navigation', 'errors', 'async', 'other')
$qs = New-Object System.Collections.ArrayList

foreach ($line in ($raw -split "`n")) {
    $t = $line.Trim()
    if ($t -eq '') { continue }
    try { $d = $t | ConvertFrom-Json } catch { continue }
    if ($null -eq $d) { continue }
    # Session scoping: when this session's id is known and the record carries a
    # non-null sessionId, surface only this session's rows.
    if ($sess) {
        $rid = $null
        if ($d.PSObject.Properties.Name -contains 'sessionId') { $rid = $d.sessionId }
        if ($rid -and ($rid -ne $sess)) { continue }
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
if ($briefing.Length -gt 200) { $briefing = $briefing.Substring(0, 197) + '...' }

$out = [ordered]@{ count = $count; questions = @($qs); briefing = $briefing }
[Console]::Out.Write(($out | ConvertTo-Json -Compress -Depth 6))
exit 0
