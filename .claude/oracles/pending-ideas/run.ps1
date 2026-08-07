# pending-ideas oracle (Windows) -- INJECT-08 (FROZEN SCHEMA)
#
# Byte-parallel with run.sh. Surfaces UN-TRIAGED (status: PROPOSED) items in
# docs/planning/deferred/ at session-start, and (DEC-281 / DEFER-109) PARKED
# items -- triaged, still open, deliberately not being built, with a stated
# re-arm condition -- on their OWN fields so the frozen PROPOSED contract does
# not move. Feeder-side visibility for /0-uldf-inject.
#
# Read the run.sh header for the full contract, including why PARKED does not
# join pending_count, why the re-arm condition is presence-checked and never
# evaluated (OVALID-02), and why there is deliberately no stale-PARKED signal.
#
# DEC-317 (DEFER-151), also in run.sh's header: the status WORD is the first
# token of the status line, not the whole line -- 28 of 160 briefs write a
# rationale after it and the old whole-line compare dropped every one of them in
# SILENCE. And a word outside the surfaced set is now REPORTED
# (unknown_status_count / unknown_status[]) rather than dropped, because a file
# this oracle silently drops is indistinguishable from a file that is not there.
# The surfaced SET is unchanged; the filter is not widened.
#
# Briefing constraint: the session-start fan-out extracts the briefing with a
# no-double-quote regex, so the briefing string MUST NOT contain a double-quote.
# JSON is hand-assembled (not ConvertTo-Json) to stay byte-identical with the
# bash oracle. ASCII-only strings (PW-005 lineage: no em-dashes).

$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$deferredDir = "docs/planning/deferred"

function Esc([string]$s) {
    if ($null -eq $s) { return "" }
    return ($s -replace '\\','\\' -replace '"','\"')
}
# Double-quote-free, single-line briefing text.
function BriefSafe([string]$s) {
    if ($null -eq $s) { return "" }
    $t = $s -replace '"',''
    $t = $t -replace "[`r`n`t]",' '
    $t = $t -replace ' +',' '
    return $t.Trim()
}

# Status words that mean DONE -- correctly invisible, and kept OUT of the
# unknown-status clause so the signal cannot drown in 130 lines of noise.
# Byte-parallel with run.sh's KNOWN_DONE_STATUSES -- read that comment for the
# census the set is derived from, and for why re-deriving beats editing by hand.
$KnownDoneStatuses = @('RESOLVED','RESOLVED-DECLINED','IMPLEMENTED','PHASE-1-IMPLEMENTED','COMPLETED','TRIAGED','IN-PROGRESS','DISMISSED','CLOSED','SUPERSEDED','DECLINED','APPLIED','FOLDED')

# The leading token of a status line, uppercased. Cuts at the first character
# outside [A-Za-z0-9_-]: handles "PARKED 2026-08-06 -- ...",
# "IMPLEMENTED--2026-07-12" and "COMPLETED (2026-07-02 ...)" alike, while
# keeping RESOLVED-DECLINED / IN-PROGRESS whole.
function Get-StatusWord([string]$raw) {
    if ([string]::IsNullOrEmpty($raw)) { return "" }
    $t = $raw.TrimStart()
    if ($t -match '^([A-Za-z0-9_-]*)') { return $matches[1].ToUpper() }
    return ""
}

# The re-arm condition when it was written into the status rationale rather
# than a `re-arm:` front-matter field. Presence only, never evaluated
# (OVALID-02) -- so the oracle stops SAYING "no re-arm condition declared"
# about a brief that declared one.
function Get-RearmFromStatus([string]$raw) {
    if ([string]::IsNullOrEmpty($raw)) { return "" }
    # The em-dash is written as the six-character .NET regex escape \u2014, NOT as a
    # literal character: the PS1 encoding lint rejects non-ASCII inside a string
    # literal (and PW-005 makes a literal risky under the OEM code page anyway).
    # Both engines resolve it identically because it is a REGEX feature, not a
    # PS6+ string feature -- so 5.1 handles it too. Behaviour is unchanged; the
    # class still matches ASCII hyphen, colon and em-dash. Do NOT 'simplify' the
    # escape to a literal dash: that silently stops matching em-dash rationales,
    # which is most of the corpus, and run.sh's twin would diverge in silence.
    if ($raw -match '[Rr][Ee]-[Aa][Rr][Mm]\s*[-:\u2014]*\s*(.+)$') { return $matches[1].Trim() }
    return ""
}

# Graceful absence: no deferred dir at all (NOT no-data).
if (-not (Test-Path -LiteralPath $deferredDir -PathType Container)) {
    Write-Output '{"pending_count":0,"items":[],"parked_count":0,"parked":[],"unknown_status_count":0,"unknown_status":[],"no_data":false,"briefing":""}'
    exit 0
}

