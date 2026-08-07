# ldis-contexturgy oracle (Windows) -- PowerShell mirror of run.sh (CTXY-01,
# scrutiny 03 ADD-1). Code-verified Contexturgy: did recent LDIS invocations
# in THIS project leave crystallized planning artifacts? ADVISORY: status is
# pass|warn, NEVER fail. Output is byte-parity JSON with run.sh.
# READ-ONLY. NOTE: ASCII-only string literals (template convention).

$ErrorActionPreference = 'SilentlyContinue'

$windowHours = 24
if ($env:CLAUDE_CTXY_WINDOW_HOURS -match '^[0-9]+$') { $windowHours = [int]$env:CLAUDE_CTXY_WINDOW_HOURS }

function Esc([string]$s) {
    if ($null -eq $s) { return '' }
    return ($s -replace '\\', '\\' -replace '"', '\"')
}

function Emit([string]$Status, [int]$Checked, [string]$GapsJson, [string]$Note, [string]$Briefing) {
    Write-Output ('{"status":"' + $Status + '","details":{"checked":' + $Checked + ',"window_hours":' + $windowHours + ',"gaps":[' + $GapsJson + '],"note":"' + (Esc $Note) + '"},"briefing":"' + (Esc $Briefing) + '"}')
    exit 0
}

$projectName = Split-Path -Leaf (Get-Location).Path

# ---- Ledger resolution -------------------------------------------------------
$ledgerDir = $env:CLAUDE_USAGE_LEDGER_DIR
if (-not $ledgerDir) {
    if ((Test-Path 'claude-usage') -and @(Get-ChildItem 'claude-usage/*.jsonl' -ErrorAction SilentlyContinue).Count -gt 0) {
        $ledgerDir = 'claude-usage'
    } else {
        $homeDir = $env:USERPROFILE
        if (-not $homeDir) { $homeDir = $env:HOME }
        $cand = Join-Path $homeDir '.claude/command-usage'
        if ($homeDir -and @(Get-ChildItem (Join-Path $cand '*.jsonl') -ErrorAction SilentlyContinue).Count -gt 0) {
            $ledgerDir = $cand
        }
    }
}
if (-not $ledgerDir -or @(Get-ChildItem (Join-Path $ledgerDir '*.jsonl') -ErrorAction SilentlyContinue).Count -eq 0) {
    Emit 'pass' 0 '' 'NO-DATA: no command-usage ledger found (tracker hook not installed?); Contexturgy not verifiable this session' ''
}

# ---- Window start -- one per zone form (DEC-302 / LEDGER-ZONE-02) -----------
# The ledger's `at` is UTC-with-`Z` from DEC-302 onward and BARE (legacy LOCAL)
# before it; one file holds both across the transition. Compute the window once
# in each form and pick per row by the row's own marker. Before DEC-302 this was
# LOCAL-only, matching the `.ps1` producer by luck and diverging by a full offset
# from the `.sh` producer -- while usage-stats/review-recency assumed the other.
# InvariantCulture is mandatory: a custom format with no provider renders the
# CURRENT CULTURE'S CALENDAR (DEFER-122), which round-trips and so fails silently.
$ctxyInv = [System.Globalization.CultureInfo]::InvariantCulture
$windowStartLocal = [DateTime]::Now.AddHours(-$windowHours).ToString('yyyy-MM-ddTHH:mm:ss', $ctxyInv)
$windowStartUtc   = [DateTime]::UtcNow.AddHours(-$windowHours).ToString("yyyy-MM-ddTHH:mm:ss'Z'", $ctxyInv)

function Get-ArtifactHome([string]$Cmd) {
    if ($Cmd -match 'ldis-intake$') { return 'docs/planning/intakes' }
    if ($Cmd -match 'ldis-plan$')   { return 'docs/planning/plans' }
    if ($Cmd -match 'ldis-ideate$') { return 'docs/planning/ideations' }
    if ($Cmd -match 'ldis-spec$')   { return 'docs/specs' }
    return ''
}

$checked = 0
$gapParts = @()

