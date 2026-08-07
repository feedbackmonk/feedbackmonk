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
function Invoke-In([string]$dir, [string]$sid) {
    Push-Location $dir
    try {
        if ($sid) { $env:CLAUDE_SESSION_ID = $sid } else { Remove-Item Env:CLAUDE_SESSION_ID -ErrorAction SilentlyContinue }
        return (& powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String).Trim()
    } finally { Pop-Location; Remove-Item Env:CLAUDE_SESSION_ID -ErrorAction SilentlyContinue }
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
$o4 = Invoke-In $s4 'sess-A'
Check 'session-scope keeps A' '"count":1' $o4
Check 'session-scope keeps A-question' 'A-question' $o4
if ($o4 -like '*B-question*') { $fail++; Write-Error 'FAIL: session-scope leaked B-question' -ErrorAction Continue } else { $pass++ }

Remove-Item -Recurse -Force $s1, $s2, $s3, $s4 -ErrorAction SilentlyContinue

Write-Host "runtime-perception-questions validate: $pass passed, $fail failed"
if ($fail -ne 0) { exit 1 }
exit 0
