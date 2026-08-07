# trajectory-cap oracle (Windows PowerShell)
#
# Verification Oracle (kind: verification). Auto-discovered by /0-uldf-finalize
# Phase 1a sec 1a.6. Detects when docs/PROJECT_TRAJECTORY.md has breached its
# documented size caps -- the signature of a Phase 12 (TRAJECTORY-02) run that
# APPENDED instead of reshaping. Phase 12's caps are prose-only (agent-executed),
# so a run that skipped pruning accumulates silently; this makes the breach
# deterministic and every-finalize.
#
# Four signals (any one => warn):
#   - total lines  > max_lines      (accumulation)
#   - total bytes  > max_bytes      (accumulation by content weight)
#   - longest line > max_line_chars (the giant-single-line append -- a line-count
#                                    check alone MISSES this; SessionHelm was only
#                                    173 lines with a 57k-char single line)
#   - mojibake markers present      (UTF-8/CP1252 round-trip corruption on write)
#
# Output: JSON ({status, details, briefing}). ADVISORY: status is "pass" or
# "warn" and the script ALWAYS exits 0 on a real run -- the breach is
# self-correcting within the same /0-uldf-finalize via the Phase 12 REPAIR path,
# so blocking the commit here would prevent the repair from running. -SelfTest
# asserts the detector fires on a synthetic bloated sample (exit 1 if it does not).

param([switch]$SelfTest)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# --- Caps (mirror oracle.json config; edit here to change scope) ---
$Target       = "docs/PROJECT_TRAJECTORY.md"
$MaxLines     = 250
$MaxBytes     = 32768
$MaxLineChars = 2000

# Mojibake markers: "â€" (U+00E2 U+20AC) and lone "Ã" (U+00C3) -- strong
# signals of a UTF-8<->CP1252 round-trip.
$mojiPattern = "$([char]0x00E2)$([char]0x20AC)|$([char]0x00C3)"

function Get-Analysis {
    param([string]$Path)
    $bytes = (Get-Item -LiteralPath $Path).Length
    # Read line-wise without trailing-newline ambiguity.
    $lines = [System.IO.File]::ReadAllLines($Path, [System.Text.UTF8Encoding]::new($false))
    $lineCount = $lines.Count
    $longest = 0; $longestNo = 0; $over = 0; $moji = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $len = $lines[$i].Length
        if ($len -gt $longest) { $longest = $len; $longestNo = $i + 1 }
        if ($len -gt $MaxLineChars) { $over++ }
        if ([regex]::IsMatch($lines[$i], $mojiPattern)) { $moji++ }
    }
    $violations = New-Object System.Collections.Generic.List[string]
    if ($lineCount -gt $MaxLines)   { $violations.Add("lines:$lineCount>$MaxLines") }
    if ($bytes -gt $MaxBytes)       { $violations.Add("bytes:$bytes>$MaxBytes") }
    if ($longest -gt $MaxLineChars) { $violations.Add("longest_line:$longest>$MaxLineChars(line $longestNo)") }
    if ($moji -gt 0)                { $violations.Add("mojibake:$moji line(s)") }
    [pscustomobject]@{
        lines = $lineCount; bytes = $bytes; longest = $longest; longestNo = $longestNo
        over = $over; moji = $moji; violations = $violations
    }
}

if ($SelfTest) {
    $bad = [System.IO.Path]::GetTempFileName()
    $ok  = [System.IO.Path]::GetTempFileName()
    $enc = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($bad, "# Project Trajectory`n`n## Current Focus`n`n" + ('x' * 60000) + "`n", $enc)
    [System.IO.File]::WriteAllText($ok,  "# Project Trajectory`n`n## Current Focus`n`nShort and clean.`n", $enc)
    $badA = Get-Analysis -Path $bad
    $okA  = Get-Analysis -Path $ok
    Remove-Item $bad, $ok -ErrorAction SilentlyContinue
    $firedOnBloat  = $badA.violations.Count -gt 0
    $quietOnClean  = $okA.violations.Count -eq 0
    $pass = $firedOnBloat -and $quietOnClean
    [pscustomobject]@{
        status   = if ($pass) { "pass" } else { "fail" }
        details  = @{ self_test = $true; detector_fired_on_bloat = $firedOnBloat; detector_quiet_on_clean = $quietOnClean }
        briefing = if ($pass) { "Self-test PASS: detector flags a giant-single-line sample and stays quiet on a clean one." } else { "Self-test FAIL: cap detector is silently broken." }
    } | ConvertTo-Json -Depth 5
    if ($pass) { exit 0 } else { exit 1 }
}

$started = Get-Date
$repoRoot = (Resolve-Path "$PSScriptRoot/../../..").Path
$f = Join-Path $repoRoot $Target

if (-not (Test-Path -LiteralPath $f)) {
    [pscustomobject]@{
        status  = "pass"
        details = @{ file_exists = $false; path = $Target; scan_duration_ms = [int]((Get-Date) - $started).TotalMilliseconds }
        briefing = "No $Target -- nothing to check."
    } | ConvertTo-Json -Depth 5
    exit 0
}

$a = Get-Analysis -Path $f
$ok = ($a.violations.Count -eq 0)
$briefing = if ($ok) {
    "Within caps: $($a.lines) lines, $($a.bytes) bytes, longest line $($a.longest) chars."
} else {
    "TRAJECTORY BLOAT: docs/PROJECT_TRAJECTORY.md breached its size caps (Phase 12 appended instead of reshaping). Phase 12 REPAIR mode will reshape it on the next /0-uldf-finalize (salvage parseable signal, discard bloat, full rewrite). Non-blocking."
}

[pscustomobject]@{
    status  = if ($ok) { "pass" } else { "warn" }
    details = @{
        file_exists         = $true
        path                = $Target
        lines               = $a.lines
        bytes               = $a.bytes
        longest_line_chars  = $a.longest
        longest_line_number = $a.longestNo
        lines_over_char_cap = $a.over
        mojibake_lines      = $a.moji
        violations          = $a.violations
        caps                = @{ max_lines = $MaxLines; max_bytes = $MaxBytes; max_line_chars = $MaxLineChars }
        scan_duration_ms    = [int]((Get-Date) - $started).TotalMilliseconds
    }
    briefing = $briefing
} | ConvertTo-Json -Depth 6

# Advisory oracle: always exit 0 on a real run.
exit 0
