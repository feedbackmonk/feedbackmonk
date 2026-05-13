# pending-followups oracle (Windows PowerShell)
# Parses CLAUDE.md 'Pending Follow-Ups' section and identifies overdue items.

$ErrorActionPreference = "Continue"
# Force UTF-8 I/O so non-ASCII content in CLAUDE.md (em-dashes, curly quotes) survives round-tripping.
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$claudeMd = $null
if (Test-Path "CLAUDE.md") { $claudeMd = "CLAUDE.md" }
elseif (Test-Path ".claude/CLAUDE.md") { $claudeMd = ".claude/CLAUDE.md" }

if (-not $claudeMd) {
    $empty = [ordered]@{
        has_followups_section = $false
        total = 0
        overdue = 0
        items = @()
    }
    $empty | ConvertTo-Json -Compress -Depth 4
    exit 0
}

$content = Get-Content $claudeMd -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
if (-not $content) {
    $empty = [ordered]@{
        has_followups_section = $false
        total = 0
        overdue = 0
        items = @()
    }
    $empty | ConvertTo-Json -Compress -Depth 4
    exit 0
}

# Extract the "Pending Follow-Ups" section
$sectionMatch = [regex]::Match($content, '(?ms)^## Pending Follow-?[Uu]ps\s*$(.*?)(?=^## |\z)')
if (-not $sectionMatch.Success) {
    $empty = [ordered]@{
        has_followups_section = $false
        total = 0
        overdue = 0
        items = @()
    }
    $empty | ConvertTo-Json -Compress -Depth 4
    exit 0
}

$section = $sectionMatch.Groups[1].Value
$today = Get-Date

$items = @()
$overdueCount = 0

# Parse bullet items
$lines = $section -split "`n"
foreach ($line in $lines) {
    # Extract "Details: `docs/pending/<slug>.md`" pointer if present (added by P2 externalization).
    # Encoded as JSON null when absent, string when present.
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
                $overdueCount++
            }
        } catch {}

        $items += [ordered]@{
            title = $title
            due = $due
            overdue = $isOverdue
            days_overdue = $daysOverdue
            detail_path = $detailPath
        }
        continue
    }

    # Non-date label
    $labelMatch = [regex]::Match($line, '^-\s+\*\*([^*]+)\*\*:?\s*(.*)')
    if ($labelMatch.Success) {
        $label = $labelMatch.Groups[1].Value
        $title = $labelMatch.Groups[2].Value
        if ($title.Length -gt 120) { $title = $title.Substring(0, 120) }
        $items += [ordered]@{
            title = $title
            due = $label
            overdue = $false
            days_overdue = 0
            detail_path = $detailPath
        }
    }
}

$result = [ordered]@{
    has_followups_section = $true
    total = $items.Count
    overdue = $overdueCount
    items = @($items)
}

$result | ConvertTo-Json -Compress -Depth 5
