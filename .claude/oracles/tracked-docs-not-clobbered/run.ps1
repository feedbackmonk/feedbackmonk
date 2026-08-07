# tracked-docs-not-clobbered oracle (Windows PowerShell)
#
# Verification Oracle (kind: verification). Auto-discovered by /0-uldf-finalize
# Phase 1a sec 1a.6. Detects when a load-bearing tracked doc (CLAUDE.md, root
# README.md, docs/specs/*.md) has been clobbered with raw
# dev-server / build log output -- the signature of a misdirected dev-server
# stdout redirect (e.g. `npm run dev > CLAUDE.md`, `cargo tauri dev > README.md`)
# whose detached child process inherits the open file descriptor and re-writes
# the file on every HMR / rebuild reload, even after the launching shell dies.
#
# This destroys the file silently and (on Windows) the held handle blocks
# `git checkout` / `rm` recovery until the offending process is killed. The
# oracle makes the catch deterministic and every-finalize rather than relying
# on a human noticing the bad diff.
#
# Output: JSON ({status, details, briefing}). Exit 0 when clean, 1 when
# contamination found (so both the `status` field and a plain exit-code check
# are honored as a commit gate).
#
# `-SelfTest` runs the detector against a synthetic clobbered sample and
# asserts it fires (anti-silent-breakage check). Exit 0 if it correctly fires.

param([switch]$SelfTest)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$esc = [char]27

# High-confidence raw-log signatures. ANSI escapes are decisive on their own;
# textual banners require >= 2 distinct hits so a doc that quotes one example
# log line in a code fence is not a false positive.
$bannerPatterns = @(
    'VITE v\d',                       # Vite startup banner
    'ready in \d+\s*ms',              # Vite ready line
    'Compiling \S+ v\d',              # cargo compile
    'Finished `?dev`? profile',       # cargo dev finish
    'Finished .dev. \[unoptimized',   # cargo dev finish (alt)
    'Running BeforeDevCommand',       # tauri dev
    'Running `?beforeDevCommand`?',   # tauri dev (alt)
    'Watching .+ for changes',        # tauri/vite watcher
    'hmr update ',                    # vite hmr line
    'webpack compiled',               # webpack dev
    'Compiled successfully',          # CRA / webpack dev
    'webview2?:\s',                   # tauri webview log prefix
    '\[tauri\]',                      # tauri log prefix
    'Local:\s+https?://localhost'     # dev-server local URL line
)

function Test-Clobbered {
    param([string]$Content)
    $result = [pscustomobject]@{ ansi = $false; banners = @(); contaminated = $false }
    if ([string]::IsNullOrEmpty($Content)) { return $result }
    if ($Content.Contains("$esc[")) { $result.ansi = $true }
    foreach ($pat in $bannerPatterns) {
        if ([regex]::Match($Content, $pat).Success) { $result.banners += $pat }
    }
    $result.contaminated = $result.ansi -or ($result.banners.Count -ge 2)
    return $result
}

if ($SelfTest) {
    # Combined sample (ANSI + textual banners) -- the original broad check.
    $sample = "Some heading`n$esc[32mVITE v5.4.2$esc[0m  ready in 412 ms`n  Local:   http://localhost:5173/`nCompiling app v0.1.0"
    # ANSI-ONLY sample -- carries an ESC '[' sequence but NO text matching any
    # banner pattern, so it can ONLY be caught by the ANSI-escape check. This
    # guards the "ANSI = decisive" path independently (parity with run.sh's
    # portability guard): if that path regresses, the combined sample would
    # still pass via textual banners and mask the break.
    $ansiOnly = "Project Title`n$esc[38;5;204msome arbitrary colored prose with no log keywords$esc[0m`nmore ordinary documentation text"

    $r1 = Test-Clobbered -Content $sample
    $r2 = Test-Clobbered -Content $ansiOnly
    $fired = $r1.contaminated -and $r2.contaminated
    $briefing = if (-not $r1.contaminated) {
        "Self-test FAIL: detector did NOT flag an obviously-clobbered sample -- the gate is silently broken."
    } elseif (-not $r2.contaminated) {
        "Self-test FAIL: ANSI-only sample not flagged -- the decisive ANSI-escape path is broken."
    } else {
        "Self-test PASS: detector flags both a combined clobber sample and an ANSI-only sample (decisive ANSI path verified)."
    }
    [pscustomobject]@{
        status   = if ($fired) { "pass" } else { "fail" }
        details  = @{ self_test = $true; detector_fired = $fired }
        briefing = $briefing
    } | ConvertTo-Json -Depth 5
    if ($fired) { exit 0 } else { exit 1 }
}

$started = Get-Date
$repoRoot = (Resolve-Path "$PSScriptRoot/../../..").Path

# Scan set: root CLAUDE.md + README.md, plus docs/specs/*.md. Edit here to
# change scope; keep the <2s speed contract (Part 11) in mind (avoid broad
# recursive doc trees).
$targets = New-Object System.Collections.Generic.List[string]
foreach ($rel in @("CLAUDE.md", "README.md")) {
    $p = Join-Path $repoRoot $rel
    if (Test-Path $p) { [void]$targets.Add((Resolve-Path $p).Path) }
}
foreach ($sub in @("docs/specs")) {
    $d = Join-Path $repoRoot $sub
    if (Test-Path $d) {
        Get-ChildItem $d -Filter "*.md" -File -ErrorAction SilentlyContinue |
            ForEach-Object { [void]$targets.Add($_.FullName) }
    }
}
$targets = $targets | Select-Object -Unique

$contaminated = @()
foreach ($t in $targets) {
    $rel = $t.Substring($repoRoot.Length).TrimStart('\','/') -replace '\\','/'
    $content = Get-Content $t -Raw -ErrorAction SilentlyContinue
    $r = Test-Clobbered -Content $content
    if ($r.contaminated) {
        $firstLine = ($content -split "`n" | Where-Object { $_ -match ($bannerPatterns -join '|') -or $_.Contains("$esc[") } | Select-Object -First 1)
        $contaminated += [pscustomobject]@{
            path       = $rel
            ansi       = $r.ansi
            signatures = $r.banners
            excerpt    = ($firstLine -replace [regex]::Escape("$esc"), '<ESC>').Trim()
        }
    }
}

$ok = ($contaminated.Count -eq 0)
$briefing = if ($ok) {
    "Clean: $($targets.Count) tracked docs free of dev-server/build log contamination."
} else {
    "CLOBBER DETECTED: $($contaminated.Count) tracked doc(s) contain raw dev-server/build log output. A dev server was likely launched with stdout redirected into a tracked file. Do NOT commit -- restore the file from HEAD and kill any orphaned dev process still holding its file handle."
}

[pscustomobject]@{
    status  = if ($ok) { "pass" } else { "fail" }
    details = @{
        checked            = $targets.Count
        contaminated_count = $contaminated.Count
        contaminated       = $contaminated
        scan_duration_ms   = [int]((Get-Date) - $started).TotalMilliseconds
    }
    briefing = $briefing
} | ConvertTo-Json -Depth 6

if ($ok) { exit 0 } else { exit 1 }
