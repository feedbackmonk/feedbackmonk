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

# OVALID-05 (DEC-220): this oracle asserts "the citation resolves" but measures
# "a path exists in the WORKING TREE" -- and that tree is shared with live
# sibling sessions. A target listed by `git ls-files --deleted` is TRACKED and
# present in HEAD; only an UNCOMMITTED deletion removed it from the tree, so the
# citation is correct and the doc is not broken. Failing on it let any peer's
# in-flight WIP redden a commit gate, and the remediation advice it produced was
# actively wrong (repoint a CORRECT citation at some other path).
#
# The gate is DEFERRED, not weakened: such targets go to an informational
# uncommitted_deletions bucket and do not set status. Commit the deletion and the
# path leaves this set, at which point it fails as a genuine broken link. Not a
# blanket amnesty -- every other failure class still fails, in the same run,
# alongside a suppressed one (asserted invertibly by validate.ps1 case 6).
# Graceful absence: no git / not a work tree -> empty set -> strict as before.
$DeletedSet = New-Object System.Collections.Generic.HashSet[string]
try {
    $null = & git rev-parse --is-inside-work-tree 2>$null
    if ($LASTEXITCODE -eq 0) {
        foreach ($d in (& git ls-files --deleted 2>$null)) {
            if (-not [string]::IsNullOrWhiteSpace($d)) {
                $null = $DeletedSet.Add($d.Trim().Replace('\', '/'))
            }
        }
    }
} catch {
    # git unavailable -- leave the set empty.
}

# Textual path canonicalization. `git ls-files` emits repo-root-relative paths
# with no `.`/`..` segments, while this oracle resolves links against the citing
# file's directory and so produces e.g. `docs/../FOUNDATIONS/X.md`. Without
# normalization the membership test would silently never match for any
# parent-relative citation -- a proxy bug inside the fix for a proxy bug.
function Get-NormalizedRelPath {
    param([string]$Path)
    $p = $Path.Replace('\', '/')
    $out = New-Object System.Collections.Generic.List[string]
    foreach ($seg in $p.Split('/')) {
        if ($seg -eq '' -or $seg -eq '.') { continue }
        if ($seg -eq '..') {
            if ($out.Count -gt 0 -and $out[$out.Count - 1] -ne '..') {
                $out.RemoveAt($out.Count - 1)
            } else {
                $out.Add('..') | Out-Null
            }
            continue
        }
        $out.Add($seg) | Out-Null
    }
    return ($out -join '/')
}

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
$uncommitted = New-Object System.Collections.Generic.List[object]

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
                        $entry = [ordered]@{
                            source = $file
                            line = $lineNum
                            link = $dest
                            resolved_path = $resolved
                        }
                        # OVALID-05: tracked-and-present-in-HEAD, removed only by
                        # an uncommitted deletion -> informational, not a failure.
                        if ($DeletedSet.Contains((Get-NormalizedRelPath $resolved))) {
                            $uncommitted.Add($entry) | Out-Null
                        } else {
                            $broken.Add($entry) | Out-Null
                        }
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

# Materialize the array with .ToArray(), then assign the two ordered maps
# incrementally. Windows PowerShell 5.1 throws "Argument types do not match"
# (ArgumentException) on `@($broken)` when $broken is a List[object] whose
# elements are [ordered] maps (OrderedDictionary) -- and doubly so when such an
# array is placed inside an `[ordered]@{ ... }` *literal*. Either form left the
# oracle emitting nothing / a null broken[] and silently exiting 0 on 5.1 (the
# framework's primary Windows shell). List.ToArray() sidesteps the `@()`
# operator entirely and works on both 5.1 and PS 7.
$brokenArr = $broken.ToArray()
$uncommittedArr = $uncommitted.ToArray()
$details = [ordered]@{}
$details['checked'] = $checked
$details['broken_count'] = $broken.Count
$details['scanned_files'] = $files.Count
$details['scan_duration_ms'] = $durationMs
$details['broken'] = $brokenArr
$details['uncommitted_deletion_count'] = $uncommitted.Count
$details['uncommitted_deletions'] = $uncommittedArr
$result = [ordered]@{}
$result['status'] = $status
$result['details'] = $details

$result | ConvertTo-Json -Compress -Depth 6
