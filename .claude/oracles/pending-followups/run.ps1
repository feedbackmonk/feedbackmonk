# pending-followups oracle (Windows PowerShell)
# Parses 'Pending Follow-Ups' sections and identifies overdue items.
# Sources (merged, additive "scope" field on each item):
#   1. Project CLAUDE.md (scope "project") -- as always.
#   2. An EXTRACTED follow-ups file the CLAUDE.md section points at, or one
#      named by .claude/config.json `pendingFollowups.path` (scope "project").
#      Projects legitimately move this content out of CLAUDE.md to keep
#      auto-loaded context lean, leaving a pointer paragraph behind (DEC-346).
#   3. ~/.claude/MACHINE_CONFIG.md (scope "global") -- machine-global reminders
#      written by /0-uldf-schedule --scope=global. MACHINE_CONFIG.md is the
#      sync-survivor: ~/.claude/CLAUDE.md is overwritten by every framework
#      sync, so reminders there would silently vanish (scrutiny 07 F3).
#
# NO-DATA IS NEVER ZERO (DEC-346). Before this oracle followed pointers it
# reported {"has_followups_section":true,"total":0} against a project with ~80
# live entries -- it asserted it had FOUND the section and then reported nothing
# pending, which is worse than a plain miss. `status` now separates the three
# cases a bare `total: 0` used to collapse:
#   ok      -- the sources were read and the count is trustworthy
#   no-data -- a section/pointer exists that we could NOT turn into entries
# A consumer must not read total==0 as an all-clear unless status=="ok".
#
# Parity contract: this file and run.sh must agree on every case in
# scripts/smoke-tests/oracle-manifest-admission-smoke.sh (TWIN-01).

$ErrorActionPreference = "Continue"
# Force UTF-8 I/O so non-ASCII content (em-dashes, curly quotes) survives round-tripping.
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# TWIN-01: date-only, to match run.sh, which compares `date -d <YYYY-MM-DD>`
# epochs (both midnight). With the wall clock left in, an entry due TODAY reads
# overdue here and not-overdue there, and every `days_overdue` is a fraction.
$today = (Get-Date).Date
$script:items = @()
$script:overdueCount = 0
# FOLLOWUP-02: entries that DECLARE a date trigger whose date this oracle could
# not turn into a comparison. Rendered `overdue:null` -- never `false`
# (CSI-36/DEC-342: unknown is not a verdict). They make `overdue` a floor rather
# than a magnitude (QUIESCE-09's shape).
$script:unevaluableCount = 0
# The single most-overdue date, for the briefing line. Dates and integers only:
# nothing copied out of an entry title ever reaches the briefing string.
$script:oldestDue = ""
$script:oldestDays = 0
$script:hasSection = $false
$script:sources = @()
$script:unresolved = @()
# Non-blank, non-heading lines seen inside a parsed section. A section that
# carries content but yields no entries is the false-all-clear shape.
$script:contentLines = 0
$script:foundPointer = ""

# Echoes a relative *.md path referenced by the line, or "".
# Per-item "Details: `docs/pending/<slug>.md`" tokens are stripped first: those
# name one entry's detail body, not an extracted follow-ups file, and following
# them would recurse into every entry.
function Get-PointerCandidate {
    param([string]$Line)
    $clean = [regex]::Replace($Line, 'Details:\s*`[^`]*`', '')
    if ($clean -match 'https?://') { return "" }
    $m = [regex]::Match($clean, '\]\(([^\)\s]+\.md)\)')
    if ($m.Success) { return $m.Groups[1].Value }
    $m = [regex]::Match($clean, '`([^`]+\.md)`')
    if ($m.Success) { return $m.Groups[1].Value }
    $m = [regex]::Match($clean, '([A-Za-z0-9_][A-Za-z0-9_./-]*\.md)')
    if ($m.Success) { return $m.Groups[1].Value }
    return ""
}

