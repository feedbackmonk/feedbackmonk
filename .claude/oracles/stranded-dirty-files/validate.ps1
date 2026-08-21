# stranded-dirty-files oracle self-test (Windows)
#
# Sandbox-builds five scenarios and asserts the oracle output matches the
# FROZEN output schema (oracle.json):
#
#   T1. no-stranded                 -> count==0, briefing==""
#   T2. small-stranded              -> count>0, briefing references "no live owner"
#   T3. large-stranded              -> count>=50, briefing references "significant accumulation"
#   T4. detection-skipped-too-many  -> count==-1, briefing references "detection skipped"
#   T5. live-peer-owns-file         -> peer's claimed file excluded from sample, count<dirty
#
# Each test creates a fresh git sandbox under $env:TEMP, runs the oracle from
# the project root, and asserts the JSON output's shape + key fields.

$ErrorActionPreference = 'Continue'
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$Pass = 0
$Fail = 0
function Pass($msg) { $script:Pass++; Write-Host "PASS: $msg" }
function Fail($msg) { $script:Fail++; Write-Host "FAIL: $msg" -ForegroundColor Red }

$Sandbox = $null

function Cleanup {
    if ($script:Sandbox -and (Test-Path -LiteralPath $script:Sandbox)) {
        Remove-Item -Recurse -Force -LiteralPath $script:Sandbox -ErrorAction SilentlyContinue
    }
    $script:Sandbox = $null
}

function Mk-Sandbox {
    $rawSandbox = Join-Path $env:TEMP ("sdfix-" + [guid]::NewGuid().ToString("N").Substring(0,8))
    $proj = Join-Path $rawSandbox "project"
    $oracleSub = Join-Path $proj ".claude/oracles/stranded-dirty-files"
    New-Item -ItemType Directory -Path $oracleSub -Force | Out-Null
    # Canonicalize to long path form. $env:TEMP often resolves to a DOS 8.3
    # short name (e.g. C:\Users\SOMEUS~1\...) on Windows, but the spawned
    # oracle's (Get-Location).Path returns the long form (C:\Users\someuser\...).
    # If we don't canonicalize, T5's workDir comparison (registry vs. oracle's
    # Get-Location) silently mismatches. Push-Location + Get-Location resolves
    # the long form via the filesystem.
    # DEFER-077: -ErrorAction Stop is load-bearing here even though nothing is
    # written inside the push/pop window -- a FAILED Push-Location would leave
    # the REAL repo as cwd and $script:Sandbox would silently become the real
    # repo root, so every later fixture write would land in the live tree.
    if (-not $rawSandbox) { throw 'DEFER-077: empty sandbox path -- refusing to run git in the CWD' }
    Push-Location -LiteralPath $rawSandbox -ErrorAction Stop
    $script:Sandbox = (Get-Location).Path
    Pop-Location
    Copy-Item (Join-Path $oracleDir "run.ps1")      (Join-Path $oracleSub "run.ps1")      -Force
    Copy-Item (Join-Path $oracleDir "oracle.json")  (Join-Path $oracleSub "oracle.json")  -Force

    if (-not $proj) { throw 'DEFER-077: empty sandbox path -- refusing to run git in the CWD' }
    Push-Location -LiteralPath $proj -ErrorAction Stop   # DEFER-077
    try {
        & git init -q -b main 2>$null
        if ($LASTEXITCODE -ne 0) { & git init -q 2>$null | Out-Null }
        & git config user.email "test@stranded.local"
        & git config user.name  "stranded-validate"
        Set-Content -Path "seed.txt" -Value "seed" -Encoding UTF8 -NoNewline
        & git add seed.txt 2>$null | Out-Null
        $env:GIT_AUTHOR_DATE = "2026-04-01T00:00:00Z"
        $env:GIT_COMMITTER_DATE = "2026-04-01T00:00:00Z"
        & git commit -q -m "seed commit" 2>$null | Out-Null
        Remove-Item Env:GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
        Remove-Item Env:GIT_COMMITTER_DATE -ErrorAction SilentlyContinue
    } finally {
        Pop-Location
    }
}

