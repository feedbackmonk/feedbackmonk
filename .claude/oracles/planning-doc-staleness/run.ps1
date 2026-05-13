# planning-doc-staleness oracle (Windows PowerShell)
# Verification Oracle (kind: "verification" per Oraculurgy Part 11): partitions
# planning docs in docs/planning/{intakes,plans}/ into {stale, fresh, unknown}.
# Read-only and idempotent. Action leg lives in /0-uldf-finalize Phase 8.7.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$IntakesDir = "docs/planning/intakes"
$PlansDir = "docs/planning/plans"
$SpecFile = "docs/specs/SPECIFICATION.md"
$CommitWindowDays = 60
$FreshMtimeDays = 14
$ArchivePrefix = "docs/planning/archive"

# Collect candidate planning docs (depth 1).
$files = New-Object System.Collections.Generic.List[string]
foreach ($dir in @($IntakesDir, $PlansDir)) {
    if (Test-Path -LiteralPath $dir -PathType Container) {
        Get-ChildItem -LiteralPath $dir -File -Filter *.md -ErrorAction SilentlyContinue |
            ForEach-Object {
                $rel = (Resolve-Path -LiteralPath $_.FullName -Relative).TrimStart('.\').Replace('\', '/')
                if (-not $rel.StartsWith($ArchivePrefix)) {
                    $files.Add($rel) | Out-Null
                }
            }
    }
}

# Build commit-message corpus (last 60 days).
$commitLogLower = ""
$gitCmd = Get-Command git -ErrorAction SilentlyContinue
if ($gitCmd) {
    try {
        $insideRepo = (& git rev-parse --is-inside-work-tree 2>$null)
        if ($insideRepo -eq "true") {
            $raw = & git log "--since=$CommitWindowDays days ago" --pretty=format:%s 2>$null
            if ($raw) {
                $commitLogLower = ([string]::Join("`n", $raw)).ToLowerInvariant()
            }
        }
    } catch { }
}

# Read SPECIFICATION.md once.
$specContent = ""
if (Test-Path -LiteralPath $SpecFile -PathType Leaf) {
    try {
        $specContent = [System.IO.File]::ReadAllText($SpecFile)
    } catch { $specContent = "" }
}

$specHeadingRegex = [regex]::new('(?m)^#+\s+([A-Z][A-Z0-9]*-[0-9]+):.*$')
# Build a map of spec ID → heading-line (first occurrence).
$specHeadings = @{}
if (-not [string]::IsNullOrEmpty($specContent)) {
    foreach ($m in $specHeadingRegex.Matches($specContent)) {
        $id = $m.Groups[1].Value
        if (-not $specHeadings.ContainsKey($id)) {
            $specHeadings[$id] = $m.Value
        }
    }
}

$now = [DateTime]::UtcNow
$freshThresholdSeconds = $FreshMtimeDays * 86400

$timestampPrefixRegex = [regex]::new('^[0-9]{8}T[0-9]{6}-')
$specRefRegex = [regex]::new('[A-Z][A-Z0-9]*-[0-9]+')

$staleEntries = @()
$freshEntries = @()
$unknownEntries = @()

foreach ($file in $files) {
    $base = [System.IO.Path]::GetFileName($file)
    $name = [System.IO.Path]::GetFileNameWithoutExtension($file)
    $slug = $timestampPrefixRegex.Replace($name, '')

    # Heuristic 1: commit-hash-found.
    $sig1 = $false
    if (-not [string]::IsNullOrEmpty($commitLogLower)) {
        $slugLower = $slug.ToLowerInvariant()
        $baseLower = $base.ToLowerInvariant()
        if ($slugLower.Length -ge 4 -and $commitLogLower.Contains($slugLower)) {
            $sig1 = $true
        }
        if (-not $sig1 -and $baseLower.Length -ge 4 -and $commitLogLower.Contains($baseLower)) {
            $sig1 = $true
        }
    }

    # Heuristic 2: all-spec-entries-done.
    $sig2 = $false
    $refsFound = 0
    $refsDone = 0
    if ($specHeadings.Count -gt 0 -and (Test-Path -LiteralPath $file -PathType Leaf)) {
        try {
            $body = [System.IO.File]::ReadAllText($file)
        } catch { $body = "" }
        if (-not [string]::IsNullOrEmpty($body)) {
            $refs = New-Object System.Collections.Generic.HashSet[string]
            foreach ($m in $specRefRegex.Matches($body)) {
                $null = $refs.Add($m.Value)
            }
            foreach ($ref in $refs) {
                if ($specHeadings.ContainsKey($ref)) {
                    $refsFound++
                    $heading = $specHeadings[$ref]
                    if ($heading.Contains('[DONE]') -or $heading.Contains('[DELIVERED]')) {
                        $refsDone++
                    }
                }
            }
            if ($refsFound -gt 0 -and $refsFound -eq $refsDone) {
                $sig2 = $true
            }
        }
    }

    $signal = ""
    if ($sig1 -and $sig2) { $signal = "both" }
    elseif ($sig1) { $signal = "commit-hash-found" }
    elseif ($sig2) { $signal = "all-spec-entries-done" }

    # mtime → ISO-8601 UTC + age in days.
    try {
        $fi = Get-Item -LiteralPath $file -ErrorAction Stop
        $mtimeUtc = $fi.LastWriteTimeUtc
    } catch {
        $mtimeUtc = $now
    }
    $ageSpan = $now - $mtimeUtc
    $ageSeconds = [int][Math]::Max(0, $ageSpan.TotalSeconds)
    $ageDays = [int][Math]::Floor($ageSpan.TotalDays)
    if ($ageDays -lt 0) { $ageDays = 0 }
    $isoMtime = $mtimeUtc.ToString("yyyy-MM-ddTHH:mm:ssZ")

    if (-not [string]::IsNullOrEmpty($signal)) {
        $staleEntries += [pscustomobject]@{
            path = $file
            staleness_signal = $signal
            last_modified = $isoMtime
        }
    } elseif ($ageSeconds -lt $freshThresholdSeconds) {
        $freshEntries += [pscustomobject]@{ path = $file }
    } else {
        $reason = "no commit reference; no spec mapping detectable; mtime $ageDays days"
        $unknownEntries += [pscustomobject]@{
            path = $file
            reason = $reason
        }
    }
}

$staleCount = @($staleEntries).Count
$briefing = ""
if ($staleCount -gt 0) {
    $briefing = "[planning-doc-staleness] $staleCount stale planning docs, run /0-uldf-finalize to archive"
}

$result = [pscustomobject]@{
    stale = @($staleEntries)
    fresh = @($freshEntries)
    unknown = @($unknownEntries)
    briefing = $briefing
}

$result | ConvertTo-Json -Compress -Depth 6
