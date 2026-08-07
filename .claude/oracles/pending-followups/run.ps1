# pending-followups oracle (Windows PowerShell)
# Parses 'Pending Follow-Ups' sections and identifies overdue items.
# Sources (merged, additive "scope" field on each item):
#   1. Project CLAUDE.md (scope "project") -- as always.
#   2. ~/.claude/MACHINE_CONFIG.md (scope "global") -- machine-global reminders
#      written by /0-uldf-schedule --scope=global. MACHINE_CONFIG.md is the
#      sync-survivor: ~/.claude/CLAUDE.md is overwritten by every framework
#      sync, so reminders there would silently vanish (scrutiny 07 F3).

$ErrorActionPreference = "Continue"
# Force UTF-8 I/O so non-ASCII content (em-dashes, curly quotes) survives round-tripping.
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$today = Get-Date
$script:items = @()
$script:overdueCount = 0
$script:hasSection = $false

function Parse-FollowupFile {
    param([string]$Path, [string]$Scope)

    if (-not (Test-Path $Path)) { return }
    $content = Get-Content $Path -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (-not $content) { return }

    $sectionMatch = [regex]::Match($content, '(?ms)^## Pending Follow-?[Uu]ps\s*$(.*?)(?=^## |\z)')
    if (-not $sectionMatch.Success) { return }
    $script:hasSection = $true

    $section = $sectionMatch.Groups[1].Value
    $lines = $section -split "`n"
    foreach ($line in $lines) {
        # Extract "Details: `docs/pending/<slug>.md`" pointer if present (P2 externalization).
        $detailPath = $null
        $detailMatch = [regex]::Match($line, 'Details:\s+`(docs/pending/[^\s`]+\.md)`')
        if ($detailMatch.Success) {
            $detailPath = $detailMatch.Groups[1].Value
        }

        # Match "- **After YYYY-MM-DD**" or "- **YYYY-MM-DD**"
        $dateMatch = [regex]::Match($line, '^-\s+\*\*(?:After\s+)?(\d{4}-\d{2}-\d{2})\*\*:?\s*(.*)')
        if ($dateMatch.Success) {
            $due = $dateMatch.Groups[1].Value
            $title = $dateMatch.Groups[2].Value
            if ($title.Length -gt 120) { $title = $title.Substring(0, 120) }

            $isOverdue = $false
            $daysOverdue = 0
            try {
                $dueDate = [DateTime]::Parse($due)
                if ($today -gt $dueDate) {
                    $isOverdue = $true
                    $daysOverdue = [int]($today - $dueDate).TotalDays
                    $script:overdueCount++
                }
            } catch {}

            $script:items += [ordered]@{
                title = $title
                due = $due
                overdue = $isOverdue
                days_overdue = $daysOverdue
                detail_path = $detailPath
                scope = $Scope
            }
            continue
        }

        # Non-date label
        $labelMatch = [regex]::Match($line, '^-\s+\*\*([^*]+)\*\*:?\s*(.*)')
        if ($labelMatch.Success) {
            $label = $labelMatch.Groups[1].Value
            $title = $labelMatch.Groups[2].Value
            if ($title.Length -gt 120) { $title = $title.Substring(0, 120) }
            $script:items += [ordered]@{
                title = $title
                due = $label
                overdue = $false
                days_overdue = 0
                detail_path = $detailPath
                scope = $Scope
            }
        }
    }
}

# Source 1: project CLAUDE.md
if (Test-Path "CLAUDE.md") { Parse-FollowupFile -Path "CLAUDE.md" -Scope "project" }
elseif (Test-Path ".claude/CLAUDE.md") { Parse-FollowupFile -Path ".claude/CLAUDE.md" -Scope "project" }

# Source 2: machine-global reminders (graceful absence)
$machineConfig = Join-Path $env:USERPROFILE ".claude\MACHINE_CONFIG.md"
Parse-FollowupFile -Path $machineConfig -Scope "global"

if (-not $script:hasSection) {
    $empty = [ordered]@{
        has_followups_section = $false
        total = 0
        overdue = 0
        items = @()
    }
    $empty | ConvertTo-Json -Compress -Depth 4
    exit 0
}

$result = [ordered]@{
    has_followups_section = $true
    total = $script:items.Count
    overdue = $script:overdueCount
    items = @($script:items)
}

$result | ConvertTo-Json -Compress -Depth 5
