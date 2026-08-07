# retirement-candidates oracle (Windows PowerShell) -- CTXY-04, DEC-226
#
# Twin of run.sh (TWIN-01..03). Must produce the same candidate set over the same
# corpus under BOTH powershell.exe 5.1 and pwsh 7.
#
# Verification Oracle (kind: verification). Answers: "which passages in this
# project's living durable artifacts carry a deterministic signal that they may
# no longer earn their place?"
#
# This is the CONTENT-level sibling of SWEEP-02 (planning-doc-staleness), which
# works at whole-file granularity. The bloat this targets lives INSIDE files that
# must themselves stay alive -- a 160 KB follow-ups file that every session is
# instructed to read, 39% of which is narration of finished work. No whole-file
# sweep will ever touch that.
#
# CRITICAL CONTRACT: the output is a WORKLIST, never a delete list. Every signal
# here is a proxy. A `DONE` marker measures that somebody typed DONE, not that
# the surrounding text stopped mattering -- and a trap note that prevents a
# repeat looks identical to a war story that does not. The agent judges each
# candidate against `segments/_retirement-test.md`. The oracle never deletes and
# never recommends deletion.
#
# ADVISORY: status is "pass" or "warn"; a real run ALWAYS exits 0. -SelfTest
# asserts each detector fires on a synthetic positive and stays quiet on a
# synthetic negative (exit 1 if not).

param([switch]$SelfTest)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$MaxFiles      = 200
$MaxFileBytes  = 1048576
$MaxCandidates = 400
$ExcerptChars  = 120

# Default corpus: living artifacts that accumulate provisional content. Roots are
# repo-relative; missing ones are skipped silently. Override for tests with
# CLAUDE_RETIREMENT_CORPUS (colon-separated globs).
# The follow-ups file gets extra globs, not one literal path. Dogfooding on a
# second project found an 82 KB `docs/pending-followups.md` -- the exact artifact
# class this oracle exists for -- invisible because the literal path did not
# match. PowerShell globs are case-INsensitive, so one extra pattern covers every
# spelling the bash twin needs two for; parity is on the RESULT SET, not the
# pattern list (TWIN-01..03).
$DefaultCorpus = @(
    'CLAUDE.md'
    'docs/PENDING_FOLLOW_UPS.md'
    'docs/*follow*up*.md'
    'docs/specs/DISCOVERIES.md'
    'docs/specs/OPEN_QUESTIONS.md'
    'docs/planning/deferred/*.md'
    'docs/pending/*.md'
    'docs/reviews/*.md'
    'NEXT_SESSION*.md'
    'docs/NEXT_SESSION*.md'
)

# Files whose content is provisional BY CONSTRUCTION -- an entry here with no
# declared exit condition is a CTXY-07 authoring gap, not a judgment call.
# Spelling-tolerant for the same reason as the corpus globs above. The explicit
# character classes are redundant under PowerShell -match (case-insensitive) and
# load-bearing in the bash twin (case-sensitive); kept byte-identical between the
# twins so a reader diffing them sees one pattern, not two.
$ProvisionalRe = '(docs/planning/deferred/|docs/pending/|[Pp][Ee][Nn][Dd][Ii][Nn][Gg][-_ ]?[Ff][Oo][Ll][Ll][Oo][Ww][-_ ]?[Uu][Pp]|NEXT_SESSION)'

# TWO STRENGTHS, and the distinction is load-bearing -- see run.sh for the full
# rationale. STRONG is an actual directive and is the ONLY thing that can be
# reported as SATISFIED (with a past date, outside a table). WEAK is a mention of
# the concept and can only ever be `declared`. Without the split, prose ABOUT
# exit conditions carrying any date fires as satisfied -- DEFER-044 records the
# identical defect in `governing-doc-consistency`.
$ExitStrongRe = '(^|[^A-Za-z])([Rr]emove|[Dd]elete|[Dd]rop|[Rr]etire|[Pp]rune)[^.]{0,60}(once|when|after|upon)'
$ExitWeakRe   = '([Ee]xit condition)|([Ss]uperseded when)'
$DoneRe      = '(^|[^A-Za-z])(DONE|COMPLETE|COMPLETED|FIXED|RESOLVED|SUPERSEDED|DISCHARGED|SHIPPED|LANDED|CLOSED)([^A-Za-z]|$)'
$CorrRe      = '^[ \t>*-]*[ \t]*(CORRECTION|UPDATE|PROGRESS|AMENDMENT|REVISION|ADDENDUM|POSTSCRIPT|POST-SCRIPT)([^A-Za-z]|$)'
$SupersedeRe = '(^|[^A-Za-z])(SUPERSEDED|TOMBSTONE|OBSOLETE|DEPRECATED)([^A-Za-z]|$)'
$ProvHeadRe  = '[Pp]ending [Ff]ollow-?[Uu]ps?'
$CheckMark   = [char]0x2705

