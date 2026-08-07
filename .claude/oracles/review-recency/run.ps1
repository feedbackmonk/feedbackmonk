# review-recency oracle (Windows) — RECENCY-03/04, DEC-139.
# PowerShell parity of run.sh. Emits a [review-recency] briefing line when a
# covered heavy review/refactor skill ran on THIS project within recentDays.
# Delegates aggregation to scripts/review-recency.ps1 (RECENCY-01); this wrapper
# scopes to the current project and shapes the briefing. Gracefully absent
# (empty briefing) on nothing-recent / NO-DATA / reader-not-found.

$ErrorActionPreference = "SilentlyContinue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$project = Split-Path -Leaf (Get-Location).Path

function Emit-Quiet {
    param([int]$RecentDays = 14)
    $o = [ordered]@{
        recent  = $false
        details = [ordered]@{ project = $project; recentDays = $RecentDays; recentSkills = @() }
        briefing = ""
    }
    ($o | ConvertTo-Json -Depth 6 -Compress)
    exit 0
}

# ---- Locate the reader (deployed project -> template repo -> global) ---------
$reader = $null
foreach ($cand in @(
    (Join-Path $PSScriptRoot "..\..\scripts\review-recency.ps1"),
    (Join-Path $PSScriptRoot "..\..\..\claude-template\scripts\review-recency.ps1"),
    (Join-Path $env:USERPROFILE ".claude\scripts\review-recency.ps1")
)) {
    if (Test-Path $cand) { $reader = $cand; break }
}
if (-not $reader) { Emit-Quiet }

# ---- Run the reader, project-scoped ------------------------------------------
$out = & powershell.exe -NoProfile -File $reader -Project $project -Json 2>$null
if ($LASTEXITCODE -ne 0 -or -not $out) { Emit-Quiet }

try { $doc = ($out | Out-String | ConvertFrom-Json) } catch { Emit-Quiet }
if (-not $doc) { Emit-Quiet }

$recentDays = if ($doc.recentDays) { [int]$doc.recentDays } else { 14 }

$recentSkills = @()
if ($doc.projects -and $doc.projects.Count -gt 0) {
    $skills = $doc.projects[0].skills
    foreach ($p in $skills.PSObject.Properties) {
        if ($p.Value.status -eq "recent") {
            $recentSkills += [ordered]@{ skill = $p.Name; lastUsed = $p.Value.lastUsed; lastArg1 = $p.Value.lastArg1 }
        }
    }
}

if ($recentSkills.Count -eq 0) { Emit-Quiet -RecentDays $recentDays }

$summary = ($recentSkills | ForEach-Object { "$($_.skill) $(if ($_.lastUsed) { $_.lastUsed } else { '?' })" }) -join "; "
$briefing = "reviewed on this project within ${recentDays}d - ${summary}; a re-run may be redundant (see /0-uldf-portfolio recency)"

$o = [ordered]@{
    recent  = $true
    details = [ordered]@{ project = $project; recentDays = $recentDays; recentSkills = $recentSkills }
    briefing = $briefing
}
($o | ConvertTo-Json -Depth 6 -Compress)
exit 0