# Unreadable dir -> NO-DATA.
$files = $null
try {
    $files = Get-ChildItem -LiteralPath $deferredDir -Filter 'DEFER-*.md' -File -ErrorAction Stop
} catch {
    Write-Output '{"pending_count":0,"items":[],"parked_count":0,"parked":[],"unknown_status_count":0,"unknown_status":[],"no_data":true,"briefing":"docs/planning/deferred/ present but unreadable -- NO-DATA (cannot confirm pending ideas)"}'
    exit 0
}

# fm_field: value of a front-matter `key:` line within the leading ---..--- block.
function Get-FmField([string[]]$lines, [string]$key) {
    if ($lines.Count -eq 0) { return "" }
    if ($lines[0].TrimEnd() -ne '---') { return "" }
    for ($i = 1; $i -lt $lines.Count; $i++) {
        $line = $lines[$i] -replace "`r$",''
        if ($line.TrimEnd() -eq '---') { break }
        if ($line -match ("^" + [regex]::Escape($key) + "\s*:\s*(.*)$")) {
            return $matches[1].Trim()
        }
    }
    return ""
}

$itemsJson = New-Object System.Collections.Generic.List[string]
$count = 0
$malformed = 0
$briefingItems = New-Object System.Collections.Generic.List[string]
$parkedJson = New-Object System.Collections.Generic.List[string]
$parkedCount = 0
$parkedBriefing = New-Object System.Collections.Generic.List[string]
$unknownJson = New-Object System.Collections.Generic.List[string]
$unknownCount = 0
$unknownBriefing = New-Object System.Collections.Generic.List[string]

foreach ($f in $files) {
    $lines = @()
    # -Encoding UTF8 is load-bearing, not decoration (PW-005 class). Windows
    # PowerShell 5.1's Get-Content defaults to the ANSI code page, so a brief
    # title carrying an em-dash came back as mojibake and this twin diverged from
    # run.sh on the REAL corpus (measured 2026-08-06 on DEFER-017's title:
    # bash/pwsh7 emitted the em-dash, 5.1 emitted a 3-char garble). pwsh 7
    # already defaults to UTF-8, so this is a no-op there and a fix on 5.1.
    # The defect predates DEC-317 and was invisible because every self-test
    # fixture is pure ASCII -- a fixture LACKING a property the real tree has.
    try { $lines = Get-Content -LiteralPath $f.FullName -Encoding UTF8 -ErrorAction Stop } catch { $malformed++; continue }

    $statusRaw = Get-FmField $lines 'status'
    if (-not $statusRaw) {
        $legacy = $lines | Where-Object { $_ -match '^\*\*Status\*\*:' } | Select-Object -First 1
        if ($legacy) { $statusRaw = ($legacy -replace "`r$",'' -replace '^\*\*Status\*\*:\s*','') }
    }
    # DEC-317: the WORD, not the line.
    $status = Get-StatusWord $statusRaw

    if (-not $status) { $malformed++; continue }

    # PARKED, handled BEFORE the PROPOSED filter and on its own fields.
    if ($status -eq 'PARKED') {
        $pbase = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
        $pid_ = ''
        if ($pbase -match '^(DEFER-[0-9]+)') { $pid_ = $matches[1] }
        if (-not $pid_) { $pid_ = Get-FmField $lines 'id' }
        if (-not $pid_) { $pid_ = $pbase }
        $ptitle = Get-FmField $lines 'title'
        if (-not $ptitle) {
            $ph = $lines | Where-Object { $_ -match '^#\s+DEFER-[0-9]+:' } | Select-Object -First 1
            if ($ph) { $ptitle = ($ph -replace "`r$",'' -replace '^#\s+DEFER-[0-9]+:\s*','') }
        }
        if (-not $ptitle) { $ptitle = $pbase }
        # PRESENCE only -- never evaluated (OVALID-02).
        $rearm = Get-FmField $lines 're-arm'
        if (-not $rearm) { $rearm = Get-FmField $lines 're_arm' }
        if (-not $rearm) { $rearm = Get-RearmFromStatus $statusRaw }

        $parkedJson.Add('{"id":"' + (Esc $pid_) + '","title":"' + (Esc $ptitle) + '","re_arm":"' + (Esc $rearm) + '"}')
        $parkedCount++
        $pfrag = "$pid_ " + (BriefSafe $ptitle)
        if ($rearm) { $pfrag = $pfrag + " (re-arm: " + (BriefSafe $rearm) + ")" }
        else        { $pfrag = $pfrag + " (no re-arm condition declared)" }
        $parkedBriefing.Add($pfrag)
        continue
    }

    # Words outside the surfaced set (DEC-317): done-words stay silently
    # invisible; anything else is REPORTED, never promoted into pending_count.
    if ($status -ne 'PROPOSED') {
        if ($KnownDoneStatuses -contains $status) { continue }
        $ubase = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
        $uid = ''
        if ($ubase -match '^(DEFER-[0-9]+)') { $uid = $matches[1] }
        if (-not $uid) { $uid = Get-FmField $lines 'id' }
        if (-not $uid) { $uid = $ubase }
        $unknownJson.Add('{"id":"' + (Esc $uid) + '","status":"' + (Esc $status) + '"}')
        $unknownCount++
        if ($unknownCount -le 3) {
            $unknownBriefing.Add((BriefSafe $uid) + " (" + (BriefSafe $status) + ")")
        }
        continue
    }

    $base = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
    # id: filename-derived, then front-matter `id:`, then the basename. The
    # basename leg is DEC-317 -- a slug-form brief previously emitted "" into
    # INJECT-08's frozen id field, an empty key a consumer joins on silently.
    $id = ""
    if ($base -match '^(DEFER-[0-9]+)') { $id = $matches[1] }
    if (-not $id) { $id = Get-FmField $lines 'id' }
    if (-not $id) { $id = $base }

    $title = Get-FmField $lines 'title'
    if (-not $title) {
        $h = $lines | Where-Object { $_ -match '^#\s+DEFER-[0-9]+:' } | Select-Object -First 1
        if ($h) { $title = ($h -replace "`r$",'' -replace '^#\s+DEFER-[0-9]+:\s*','') }
    }
    if (-not $title) { $title = $base }

    $origin = Get-FmField $lines 'origin'
    if (-not $origin) { $origin = 'defer-local' }

    $injectedAt = Get-FmField $lines 'injected-at'
    if (-not $injectedAt) { $injectedAt = Get-FmField $lines 'injected_at' }

    $sourceProject = Get-FmField $lines 'source-project'
    if (-not $sourceProject) { $sourceProject = Get-FmField $lines 'source_project' }

    $itemsJson.Add('{"id":"' + (Esc $id) + '","title":"' + (Esc $title) + '","origin":"' + (Esc $origin) + '","injected_at":"' + (Esc $injectedAt) + '","source_project":"' + (Esc $sourceProject) + '"}')
    $count++

    $frag = "$id " + (BriefSafe $title)
    if ($origin -eq 'inject' -and $sourceProject) {
        $frag = $frag + " (inject <- " + (BriefSafe $sourceProject) + ")"
    }
    $briefingItems.Add($frag)
}