$Today = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')

function Trunc([string]$s) {
    if ($null -eq $s) { return "" }
    if ($s.Length -gt $ExcerptChars) { return $s.Substring(0, $ExcerptChars) + "..." }
    return $s
}

# ---------------------------------------------------------------- Scan-File
# Returns an array of PSCustomObjects: Path, LineStart, LineEnd, Signal, Heading, Excerpt
function Scan-File {
    param([string]$RelPath, [string]$FullPath)

    $out = New-Object System.Collections.ArrayList
    $lines = @()
    try { $lines = [System.IO.File]::ReadAllLines($FullPath, [System.Text.UTF8Encoding]::new($false)) } catch { return $out }

    $bstart = 0; $bhead = ""
    $bExitSat = $false; $bExitDecl = $false; $bDone = $false; $bCorr = 0
    $bExitTxt = ""; $bDoneTxt = ""; $bCorrTxt = ""
    $fileHasExit = $false; $isProvFile = $false; $provLine = 0; $provHead = ""
    $supersedeSeen = $false; $supersedeLine = 0; $supersedeTxt = ""
    $inCode = $false

    function New-Rec($p, $ls, $le, $sig, $head, $exc) {
        [PSCustomObject]@{
            Path = $p; LineStart = $ls; LineEnd = $le
            Signal = $sig; Heading = $head; Excerpt = (Trunc $exc)
        }
    }

    $flush = {
        param($endline)
        if ($bstart -eq 0) { return }
        if ($bExitSat)        { [void]$out.Add((New-Rec $RelPath $bstart $endline "exit-condition-satisfied" $bhead $bExitTxt)) }
        elseif ($bExitDecl)   { [void]$out.Add((New-Rec $RelPath $bstart $endline "exit-condition-declared"  $bhead $bExitTxt)) }
        if ($bDone)           { [void]$out.Add((New-Rec $RelPath $bstart $endline "done-marker"              $bhead $bDoneTxt)) }
        if ($bCorr -ge 2)     { [void]$out.Add((New-Rec $RelPath $bstart $endline "correction-strata"        $bhead $bCorrTxt)) }
        $script:__reset = $true
    }

    for ($i = 0; $i -lt $lines.Count; $i++) {
        $ln = $i + 1
        $line = $lines[$i]

        # Fenced code blocks are never prose -- their markers are examples.
        if ($line -match '^[ \t]*(```|~~~)') { $inCode = -not $inCode; continue }
        if ($inCode) { continue }

        # self-supersession: a banner in the first 15 lines of a file that keeps going.
        if (-not $supersedeSeen -and (($ln -le 15 -and $line -cmatch $SupersedeRe) -or ($line -match 'kept for (provenance|lineage|history)'))) {
            $supersedeSeen = $true; $supersedeLine = $ln; $supersedeTxt = $line
        }

        # block boundaries (markdown headings, ANY level 1-6). Levels 2-4 was the
        # original range and it produced a SILENT FALSE PASS -- a file whose only
        # heading is a level-1 title never opened a block, so every content line
        # hit the `$bstart -eq 0` guard below and an 82 KB follow-ups file scanned
        # to zero candidates. Heading DEPTH is a formatting choice; it must not
        # decide whether text is scanned at all. See run.sh for the full note.
        if ($line -match '^#{1,6}[ \t]') {
            # inline flush (scriptblock scoping across PS engines is a trap -- do it here)
            if ($bstart -ne 0) {
                $endline = $ln - 1
                if ($bExitSat)      { [void]$out.Add((New-Rec $RelPath $bstart $endline "exit-condition-satisfied" $bhead $bExitTxt)) }
                elseif ($bExitDecl) { [void]$out.Add((New-Rec $RelPath $bstart $endline "exit-condition-declared"  $bhead $bExitTxt)) }
                if ($bDone)         { [void]$out.Add((New-Rec $RelPath $bstart $endline "done-marker"              $bhead $bDoneTxt)) }
                if ($bCorr -ge 2)   { [void]$out.Add((New-Rec $RelPath $bstart $endline "correction-strata"        $bhead $bCorrTxt)) }
            }
            $bExitSat = $false; $bExitDecl = $false; $bDone = $false; $bCorr = 0
            $bExitTxt = ""; $bDoneTxt = ""; $bCorrTxt = ""
            $bstart = $ln
            $bhead = $line -replace '^#+[ \t]*', ''
            if ($bhead -match $ProvHeadRe) { $isProvFile = $true; $provLine = $ln; $provHead = $bhead }
            continue
        }
        if ($bstart -eq 0) { continue }

        # exit conditions -- STRONG (directive) first
        if ($line -match $ExitStrongRe) {
            $fileHasExit = $true
            $d = ""
            $m = [regex]::Match($line, '[0-9]{4}-[0-9]{2}-[0-9]{2}')
            if ($m.Success) { $d = $m.Value }
            $isTable = ($line -match '^[ \t]*\|')
            # String comparison on ISO dates is correct and engine-independent --
            # deliberately NOT [datetime] parsing, which diverges between
            # powershell.exe 5.1 and pwsh 7 on ISO coercion (DISC-ARC-01).
            if ($d -ne "" -and ([string]::CompareOrdinal($d, $Today) -lt 0) -and -not $isTable) {
                $bExitSat = $true; $bExitTxt = $line
            } elseif (-not $bExitSat) { $bExitDecl = $true; $bExitTxt = $line }
            continue
        }
        # WEAK (mention of the concept) -- can never reach SATISFIED
        if ($line -match $ExitWeakRe) {
            $fileHasExit = $true
            if (-not $bExitSat -and -not $bExitDecl) { $bExitDecl = $true; $bExitTxt = $line }
            continue
        }

        # done markers -- uppercase tokens (case-SENSITIVE) and the check emoji only
        if ($line -cmatch $DoneRe) { if (-not $bDone) { $bDone = $true; $bDoneTxt = $line }; continue }
        if ($line.IndexOf($CheckMark) -ge 0) { if (-not $bDone) { $bDone = $true; $bDoneTxt = $line }; continue }

        # correction strata (CTXY-08)
        if ($line -cmatch $CorrRe) {
            $bCorr++
            if ($bCorrTxt -eq "") { $bCorrTxt = $line }
            continue
        }
    }

    # close the final block
    $last = $lines.Count
    if ($bstart -ne 0) {
        if ($bExitSat)      { [void]$out.Add((New-Rec $RelPath $bstart $last "exit-condition-satisfied" $bhead $bExitTxt)) }
        elseif ($bExitDecl) { [void]$out.Add((New-Rec $RelPath $bstart $last "exit-condition-declared"  $bhead $bExitTxt)) }
        if ($bDone)         { [void]$out.Add((New-Rec $RelPath $bstart $last "done-marker"              $bhead $bDoneTxt)) }
        if ($bCorr -ge 2)   { [void]$out.Add((New-Rec $RelPath $bstart $last "correction-strata"        $bhead $bCorrTxt)) }
    }

    # provisional-no-exit is a FILE-level verdict, not a per-heading one. A
    # deferred brief is one artifact with many headings; firing per heading turns
    # a 74-file scan into 312 "candidates" -- noise in a worklist costume.
    if ($RelPath -match $ProvisionalRe) {
        $isProvFile = $true
        if ($provLine -eq 0) { $provLine = 1; $provHead = "(whole file)" }
    }
    if ($isProvFile -and -not $fileHasExit) {
        [void]$out.Add((New-Rec $RelPath $provLine $last "provisional-no-exit" $provHead "provisional artifact with no declared exit condition (CTXY-07)"))
    }
    if ($supersedeSeen -and ($last - $supersedeLine) -ge 40) {
        [void]$out.Add((New-Rec $RelPath $supersedeLine $last "self-supersession" "(file head)" $supersedeTxt))
    }
    return $out
}

