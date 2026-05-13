# recent-activity oracle (Windows PowerShell)
# Reports recent commits, touched areas, and commit cadence.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$gitDir = git rev-parse --git-dir 2>$null
if (-not $gitDir) {
    $empty = [ordered]@{
        last_commits = @()
        touched_directories_last_5 = @()
        commits_last_7_days = 0
        commits_last_30_days = 0
    }
    $empty | ConvertTo-Json -Compress -Depth 4
    exit 0
}

# Last 5 commits
$rawCommits = @(git log -5 --format='%h|%s|%an|%ad' --date=short 2>$null)
$commits = @()
foreach ($line in $rawCommits) {
    if ([string]::IsNullOrEmpty($line)) { continue }
    $parts = $line -split '\|', 4
    if ($parts.Length -lt 4) { continue }
    $commits += [ordered]@{
        hash = $parts[0]
        subject = $parts[1]
        author = $parts[2]
        date = $parts[3]
    }
}

# Touched top-level directories from last 5 commits
$rawPaths = @(git log -5 --name-only --format='' 2>$null)
$dirs = @()
foreach ($path in $rawPaths) {
    if ([string]::IsNullOrEmpty($path)) { continue }
    $topDir = $path.Split('/')[0]
    if ($topDir -and ($dirs -notcontains $topDir)) {
        $dirs += $topDir
    }
}

# Counts
$commits7 = 0
$log7 = @(git log --since='7 days ago' --oneline 2>$null)
$commits7 = $log7.Count

$commits30 = 0
$log30 = @(git log --since='30 days ago' --oneline 2>$null)
$commits30 = $log30.Count

$result = [ordered]@{
    last_commits = @($commits)
    touched_directories_last_5 = @($dirs)
    commits_last_7_days = $commits7
    commits_last_30_days = $commits30
}

$result | ConvertTo-Json -Compress -Depth 5
