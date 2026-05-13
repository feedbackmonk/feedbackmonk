# markdown-link-validity oracle (Windows PowerShell)
# Verification Oracle: checks that all internal markdown links in tracked
# documentation files resolve to existing targets. Read-only and idempotent.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

# Scan scope. Keep this aligned with oracle.json's config.scan_* fields.
$ScanDirs = @('claude-template', 'docs', 'FOUNDATIONS')
$ScanRootFiles = @('CLAUDE.md', 'README.md')

$start = [DateTime]::UtcNow

# Collect markdown files to scan.
$files = New-Object System.Collections.Generic.List[string]
foreach ($d in $ScanDirs) {
    if (Test-Path -LiteralPath $d -PathType Container) {
        Get-ChildItem -LiteralPath $d -Recurse -File -Filter *.md -ErrorAction SilentlyContinue |
            ForEach-Object {
                $rel = (Resolve-Path -LiteralPath $_.FullName -Relative).TrimStart('.\').Replace('\', '/')
                $files.Add($rel) | Out-Null
            }
    }
}
foreach ($rf in $ScanRootFiles) {
    if (Test-Path -LiteralPath $rf -PathType Leaf) {
        $files.Add($rf) | Out-Null
    }
}

# Pre-compiled link regex: !? optional, [text](dest)
$linkRegex = [regex]::new('\[[^\]]*\]\(([^)]+)\)')
# Strip an optional `"title"` suffix inside the parens.
$titleRegex = [regex]::new('\s+"[^"]*"\s*$')

$checked = 0
$broken = New-Object System.Collections.Generic.List[object]

foreach ($file in $files) {
    $dir = [System.IO.Path]::GetDirectoryName($file)
    if ([string]::IsNullOrEmpty($dir)) { $dir = '.' }

    $lineNum = 0
    try {
        $reader = [System.IO.StreamReader]::new($file)
        try {
            while (-not $reader.EndOfStream) {
                $line = $reader.ReadLine()
                $lineNum++
                if ([string]::IsNullOrEmpty($line)) { continue }
                $matches = $linkRegex.Matches($line)
                foreach ($m in $matches) {
                    $dest = $m.Groups[1].Value
                    # Strip optional title and surrounding whitespace.
                    $dest = $titleRegex.Replace($dest, '').Trim()

                    # Skip protocol/external links and same-page anchors.
                    if ([string]::IsNullOrEmpty($dest)) { continue }
                    if ($dest -match '^(https?|ftp|mailto|tel):') { continue }
                    if ($dest.StartsWith('#')) { continue }

                    # Strip anchor and query for filesystem resolution.
                    $target = $dest
                    $hashIdx = $target.IndexOf('#')
                    if ($hashIdx -ge 0) { $target = $target.Substring(0, $hashIdx) }
                    $qIdx = $target.IndexOf('?')
                    if ($qIdx -ge 0) { $target = $target.Substring(0, $qIdx) }
                    if ([string]::IsNullOrEmpty($target)) { continue }

                    $checked++

                    # Resolve relative to source file's directory; absolute paths kept as-is.
                    if ($target.StartsWith('/')) {
                        $resolved = $target
                    } else {
                        $resolved = (Join-Path $dir $target).Replace('\', '/')
                    }

                    if (-not (Test-Path -LiteralPath $resolved)) {
                        $broken.Add([ordered]@{
                            source = $file
                            line = $lineNum
                            link = $dest
                            resolved_path = $resolved
                        }) | Out-Null
                    }
                }
            }
        } finally {
            $reader.Close()
            $reader.Dispose()
        }
    } catch {
        # Best-effort: skip unreadable files. Graceful absence per oracle contract.
        continue
    }
}

$durationMs = [int]([DateTime]::UtcNow - $start).TotalMilliseconds
if ($durationMs -lt 0) { $durationMs = 0 }

$status = if ($broken.Count -eq 0) { 'pass' } else { 'fail' }

$result = [ordered]@{
    status = $status
    details = [ordered]@{
        checked = $checked
        broken_count = $broken.Count
        scanned_files = $files.Count
        scan_duration_ms = $durationMs
        broken = @($broken)
    }
}

$result | ConvertTo-Json -Compress -Depth 6