function ConvertTo-JsonString([string]$s) {
    if ($null -eq $s) { return "" }
    $s = $s -replace '\\', '\\\\'
    $s = $s -replace '"', '\"'
    $s = $s -replace "`t", ' '
    $s = $s -replace "`r", ''
    $s = $s -replace "`n", ' '
    return $s
}

# ------------------------------------------------------------------ self-test
if ($SelfTest) {
    $td = Join-Path ([System.IO.Path]::GetTempPath()) ("retcand-" + [System.Guid]::NewGuid().ToString('N'))
    $pos = Join-Path $td "docs/pending"
    New-Item -ItemType Directory -Force -Path $pos | Out-Null
    $positive = @"
# Sample

## Entry one
Remove this entry once the installer ships after 2020-01-01.
Body text.

## Entry two
Status: DONE -- the migration landed.

## Entry three
Head statement.
**CORRECTION**: actually it was the other module.
**UPDATE**: reverted again.

## Entry four
Body with no exit condition of its own.
"@
    # provisional-no-exit is FILE-level, so its positive sample must be a
    # provisional file with NO exit condition anywhere.
    $noExit = @"
# Deferred idea

## The idea
Ship a thing. Nothing here says what would make this brief deletable.
"@
    $clean = @"
# Clean doc

## A real trap
Calling flush() before the lock is taken corrupts the index. There is no
regression test for this yet, so this paragraph is the guard.

## Why we rejected the queue
It serialized the writer, which is the whole point of the module.
"@
    [System.IO.File]::WriteAllText((Join-Path $pos "positive.md"), $positive, [System.Text.UTF8Encoding]::new($false))
    [System.IO.File]::WriteAllText((Join-Path $pos "no-exit.md"),  $noExit,   [System.Text.UTF8Encoding]::new($false))
    [System.IO.File]::WriteAllText((Join-Path $td  "clean.md"),    $clean,    [System.Text.UTF8Encoding]::new($false))

    $hits = @()
    $hits += Scan-File "docs/pending/positive.md" (Join-Path $pos "positive.md")
    $hits += Scan-File "docs/pending/no-exit.md"  (Join-Path $pos "no-exit.md")
    $miss = @(Scan-File "clean.md" (Join-Path $td "clean.md"))

    Remove-Item -Recurse -Force $td -ErrorAction SilentlyContinue

    $need = @("exit-condition-satisfied", "done-marker", "correction-strata", "provisional-no-exit")
    $gotSignals = @($hits | ForEach-Object { $_.Signal })
    $missing = @($need | Where-Object { $gotSignals -notcontains $_ })
    $noise = @($miss).Count

    if ($missing.Count -eq 0 -and $noise -eq 0) {
        Write-Output '{"status":"pass","details":{"self_test":true,"detectors_fired":true,"quiet_on_clean":true},"briefing":"Self-test PASS: all four block detectors fire on a synthetic positive and stay silent on a clean trap/rationale doc."}'
        exit 0
    }
    Write-Output ('{"status":"fail","details":{"self_test":true,"missing_detectors":"' + ($missing -join ' ') + '","false_positives_on_clean":' + $noise + '},"briefing":"Self-test FAIL: retirement detectors are silently broken."}')
    exit 1
}

