# markdown-link-validity oracle self-test (Windows PowerShell)
#
# Behaviour-equivalent twin of validate.sh. Builds hermetic sandbox doc trees
# (no dependence on this repo's own docs) and asserts the detector's real
# behaviour against the FROZEN output schema (oracle.json):
#
#   T1. clean-tree        -> status=pass, broken_count=0, external link excluded
#   T2. one-broken        -> status=fail, broken_count=1, the BROKEN link named,
#                            the GOOD link NOT reported (anti-silent-breakage)
#   T3. exclusions-only   -> http/https/ftp/mailto/tel/#anchor not checked
#   T4. relative-nesting  -> ../ resolution from a nested dir, good vs broken
#   T5. empty-scope       -> no scanned dirs -> pass, scanned_files=0
#   T6. foreign-uncommitted-deletion (OVALID-05, DEC-220) -> targets TRACKED and
#                            present in HEAD, removed only by an uncommitted
#                            deletion, are informational and do NOT fail (incl. a
#                            parent-relative path, which pins the canonicalization);
#                            a genuinely absent target in the SAME run still fails
#                            (not a blanket amnesty); committing the deletions
#                            makes them fail for real (deferred, not weakened)

$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()

$OracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path

$script:Pass = 0
$script:Fail = 0
function Note-Pass([string]$m) { Write-Output "PASS: $m"; $script:Pass++ }
function Note-Fail([string]$m) { Write-Output "FAIL: $m"; $script:Fail++ }

$Sandbox = $null
function New-Sandbox {
    $dir = Join-Path ([System.IO.Path]::GetTempPath()) ("mlvix-" + [System.Guid]::NewGuid().ToString('N').Substring(0, 12))
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $oraclePath = Join-Path $dir 'project\.claude\oracles\markdown-link-validity'
    New-Item -ItemType Directory -Path $oraclePath -Force | Out-Null
    Copy-Item (Join-Path $OracleDir 'run.ps1')     (Join-Path $oraclePath 'run.ps1')     -Force
    Copy-Item (Join-Path $OracleDir 'oracle.json') (Join-Path $oraclePath 'oracle.json') -Force
    return $dir
}
function Remove-Sandbox([string]$dir) {
    if ($dir -and (Test-Path -LiteralPath $dir)) { Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue }
}
function Invoke-Oracle([string]$dir) {
    $proj = Join-Path $dir 'project'
    Push-Location $proj
    try {
        # Read run.ps1's JSON from stdout ONLY. Stderr is discarded so a
        # non-fatal warning from the oracle can never corrupt the JSON parse.
        $out = & powershell -NoProfile -ExecutionPolicy Bypass -File '.claude\oracles\markdown-link-validity\run.ps1' 2>$null
        return ($out | Out-String).Trim()
    } finally {
        Pop-Location
    }
}

$SchemaFields = @('status', 'checked', 'broken_count', 'scanned_files', 'scan_duration_ms', 'broken', 'uncommitted_deletion_count', 'uncommitted_deletions')
function Assert-Schema([string]$out, [string]$label) {
    try {
        $obj = $out | ConvertFrom-Json -ErrorAction Stop
    } catch {
        Note-Fail "${label}: output is not valid JSON: $out"
        return $null
    }
    foreach ($f in $SchemaFields) {
        if (-not ($obj.details.PSObject.Properties.Name -contains $f) -and $f -ne 'status') {
            Note-Fail "${label}: missing schema field 'details.$f' in: $out"
            return $null
        }
    }
    if (-not ($obj.PSObject.Properties.Name -contains 'status')) {
        Note-Fail "${label}: missing schema field 'status' in: $out"
        return $null
    }
    return $obj
}

# Helper: does the broken[] list contain an entry whose 'link' equals $link ?
function Broken-Has([object]$obj, [string]$link) {
    foreach ($b in @($obj.details.broken)) {
        if ($b.link -eq $link) { return $true }
    }
    return $false
}
# Helper: does ANY broken entry mention the substring (used for negative check)?
function Broken-Mentions([object]$obj, [string]$needle) {
    foreach ($b in @($obj.details.broken)) {
        if ("$($b.link)$($b.source)$($b.resolved_path)" -like "*$needle*") { return $true }
    }
    return $false
}