function Parse-SectionText {
    param([string]$Text, [string]$ScopeLabel, [int]$Collect)

    $lines = $Text -split "`n"
    foreach ($line in $lines) {
        $line = $line -replace "`r$", ""
        $matchedItem = $false

        $trimmed = $line.Trim()
        if ($trimmed.Length -gt 0 -and -not $trimmed.StartsWith("#")) {
            $script:contentLines++
        }

        # Extract "Details: `docs/pending/<slug>.md`" pointer if present (P2 externalization).
        $detailPath = $null
        $detailMatch = [regex]::Match($line, 'Details:\s+`(docs/pending/[^\s`]+\.md)`')
        if ($detailMatch.Success) {
            $detailPath = $detailMatch.Groups[1].Value
        }

        # FOLLOWUP-01: a date TRIGGER is DECLARED by the date opening the bold
        # prefix, optionally behind a single leading word (`After`, `On`, `By`,
        # ...). A declaration test, not an enumeration of trigger words (DEC-343).
        # Anything is allowed between the date and the closing `**` -- which is
        # exactly what /0-uldf-schedule mandates: `**After YYYY-MM-DD ({title})**`.
        #
        # Position is load-bearing. Measured over 15 projects: 61 bold prefixes
        # carry a YYYY-MM-DD somewhere and only 2 declare a trigger this way; the
        # other 59 are status labels merely MENTIONING a date. They are not dated
        # entries, so they keep falling to the label branch below -- an unknown
        # may only be emitted where a real question existed (DEC-379).
        #
        # Day/month are matched loosely (\d{1,2}) on purpose, then validated
        # strictly: a prefix reading `**After 2026-8-12 (...)**` HAS declared a
        # trigger, just not in the mandated form, and must surface as a visible
        # unknown rather than a silent "not due".
        #
        # The title is taken by a SEPARATE strip rather than a capture group so
        # that it does not depend on the shape of the prefix (parity with run.sh's
        # sed, TWIN-01).
        $dateMatch = [regex]::Match($line, '^-\s+\*\*(?:[A-Za-z]+\s+)?(\d{4}-\d{1,2}-\d{1,2})([^*]*)\*\*')
        if ($dateMatch.Success) {
            $matchedItem = $true
            $due = $dateMatch.Groups[1].Value
            $title = [regex]::Replace($line, '^-\s+\*\*[^*]+\*\*:?\s*', '')
            if ($title.Length -gt 120) { $title = $title.Substring(0, 120) }

            $isOverdue = $false
            $daysOverdue = 0
            $evaluable = $true
            # Strict ISO is the only form we will compute against. Checked here
            # rather than left to [DateTime]::Parse, which accepts `2026-8-12`
            # and would silently disagree with run.sh's `case` guard.
            if ($due -notmatch '^\d{4}-\d{2}-\d{2}$') {
                $evaluable = $false
            } else {
                try {
                    $dueDate = [DateTime]::ParseExact($due, 'yyyy-MM-dd', [System.Globalization.CultureInfo]::InvariantCulture)
                    if ($today -gt $dueDate) {
                        $isOverdue = $true
                        # [Math]::Floor, never [int]: PowerShell's [int] cast is
                        # BANKER'S rounding ([int]7.65 -> 8, [int]2.5 -> 2), so it
                        # disagrees with run.sh's integer division. Measured
                        # 2026-08-19: bash 7d vs ps1 8d for the same entry.
                        $daysOverdue = [int][Math]::Floor(($today - $dueDate).TotalDays)
                        $script:overdueCount++
                        if ([string]::IsNullOrEmpty($script:oldestDue) -or $daysOverdue -gt $script:oldestDays) {
                            $script:oldestDue = $due
                            $script:oldestDays = $daysOverdue
                        }
                    }
                } catch { $evaluable = $false }
            }

            if (-not $evaluable) {
                $script:unevaluableCount++
                $script:items += [ordered]@{
                    title = $title
                    due = $due
                    overdue = $null
                    days_overdue = $null
                    detail_path = $detailPath
                    scope = $ScopeLabel
                }
            } else {
                $script:items += [ordered]@{
                    title = $title
                    due = $due
                    overdue = $isOverdue
                    days_overdue = $daysOverdue
                    detail_path = $detailPath
                    scope = $ScopeLabel
                }
            }
        }
        else {
            # Non-date label
            $labelMatch = [regex]::Match($line, '^-\s+\*\*([^*]+)\*\*:?\s*(.*)')
            if ($labelMatch.Success) {
                $matchedItem = $true
                $label = $labelMatch.Groups[1].Value
                $title = $labelMatch.Groups[2].Value
                if ($title.Length -gt 120) { $title = $title.Substring(0, 120) }
                $script:items += [ordered]@{
                    title = $title
                    due = $label
                    overdue = $false
                    days_overdue = 0
                    detail_path = $detailPath
                    scope = $ScopeLabel
                }
            }
        }

        if ($Collect -eq 1 -and -not $matchedItem -and [string]::IsNullOrEmpty($script:foundPointer)) {
            $cand = Get-PointerCandidate -Line $line
            if (-not [string]::IsNullOrEmpty($cand)) { $script:foundPointer = $cand }
        }
    }
}

# Returns the file's "Pending Follow-Ups" section body, or "" when absent.
function Get-FollowupSection {
    param([string]$Content)
    $sectionMatch = [regex]::Match($Content, '(?ms)^## Pending Follow-?[Uu]ps\s*$(.*?)(?=^## |\z)')
    if ($sectionMatch.Success) { return $sectionMatch.Groups[1].Value }
    return ""
}