# ---------------------------------------------------------------------- main
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Push-Location $repoRoot
$sw = [System.Diagnostics.Stopwatch]::StartNew()

try {
    $globs = $DefaultCorpus
    if ($env:CLAUDE_RETIREMENT_CORPUS) { $globs = $env:CLAUDE_RETIREMENT_CORPUS -split ':' }

    $files = New-Object System.Collections.ArrayList
    foreach ($g in $globs) {
        if ([string]::IsNullOrWhiteSpace($g)) { continue }
        $matched = @(Get-ChildItem -Path $g -File -ErrorAction SilentlyContinue)
        foreach ($m in $matched) {
            if ($files.Count -ge $MaxFiles) { break }
            if ($m.Length -gt $MaxFileBytes) { continue }
            $rel = $m.FullName.Substring($repoRoot.Length).TrimStart('\', '/') -replace '\\', '/'
            [void]$files.Add([PSCustomObject]@{ Rel = $rel; Full = $m.FullName })
        }
    }

    $candidates = New-Object System.Collections.ArrayList
    $surfaced   = New-Object System.Collections.ArrayList

    foreach ($f in $files) {
        foreach ($rec in (Scan-File $f.Rel $f.Full)) {
            if ($rec.Signal -eq "exit-condition-declared") { [void]$surfaced.Add($rec); continue }
            if ($candidates.Count -ge $MaxCandidates) { break }
            [void]$candidates.Add($rec)
        }
    }

    # File-level: zero inbound references anywhere in the tree. ONE multi-pattern
    # pass, never one grep per file (the naive shape cost ~9s of pure process
    # overhead on a 74-file corpus in the bash twin).
    $basenames = @($files | ForEach-Object { Split-Path $_.Rel -Leaf } | Sort-Object -Unique)
    $referenced = @{}
    if ($basenames.Count -gt 0) {
        $inGit = $false
        try { git rev-parse --is-inside-work-tree 2>$null | Out-Null; $inGit = ($LASTEXITCODE -eq 0) } catch { $inGit = $false }
        $patArgs = @()
        foreach ($b in $basenames) { $patArgs += @('-e', $b) }
        $hits = @()
        if ($inGit) {
            $hits = @(& git grep -I -F -n @patArgs -- . 2>$null)
        } else {
            $hits = @(Get-ChildItem -Recurse -File -Include *.md, *.ps1, *.sh, *.json, *.rs, *.go, *.ts, *.js -ErrorAction SilentlyContinue |
                      Select-String -SimpleMatch -Pattern $basenames -ErrorAction SilentlyContinue |
                      ForEach-Object { "$($_.Path):$($_.LineNumber):$($_.Line)" })
        }
        foreach ($h in $hits) {
            $m = [regex]::Match($h, '^(.*?):[0-9]+:')
            if (-not $m.Success) { continue }
            $path = $m.Groups[1].Value
            $base = Split-Path $path -Leaf
            foreach ($b in $basenames) {
                if ($b -ne $base -and $h.IndexOf($b, [StringComparison]::Ordinal) -ge 0) { $referenced[$b] = $true }
            }
        }
    }
    foreach ($f in $files) {
        if ($f.Rel -eq 'CLAUDE.md' -or $f.Rel -like 'docs/specs/*') { continue }  # read by convention
        $base = Split-Path $f.Rel -Leaf
        if ($referenced.ContainsKey($base)) { continue }
        if ($candidates.Count -ge $MaxCandidates) { break }
        [void]$candidates.Add([PSCustomObject]@{
            Path = $f.Rel; LineStart = 1; LineEnd = 0; Signal = "no-inbound-refs"
            Heading = "(whole file)"; Excerpt = "no other tracked file mentions this filename"
        })
    }

    $bySignal = @{}
    foreach ($c in $candidates) {
        if ($bySignal.ContainsKey($c.Signal)) { $bySignal[$c.Signal]++ } else { $bySignal[$c.Signal] = 1 }
    }
    $bySignalJson = (($bySignal.Keys | Sort-Object | ForEach-Object { '"' + $_ + '":' + $bySignal[$_] }) -join ',')

    $candJson = (($candidates | ForEach-Object {
        '{"path":"' + (ConvertTo-JsonString $_.Path) + '","line_start":' + $_.LineStart +
        ',"line_end":' + $_.LineEnd + ',"signal":"' + $_.Signal +
        '","heading":"' + (ConvertTo-JsonString $_.Heading) + '","excerpt":"' + (ConvertTo-JsonString $_.Excerpt) + '"}'
    }) -join ',')

    $surfJson = (($surfaced | ForEach-Object {
        '{"path":"' + (ConvertTo-JsonString $_.Path) + '","line":' + $_.LineStart +
        ',"signal":"' + $_.Signal + '","heading":"' + (ConvertTo-JsonString $_.Heading) +
        '","excerpt":"' + (ConvertTo-JsonString $_.Excerpt) +
        '","note":"exit condition present but not machine-evaluable -- verify against the system, not against the entry''s own claim"}'
    }) -join ',')

    $sw.Stop()
    $dur = [int]$sw.ElapsedMilliseconds

    if ($candidates.Count -eq 0) {
        $status = "pass"; $briefing = ""
    } else {
        $status = "warn"
        $briefing = "RETIREMENT: $($candidates.Count) candidate passage(s) across $($files.Count) living artifact(s) carry a deterministic retirement signal. These are a WORKLIST, not a delete list -- judge each against segments/_retirement-test.md. Acted on by /0-uldf-finalize Phase 8.8; sweep with /0-uldf-context-audit --retire."
    }

    Write-Output ('{"status":"' + $status + '","details":{"files_scanned":' + $files.Count +
        ',"candidate_count":' + $candidates.Count + ',"surfaced_count":' + $surfaced.Count +
        ',"by_signal":{' + $bySignalJson + '},"scan_duration_ms":' + $dur +
        ',"contract":"worklist -- the oracle never deletes and never recommends deletion; every signal is a proxy for utility, which is not machine-visible"},"candidates":[' +
        $candJson + '],"surfaced":[' + $surfJson + '],"briefing":"' + (ConvertTo-JsonString $briefing) + '"}')
}
finally { Pop-Location }

exit 0