# -----------------------------------------------------------------------------
# T1. clean-tree
# -----------------------------------------------------------------------------
$Sandbox = New-Sandbox
$proj = Join-Path $Sandbox 'project'
New-Item -ItemType Directory -Path (Join-Path $proj 'docs') -Force | Out-Null
Set-Content -LiteralPath (Join-Path $proj 'docs\good-target.md') -Value "# Target" -Encoding UTF8
@(
    '# Index'
    'A [good link](good-target.md) here.'
    'An [anchored good link](good-target.md#section) here.'
    'An [external](https://example.com/page) link.'
) | Set-Content -LiteralPath (Join-Path $proj 'docs\index.md') -Encoding UTF8
$out = Invoke-Oracle $Sandbox
$obj = Assert-Schema $out 'T1'
if ($obj) {
    if ($obj.status -eq 'pass' -and $obj.details.broken_count -eq 0 -and $obj.details.checked -eq 2 -and $obj.details.scanned_files -eq 2 -and @($obj.details.broken).Count -eq 0) {
        Note-Pass 'T1: clean-tree -> pass, checked=2 (external excluded), broken_count=0, broken=[]'
    } else {
        Note-Fail "T1: expected pass/checked=2/broken_count=0/scanned_files=2; got status=$($obj.status) checked=$($obj.details.checked) broken_count=$($obj.details.broken_count) scanned_files=$($obj.details.scanned_files)"
    }
}
Remove-Sandbox $Sandbox; $Sandbox = $null

# -----------------------------------------------------------------------------
# T2. one-broken -- core anti-silent-breakage assertion
# -----------------------------------------------------------------------------
$Sandbox = New-Sandbox
$proj = Join-Path $Sandbox 'project'
New-Item -ItemType Directory -Path (Join-Path $proj 'docs') -Force | Out-Null
Set-Content -LiteralPath (Join-Path $proj 'docs\good-target.md') -Value "# Target" -Encoding UTF8
@(
    '# Index'
    'A [good link](good-target.md) here.'
    'A [broken link](nope-missing.md) here.'
) | Set-Content -LiteralPath (Join-Path $proj 'docs\index.md') -Encoding UTF8
$out = Invoke-Oracle $Sandbox
$obj = Assert-Schema $out 'T2'
if ($obj) {
    $ok = $true
    if ($obj.status -ne 'fail') { $ok = $false }
    if ($obj.details.broken_count -ne 1) { $ok = $false }
    if ($obj.details.checked -ne 2) { $ok = $false }
    $b = @($obj.details.broken)[0]
    if (-not $b -or $b.source -ne 'docs/index.md' -or $b.line -ne 3 -or $b.link -ne 'nope-missing.md' -or $b.resolved_path -ne 'docs/nope-missing.md') { $ok = $false }
    if (Broken-Mentions $obj 'good-target') { $ok = $false }
    if ($ok) {
        Note-Pass 'T2: one-broken -> fail, broken_count=1, names nope-missing.md at docs/index.md:3, good link NOT reported'
    } else {
        Note-Fail "T2: expected fail/broken_count=1 naming only the broken link; got $out"
    }
}
Remove-Sandbox $Sandbox; $Sandbox = $null

# -----------------------------------------------------------------------------
# T3. exclusions-only
# -----------------------------------------------------------------------------
$Sandbox = New-Sandbox
$proj = Join-Path $Sandbox 'project'
New-Item -ItemType Directory -Path (Join-Path $proj 'docs') -Force | Out-Null
@(
    '# Index'
    '[http](http://example.com/a)'
    '[https](https://example.com/b)'
    '[ftp](ftp://example.com/c)'
    '[mail](mailto:someone@example.com)'
    '[tel](tel:+15551234567)'
    '[anchor](#same-page-section)'
) | Set-Content -LiteralPath (Join-Path $proj 'docs\index.md') -Encoding UTF8
$out = Invoke-Oracle $Sandbox
$obj = Assert-Schema $out 'T3'
if ($obj) {
    if ($obj.status -eq 'pass' -and $obj.details.checked -eq 0 -and $obj.details.broken_count -eq 0) {
        Note-Pass 'T3: exclusions-only -> pass, checked=0 (http/https/ftp/mailto/tel/#anchor all skipped)'
    } else {
        Note-Fail "T3: expected pass/checked=0/broken_count=0; got status=$($obj.status) checked=$($obj.details.checked) broken_count=$($obj.details.broken_count)"
    }
}
Remove-Sandbox $Sandbox; $Sandbox = $null