function Parse-FollowupFile {
    param([string]$Path, [string]$ScopeLabel, [int]$Collect = 0)

    if (-not (Test-Path $Path)) { return }
    $content = Get-Content $Path -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (-not $content) { return }

    $section = Get-FollowupSection -Content $content
    if ([string]::IsNullOrEmpty($section)) {
        # No heading: an extracted follow-ups file may be a bare list. Never do
        # this for CLAUDE.md / MACHINE_CONFIG.md, whose bodies are full of
        # unrelated bullets -- only for a file we were explicitly pointed at.
        if ($Collect -eq 2) { $section = $content } else { return }
        if ([string]::IsNullOrEmpty($section)) { return }
    }
    else {
        $script:hasSection = $true
    }
    $script:sources += $Path
    Parse-SectionText -Text $section -ScopeLabel $ScopeLabel -Collect $Collect
}

# --- Source 1: project CLAUDE.md (and the file it points at) ------------------
$projectMd = ""
if (Test-Path "CLAUDE.md") { $projectMd = "CLAUDE.md" }
elseif (Test-Path ".claude/CLAUDE.md") { $projectMd = ".claude/CLAUDE.md" }

if ($projectMd -ne "") { Parse-FollowupFile -Path $projectMd -ScopeLabel "project" -Collect 1 }

# --- Source 2: the extracted follow-ups file ---------------------------------
# Explicit config wins over a pointer discovered in the section.
$configured = ""
if (Test-Path ".claude/config.json") {
    try {
        $cfg = Get-Content ".claude/config.json" -Raw -Encoding UTF8 -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        if ($cfg.pendingFollowups -and $cfg.pendingFollowups.path) {
            $configured = [string]$cfg.pendingFollowups.path
        }
    } catch { }
}
if ($configured -ne "") { $script:foundPointer = $configured }

if ((-not [string]::IsNullOrEmpty($script:foundPointer)) -and ($script:foundPointer -ne $projectMd)) {
    if (Test-Path $script:foundPointer) {
        Parse-FollowupFile -Path $script:foundPointer -ScopeLabel "project" -Collect 2
    } else {
        $script:unresolved += $script:foundPointer
    }
}

# --- Source 3: machine-global reminders (graceful absence) -------------------
$machineConfig = Join-Path $env:USERPROFILE ".claude\MACHINE_CONFIG.md"
Parse-FollowupFile -Path $machineConfig -ScopeLabel "global" -Collect 0

if (-not $script:hasSection -and $script:items.Count -eq 0) {
    # Graceful absence: an empty `briefing` suppresses the session-start line
    # entirely (the documented convention), so a project with no follow-ups
    # costs nothing.
    $empty = [ordered]@{
        briefing = ""
        has_followups_section = $false
        status = "ok"
        total = 0
        overdue = 0
        overdue_evaluable = $true
        unevaluable = 0
        items = @()
        sources = @($script:sources)
        unresolved = @($script:unresolved)
    }
    $empty | ConvertTo-Json -Compress -Depth 4
    exit 0
}

# NO-DATA is never zero: either a pointer we could not read, or a section that
# carried content and yielded no entries (content lives somewhere we cannot see).
$status = "ok"
if ($script:unresolved.Count -gt 0) { $status = "no-data" }
elseif ($script:items.Count -eq 0 -and $script:contentLines -gt 0) { $status = "no-data" }

# FOLLOWUP-02: the briefing line.
#
# Until now this oracle emitted NO `briefing` field, so session-start fell back
# to dumping the whole JSON as the line -- and dropped it silently whenever that
# exceeded MAX_CHARS (3000). Measured 2026-08-19: this repo 3824 chars, a
# sibling project 75081. So on exactly the projects that HAVE follow-ups the
# line never appeared at all, with no `deferred`/`killed` note to say so. A
# check that goes dark is worse than one plainly absent (OVALID-09).
#
# The string carries no `"` and nothing copied out of the corpus -- the bash
# consumer extracts it with `"briefing"..."[^"]*"`, so one quote inside an entry
# title would truncate the line. Dates and counts only. `briefing` is emitted
# FIRST so an extractor's `head -1` can never pick up a same-named token from an
# item body.
$overdueEvaluable = ($script:unevaluableCount -eq 0)

if ($status -eq "no-data") {
    $briefing = "NO-DATA -- a follow-up source exists that could not be turned into entries; total=$($script:items.Count) is NOT an all-clear"
} elseif ($script:overdueCount -gt 0) {
    $briefing = "$($script:items.Count) follow-ups, $($script:overdueCount) OVERDUE (oldest $($script:oldestDue), $($script:oldestDays)d past)"
} else {
    $briefing = "$($script:items.Count) follow-ups, none date-overdue"
}
if ($script:unevaluableCount -gt 0) {
    $briefing = "$briefing; $($script:unevaluableCount) declare a date this oracle cannot read -- the overdue count is a FLOOR, not a verdict"
}

$result = [ordered]@{
    briefing = $briefing
    has_followups_section = $true
    status = $status
    total = $script:items.Count
    overdue = $script:overdueCount
    overdue_evaluable = $overdueEvaluable
    unevaluable = $script:unevaluableCount
    items = @($script:items)
    sources = @($script:sources)
    unresolved = @($script:unresolved)
}

$result | ConvertTo-Json -Compress -Depth 5
