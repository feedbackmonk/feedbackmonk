# retirement-candidates oracle self-test (Windows PowerShell) -- CTXY-04
#
# Twin of validate.sh. Same fixture corpus, same expectations, driven through
# run.ps1 so a PowerShell-only environment still gets real coverage (TWIN-01..03).
# The cross-engine parity assertion itself lives in validate.sh T10, which drives
# BOTH twins over one sandbox; this file asserts the PS twin is correct on its own.
#
# T8 is the load-bearing case: a retirement detector that fires on guardrails is
# worse than no detector at all.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()

$OracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Pass = 0; $Fail = 0
function Mark-Pass($m) { Write-Host "PASS: $m"; $script:Pass++ }
function Mark-Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; $script:Fail++ }

$sb = Join-Path ([System.IO.Path]::GetTempPath()) ("retcand-v-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path (Join-Path $sb ".claude/oracles/retirement-candidates") | Out-Null
Copy-Item (Join-Path $OracleDir "run.ps1") (Join-Path $sb ".claude/oracles/retirement-candidates/run.ps1")
foreach ($d in @("docs/pending", "docs/reviews", "docs/planning/deferred")) {
    New-Item -ItemType Directory -Force -Path (Join-Path $sb $d) | Out-Null
}

function Put($rel, $text) {
    [System.IO.File]::WriteAllText((Join-Path $sb $rel), $text, [System.Text.UTF8Encoding]::new($false))
}

Put "docs/pending/t1-satisfied.md" @"
# T1

## Waiting on the installer
Remove this entry once the installer ships, after 2020-01-01.
Body.
"@

Put "docs/pending/t2-declared.md" @"
# T2

## Waiting on a vendor
Remove this entry once the vendor confirms the SLA.
Body.
"@

Put "docs/reviews/t3-done.md" @"
# T3

## The migration entry
Delete when the shim is gone.
Status: RESOLVED -- landed in the 3.2 release.
"@

Put "docs/reviews/t4-strata.md" @"
# T4

## The original claim
Delete when the parser is replaced.
The parser mis-handles CRLF.
**CORRECTION**: it was the tokenizer, not the parser.
**UPDATE**: reverted; the tokenizer was fine.
"@

Put "docs/planning/deferred/t5-noexit.md" @"
# T5 provisional, no exit condition

## The idea
Nothing here declares what would make this brief deletable.
"@

Put "docs/planning/deferred/t5-hasexit.md" @"
# T5b provisional WITH an exit condition

## The idea
Remove this brief once the CLI flag lands.
"@

$body = (1..60 | ForEach-Object { "Line of stale body text number $_." }) -join "`n"
Put "docs/reviews/t6-supersede.md" @"
# T6

> SUPERSEDED by the new design; kept for provenance.

## Body that is still sitting here being read
$body
"@

# T11/T12 regression: prose ABOUT exit conditions, and a table row documenting
# one, must NOT be reported as satisfied. Both fired as false positives during
# dogfooding; DEFER-044 records the identical defect in governing-doc-consistency
# ("a stale date appears anywhere on the line"), where the prescribed remedy
# would have written a lie into the docs.
Put "docs/reviews/t11-meta.md" @"
# T11 prose about exit conditions

## What the monitor did
None of its four exit conditions (ALL_COMPLETE | TIMEOUT | ERROR) had fired
during the 2026-07-01 session, so the LD never woke.

## Table documenting a remove-when rule
| oracle | asserts | measures |
| --- | --- | --- |
| ``governing-doc-consistency`` | this remove-when deadline has passed | a stale date anywhere on the line (2026-07-13 review) |
"@

Put "docs/reviews/t8-clean.md" @"
# T8 clean

## A trap with no mechanical guard
Calling flush() before the lock is taken corrupts the index. No regression test
covers this yet, so this paragraph is the guard.

## Why we rejected the queue approach
It serialized the writer, which is the entire point of the module.
"@

$fence = '```'
Put "docs/reviews/t9-fenced.md" @"
# T9 fenced

## Example output, not a claim
The tool prints:

$fence
Status: DONE
Remove this entry once acted on, after 2020-01-01.
CORRECTION: ignored
UPDATE: ignored
$fence

That block is sample output, not an assertion about this document.
"@

# T13/T14 regression -- the SILENT FALSE PASS found by dogfooding on a second
# project, 2026-08-02. See validate.sh for the full note: a level-1-only heading
# meant nothing was scanned, and a kebab-cased filename matched neither the
# corpus nor the provisional regex. Either alone silences the whole file.
Put "docs/reviews/t13-h1only.md" @"
# T13 entries under a single level-one heading

Status: RESOLVED -- the shim shipped.
Remove this entry once the shim is gone, after 2020-01-01.
"@

Put "docs/pending-followups.md" @"
# Pending Follow-Ups

- **A kebab-cased follow-ups file.** Nothing here declares what would make it
  removable. Provisional by construction; spelling must not decide visibility.
"@

Put "docs/index.md" @"
# Index
See t1-satisfied.md, t2-declared.md, t3-done.md, t4-strata.md, t11-meta.md,
t5-noexit.md, t5-hasexit.md, t6-supersede.md, t8-clean.md, t9-fenced.md,
t13-h1only.md, pending-followups.md.
"@

Put "docs/reviews/t7-orphan.md" @"
# T7 orphan

## Nothing references this file
Remove this entry once the audit closes.
"@

# DEFER-077: -ErrorAction Stop is load-bearing. $ErrorActionPreference is
# 'Continue' file-wide, so a FAILED Push-Location would leave the REAL repo as
# cwd and the `git config` / `git add -A` / `git commit` below would run against it.
if (-not $sb) { throw 'DEFER-077: empty sandbox path -- refusing to run git in the CWD' }
Push-Location -LiteralPath $sb -ErrorAction Stop
try {
    & git init -q 2>$null; & git config user.email "t@t" 2>$null; & git config user.name "t" 2>$null
    & git add -A 2>$null; & git commit -q -m "fixtures" 2>$null

    $env:CLAUDE_RETIREMENT_CORPUS = 'docs/pending/*.md:docs/reviews/*.md:docs/planning/deferred/*.md'
    $raw = (& powershell -NoProfile -File ".claude/oracles/retirement-candidates/run.ps1" 2>$null | Out-String).Trim()
    Remove-Item Env:\CLAUDE_RETIREMENT_CORPUS -ErrorAction SilentlyContinue

    # T14 runs the DEFAULT corpus -- the point is whether the shipped globs and
    # provisional regex see a kebab-cased follow-ups file at all. Asserting it
    # through the override would test nothing.
    $rawDef = (& powershell -NoProfile -File ".claude/oracles/retirement-candidates/run.ps1" 2>$null | Out-String).Trim()
} finally { Pop-Location }

$d = $null
try { $d = $raw | ConvertFrom-Json } catch { }
if ($null -eq $d) {
    Write-Host "FAIL: oracle output did not parse as JSON" -ForegroundColor Red
    Write-Host $raw
    Remove-Item -Recurse -Force $sb -ErrorAction SilentlyContinue
    exit 1
}

function Has($suffix, $sig) {
    return [bool](@($d.candidates | Where-Object { $_.path.EndsWith($suffix) -and $_.signal -eq $sig }).Count -gt 0)
}
function Surf($suffix, $sig) {
    return [bool](@($d.surfaced | Where-Object { $_.path.EndsWith($suffix) -and $_.signal -eq $sig }).Count -gt 0)
}
function CountFor($suffix) { return @($d.candidates | Where-Object { $_.path.EndsWith($suffix) }).Count }
function Check($actual, $expect, $label) {
    if ("$actual" -eq "$expect") { Mark-Pass $label } else { Mark-Fail "$label (expected '$expect', got '$actual')" }
}

Check (Has 't1-satisfied.md' 'exit-condition-satisfied') $true  "T1: past-dated exit condition -> candidate"
Check (Surf 't2-declared.md' 'exit-condition-declared')  $true  "T2: non-evaluable condition -> surfaced"
Check (Has 't2-declared.md' 'exit-condition-declared')   $false "T2: non-evaluable condition is NOT a candidate"
Check (Has 't3-done.md' 'done-marker')                   $true  "T3: uppercase RESOLVED -> done-marker"
Check (Has 't4-strata.md' 'correction-strata')           $true  "T4: two correction layers -> correction-strata"
Check (Has 't5-noexit.md' 'provisional-no-exit')         $true  "T5: provisional file without exit condition fires"
Check (Has 't5-hasexit.md' 'provisional-no-exit')        $false "T5: provisional file WITH exit condition is silent"
Check (Has 't6-supersede.md' 'self-supersession')        $true  "T6: head banner over long body -> self-supersession"
Check (Has 't7-orphan.md' 'no-inbound-refs')             $true  "T7: unreferenced file -> no-inbound-refs"
Check (Has 't8-clean.md' 'done-marker')                  $false "T8: trap note produces no done-marker"
Check (CountFor 't8-clean.md')                           0      "T8: clean guardrail doc yields ZERO candidates"
Check (Has 't11-meta.md' 'exit-condition-satisfied')     $false "T11: prose ABOUT exit conditions never reads as satisfied"
Check (Surf 't11-meta.md' 'exit-condition-declared')      $true  "T11: it is surfaced as a mention instead"
Check (Has 't11-meta.md' 'done-marker')                   $false "T12: a remove-when table row is not a satisfied condition"
Check (CountFor 't9-fenced.md')                          0      "T9: fenced sample output never fires"
Check (Has 't13-h1only.md' 'done-marker')                 $true  "T13: a level-1-only file is scanned (done-marker)"
Check (Has 't13-h1only.md' 'exit-condition-satisfied')    $true  "T13: a level-1-only file is scanned (satisfied exit condition)"
Check $d.status                                          "warn" "status is warn when candidates exist"
Check ($d.briefing.Contains("WORKLIST"))                 $true  "briefing states the worklist contract"

$dDef = $null
try { $dDef = $rawDef | ConvertFrom-Json } catch { }
$t14 = [bool]($null -ne $dDef -and @($dDef.candidates | Where-Object {
    $_.path.EndsWith('pending-followups.md') -and $_.signal -eq 'provisional-no-exit' }).Count -gt 0)
Check $t14 $true "T14: default corpus + provisional regex see docs/pending-followups.md"

Remove-Item -Recurse -Force $sb -ErrorAction SilentlyContinue

Write-Host "----"
Write-Host "Total: PASS=$Pass  FAIL=$Fail"
if ($Fail -gt 0) { exit 1 }
exit 0
