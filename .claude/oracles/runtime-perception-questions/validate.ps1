# runtime-perception-questions oracle self-test (Windows) -- ARIA-09
# Builds throwaway sandboxes with a marker log and asserts count / questions /
# briefing / session-scoping / NO-DATA behavior. ASCII-only source (PW-005).
$ErrorActionPreference = 'Stop'
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$run = Join-Path $oracleDir 'run.ps1'
$pass = 0; $fail = 0

function Check([string]$name, [string]$needle, [string]$actual) {
    if ($actual -like "*$needle*") { $script:pass++ }
    else { $script:fail++; Write-Error "FAIL: $name -- expected to contain '$needle'; got: $actual" -ErrorAction Continue }
}
function JsonOk([string]$name, [string]$actual) {
    try { $null = $actual | ConvertFrom-Json; $script:pass++ }
    catch { $script:fail++; Write-Error "FAIL: $name not valid JSON: $actual" -ErrorAction Continue }
}
function New-Sandbox {
    $d = Join-Path ([System.IO.Path]::GetTempPath()) ("rpq-" + [System.Guid]::NewGuid().ToString('N').Substring(0,8))
    New-Item -ItemType Directory -Path (Join-Path $d '.claude/session-state') -Force | Out-Null
    return $d
}
# ARIA-25: scrub EVERY rung, not just CLAUDE_SESSION_ID. The ladder also reads
# CLAUDE_CODE_SESSION_ID and CLAUDE_PID, both of which this validate run inherits
# from a real session -- a cell that forgot to scrub would be graded against a
# rung it never meant to test, and would pass for the wrong reason.
function Invoke-In([string]$dir, [string]$sid, [string]$harnessId, [string]$claudePid, [string[]]$argv) {
    $keys = @('CLAUDE_SESSION_ID', 'CLAUDE_CODE_SESSION_ID', 'CLAUDE_PID', 'ARIA_PROBE_SCOPE')
    $saved = @{}
    foreach ($k in $keys) { $saved[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $null) }
    Push-Location $dir
    try {
        if ($sid)       { $env:CLAUDE_SESSION_ID = $sid }
        if ($harnessId) { $env:CLAUDE_CODE_SESSION_ID = $harnessId }
        if ($claudePid) { $env:CLAUDE_PID = $claudePid }
        if ($argv -and $argv.Count -gt 0) {
            return (& powershell -NoProfile -ExecutionPolicy Bypass -File $run @argv 2>&1 | Out-String).Trim()
        }
        return (& powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String).Trim()
    } finally {
        Pop-Location
        foreach ($k in $keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
    }
}

# Case 1: NO-DATA
$s1 = New-Sandbox
$o1 = Invoke-In $s1 $null
JsonOk 'no-data valid json' $o1
Check 'no-data count' '"count":0' $o1
Check 'no-data empty briefing' '"briefing":""' $o1

# Case 2: empty file
$s2 = New-Sandbox
New-Item -ItemType File -Path (Join-Path $s2 '.claude/session-state/aria-probe-candidates.jsonl') -Force | Out-Null
$o2 = Invoke-In $s2 $null
Check 'empty-file count' '"count":0' $o2

# Case 3: two candidates
$s3 = New-Sandbox
$log3 = Join-Path $s3 '.claude/session-state/aria-probe-candidates.jsonl'
@(
'{"schemaVersion":"1","ts":"2026-06-29T01:00:00Z","sessionId":"sess-A","category":"async","question":"Did the live interpret round-trip compute the aggregate?","capability":"command-invoke:interpretLive","aria_could_answer":true,"surface_present":false}'
'not-json-junk-line'
'{"schemaVersion":"1","ts":"2026-06-29T01:05:00Z","sessionId":"sess-A","category":"errors","question":"Is there an error in the console?","capability":"state-dump:errors","aria_could_answer":true,"surface_present":true}'
) | Set-Content -LiteralPath $log3 -Encoding UTF8
$o3 = Invoke-In $s3 $null
JsonOk 'two-candidate valid json' $o3
Check 'two-candidate count' '"count":2' $o3
Check 'two-candidate briefing nonempty' 'human-relay probe candidate' $o3
Check 'two-candidate carries capability' 'command-invoke:interpretLive' $o3

# Case 4: session scoping
$s4 = New-Sandbox
$log4 = Join-Path $s4 '.claude/session-state/aria-probe-candidates.jsonl'
@(
'{"schemaVersion":"1","ts":"2026-06-29T01:00:00Z","sessionId":"sess-A","category":"async","question":"A-question","capability":"command-invoke:a","aria_could_answer":true}'
'{"schemaVersion":"1","ts":"2026-06-29T01:05:00Z","sessionId":"sess-B","category":"async","question":"B-question","capability":"command-invoke:b","aria_could_answer":true}'
) | Set-Content -LiteralPath $log4 -Encoding UTF8
$o4 = Invoke-In $s4 'sess-A' $null $null $null
Check 'session-scope keeps A' '"count":1' $o4
Check 'session-scope keeps A-question' 'A-question' $o4
if ($o4 -like '*B-question*') { $fail++; Write-Error 'FAIL: session-scope leaked B-question' -ErrorAction Continue } else { $pass++ }

# Case 5: ARIA-25 -- an unlabelled record is UNKNOWN, never MINE.
# The defect: the filter admitted a null-sessionId record into EVERY session's
# output, so Phase 11.5 was handed other sessions' relay events labelled as its
# own and correctly refused to promote them, forever. This self-test had no cell
# here at all -- written in the measurement's own vocabulary, it could not see
# the gap (OVALID-03). Every must-NOT-fire cell is PAIRED with a must-STILL-fire
# control: a "fix" that merely stops reporting passes the negatives and deletes
# the mechanism.
$s5 = New-Sandbox
$log5 = Join-Path $s5 '.claude/session-state/aria-probe-candidates.jsonl'
@(
'{"schemaVersion":"1","ts":"2026-07-01T01:00:00Z","sessionId":"sess-OTHER","category":"async","question":"OTHER-question","aria_could_answer":true}'
'{"schemaVersion":"1","ts":"2026-07-03T01:00:00Z","sessionId":null,"category":"errors","question":"NULL-legacy-question","aria_could_answer":true}'
'{"schemaVersion":"1","ts":"2026-08-12T01:00:00Z","sessionId":"sess-ME","category":"navigation","question":"MINE-question","aria_could_answer":true}'
) | Set-Content -LiteralPath $log5 -Encoding UTF8
$o5 = Invoke-In $s5 'sess-ME' $null $null $null
if ($o5 -like '*NULL-legacy-question*') { $fail++; Write-Error 'FAIL: ARIA-25 unlabelled record leaked into an identified session' -ErrorAction Continue } else { $pass++ }
Check 'ARIA-25 own record still surfaces' 'MINE-question' $o5
Check 'ARIA-25 count excludes the unlabelled row' '"count":1' $o5
Check 'ARIA-25 exclusion is reported, not silent' '"skippedUnlabelled":1' $o5
Check 'ARIA-25 scoped flag true when identified' '"scoped":true' $o5
if ($o5 -like '*OTHER-question*') { $fail++; Write-Error 'FAIL: ARIA-25 peer record leaked' -ErrorAction Continue } else { $pass++ }

# Case 6: no identity at all -> no filter, and the output SAYS so. Honest, but
# NOT the assertion -- a consumer reading the list while ignoring scoped:false
# re-creates the defect, so the FLAG is asserted here, not just the count.
$o6 = Invoke-In $s5 $null $null $null $null
Check 'no-identity scoped flag false' '"scoped":false' $o6
Check 'no-identity briefing declares the whole-log read' 'WHOLE LOG' $o6
Check 'no-identity surfaces the unlabelled row' 'NULL-legacy-question' $o6
Check 'no-identity count is the whole log' '"count":3' $o6

# Case 7: --all reaches the descoped legacy backlog (it is descoped, not deleted)
$o7 = Invoke-In $s5 'sess-ME' $null $null @('--all')
Check '--all reports unscoped' '"scoped":false' $o7
Check '--all reaches the legacy null record' 'NULL-legacy-question' $o7
Check '--all reaches a peer record' 'OTHER-question' $o7

# Case 8: the harness rung -- the rung the whole defect turned on. SessionHelm's
# records 9-18 were null precisely because CLAUDE_SESSION_ID was not exported.
$s8 = New-Sandbox
$log8 = Join-Path $s8 '.claude/session-state/aria-probe-candidates.jsonl'
@(
'{"schemaVersion":"1","ts":"2026-08-12T01:00:00Z","sessionId":"uuid-HARNESS","category":"other","question":"HARNESS-question","aria_could_answer":false}'
'{"schemaVersion":"1","ts":"2026-08-12T02:00:00Z","sessionId":null,"category":"other","question":"NULL-question","aria_could_answer":false}'
) | Set-Content -LiteralPath $log8 -Encoding UTF8
$o8 = Invoke-In $s8 $null 'uuid-HARNESS' $null $null
Check 'harness-rung identifies the session' '"idSource":"harness"' $o8
Check 'harness-rung surfaces its own record' 'HARNESS-question' $o8
if ($o8 -like '*NULL-question*') { $fail++; Write-Error 'FAIL: ARIA-25 unlabelled record leaked under the harness rung' -ErrorAction Continue } else { $pass++ }

# Case 9: SELF IS A SET, not the top rung (DEC-337's other half). A record
# stamped under rung 2 while the reader also holds rung 1 must still be
# recognised -- else the session loses its OWN candidate (the defect inverted).
$s9 = New-Sandbox
$log9 = Join-Path $s9 '.claude/session-state/aria-probe-candidates.jsonl'
@(
'{"schemaVersion":"1","ts":"2026-08-12T03:00:00Z","sessionId":"uuid-BOTH","category":"other","question":"ALIAS-question","aria_could_answer":false}'
'{"schemaVersion":"1","ts":"2026-08-12T03:05:00Z","sessionId":"uuid-FOREIGN","category":"other","question":"FOREIGN-question","aria_could_answer":false}'
) | Set-Content -LiteralPath $log9 -Encoding UTF8
$o9 = Invoke-In $s9 'agent-X' 'uuid-BOTH' $null $null
Check 'alias-set: a lower-rung stamp is still recognised' 'ALIAS-question' $o9
if ($o9 -like '*FOREIGN-question*') { $fail++; Write-Error 'FAIL: alias set matched a foreign harness id' -ErrorAction Continue } else { $pass++ }

Remove-Item -Recurse -Force $s1, $s2, $s3, $s4, $s5, $s8, $s9 -ErrorAction SilentlyContinue

Write-Host "runtime-perception-questions validate: $pass passed, $fail failed"
if ($fail -ne 0) { exit 1 }
exit 0