# -----------------------------------------------------------------------------
# T4. relative-nesting
# -----------------------------------------------------------------------------
$Sandbox = New-Sandbox
$proj = Join-Path $Sandbox 'project'
New-Item -ItemType Directory -Path (Join-Path $proj 'docs') -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $proj 'FOUNDATIONS\deep') -Force | Out-Null
Set-Content -LiteralPath (Join-Path $proj 'docs\good-target.md') -Value "# Target" -Encoding UTF8
@(
    '# Nested'
    'Up-and-over [good](../../docs/good-target.md).'
    'Up-one [broken](../absent-sibling.md).'
) | Set-Content -LiteralPath (Join-Path $proj 'FOUNDATIONS\deep\nested.md') -Encoding UTF8
$out = Invoke-Oracle $Sandbox
$obj = Assert-Schema $out 'T4'
if ($obj) {
    $ok = $true
    if ($obj.status -ne 'fail') { $ok = $false }
    if ($obj.details.broken_count -ne 1) { $ok = $false }
    if ($obj.details.checked -ne 2) { $ok = $false }
    if (-not (Broken-Has $obj '../absent-sibling.md')) { $ok = $false }
    $b = @($obj.details.broken)[0]
    if (-not $b -or $b.resolved_path -ne 'FOUNDATIONS/deep/../absent-sibling.md') { $ok = $false }
    if (Broken-Mentions $obj 'good-target') { $ok = $false }
    if ($ok) {
        Note-Pass 'T4: relative-nesting -> ../../ good link resolves, ../ broken link reported once'
    } else {
        Note-Fail "T4: expected fail/checked=2/broken_count=1 naming only ../absent-sibling.md; got $out"
    }
}
Remove-Sandbox $Sandbox; $Sandbox = $null

# -----------------------------------------------------------------------------
# T5. empty-scope
# -----------------------------------------------------------------------------
$Sandbox = New-Sandbox
$out = Invoke-Oracle $Sandbox
$obj = Assert-Schema $out 'T5'
if ($obj) {
    if ($obj.status -eq 'pass' -and $obj.details.broken_count -eq 0 -and $obj.details.scanned_files -eq 0) {
        Note-Pass 'T5: empty-scope -> pass, scanned_files=0, broken_count=0'
    } else {
        Note-Fail "T5: expected pass/scanned_files=0/broken_count=0; got status=$($obj.status) broken_count=$($obj.details.broken_count) scanned_files=$($obj.details.scanned_files)"
    }
}
Remove-Sandbox $Sandbox; $Sandbox = $null

