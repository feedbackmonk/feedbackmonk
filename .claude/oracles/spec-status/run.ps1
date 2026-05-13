# spec-status oracle (Windows PowerShell)
# Counts DONE / PENDING / IN_PROGRESS / REMOVED items in the project specification.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$specFile = $null
if (Test-Path "docs/specs/SPECIFICATION.md") { $specFile = "docs/specs/SPECIFICATION.md" }
elseif (Test-Path "docs/specs/PROJECT_SPEC.md") { $specFile = "docs/specs/PROJECT_SPEC.md" }
elseif (Test-Path "docs/specs/spec.md") { $specFile = "docs/specs/spec.md" }

if (-not $specFile) {
    $empty = [ordered]@{
        spec_exists = $false
        spec_file = $null
        total_items = 0
        done = 0
        pending = 0
        in_progress = 0
        removed = 0
        progress_percent = 0
    }
    $empty | ConvertTo-Json -Compress -Depth 4
    exit 0
}

$content = Get-Content $specFile -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
if (-not $content) { $content = "" }

# Count by Status field markers (case-insensitive)
$doneMatches = [regex]::Matches($content, '(?im)(\*\*|^)status(\*\*)?\s*:\s*(done|complete|completed)')
$pendingMatches = [regex]::Matches($content, '(?im)(\*\*|^)status(\*\*)?\s*:\s*(pending|todo|not[\s_]?started)')
$inProgressMatches = [regex]::Matches($content, '(?im)(\*\*|^)status(\*\*)?\s*:\s*(in[\s_]?progress|wip|active)')
$removedMatches = [regex]::Matches($content, '(?im)(\*\*|^)status(\*\*)?\s*:\s*(removed|cancelled|deferred)')

$doneCount = $doneMatches.Count
$pendingCount = $pendingMatches.Count
$inProgressCount = $inProgressMatches.Count
$removedCount = $removedMatches.Count

# Checkbox fallback
$checkboxDone = [regex]::Matches($content, '(?m)^\s*-\s*\[x\]').Count
$checkboxPending = [regex]::Matches($content, '(?m)^\s*-\s*\[\s\]').Count

if ($checkboxDone -gt $doneCount -or $checkboxPending -gt $pendingCount) {
    $doneCount = $checkboxDone
    $pendingCount = $checkboxPending
}

$total = $doneCount + $pendingCount + $inProgressCount
$progressPct = 0
if ($total -gt 0) {
    $progressPct = [math]::Floor(($doneCount * 100) / $total)
}

$result = [ordered]@{
    spec_exists = $true
    spec_file = $specFile
    total_items = $total
    done = $doneCount
    pending = $pendingCount
    in_progress = $inProgressCount
    removed = $removedCount
    progress_percent = $progressPct
}

$result | ConvertTo-Json -Compress -Depth 4
