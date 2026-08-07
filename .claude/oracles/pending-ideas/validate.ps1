# pending-ideas oracle self-test (Windows) -- INJECT-08 + DEC-281 (PARKED)
# Byte-parallel with validate.sh: drives run.ps1 across the same contract cases.
#
# TWIN-02: run.ps1 is driven by the HOSTING engine, not a pinned powershell.exe.
# Pinning is how DEFER-104's engine widening scored 24/24 green against a live
# pwsh-only defect -- `pwsh validate.ps1` must actually exercise pwsh.
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$run = Join-Path $oracleDir 'run.ps1'
$fails = 0
function Pass($m) { Write-Output "PASS: $m" }
function Fail($m) { Write-Error "FAIL: $m"; $script:fails++ }

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pending-ideas-validate-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $sandbox | Out-Null
try {
    $script:PsHost = try { [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName } catch { 'powershell' }
    function RunIn($dir) {
        Push-Location $dir
        try { return (& $script:PsHost -NoProfile -File $run 2>$null | Out-String).Trim() }
        finally { Pop-Location }
    }
    function JGet($json, $key) {
        if ($json -match ('"' + [regex]::Escape($key) + '"\s*:\s*("([^"]*)"|([a-z0-9]+))')) {
            if ($matches[2]) { return $matches[2] } else { return $matches[3] }
        }
        return $null
    }

    # Case 1: no deferred dir
    $c1 = Join-Path $sandbox 'c1'; New-Item -ItemType Directory -Force -Path $c1 | Out-Null
    $out = RunIn $c1
    if ((JGet $out 'pending_count') -eq '0' -and (JGet $out 'no_data') -eq 'false' -and ($out -match '"briefing"\s*:\s*""')) {
        Pass 'case1 absent -> empty briefing, no_data:false'
    } else { Fail "case1 absent (got: $out)" }

    # Case 2: one PROPOSED item
    $c2d = Join-Path $sandbox 'c2\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c2d | Out-Null
    @'
---
id: DEFER-001
title: Sample injected idea
status: PROPOSED
origin: inject
source-project: GitCellar
injected-at: 2026-07-08T21:00:00Z
autonomy-hint: collaborative
suggested-entry-point: spec
scope-estimate: needs-scoping
content-hash: abc123
---

# DEFER-001: Sample injected idea

## Idea
A sample.
'@ | Set-Content -LiteralPath (Join-Path $c2d 'DEFER-001_sample-idea.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c2')
    if ((JGet $out 'pending_count') -eq '1' -and ($out -match 'DEFER-001') -and ($out -match 'GitCellar') -and -not ($out -match '"briefing"\s*:\s*""')) {
        Pass 'case2 one PROPOSED -> count 1, briefing lists it'
    } else { Fail "case2 PROPOSED (got: $out)" }

    # Case 3: one TRIAGED item (excluded)
    $c3d = Join-Path $sandbox 'c3\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c3d | Out-Null
    @'
---
id: DEFER-002
title: Already triaged
status: TRIAGED
origin: defer-local
---

# DEFER-002: Already triaged
'@ | Set-Content -LiteralPath (Join-Path $c3d 'DEFER-002_triaged.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c3')
    if ((JGet $out 'pending_count') -eq '0' -and ($out -match '"briefing"\s*:\s*""')) {
        Pass 'case3 TRIAGED -> excluded, empty briefing'
    } else { Fail "case3 TRIAGED (got: $out)" }

    # Case 4: DEFER file with no parseable status -> NO-DATA
    $c4d = Join-Path $sandbox 'c4\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c4d | Out-Null
    @'
# DEFER-003: No status anywhere

Just prose, no front-matter status and no legacy Status line.
'@ | Set-Content -LiteralPath (Join-Path $c4d 'DEFER-003_broken.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c4')
    if ((JGet $out 'no_data') -eq 'true' -and -not ($out -match '"briefing"\s*:\s*""')) {
        Pass 'case4 unparseable -> no_data:true, NO-DATA briefing'
    } else { Fail "case4 NO-DATA (got: $out)" }

    # Case 5: PARKED surfaces (DEC-281 / DEFER-109). The gap is INVERTED --
    # surfacing only PROPOSED made a triaged-but-still-open brief invisible at
    # every session start, forever.
    $c5d = Join-Path $sandbox 'c5\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c5d | Out-Null
    @(
      '---',
      'id: DEFER-003',
      'title: AOR-drive re-arm at 0 of 10',
      'status: PARKED',
      're-arm: R2 reaches 10 tracked sessions',
      '---'
    ) | Set-Content -LiteralPath (Join-Path $c5d 'DEFER-003_parked-with-condition.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c5')
    if ((JGet $out 'parked_count') -eq '1' -and (JGet $out 'pending_count') -eq '0' -and ($out -match 'DEFER-003') -and ($out -match 're-arm: R2 reaches 10 tracked sessions') -and -not ($out -match '"briefing"\s*:\s*""')) {
        Pass 'case5 PARKED -> surfaced at session start with its re-arm condition'
    } else { Fail "case5 PARKED (got: $out)" }

    # 5b. PARKED must NOT join the frozen PROPOSED contract: INJECT-08 says
    #     items[] is PROPOSED-only and un-triaged, and a parked item IS triaged.
    if ($out -match '"items"\s*:\s*\[\]') {
        Pass 'case5b PARKED stays out of the frozen items[] / pending_count'
    } else { Fail "case5b PARKED leaked into the frozen PROPOSED contract (got: $out)" }

    # 5c. Presence-checked, never evaluated (OVALID-02): a parked brief with no
    #     declared condition is still surfaced, and said to have none.
    @('---','id: DEFER-017','title: TUTOR Arc 3 centrepiece','status: PARKED','---') |
        Set-Content -LiteralPath (Join-Path $c5d 'DEFER-017_parked-no-condition.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c5')
    if ((JGet $out 'parked_count') -eq '2' -and ($out -match 'no re-arm condition declared')) {
        Pass 'case5c parked-without-a-condition is surfaced AND named as such'
    } else { Fail "case5c parked without re-arm (got: $out)" }

    # Case 6: THE RED-FIRST HALF -- a finished brief must stay hidden. A PARKED
    # surface that also lights up for finished briefs is WORSE than no surface:
    # it trains agents to ignore the lane. Every spelling of done the corpus
    # actually uses is seeded, because a cell testing only RESOLVED would be
    # green against a check that matched on merely not-PROPOSED.
    $c6d = Join-Path $sandbox 'c6\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c6d | Out-Null
    $i = 0
    foreach ($st in @('RESOLVED','IMPLEMENTED','COMPLETED','CLOSED','APPLIED','FOLDED','PHASE-1-IMPLEMENTED','RESOLVED-DECLINED','TRIAGED','DISMISSED','IN-PROGRESS')) {
        $i++
        $nm = 'DEFER-{0:d3}' -f $i
        @('---', "id: $nm", "title: finished ($st)", "status: $st", '---') |
            Set-Content -LiteralPath (Join-Path $c6d ($nm + '_done.md')) -Encoding UTF8
    }
    $out = RunIn (Join-Path $sandbox 'c6')
    if ((JGet $out 'parked_count') -eq '0' -and (JGet $out 'pending_count') -eq '0' -and (JGet $out 'no_data') -eq 'false' -and ($out -match '"briefing"\s*:\s*""')) {
        Pass 'case6 RED-FIRST: 11 finished/triaged spellings stay hidden, briefing empty'
    } else { Fail "case6 finished briefs leaked into the briefing (got: $out)" }

    # 6b. TRIAGED specifically. PARKED is a NEW label, not a rename of TRIAGED --
    #     if a future edit made TRIAGED surface, all 21 long-shipped briefs light up.
    @('---','id: DEFER-099','title: triaged only','status: TRIAGED','---') |
        Set-Content -LiteralPath (Join-Path $c6d 'DEFER-099_triaged.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c6')
    if ((JGet $out 'parked_count') -eq '0' -and ($out -match '"briefing"\s*:\s*""')) {
        Pass 'case6b TRIAGED is not PARKED -- still hidden'
    } else { Fail "case6b TRIAGED leaked into the parked surface (got: $out)" }

    # Case 7: composition -- the NO-DATA note TRAILS what was surfaced, so it can
    # never displace it.
    $c7d = Join-Path $sandbox 'c7\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c7d | Out-Null
    @('---','id: DEFER-050','title: fresh idea','status: PROPOSED','---') |
        Set-Content -LiteralPath (Join-Path $c7d 'DEFER-050_fresh.md') -Encoding UTF8
    @('---','id: DEFER-051','title: parked thing','status: PARKED','re-arm: a second consumer appears','---') |
        Set-Content -LiteralPath (Join-Path $c7d 'DEFER-051_parked.md') -Encoding UTF8
    @('# DEFER-052: no status anywhere') |
        Set-Content -LiteralPath (Join-Path $c7d 'DEFER-052_broken.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c7')
    if ((JGet $out 'pending_count') -eq '1' -and (JGet $out 'parked_count') -eq '1' -and (JGet $out 'no_data') -eq 'true' -and ($out -match 'DEFER-050') -and ($out -match 'DEFER-051') -and ($out -match 'unparseable')) {
        Pass 'case7 pending + parked + NO-DATA compose, none displacing another'
    } else { Fail "case7 composition (got: $out)" }

    # 7b. The briefing must stay double-quote-free -- the session-start fan-out
    #     extracts it with a no-quote regex, and parked titles / re-arm text are
    #     NEW caller-controlled text on that path.
    @('---','id: DEFER-053','title: a "quoted" title','status: PARKED','re-arm: when "x" happens','---') |
        Set-Content -LiteralPath (Join-Path $c7d 'DEFER-053_quoted.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c7')
    $br = ''
    if ($out -match '"briefing"\s*:\s*"(.*)"\}\s*$') { $br = $matches[1] }
    if ($br -match '\\"') {
        Fail 'case7b a quote reached the briefing string (would truncate the fan-out line)'
    } else { Pass 'case7b parked titles and re-arm text stay double-quote-free' }

    # Case 8: THE RED-FIRST CELL for DEC-317 (DEFER-151). The status WORD is the
    # first token; a rationale after it is house style, not a different status.
    # Before the fix the whole line was uppercased with ALL whitespace stripped
    # and compared exactly, so a parked-with-a-rationale brief collapsed to one
    # ~400-char token that matched nothing and vanished in silence. Measured live
    # on DEFER-146/DEFER-147. This cell REDDENS against the pre-fix run.
    $c8d = Join-Path $sandbox 'c8\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c8d | Out-Null
    @('---','id: DEFER-146','title: parked with a rationale on the status line',
      'status: PARKED 2026-08-06 -- the behavior change is SHIPPED (ef0c93fb); what remains is the spec reconciliation. RE-ARM -- next session finding docs/specs clean','---') |
        Set-Content -LiteralPath (Join-Path $c8d 'DEFER-146_parked-with-rationale.md') -Encoding UTF8
    # ...and a DONE word with a rationale must STAY hidden. Without this control
    # a leading-token fix and a match-any-prefix fix are indistinguishable, and
    # the second lights up 105 finished briefs.
    @('---','id: DEFER-147','title: resolved with a rationale on the status line',
      'status: RESOLVED 2026-08-06 (DEC-999) -- shipped, and a long rationale that mentions PROPOSED and PARKED in passing','---') |
        Set-Content -LiteralPath (Join-Path $c8d 'DEFER-147_resolved-with-rationale.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c8')
    if ((JGet $out 'parked_count') -eq '1' -and (JGet $out 'pending_count') -eq '0' -and
        (JGet $out 'unknown_status_count') -eq '0' -and ($out -match 'DEFER-146') -and -not ($out -match 'DEFER-147')) {
        Pass 'case8 the status WORD is parsed out of the line (rationale after it does not hide the brief)'
    } else { Fail "case8 status-word parse (got: $out)" }

    # 8b. A re-arm written INTO the status rationale is reported. Presence only,
    #     never evaluated (OVALID-02) -- so the oracle stops SAYING 'no re-arm
    #     condition declared' about a brief that declared one.
    if ($out -match 're-arm: next session finding docs/specs clean') {
        Pass 'case8b re-arm declared inside the status rationale is surfaced'
    } else { Fail "case8b re-arm from status rationale (got: $out)" }

    # 8c. Hyphenated words survive the token cut whole -- RESOLVED-DECLINED must
    #     not be reported as unrecognized (noise).
    $c8cd = Join-Path $sandbox 'c8c\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c8cd | Out-Null
    @('---','id: DEFER-149','title: hyphenated done-word',
      'status: RESOLVED-DECLINED 2026-08-06 -- retro-demoted to a ledger line','---') |
        Set-Content -LiteralPath (Join-Path $c8cd 'DEFER-149_hyphen.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c8c')
    if ((JGet $out 'unknown_status_count') -eq '0' -and (JGet $out 'pending_count') -eq '0' -and ($out -match '"briefing"\s*:\s*""')) {
        Pass 'case8c hyphenated done-words survive the token cut and stay silent'
    } else { Fail "case8c hyphenated done-word (got: $out)" }

    # Case 9: a word outside the set is REPORTED, not dropped (DEC-317). The
    # trigger instance carried `**Status**: OPEN` -- the framework's own word for
    # an open DISCOVERIES.md entry -- and was invisible here AND unaudited by
    # dec-alloc-guard --check-defer. A file this oracle silently drops is
    # indistinguishable from a file that is not there (DEC-292, one door over).
    $c9d = Join-Path $sandbox 'c9\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c9d | Out-Null
    @('# A grant request outlives its subject','','**Status**: OPEN','**Injected-From**: Table') |
        Set-Content -LiteralPath (Join-Path $c9d 'DEFER-table-20260806_stale-grant.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c9')
    if ((JGet $out 'unknown_status_count') -eq '1' -and ($out -match 'DEFER-table-20260806_stale-grant') -and ($out -match 'unrecognized status word')) {
        Pass 'case9 an unrecognized status word is reported rather than silently dropped'
    } else { Fail "case9 unknown-status reporting (got: $out)" }

    # 9b. AND IT IS NOT PROMOTED. The filter is correct and is NOT widened:
    #     INJECT-08 freezes items[] as PROPOSED-only. A permissive fix passes 9
    #     and is exactly what the brief said must not survive.
    if ((JGet $out 'pending_count') -eq '0' -and (JGet $out 'parked_count') -eq '0' -and -not ($out -match '"items"\s*:\s*\[\{')) {
        Pass 'case9b an unrecognized word does NOT join pending_count/items[]'
    } else { Fail "case9b unknown must not be promoted (got: $out)" }

    # 9c. Anti-noise control: the 105 RESOLVED / 28 IMPLEMENTED briefs in the
    #     real corpus must NOT reach this clause. A signal that fires 130 times
    #     is ignored, which is how a real unknown word hides.
    $o6 = RunIn (Join-Path $sandbox 'c6')
    if ((JGet $o6 'unknown_status_count') -eq '0') {
        Pass 'case9c every spelling of done stays out of the unrecognized clause'
    } else { Fail "case9c done-words leaked into the unrecognized clause (got: $o6)" }

    # Case 10: no empty string in the frozen items[].id (DEC-317). A slug-form
    # brief with no front-matter id: used to emit "" -- an empty required key a
    # consumer joining on id gets silently -- and a double space in the briefing.
    $c10d = Join-Path $sandbox 'c10\docs\planning\deferred'; New-Item -ItemType Directory -Force -Path $c10d | Out-Null
    @('# A slug-form brief with no front matter at all','','**Status**: PROPOSED') |
        Set-Content -LiteralPath (Join-Path $c10d 'DEFER-slugform_no-numeric-id.md') -Encoding UTF8
    $out = RunIn (Join-Path $sandbox 'c10')
    if ((JGet $out 'pending_count') -eq '1' -and -not ($out -match '"id":""') -and
        ($out -match 'DEFER-slugform_no-numeric-id') -and -not ($out -match 'idea[s]?:  ')) {
        Pass 'case10 a slug-form brief gets a non-empty id and no double space'
    } else { Fail "case10 empty id fallback (got: $out)" }

    # Schema field presence
    $out = RunIn (Join-Path $sandbox 'c2')
    foreach ($field in @('pending_count','items','parked_count','parked','unknown_status_count','unknown_status','no_data','briefing')) {
        if (-not ($out -match ('"' + $field + '"'))) { Fail "missing schema field '$field'" }
    }
    # The new fields must ride EVERY emission path, including the early exits --
    # a consumer reading parked_count would otherwise break on a graceful run.
    foreach ($c in @('c1','c4','c6')) {
        $o = RunIn (Join-Path $sandbox $c)
        if (-not ($o -match '"parked_count"')) { Fail "parked_count missing from an early-exit emission ($c)" }
        if (-not ($o -match '"unknown_status_count"')) { Fail "unknown_status_count missing from an early-exit emission ($c)" }
    }
} finally {
    Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

if ($fails -eq 0) {
    Write-Output 'PASS: pending-ideas oracle validates (18 checks + schema on every path)'
    exit 0
} else {
    Write-Error "FAIL: pending-ideas oracle -- $fails check(s) failed"
    exit 1
}