foreach ($f in @(Get-ChildItem (Join-Path $ledgerDir '*.jsonl') -ErrorAction SilentlyContinue)) {
    $lines = @()
    try { $lines = [System.IO.File]::ReadAllLines($f.FullName) } catch { continue }
    foreach ($line in $lines) {
        if ($line -notmatch 'ldis-') { continue }
        if ($line -notmatch '"cmd"\s*:\s*"([^"]*)"') { continue }
        $cmd = $Matches[1]
        if ($cmd -notmatch 'ldis-(intake|plan|ideate|spec)$') { continue }
        if ($line -notmatch '"project"\s*:\s*"([^"]*)"') { continue }
        if ($Matches[1] -ne $projectName) { continue }
        if ($line -notmatch '"at"\s*:\s*"([^"]*)"') { continue }
        $at = $Matches[1]
        # Compare against the window in the row's OWN zone form (DEC-302).
        if ($at.EndsWith('Z')) {
            if ([string]::CompareOrdinal($at, $windowStartUtc) -lt 0) { continue }
        } elseif ([string]::CompareOrdinal($at, $windowStartLocal) -lt 0) { continue }

        $checked++
        # NOTE: $home is a PowerShell AUTOMATIC variable (user home; assignment
        # silently fails under SilentlyContinue and the stale value scans the
        # whole home dir) -- use $artHome. Same gotcha class as the param($Root)
        # case-collision documented in CLAUDE.md CSI build notes.
        $artHome = Get-ArtifactHome $cmd
        $found = $false
        if ($artHome -and (Test-Path $artHome -PathType Container)) {
            # ZONE (DEC-302 / LEDGER-ZONE-02) -- the SECOND zone coupling in this
            # oracle, and the one that decides the verdict rather than a window
            # edge: $_.LastWriteTime below is LOCAL, so `at` must be resolved to
            # LOCAL here. A marked row is UTC and converts; a bare row is legacy
            # LOCAL and must NOT. Made explicit because the old code reached the
            # right answer for a marked row only by ParseExact THROWING and the
            # `[datetime]` cast picking up the Z -- correct by accident, via an
            # exception path, one refactor away from silently breaking.
            # InvariantCulture: see the window note above (DEFER-122).
            $atDt = $null
            try {
                if ($at.EndsWith('Z')) {
                    $atDt = [datetime]::ParseExact($at, "yyyy-MM-ddTHH:mm:ss'Z'", $ctxyInv,
                            ([System.Globalization.DateTimeStyles]::AdjustToUniversal -bor `
                             [System.Globalization.DateTimeStyles]::AssumeUniversal)).ToLocalTime()
                } else {
                    $atDt = [datetime]::ParseExact($at, 'yyyy-MM-ddTHH:mm:ss', $ctxyInv)
                }
            } catch { }
            if ($null -eq $atDt) { try { $atDt = [datetime]$at } catch { } }
            if ($null -ne $atDt) {
                $filter = if ($artHome -eq 'docs/specs') { '*.md' } else { '*' }
                $newer = Get-ChildItem -Path $artHome -Recurse -File -Filter $filter -ErrorAction SilentlyContinue |
                    Where-Object { $_.LastWriteTime -ge $atDt } | Select-Object -First 1
                if ($newer) { $found = $true }
            } else {
                $found = $true   # unparseable timestamp: fail-open, never a false warn
            }
        }
        if (-not $found) {
            $gapParts += ('{"skill":"' + (Esc $cmd) + '","at":"' + (Esc $at) + '","expected":"' + (Esc $artHome) + '"}')
        }
    }
}

if ($gapParts.Count -gt 0) {
    Emit 'warn' $checked ($gapParts -join ',') '' ("ldis-contexturgy: " + $gapParts.Count + " LDIS invocation(s) in the last " + $windowHours + "h left no crystallized artifact (Ephemeral Planning?) -- advisory, crystallize before finalize")
}
if ($checked -eq 0) {
    Emit 'pass' 0 '' ("no LDIS invocations recorded for this project in the last " + $windowHours + "h") ''
}
Emit 'pass' $checked '' '' ''