function Mk-OldDirty([string]$rel, [string]$content = "old") {
    $abs = Join-Path (Join-Path $script:Sandbox "project") $rel
    $dir = Split-Path -Parent $abs
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    Set-Content -Path $abs -Value $content -Encoding UTF8 -NoNewline
    # Force mtime to 2026-03-15 (before seed commit at 2026-04-01)
    $oldDate = [DateTime]::ParseExact("2026-03-15T00:00:00Z", "yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture, [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal)
    (Get-Item -LiteralPath $abs).LastWriteTimeUtc = $oldDate
}

function Mk-NewDirty([string]$rel, [string]$content = "new") {
    $abs = Join-Path (Join-Path $script:Sandbox "project") $rel
    $dir = Split-Path -Parent $abs
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    Set-Content -Path $abs -Value $content -Encoding UTF8 -NoNewline
}

function Run-Oracle {
    # Spawned powershell.exe inherits the OS cwd ([Environment]::CurrentDirectory),
    # NOT the parent's Push-Location state. Set both before launching so the
    # oracle's (Get-Location).Path matches the sandbox project root.
    $proj = Join-Path $script:Sandbox "project"
    Push-Location $proj
    $prevEnvCwd = [Environment]::CurrentDirectory
    [Environment]::CurrentDirectory = $proj
    try {
        $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".claude/oracles/stranded-dirty-files/run.ps1" 2>&1
        return ($out | Out-String).Trim()
    } finally {
        [Environment]::CurrentDirectory = $prevEnvCwd
        Pop-Location
    }
}

function Assert-ValidJson($out, $label) {
    try {
        $null = $out | ConvertFrom-Json -ErrorAction Stop
        return $true
    } catch {
        Fail "${label}: output is not valid JSON: $out"
        return $false
    }
}

$schemaFields = @('has_stranded','count','oldest_mtime','sample','live_peer_count','provably_unowned','attribution','last_finalize_at','briefing')

function Assert-SchemaFields($out, $label) {
    foreach ($f in $schemaFields) {
        if ($out -notmatch "`"$f`"") {
            Fail "${label}: missing schema field '$f' in: $out"
            return $false
        }
    }
    return $true
}

# -----------------------------------------------------------------------------
# T1. no-stranded
# -----------------------------------------------------------------------------
Mk-Sandbox
Mk-NewDirty "post-commit-mod.txt" "fresh"
$out = Run-Oracle
[void](Assert-ValidJson $out "T1")
[void](Assert-SchemaFields $out "T1")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.has_stranded -eq $false -and $obj.count -eq 0 -and [string]::IsNullOrEmpty($obj.briefing)) {
    Pass 'T1: no-stranded -> has_stranded=false count=0 briefing=""'
} else {
    Fail "T1: expected has_stranded=false count=0 briefing=''; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T2. small-stranded
# -----------------------------------------------------------------------------
Mk-Sandbox
Mk-OldDirty "stranded-1.txt"
Mk-OldDirty "stranded-2.txt"
Mk-OldDirty "stranded-3.txt"
$out = Run-Oracle
[void](Assert-ValidJson $out "T2")
[void](Assert-SchemaFields $out "T2")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.has_stranded -eq $true -and $obj.count -eq 3 -and $obj.briefing -match 'no live owner' -and $obj.attribution -eq 'no-peers' -and $obj.briefing -match 'for cleanup') {
    Pass "T2: small-stranded (no registry) -> count=3, attribution=no-peers, sweep still recommended"
} else {
    Fail "T2: expected count=3 attribution=no-peers briefing matches 'no live owner'+'for cleanup'; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T3. large-stranded
# -----------------------------------------------------------------------------
Mk-Sandbox
1..55 | ForEach-Object { Mk-OldDirty ("stranded-$_.txt") }
$out = Run-Oracle
[void](Assert-ValidJson $out "T3")
[void](Assert-SchemaFields $out "T3")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.has_stranded -eq $true -and $obj.count -eq 55 -and $obj.briefing -match 'significant accumulation') {
    Pass "T3: large-stranded -> count=55 briefing references 'significant accumulation'"
} else {
    Fail "T3: expected has_stranded=true count=55 briefing references 'significant accumulation'; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T4. detection-skipped-too-many
# -----------------------------------------------------------------------------
Mk-Sandbox
$projRoot = Join-Path $script:Sandbox "project"
1..2001 | ForEach-Object {
    Set-Content -Path (Join-Path $projRoot ("f-$_.txt")) -Value "x" -Encoding UTF8 -NoNewline
}
$out = Run-Oracle
[void](Assert-ValidJson $out "T4")
[void](Assert-SchemaFields $out "T4")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.has_stranded -eq $false -and $obj.count -eq -1 -and $obj.briefing -match 'detection skipped') {
    Pass "T4: detection-skipped -> count=-1 briefing references 'detection skipped'"
} else {
    Fail "T4: expected has_stranded=false count=-1 briefing references 'detection skipped'; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T5. live-peer-owns-file
# -----------------------------------------------------------------------------
Mk-Sandbox
Mk-OldDirty "peer-claimed.txt"
Mk-OldDirty "unclaimed.txt"
$projRoot = Join-Path $script:Sandbox "project"
$projRootNorm = ($projRoot -replace '\\', '/').TrimEnd('/')
$livePid = $PID
$collabDir = Join-Path $projRoot ".claude/collaboration"
New-Item -ItemType Directory -Path $collabDir -Force | Out-Null
$registry = @"
{
  "registryVersion": 2,
  "sessions": [
    {
      "id": "test-peer-1",
      "status": "active",
      "claudeShellPid": $livePid,
      "workDir": "$projRootNorm",
      "spawnedAt": "2026-05-07T00:00:00Z",
      "dirtyFiles": ["peer-claimed.txt"]
    }
  ],
  "closed": []
}
"@
Set-Content -Path (Join-Path $collabDir "active-sessions.json") -Value $registry -Encoding UTF8

$out = Run-Oracle
[void](Assert-ValidJson $out "T5")
[void](Assert-SchemaFields $out "T5")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
$samplePaths = @()
if ($null -ne $obj -and $obj.sample) { $samplePaths = @($obj.sample | ForEach-Object { $_.path }) }
if ($null -ne $obj -and $obj.has_stranded -eq $true -and $obj.count -eq 1 -and $obj.live_peer_count -eq 1 -and $obj.attribution -eq 'measured' -and ($samplePaths -contains 'unclaimed.txt') -and (-not ($samplePaths -contains 'peer-claimed.txt')) -and $obj.briefing -match 'for cleanup') {
    Pass "T5: live-peer-owns-file -> claimed file excluded; attribution=measured; sweep still recommended (anti-vacuity)"
} else {
    Fail "T5: expected count=1 live_peer_count=1 attribution=measured sample=[unclaimed.txt] + sweep advice; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T6. live peer, NO attribution published -> 'unavailable', sweep WITHHELD
# (DEFER-039/190). The peer is THIS process ($PID), alive by construction.
#
# STRAND-02 / DEC-372 fixture note: the peer's spawnedAt was 2026-05-07, i.e.
# AFTER the dirty files' 2026-03-15 mtime -- a peer that could not possibly have
# written them. Under the mtime-ordering proof that is now correctly
# 'predates-peers' (sweep), which would have made T6/T7/T7b assert the OPPOSITE
# of what they mean. The date is moved to 2026-01-01 so the peer CAN own these
# files, which is the situation these three cells are about. The propositions
# are unchanged; the could-NOT-have-written case is asserted by T8/T8b/T8c.
# -----------------------------------------------------------------------------
Mk-Sandbox
Mk-OldDirty "inflight-a.txt"
Mk-OldDirty "inflight-b.txt"
$projRoot = Join-Path $script:Sandbox "project"
$projRootNorm = ($projRoot -replace '\\', '/').TrimEnd('/')
$collabDir = Join-Path $projRoot ".claude/collaboration"
New-Item -ItemType Directory -Path $collabDir -Force | Out-Null
$registry = @"
{
  "registryVersion": 2,
  "sessions": [
    {
      "sessionId": "live-peer-silent",
      "status": "active",
      "claudeShellPid": $PID,
      "workDir": "$projRootNorm",
      "spawnedAt": "2026-01-01T00:00:00Z"
    }
  ],
  "closed": []
}
"@
Set-Content -Path (Join-Path $collabDir "active-sessions.json") -Value $registry -Encoding UTF8
$out = Run-Oracle
[void](Assert-ValidJson $out "T6")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.count -eq 2 -and $obj.live_peer_count -eq 1 -and $obj.attribution -eq 'unavailable' -and $obj.briefing -match 'OWNERSHIP UNKNOWN' -and $obj.briefing -notmatch 'for cleanup') {
    Pass "T6: live silent peer -> attribution=unavailable; sweep WITHHELD"
} else {
    Fail "T6: expected count=2 live_peer_count=1 attribution=unavailable briefing=OWNERSHIP UNKNOWN w/o sweep advice; got $out"
}
# Keep sandbox for T7 (same peer gains a write journal).

# -----------------------------------------------------------------------------
# T7. FALSIFICATION PAIR (GitCellar selftest.ps1, DEFER-039): the peer's write
# journal claims exactly ONE of the two old dirty files -> that one and ONLY
# that one filtered; 'journal-partial'; still withheld. Then delete the journal
# -> both back, grade drops to 'unavailable' (proves the filter did the work).
# -----------------------------------------------------------------------------
$journalDir = Join-Path $projRoot ".claude/session-state/write-journal"
New-Item -ItemType Directory -Path $journalDir -Force | Out-Null
$rec = '{"ts":"2026-03-01T00:00:00Z","path":"' + $projRootNorm + '/inflight-a.txt","op":"edit"}'
Set-Content -Path (Join-Path $journalDir "live-peer-silent.jsonl") -Value $rec -Encoding UTF8
$out = Run-Oracle
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
$samplePaths = @()
if ($null -ne $obj -and $obj.sample) { $samplePaths = @($obj.sample | ForEach-Object { $_.path }) }
if ($null -ne $obj -and $obj.count -eq 1 -and $obj.attribution -eq 'journal-partial' -and (-not ($samplePaths -contains 'inflight-a.txt')) -and ($samplePaths -contains 'inflight-b.txt') -and $obj.briefing -match 'OWNERSHIP UNPROVEN' -and $obj.briefing -notmatch 'for cleanup') {
    Pass "T7: journal HIT filters exactly its file; attribution=journal-partial; sweep still WITHHELD"
} else {
    Fail "T7: expected count=1 (inflight-b only) attribution=journal-partial briefing=OWNERSHIP UNPROVEN; got $out"
}
Remove-Item (Join-Path $journalDir "live-peer-silent.jsonl") -Force -ErrorAction SilentlyContinue
$out = Run-Oracle
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.count -eq 2 -and $obj.attribution -eq 'unavailable') {
    Pass "T7b: journal deleted -> BOTH files stranded again + grade drops to unavailable (filter was doing the work)"
} else {
    Fail "T7b: expected count=2 attribution=unavailable after journal deletion; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T8 group. STRAND-02 / DEC-372 -- the mtime-ordering proof.
#
# Timeline (seed commit 2026-04-01 is the strand boundary):
#   2026-03-15  inflight-a / inflight-b written
#   2026-03-20  a live peer that could NOT have written them starts
#   2026-03-25  inflight-c written, by a route the write journal cannot see
# -----------------------------------------------------------------------------
function Mk-DirtyAt([string]$rel, [string]$isoUtc) {
    $abs = Join-Path (Join-Path $script:Sandbox "project") $rel
    $dir = Split-Path -Parent $abs
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    Set-Content -Path $abs -Value "x" -Encoding UTF8 -NoNewline
    $dt = [DateTime]::ParseExact($isoUtc, "yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture, [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal)
    (Get-Item -LiteralPath $abs).LastWriteTimeUtc = $dt
}

function Write-PeerRegistry([string]$sessionId, [string]$projRootNorm, [string]$spawnedAt) {
    $collab = Join-Path (Join-Path $script:Sandbox "project") ".claude/collaboration"
    New-Item -ItemType Directory -Path $collab -Force | Out-Null
    if ($spawnedAt -eq 'OMIT') {
        $reg = '{"registryVersion":2,"sessions":[{"sessionId":"' + $sessionId + '","status":"active","claudeShellPid":' + $PID + ',"workDir":"' + $projRootNorm + '"}],"closed":[]}'
    } else {
        $reg = '{"registryVersion":2,"sessions":[{"sessionId":"' + $sessionId + '","status":"active","claudeShellPid":' + $PID + ',"workDir":"' + $projRootNorm + '","spawnedAt":"' + $spawnedAt + '"}],"closed":[]}'
    }
    Set-Content -Path (Join-Path $collab "active-sessions.json") -Value $reg -Encoding UTF8
}

Mk-Sandbox
Mk-DirtyAt "inflight-a.txt" "2026-03-15T00:00:00Z"
Mk-DirtyAt "inflight-b.txt" "2026-03-15T00:00:00Z"
$projRoot = Join-Path $script:Sandbox "project"
$projRootNorm = ($projRoot -replace '\\', '/').TrimEnd('/')

# T8: live peer started AFTER both files -> the proof covers the whole set.
Write-PeerRegistry "live-peer-late" $projRootNorm "2026-03-20T00:00:00Z"
$out = Run-Oracle
[void](Assert-ValidJson $out "T8")
[void](Assert-SchemaFields $out "T8")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.count -eq 2 -and $obj.live_peer_count -eq 1 -and $obj.attribution -eq 'predates-peers' -and $obj.provably_unowned -eq 2 -and $obj.briefing -match 'for cleanup') {
    Pass "T8: live peer started AFTER the files -> attribution=predates-peers, provably_unowned=2, sweep RECOMMENDED"
} else {
    Fail "T8: expected count=2 live_peer_count=1 attribution=predates-peers provably_unowned=2 + sweep advice; got $out"
}

# T8b ANTI-VACUITY + caveat (a): a third file written after the peer's start by a
# route the journal cannot see. One unproved file holds the WHOLE stage. Without
# this cell, "always grade predates-peers" passes T8 perfectly.
Mk-DirtyAt "inflight-c.txt" "2026-03-25T00:00:00Z"
$out = Run-Oracle
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.count -eq 3 -and $obj.attribution -eq 'unavailable' -and $obj.provably_unowned -eq 2 -and $obj.briefing -notmatch 'for cleanup' -and $obj.briefing -match '2 of 3') {
    Pass "T8b: one journal-invisible file written after the peer's start -> grade drops to unavailable, sweep WITHHELD, 2 of 3 still reported as proved"
} else {
    Fail "T8b: expected count=3 attribution=unavailable provably_unowned=2 + '2 of 3' clause + no sweep advice; got $out"
}

# T8c FAIL-CLOSED: same all-predating fixture as T8, peer carries NO spawnedAt.
Remove-Item (Join-Path $projRoot "inflight-c.txt") -Force -ErrorAction SilentlyContinue
Write-PeerRegistry "live-peer-anchorless" $projRootNorm "OMIT"
$out = Run-Oracle
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.count -eq 2 -and $obj.attribution -eq 'unavailable' -and $obj.provably_unowned -eq 0 -and $obj.briefing -notmatch 'for cleanup' -and $obj.briefing -notmatch 'predates all') {
    Pass "T8c: live peer with NO spawnedAt -> proof unavailable for the whole run, sweep WITHHELD, no proved-count clause"
} else {
    Fail "T8c: expected count=2 attribution=unavailable provably_unowned=0 and no proved-count clause; got $out"
}

# T8d: TWO live peers, one that COULD have written the files and one that could
# not. The floor must be the EARLIEST start. Without this cell a max()-over-peers
# implementation is indistinguishable from min() -- every other cell has one peer.
$collab = Join-Path $projRoot ".claude/collaboration"
New-Item -ItemType Directory -Path $collab -Force | Out-Null
$reg2 = '{"registryVersion":2,"sessions":[{"sessionId":"peer-early","status":"active","claudeShellPid":' + $PID + ',"workDir":"' + $projRootNorm + '","spawnedAt":"2026-01-01T00:00:00Z"},{"sessionId":"peer-late","status":"active","claudeShellPid":' + $PID + ',"workDir":"' + $projRootNorm + '","spawnedAt":"2026-03-20T00:00:00Z"}],"closed":[]}'
Set-Content -Path (Join-Path $collab "active-sessions.json") -Value $reg2 -Encoding UTF8
$out = Run-Oracle
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.count -eq 2 -and $obj.live_peer_count -eq 2 -and $obj.attribution -eq 'unavailable' -and $obj.provably_unowned -eq 0) {
    Pass "T8d: two live peers, floor is the EARLIEST start -> proof does not apply, sweep WITHHELD (a max()-over-peers floor would sweep here)"
} else {
    Fail "T8d: expected count=2 live_peer_count=2 attribution=unavailable provably_unowned=0; got $out"
}

# T8e: the fail-closed cell T8c CANNOT be. Found by running mutation M3, not by
# reading the code: silently skipping an anchorless peer (instead of disabling the
# proof) SURVIVES T8c, because with ONE anchorless peer the floor is empty either
# way. The mutation is only observable when an ANCHORED peer sets a floor that an
# anchorless peer would then be measured against.
$collab = Join-Path $projRoot ".claude/collaboration"
New-Item -ItemType Directory -Path $collab -Force | Out-Null
$reg3 = '{"registryVersion":2,"sessions":[{"sessionId":"peer-anchored","status":"active","claudeShellPid":' + $PID + ',"workDir":"' + $projRootNorm + '","spawnedAt":"2026-03-20T00:00:00Z"},{"sessionId":"peer-anchorless","status":"active","claudeShellPid":' + $PID + ',"workDir":"' + $projRootNorm + '"}],"closed":[]}'
Set-Content -Path (Join-Path $collab "active-sessions.json") -Value $reg3 -Encoding UTF8
$out = Run-Oracle
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.count -eq 2 -and $obj.live_peer_count -eq 2 -and $obj.attribution -eq 'unavailable' -and $obj.provably_unowned -eq 0) {
    Pass "T8e: one anchored + one ANCHORLESS live peer -> the anchored peer's floor must NOT be used; whole run withholds (kills the silent-skip mutation T8c cannot see)"
} else {
    Fail "T8e: expected count=2 live_peer_count=2 attribution=unavailable provably_unowned=0; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T9 group. Caveat (b) -- the identity-keying asymmetry (FINALIZE-SCOPE-11 /
# DEC-337). The property asserted is the FAILURE DIRECTION: a journal this
# oracle cannot find degrades the verdict toward withholding, never toward the
# sweep.
# -----------------------------------------------------------------------------
Mk-Sandbox
Mk-DirtyAt "inflight-a.txt" "2026-03-15T00:00:00Z"
Mk-DirtyAt "inflight-b.txt" "2026-03-15T00:00:00Z"
$projRoot = Join-Path $script:Sandbox "project"
$projRootNorm = ($projRoot -replace '\\', '/').TrimEnd('/')
# Peer started 2026-01-01: it COULD have written both, so the mtime proof does
# not apply and the journal is the only possible evidence.
Write-PeerRegistry "registry-id" $projRootNorm "2026-01-01T00:00:00Z"
$journalDir = Join-Path $projRoot ".claude/session-state/write-journal"
New-Item -ItemType Directory -Path $journalDir -Force | Out-Null
$rec = '{"ts":"2026-03-01T00:00:00Z","path":"' + $projRootNorm + '/inflight-a.txt","op":"edit"}'
Set-Content -Path (Join-Path $journalDir "interactive-shadow-id.jsonl") -Value $rec -Encoding UTF8
$out = Run-Oracle
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.count -eq 2 -and $obj.attribution -eq 'unavailable') {
    Pass "T9: journal keyed under a DIFFERENT id than the registry sessionId -> lookup misses, grade degrades to unavailable (withhold), never to a sweep"
} else {
    Fail "T9: expected count=2 attribution=unavailable with a mis-keyed journal; got $out"
}
Move-Item (Join-Path $journalDir "interactive-shadow-id.jsonl") (Join-Path $journalDir "registry-id.jsonl") -Force -ErrorAction SilentlyContinue
$out = Run-Oracle
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
$samplePaths = @()
if ($null -ne $obj -and $obj.sample) { $samplePaths = @($obj.sample | ForEach-Object { $_.path }) }
if ($null -ne $obj -and $obj.count -eq 1 -and $obj.attribution -eq 'journal-partial' -and (-not ($samplePaths -contains 'inflight-a.txt'))) {
    Pass "T9b: identical journal under the registry's own id -> file filtered, grade journal-partial (T9 measured the key, not the file)"
} else {
    Fail "T9b: expected count=1 attribution=journal-partial after re-keying the journal; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
Write-Host ""
Write-Host "================================================================"
Write-Host "  stranded-dirty-files validate: $Pass PASS / $Fail FAIL"
Write-Host "================================================================"

if ($Fail -gt 0) { exit 1 }
exit 0
