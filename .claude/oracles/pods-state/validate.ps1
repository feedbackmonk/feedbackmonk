# pods-state oracle self-test (Windows) -- PowerShell mirror of validate.sh.
# Sandboxed fixtures; asserts the frozen output schema over: inactive (no
# registry / null podsSession), active mixed-status roster, channel-derived
# open items, and the completion-synonym set. (Cross-shell parity is asserted
# by validate.sh; this file proves run.ps1 standalone on Windows.)
# NOTE: ASCII-only string literals (template convention).

$ErrorActionPreference = 'SilentlyContinue'

$thisDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$runPs = Join-Path $thisDir 'run.ps1'

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pods-state-validate-" + [System.Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $sandbox -Force | Out-Null

$script:pass = 0; $script:fail = 0
function Ok([string]$label)  { Write-Output "PASS: $label"; $script:pass++ }
function Bad([string]$label) { Write-Output "FAIL: $label"; $script:fail++ }

function Invoke-Oracle([string]$dir) {
    Push-Location $dir
    try { return ((& powershell -NoProfile -ExecutionPolicy Bypass -File $runPs) -join "`n") }
    finally { Pop-Location }
}

try {
    # Case 1: no registry -> inactive
    $p1 = Join-Path $sandbox 'p1'; New-Item -ItemType Directory -Path $p1 -Force | Out-Null
    $out = Invoke-Oracle $p1
    if ($out -match '"active":false') { Ok 'no registry -> active:false' } else { Bad "no registry (got: $out)" }

    # Case 2: null podsSession -> inactive
    $p2 = Join-Path $sandbox 'p2/.claude/collaboration'; New-Item -ItemType Directory -Path $p2 -Force | Out-Null
    Set-Content -Path (Join-Path $p2 'active-sessions.json') -Value '{"podsSession":null,"sessions":[]}' -Encoding Ascii
    $out = Invoke-Oracle (Join-Path $sandbox 'p2')
    if ($out -match '"active":false') { Ok 'null podsSession -> active:false' } else { Bad "null podsSession (got: $out)" }

    # Case 3: active session, mixed roster + open items
    $p3 = Join-Path $sandbox 'p3'
    $sid = 'collab-20990101-000000'
    $sd = Join-Path $p3 ".claude/collaboration/$sid"
    New-Item -ItemType Directory -Path (Join-Path $sd 'channels') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $sd 'workers/CLAUDE-A') -Force | Out-Null
    Set-Content -Path (Join-Path $p3 '.claude/collaboration/active-sessions.json') -Value ('{"podsSession":"' + $sid + '","sessions":[]}') -Encoding Ascii
    Set-Content -Path (Join-Path $sd 'workers/CLAUDE-A/shell.pid') -Value '1' -Encoding Ascii

    $statusMd = @(
        '# Status', '',
        '## LEAD | Lead Developer', '**Status**: COORDINATING', '',
        '## CLAUDE-A | Builder', '**Updated**: 2026-07-02 00:00', '**Status**: IN_PROGRESS', '**Progress**: 40%', '',
        '## CLAUDE-B | Reviewer', '**Status:** DONE', '**Progress**: 100%', '',
        '## CLAUDE-C | Blocked One', '**Status**: BLOCKED', '**Progress**: 10%', '',
        '## CLAUDE-D | Deferred', '**Status**: PENDING'
    )
    Set-Content -Path (Join-Path $sd 'channels/status.md') -Value ($statusMd -join "`n") -Encoding Ascii

    Set-Content -Path (Join-Path $sd 'channels/alerts.md') -Value (@(
        '# Alerts', '',
        '## ALERT-001: Live one', '**From**: CLAUDE-A', '**Status**: ACTIVE', '',
        '## ALERT-002: Old one', '**From**: CLAUDE-B', '**Status**: RESOLVED'
    ) -join "`n") -Encoding Ascii

    Set-Content -Path (Join-Path $sd 'channels/messages.md') -Value (@(
        '# Messages', '',
        '## MSG-001: For the LD', '**From**: CLAUDE-A', '**To**: LEAD', '**Status**: OPEN', '',
        '## MSG-002: Peer-to-peer', '**From**: CLAUDE-A', '**To**: CLAUDE-B', '**Status**: OPEN', '',
        '## MSG-003: Done one', '**From**: CLAUDE-B', '**To**: ALL', '**Status**: RESOLVED'
    ) -join "`n") -Encoding Ascii

    Set-Content -Path (Join-Path $sd 'channels/decisions.md') -Value (@(
        '# Decisions', '',
        '## DEC-001: Open call', '**Status**: OPEN', '',
        '## DEC-002: Settled', '**Status**: RESOLVED'
    ) -join "`n") -Encoding Ascii

    $out = Invoke-Oracle $p3
    if ($out -match '"active":true') { Ok 'active:true' } else { Bad "active flag (got: $out)" }
    if ($out -match ('"podsSession":"' + $sid + '"')) { Ok 'podsSession id' } else { Bad 'podsSession id' }
    if ($out -match '"total":4') { Ok 'total=4 (LEAD excluded)' } else { Bad "total (got: $out)" }
    if ($out -match '"complete":1') { Ok 'complete=1 (DONE synonym)' } else { Bad 'complete count' }
    if ($out -match '"blocked":1') { Ok 'blocked=1' } else { Bad 'blocked count' }
    if ($out -match '"unspawned":1') { Ok 'unspawned=1' } else { Bad 'unspawned count' }
    if ($out -match '"allComplete":false') { Ok 'allComplete=false' } else { Bad 'allComplete' }
    if ($out -match '"id":"CLAUDE-B","role":"Reviewer","status":"DONE"') { Ok 'colon-inside-bold Status form parsed' } else { Bad "Status:** form (got: $out)" }
    if ($out -match '"alerts":\["ALERT-001"\]') { Ok 'ACTIVE alert only' } else { Bad "alerts (got: $out)" }
    if ($out -match '"messagesToLead":\["MSG-001"\]') { Ok 'OPEN-to-LEAD msg only' } else { Bad "messagesToLead (got: $out)" }
    if ($out -match '"decisions":\["DEC-001"\]') { Ok 'OPEN decision only' } else { Bad "decisions (got: $out)" }

    # Case 4: completion-synonym set -> allComplete
    $statusAll = ($statusMd -join "`n") -replace '\*\*Status\*\*: IN_PROGRESS', '**Status**: COMPLETE' `
                                        -replace '\*\*Status\*\*: BLOCKED', '**Status**: COMPLETED' `
                                        -replace '\*\*Status\*\*: PENDING', '**Status**: FINISHED'
    Set-Content -Path (Join-Path $sd 'channels/status.md') -Value $statusAll -Encoding Ascii
    $out = Invoke-Oracle $p3
    if ($out -match '"allComplete":true') { Ok 'completion synonyms -> allComplete:true' } else { Bad "allComplete synonyms (got: $out)" }

    # Case 7: dict-form podsSession (DEFER-096) -- mirror of validate.sh Case 7.
    # Every fixture above uses the STRING form, which is why the string-only
    # extractor's blindness to the dict form was invisible for the life of this
    # oracle. Canonical shape is the STRING; the dict is tolerated ADDITIVELY.
    $p7 = Join-Path $sandbox 'p7'
    $sid7 = 'collab-20990202-000000'
    $sd7 = Join-Path $p7 ".claude/collaboration/$sid7"
    New-Item -ItemType Directory -Path (Join-Path $sd7 'channels') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $sd7 'workers/CLAUDE-A') -Force | Out-Null
    Set-Content -Path (Join-Path $sd7 'workers/CLAUDE-A/shell.pid') -Value '1' -Encoding Ascii
    Set-Content -Path (Join-Path $sd7 'channels/status.md') -Value (@(
        '# Status', '',
        '## CLAUDE-A | Builder', '**Status**: IN_PROGRESS', '**Progress**: 50%'
    ) -join "`n") -Encoding Ascii
    $reg7 = Join-Path $p7 '.claude/collaboration/active-sessions.json'

    # 7a: dict form, PRETTY-PRINTED across lines exactly as a live LD-written
    # registry carries it (key and id sit on DIFFERENT lines, so any
    # line-scoped extractor misses it even after the pattern is widened).
    Set-Content -Path $reg7 -Encoding Ascii -Value (@(
        '{',
        '    "sessions":  [],',
        '    "podsSession":  {',
        ('                        "sessionId":  "' + $sid7 + '",'),
        '                        "agents":  [',
        '                                       {',
        '                                           "id":  "CLAUDE-A",',
        '                                           "role":  "Builder"',
        '                                       }',
        '                                   ]',
        '                    },',
        '    "lastUpdated":  ""',
        '}'
    ) -join "`n")
    $out = Invoke-Oracle $p7
    if ($out -match '"active":true') { Ok '7a dict-form podsSession -> active:true' } else { Bad "7a dict-form active (got: $out)" }
    if ($out -match ('"podsSession":"' + $sid7 + '"')) { Ok '7a dict-form sessionId extracted' } else { Bad "7a dict-form id (got: $out)" }

    # 7b: dict form using the `id` alias csi_arc_conclude_eligibility accepts.
    Set-Content -Path $reg7 -Encoding Ascii -Value ('{"sessions":[],"podsSession":{"id":"' + $sid7 + '","agents":[{"id":"CLAUDE-A","role":"Builder"}]}}')
    $out = Invoke-Oracle $p7
    if ($out -match ('"podsSession":"' + $sid7 + '"')) { Ok '7b dict-form id alias extracted' } else { Bad "7b id alias (got: $out)" }

    # 7c: ANTI-VACUITY CONTROL. podsSession is null, but sessions[] is full of
    # decoys a whole-file scan would pick up -- the same sessionId, plus
    # siblingGroup/podsWindow values naming a REAL collab directory. An
    # unbounded matcher reports active:true here, so this cell is what keeps
    # 7a/7b from passing for the wrong reason.
    Set-Content -Path $reg7 -Encoding Ascii -Value ('{"sessions":[{"id":"CLAUDE-A","sessionId":"' + $sid7 + '","siblingGroup":"' + $sid7 + '","podsWindow":"pods-' + $sid7 + '","status":"active"}],"podsSession":null}')
    $out = Invoke-Oracle $p7
    if ($out -match '"active":false') { Ok '7c null podsSession + sessions[] decoys -> still active:false' } else { Bad "7c decoy leaked into podsSession (got: $out)" }

    # 7d: dict form naming a session dir that does not exist -> inactive (the
    # sessionDir guard still governs; tolerance widens extraction, not trust).
    Set-Content -Path $reg7 -Encoding Ascii -Value '{"sessions":[],"podsSession":{"sessionId":"collab-19700101-000000","agents":[]}}'
    $out = Invoke-Oracle $p7
    if ($out -match '"active":false') { Ok '7d dict-form id with no session dir -> active:false' } else { Bad "7d missing session dir (got: $out)" }
} finally {
    Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}

Write-Output ''
Write-Output ("pods-state validate (ps): " + $script:pass + " pass / " + $script:fail + " fail")
if ($script:fail -eq 0) { exit 0 } else { exit 1 }