# -----------------------------------------------------------------------------
# T6. foreign-uncommitted-deletion (OVALID-05, DEC-220)
#
#   T6a  uncommitted deletions of HEAD-present targets -> informational, count=2
#        (one same-dir, one PARENT-RELATIVE -- the latter pins the path
#         canonicalization, since git ls-files never emits a `..` segment)
#   T6b  a genuinely absent target in the SAME run     -> still fails
#   T6c  commit the deletions                          -> now fail for real
# -----------------------------------------------------------------------------
$gitAvailable = $null -ne (Get-Command git -ErrorAction SilentlyContinue)
if (-not $gitAvailable) {
    Write-Output 'SKIP: T6 (OVALID-05) -- git not on PATH; the informational bucket needs git ls-files --deleted'
} else {
    $Sandbox = New-Sandbox
    $proj = Join-Path $Sandbox 'project'
    New-Item -ItemType Directory -Path (Join-Path $proj 'docs') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $proj 'FOUNDATIONS') -Force | Out-Null
    '# Target'    | Set-Content -LiteralPath (Join-Path $proj 'docs\tracked-target.md') -Encoding UTF8
    '# Upstairs'  | Set-Content -LiteralPath (Join-Path $proj 'FOUNDATIONS\tracked-upstairs.md') -Encoding UTF8
    @(
        '# Index'
        'A [tracked link](tracked-target.md) here.'
        'A [genuinely broken link](never-existed.md) here.'
        'A [parent-relative tracked link](../FOUNDATIONS/tracked-upstairs.md) here.'
    ) | Set-Content -LiteralPath (Join-Path $proj 'docs\index.md') -Encoding UTF8

    # DEFER-077: -ErrorAction Stop is load-bearing. $ErrorActionPreference is
    # 'Continue' file-wide, so a FAILED Push-Location would leave the REAL repo
    # as cwd and the next line would `git add -A` it.
    if (-not $proj) { throw 'DEFER-077: empty sandbox path -- refusing to run git in the CWD' }
    Push-Location -LiteralPath $proj -ErrorAction Stop
    try {
        & git init -q . 2>$null | Out-Null
        & git config user.email 'selftest@localhost' 2>$null | Out-Null
        & git config user.name 'mlv selftest' 2>$null | Out-Null
        & git add -A 2>$null | Out-Null
        & git -c commit.gpgsign=false commit -qm 'selftest baseline' 2>$null | Out-Null
    } finally {
        Pop-Location
    }

    # T6a + T6b -- delete both tracked targets WITHOUT committing.
    Remove-Item -LiteralPath (Join-Path $proj 'docs\tracked-target.md') -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $proj 'FOUNDATIONS\tracked-upstairs.md') -Force -ErrorAction SilentlyContinue
    $out = Invoke-Oracle $Sandbox
    $obj = Assert-Schema $out 'T6'
    if ($obj) {
        $ok = $true
        # T6b -- the genuinely absent target still fails, in the same run.
        if ($obj.status -ne 'fail') { $ok = $false }
        if ($obj.details.broken_count -ne 1) { $ok = $false }
        if (-not (Broken-Has $obj 'never-existed.md')) { $ok = $false }
        # T6a -- both deletions informational; count=2 is what pins normalization.
        if ($obj.details.uncommitted_deletion_count -ne 2) { $ok = $false }
        $infoText = (@($obj.details.uncommitted_deletions) | ForEach-Object { "$($_.link)$($_.resolved_path)" }) -join '|'
        if ($infoText -notlike '*tracked-upstairs*') { $ok = $false }
        # Neither may ALSO appear in broken[] (double-counting would defeat it).
        if (Broken-Mentions $obj 'tracked-target')   { $ok = $false }
        if (Broken-Mentions $obj 'tracked-upstairs') { $ok = $false }
        if ($ok) {
            Note-Pass 'T6a/T6b: both uncommitted deletions informational (count=2, incl. the parent-relative path) while a genuinely absent target still fails in the same run -- not a blanket amnesty'
        } else {
            Note-Fail "T6a/T6b: expected fail/broken_count=1(never-existed)/uncommitted_deletion_count=2(tracked-target + ../tracked-upstairs); got $out"
        }
    }

    # T6c -- commit the deletions; the suppression must LAPSE.
    if (-not $proj) { throw 'DEFER-077: empty sandbox path -- refusing to run git in the CWD' }
    Push-Location -LiteralPath $proj -ErrorAction Stop   # DEFER-077
    try {
        & git add -A 2>$null | Out-Null
        & git -c commit.gpgsign=false commit -qm 'selftest: commit the deletions' 2>$null | Out-Null
    } finally {
        Pop-Location
    }
    $out = Invoke-Oracle $Sandbox
    $obj = Assert-Schema $out 'T6c'
    if ($obj) {
        $ok = $true
        if ($obj.status -ne 'fail') { $ok = $false }
        if ($obj.details.broken_count -ne 3) { $ok = $false }
        if ($obj.details.uncommitted_deletion_count -ne 0) { $ok = $false }
        if (-not (Broken-Mentions $obj 'tracked-target'))   { $ok = $false }
        if (-not (Broken-Mentions $obj 'tracked-upstairs')) { $ok = $false }
        if ($ok) {
            Note-Pass 'T6c: committing the deletions restores the failures (broken_count=3, uncommitted_deletion_count=0) -- the gate is deferred, not weakened'
        } else {
            Note-Fail "T6c: expected fail/broken_count=3/uncommitted_deletion_count=0 after committing the deletions; got $out"
        }
    }
    Remove-Sandbox $Sandbox; $Sandbox = $null
}

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
Write-Output ""
Write-Output "================================================================"
Write-Output "  markdown-link-validity validate: $($script:Pass) PASS / $($script:Fail) FAIL"
Write-Output "================================================================"

if ($script:Fail -gt 0) { exit 1 }
exit 0
