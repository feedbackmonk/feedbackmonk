# module-index oracle (Windows PowerShell)
# Walks the project tree to inventory modules (directories with README.md) and reports status.

$ErrorActionPreference = "Continue"
# Force UTF-8 I/O so non-ASCII content in READMEs survives round-tripping.
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$excludes = @(
    'node_modules', 'target', '.git', '.vscode', '.idea', 'dist', 'build', 'out',
    'coverage', '__pycache__', '.venv', 'venv', '.claude/oracles/cache', '.claude/checkpoints'
)

function IsExcluded($path) {
    $normalized = $path.Replace('\', '/').TrimStart('./')
    foreach ($ex in $excludes) {
        if ($normalized -eq $ex -or $normalized.StartsWith("$ex/") -or $normalized -match "/$ex(/|$)") {
            return $true
        }
    }
    return $false
}

$modules = @()
$total = 0
$withReadme = 0
$withoutReadme = 0

# Find all directories with README.md, depth-limited
$candidates = Get-ChildItem -Path "." -Directory -Recurse -Depth 3 -ErrorAction SilentlyContinue
foreach ($dir in $candidates) {
    $relPath = Resolve-Path -Relative $dir.FullName -ErrorAction SilentlyContinue
    if (-not $relPath) { continue }
    $relPath = $relPath -replace '^\.\\', '' -replace '^\.\/', ''
    $normalized = $relPath.Replace('\', '/')
    if (IsExcluded $normalized) { continue }

    $readmePath = Join-Path $dir.FullName "README.md"
    if (Test-Path $readmePath) {
        # Extract purpose from the first non-heading paragraph after the first heading.
        # IMPORTANT: Get-Content returns string objects with file-metadata note properties
        # attached. We must explicitly coerce each line to a plain string before storing,
        # otherwise ConvertTo-Json will serialize the full PSProvider tree.
        $readmeLines = @(Get-Content $readmePath -Encoding UTF8 -ErrorAction SilentlyContinue)
        [string]$purpose = ""
        $sawHeading = $false
        foreach ($rawLine in $readmeLines) {
            [string]$line = "$rawLine"
            if ($line -match '^#') {
                if ($sawHeading) { break }
                $sawHeading = $true
                continue
            }
            if ($sawHeading -and $line -match '^\S') {
                $purpose = $line
                break
            }
        }
        if ($purpose.Length -gt 200) { $purpose = $purpose.Substring(0, 200) }

        $modules += [ordered]@{
            path = [string]$normalized
            has_readme = $true
            purpose = [string]$purpose
        }
        $total++
        $withReadme++
    }
}

# Count directories that look like modules but lack README
$allDirs = Get-ChildItem -Path "." -Directory -Recurse -Depth 2 -ErrorAction SilentlyContinue
foreach ($dir in $allDirs) {
    $relPath = Resolve-Path -Relative $dir.FullName -ErrorAction SilentlyContinue
    if (-not $relPath) { continue }
    $relPath = $relPath -replace '^\.\\', '' -replace '^\.\/', ''
    $normalized = $relPath.Replace('\', '/')
    if (IsExcluded $normalized) { continue }

    $readmePath = Join-Path $dir.FullName "README.md"
    if (Test-Path $readmePath) { continue }

    $codeFiles = Get-ChildItem -Path $dir.FullName -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -in @('.rs', '.ts', '.tsx', '.js', '.jsx', '.py', '.go', '.java', '.cs') }
    if ($codeFiles.Count -gt 0) {
        $withoutReadme++
    }
}

$result = [ordered]@{
    total_modules = $total
    with_readme = $withReadme
    without_readme = $withoutReadme
    modules = @($modules)
}

$result | ConvertTo-Json -Compress -Depth 5
