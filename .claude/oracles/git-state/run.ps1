# git-state oracle (Windows PowerShell)
# Reports current git state: branch, uncommitted counts, last commit.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# Check if this is a git repo
$gitDir = git rev-parse --git-dir 2>$null
if (-not $gitDir) {
    $emptyResult = [ordered]@{
        is_git_repo = $false
        branch = $null
        modified = 0
        staged = 0
        untracked = 0
        deleted = 0
        clean = $true
        last_commit = [ordered]@{
            hash = $null
            subject = $null
            date = $null
        }
    }
    $emptyResult | ConvertTo-Json -Compress -Depth 4
    exit 0
}

$branch = git branch --show-current 2>$null
if (-not $branch) { $branch = $null }

# Parse git status --porcelain
$statusLines = @(git status --porcelain 2>$null)
$modified = 0
$staged = 0
$untracked = 0
$deleted = 0

foreach ($line in $statusLines) {
    if ([string]::IsNullOrEmpty($line)) { continue }
    if ($line.Length -lt 2) { continue }
    $xy = $line.Substring(0, 2)
    $x = $xy[0]
    $y = $xy[1]
    if ($xy -eq '??') { $untracked++; continue }
    switch ($x) {
        'M' { $staged++ }
        'A' { $staged++ }
        'R' { $staged++ }
        'C' { $staged++ }
        'D' { $deleted++; $staged++ }
    }
    switch ($y) {
        'M' { $modified++ }
        'D' { $deleted++ }
    }
}

$clean = ($modified -eq 0 -and $staged -eq 0 -and $untracked -eq 0 -and $deleted -eq 0)

# Last commit
$lastHash = $null
$lastSubject = $null
$lastDate = $null
$lastLine = git log -1 --format='%h|%s|%ad' --date=short 2>$null
if ($lastLine) {
    $parts = $lastLine -split '\|', 3
    if ($parts.Length -ge 1) { $lastHash = $parts[0] }
    if ($parts.Length -ge 2) { $lastSubject = $parts[1] }
    if ($parts.Length -ge 3) { $lastDate = $parts[2] }
}

$result = [ordered]@{
    is_git_repo = $true
    branch = $branch
    modified = $modified
    staged = $staged
    untracked = $untracked
    deleted = $deleted
    clean = $clean
    last_commit = [ordered]@{
        hash = $lastHash
        subject = $lastSubject
        date = $lastDate
    }
}

$result | ConvertTo-Json -Compress -Depth 4