$noData = 'false'
$briefing = ""

if ($count -gt 0) {
    $plural = if ($count -gt 1) { 'ideas' } else { 'idea' }
    $briefing = "$count pending $plural`: " + ($briefingItems -join '; ')
    $briefing = $briefing -replace '`',''
} elseif ($parkedCount -eq 0 -and $unknownCount -eq 0 -and $malformed -gt 0) {
    $noData = 'true'
    $briefing = "docs/planning/deferred/ has $malformed unparseable item(s) -- NO-DATA (cannot confirm pending ideas)"
}

# PARKED rides its own clause. Graceful absence is now "nothing PROPOSED and
# nothing PARKED" -- the ONE deliberately widened INJECT-08 clause (DEC-281).
if ($parkedCount -gt 0) {
    $pplural = if ($parkedCount -gt 1) { 'items' } else { 'item' }
    $pclause = "$parkedCount parked $pplural (triaged, open, not being built)`: " + ($parkedBriefing -join '; ')
    $pclause = $pclause -replace '`',''
    if ($briefing) { $briefing = "$briefing; $pclause" } else { $briefing = $pclause }
}

# Unrecognized status words ride their own clause (DEC-317) -- the second
# deliberate widening of INJECT-08's graceful-absence clause, named as such.
# pending_count/items[] are untouched.
if ($unknownCount -gt 0) {
    $uplural = if ($unknownCount -gt 1) { 'items' } else { 'item' }
    $uclause = "$unknownCount $uplural with an unrecognized status word (not surfaced; surfaced words are PROPOSED and PARKED)`: " + ($unknownBriefing -join '; ')
    if ($unknownCount -gt 3) { $uclause = $uclause + "; +" + ($unknownCount - 3) + " more" }
    $uclause = $uclause -replace '`',''
    if ($briefing) { $briefing = "$briefing; $uclause" } else { $briefing = $uclause }
}

# The NO-DATA note trails whatever was surfaced, so it can never displace it.
if ($malformed -gt 0 -and $briefing -and $noData -eq 'false') {
    $noData = 'true'
    $briefing = "$briefing; $malformed unparseable item(s) -- NO-DATA on those"
}

$itemsStr = ($itemsJson -join ',')
$parkedStr = ($parkedJson -join ',')
$unknownStr = ($unknownJson -join ',')
Write-Output ('{"pending_count":' + $count + ',"items":[' + $itemsStr + '],"parked_count":' + $parkedCount + ',"parked":[' + $parkedStr + '],"unknown_status_count":' + $unknownCount + ',"unknown_status":[' + $unknownStr + '],"no_data":' + $noData + ',"briefing":"' + (Esc $briefing) + '"}')
exit 0
