# jig-demand --self-test (Windows) -- twin of validate.sh
#
# A PASSING SELF-TEST IS NOT A TRUST VERDICT (OVALID-03): every cell is written
# in the vocabulary of what the oracle MEASURES and is blind to the gap between
# that and what it ASSERTS. Read oracle.json `assertion.known_gaps` first.
#
# The load-bearing pair is the brief's own falsifiability requirement:
#   T2  one capability across N occasions   MUST escalate
#   T3  N distinct capabilities, one each   MUST NOT escalate   <- anti-vacuity
# Without T3, T2 is also passed by a detector that always says "signal".

$ErrorActionPreference = 'Continue'
$pass = 0; $fail = 0
$thisDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$run = Join-Path $thisDir 'run.ps1'
$sb = Join-Path ([System.IO.Path]::GetTempPath()) ("jigd-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))

function Chk([string]$name, $got, $want) {
    if ("$got" -eq "$want") { $script:pass++; Write-Host "  PASS  $name ($got)" }
    else { $script:fail++; Write-Host "  FAIL  $name (expected '$want', got '$got')" }
}
function NewProj([string]$n) {
    $p = Join-Path $sb $n
    New-Item -ItemType Directory -Force -Path (Join-Path $p '.claude/session-state') | Out-Null
    return $p
}
function Add-Cand([string]$proj, [string]$ts, [string]$sid, [string]$q, [string]$cap) {
    $log = Join-Path $proj '.claude/session-state/aria-probe-candidates.jsonl'
    $rec = [ordered]@{ schemaVersion = '1'; ts = $ts }
    if ($sid -eq '-') { $rec['sessionId'] = $null } else { $rec['sessionId'] = $sid }
    $rec['category'] = 'other'; $rec['question'] = $q; $rec['capability'] = $cap
    $rec['aria_could_answer'] = $true; $rec['surface_present'] = $false
    Add-Content -LiteralPath $log -Value ($rec | ConvertTo-Json -Depth 4 -Compress) -Encoding UTF8
}
function RunIn([string]$proj) {
    Push-Location $proj
    try { $j = & $run 2>$null } finally { Pop-Location }
    if ([string]::IsNullOrWhiteSpace($j)) { return $null }
    return ($j | ConvertFrom-Json)
}

Write-Host 'jig-demand self-test (ps1)'

# T1 -- absent log
$p = NewProj 'p1'
$o = RunIn $p
Chk 'T1a absent log -> status ok'      $o.status    'ok'
Chk 'T1b absent log -> empty briefing' $o.briefing  ''
Chk 'T1c absent log -> clustered true' $o.clustered 'True'

# T2 -- LOAD-BEARING: one capability, 4 candidates, 4 distinct sessions -> SIGNAL
$p = NewProj 'p2'
Add-Cand $p '2026-07-01T10:00:00Z' 'sess-a' 'Drive the settings screen and screenshot which panel rendered' 'x'
Add-Cand $p '2026-07-08T10:00:00Z' 'sess-b' 'Drive the settings screen to screenshot the rendered panel state' 'x'
Add-Cand $p '2026-07-15T10:00:00Z' 'sess-c' 'Screenshot the settings screen panel after driving it, rendered state' 'x'
Add-Cand $p '2026-07-22T10:00:00Z' 'sess-d' 'Panel screenshot: drive settings screen, capture what rendered' 'x'
$o = RunIn $p
Chk 'T2a repeated capability -> signal' $o.status          'signal'
Chk 'T2b one cluster reported'          @($o.clusters).Count 1
Chk 'T2c briefing non-empty'            ([string]::IsNullOrWhiteSpace($o.briefing)) 'False'

# T3 -- ANTI-VACUITY CONTROL: 6 distinct capabilities, one occasion each -> ok
# Every record shares the log's house vocabulary on purpose -- see validate.sh
# T3's comment: with token-DISJOINT questions the cell stayed green under the
# "merge everything" mutation, because single-link never considers a pair with
# an empty intersection. Shared incidental vocabulary is what makes it a control.
$p = NewProj 'p3'
Add-Cand $p '2026-07-01T10:00:00Z' 's1' 'Verify audio-video sync drift in the running app without a human' 'probe:avsync'
Add-Cand $p '2026-07-02T10:00:00Z' 's2' 'Verify the exported archive uses fragmented atoms without a human' 'probe:mp4'
Add-Cand $p '2026-07-03T10:00:00Z' 's3' 'Verify database migration history in the running app without a human' 'state-dump:migrations'
Add-Cand $p '2026-07-04T10:00:00Z' 's4' 'Verify a one-time passcode arrives in the mail inbox without a human' 'companion:mail'
Add-Cand $p '2026-07-05T10:00:00Z' 's5' 'Verify which localisation strings lack translations without a human' 'oracle:i18n'
Add-Cand $p '2026-07-06T10:00:00Z' 's6' 'Verify the sandbox resets between benchmark iterations without a human' 'environment-resetter'
$o = RunIn $p
Chk 'T3a distinct singletons -> ok'   $o.status            'ok'
Chk 'T3b no clusters'                 @($o.clusters).Count 0
Chk 'T3c empty briefing'              $o.briefing          ''

# T4 -- THE DISCRIMINATOR: 4 similar candidates, ALL from ONE session -> withheld
$p = NewProj 'p4'
Add-Cand $p '2026-07-01T10:00:00Z' 'only-one' 'Drive the settings screen and screenshot which panel rendered' 'x'
Add-Cand $p '2026-07-01T10:05:00Z' 'only-one' 'Drive the settings screen to screenshot the rendered panel state' 'x'
Add-Cand $p '2026-07-01T10:10:00Z' 'only-one' 'Screenshot the settings screen panel after driving it, rendered state' 'x'
Add-Cand $p '2026-07-01T10:15:00Z' 'only-one' 'Panel screenshot: drive settings screen, capture what rendered' 'x'
$o = RunIn $p
Chk 'T4a one-session enumeration -> ok' $o.status            'ok'
Chk 'T4b withheld, no clusters'         @($o.clusters).Count 0

# T5 -- DRAIN: dispositioning silences it (the conversion side terminating)
$p = NewProj 'p5'
Add-Cand $p '2026-07-01T10:00:00Z' 'sess-a' 'Drive the settings screen and screenshot which panel rendered' 'x'
Add-Cand $p '2026-07-08T10:00:00Z' 'sess-b' 'Drive the settings screen to screenshot the rendered panel state' 'x'
Add-Cand $p '2026-07-15T10:00:00Z' 'sess-c' 'Screenshot the settings screen panel after driving it, rendered state' 'x'
Add-Cand $p '2026-07-22T10:00:00Z' 'sess-d' 'Panel screenshot: drive settings screen, capture what rendered' 'x'
$o = RunIn $p
Chk 'T5a pre-drain -> signal' $o.status 'signal'
$triage = Join-Path $thisDir '../../scripts/aria/jig-demand-triage.ps1'
if (Test-Path -LiteralPath $triage) {
    $key = @($o.clusters)[0].clusterKey
    Push-Location $p
    try { & $triage -Cluster $key -Verdict built -Reason 'self-test drain' 2>$null | Out-Null } finally { Pop-Location }
    $o = RunIn $p
    Chk 'T5b post-drain -> ok'              $o.status                    'ok'
    Chk 'T5c post-drain -> 4 dispositioned' $o.totals.dispositioned      4
    Chk 'T5d post-drain -> 0 undispositioned' $o.totals.undispositioned  0
} else {
    Write-Host '  SKIP  T5b-d (jig-demand-triage.ps1 not found beside this oracle -- not silently passed)'
}

# T7 -- null sessionId falls back to the ts DATE as the occasion key
$p = NewProj 'p7'
Add-Cand $p '2026-07-01T10:00:00Z' '-' 'Drive the settings screen and screenshot which panel rendered' 'x'
Add-Cand $p '2026-07-02T10:00:00Z' '-' 'Drive the settings screen to screenshot the rendered panel state' 'x'
Add-Cand $p '2026-07-03T10:00:00Z' '-' 'Screenshot the settings screen panel after driving it, rendered state' 'x'
$o = RunIn $p
Chk 'T7a null ids, 3 distinct DAYS -> signal' $o.status 'signal'
$p = NewProj 'p7b'
Add-Cand $p '2026-07-01T10:00:00Z' '-' 'Drive the settings screen and screenshot which panel rendered' 'x'
Add-Cand $p '2026-07-01T11:00:00Z' '-' 'Drive the settings screen to screenshot the rendered panel state' 'x'
Add-Cand $p '2026-07-01T12:00:00Z' '-' 'Screenshot the settings screen panel after driving it, rendered state' 'x'
$o = RunIn $p
Chk 'T7b null ids, ONE day -> ok (conservative)' $o.status 'ok'

# T9 -- THE REJECTED DESIGN, pinned in executable form. The brief's leading
# remedy was capability-KEYED dedup. Measured over 223 real records in 12
# projects it yields 3 duplicate groups (1.3%): free-text asks never repeat
# verbatim. Here one ask arrives in three vocabularies, as the real corpus does
# it. An exact-key implementation passes every other cell and reds only here.
# THE DISTRACTORS ARE LOAD-BEARING -- see validate.sh T9's comment: IDF weighting
# is corpus-size dependent, and the first version of this cell (the trio alone)
# failed against the shipped code because in a 3-record corpus every shared term
# is in all 3 documents and its weight collapses. The fixture was wrong, not the
# instrument; the corpus-size floor is a declared known_gap, not a tuning target.
$p = NewProj 'p9'
Add-Cand $p '2026-07-01T10:00:00Z' 's1' 'Verify the settings screen renders the right panel after driving it' 'introspect:ui-snapshot'
Add-Cand $p '2026-07-09T10:00:00Z' 's2' 'Screenshot the settings screen to confirm the panel rendered correctly' 'state-dump:screenshot'
Add-Cand $p '2026-07-19T10:00:00Z' 's3' 'Drive settings, screenshot the panel, confirm what rendered' 'ui-driver jig'
Add-Cand $p '2026-07-20T10:00:00Z' 'd1' 'Measure audio-video sync drift in a finished recording' 'probe:avsync'
Add-Cand $p '2026-07-21T10:00:00Z' 'd2' 'Confirm the exported archive uses fragmented container atoms' 'probe:mp4'
Add-Cand $p '2026-07-22T10:00:00Z' 'd3' 'Read database migration history from the running server' 'state-dump:migrations'
Add-Cand $p '2026-07-23T10:00:00Z' 'd4' 'Fetch a one-time passcode from the mail inbox' 'companion:mail'
Add-Cand $p '2026-07-24T10:00:00Z' 'd5' 'Enumerate which localisation strings lack translations' 'oracle:i18n'
Add-Cand $p '2026-07-25T10:00:00Z' 'd6' 'Reset the sandbox environment between benchmark iterations' 'environment-resetter'
Add-Cand $p '2026-07-26T10:00:00Z' 'd7' 'Assert the retry backoff schedule matches the documented curve' 'probe:backoff'
Add-Cand $p '2026-07-27T10:00:00Z' 'd8' 'Report which feature flags are enabled in the deployed build' 'state-dump:flags'
$o = RunIn $p
Chk 'T9a same ask, THREE vocabularies -> signal'      $o.status            'signal'
Chk 'T9b exactly ONE cluster (8 distractors withheld)' @($o.clusters).Count 1

# T10 -- sessionId IS consulted, not merely correlated with the date. Three
# distinct sessions on the SAME calendar day must still escalate. See
# validate.sh T10: T4 could not tell the two occasion keys apart.
$p = NewProj 'p10'
Add-Cand $p '2026-07-01T09:00:00Z' 'alpha' 'Drive the settings screen and screenshot which panel rendered' 'x'
Add-Cand $p '2026-07-01T13:00:00Z' 'beta'  'Drive the settings screen to screenshot the rendered panel state' 'x'
Add-Cand $p '2026-07-01T18:00:00Z' 'gamma' 'Screenshot the settings screen panel after driving it, rendered state' 'x'
$o = RunIn $p
Chk 'T10a 3 sessions, ONE day -> signal' $o.status 'signal'

Remove-Item -Recurse -Force -LiteralPath $sb -ErrorAction SilentlyContinue

Write-Host ''
Write-Host "jig-demand self-test (ps1): $pass passed, $fail failed"
if ($fail -gt 0) { exit 1 }
exit 0
