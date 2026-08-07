# hook-verification-coverage oracle (Windows PowerShell)
# Parity with run.sh -- see run.sh header for the EPP-03 contract.
# Read-only, idempotent, <2s. HVC_ROOT env var overrides the scan root.
$ErrorActionPreference = 'SilentlyContinue'

$root = if ($env:HVC_ROOT) { $env:HVC_ROOT } else { '.' }

# Hooks shipped before EPP-03 (2026-06-11) without a verification surface.
# Remove an entry when its test/smoke lands. Mirrored in run.sh + README.
$baseline = @('session-detect', 'command-usage-tracker', 'pre-compact')

$settings = $null; $hooksDir = $null; $smokeDirs = @()
if (Test-Path (Join-Path $root 'claude-template/settings.json')) {
    $settings = Join-Path $root 'claude-template/settings.json'
    $hooksDir = Join-Path $root 'claude-template/hooks'
    $smokeDirs = @((Join-Path $root 'claude-template/scripts/csi-tests'), (Join-Path $root 'claude-template/scripts/smoke-tests'))
} elseif (Test-Path (Join-Path $root '.claude/settings.json')) {
    $settings = Join-Path $root '.claude/settings.json'
    $hooksDir = Join-Path $root '.claude/hooks'
    $smokeDirs = @((Join-Path $root '.claude/scripts/csi-tests'), (Join-Path $root '.claude/scripts/smoke-tests'))
}

if (-not $settings) {
    Write-Output '{"status":"pass","details":{"applicable":false,"checked":0,"covered":[],"uncovered_new":[],"uncovered_legacy":[]},"briefing":""}'
    exit 0
}

$content = Get-Content -Raw -LiteralPath $settings
$bases = [regex]::Matches($content, 'hooks/([A-Za-z0-9_-]+)\.(ps1|sh)') |
    ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique

function Test-Covered([string]$base) {
    if (Test-Path (Join-Path $hooksDir "tests/$base.test.sh")) { return $true }
    if (Test-Path (Join-Path $hooksDir "tests/$base.test.ps1")) { return $true }
    foreach ($sd in $smokeDirs) {
        if (-not $sd -or -not (Test-Path $sd)) { continue }
        $hit = Get-ChildItem -LiteralPath $sd -File |
            Where-Object { Select-String -LiteralPath $_.FullName -Pattern ("{0}\.(sh|ps1)" -f [regex]::Escape($base)) -Quiet }
        if ($hit) { return $true }
    }
    return $false
}

$covered = @(); $uncNew = @(); $uncLegacy = @(); $checked = 0
foreach ($base in $bases) {
    if (-not $base) { continue }
    $checked++
    if (Test-Covered $base) { $covered += $base }
    elseif ($baseline -contains $base) { $uncLegacy += $base }
    else { $uncNew += $base }
}

$status = 'pass'; $briefing = ''
if ($uncNew.Count -gt 0) {
    $status = 'fail'
    $briefing = "EPP-03 VIOLATION: new hook(s) registered without a verification surface: [$($uncNew -join ', ')]. Ship a hooks/tests/<name>.test.{sh,ps1} or a scripts/{csi-tests,smoke-tests}/ smoke referencing the hook script, in the same change (ENFORCEMENT_PLACEMENT_PRINCIPLE.md SS 2.4)."
} elseif ($uncLegacy.Count -gt 0) {
    $status = 'warn'
    $briefing = "EPP-03: legacy hook(s) still lack a verification surface (grandfathered): [$($uncLegacy -join ', ')]."
}

function ToJsonArr([string[]]$items) {
    if (-not $items -or $items.Count -eq 0) { return '' }
    return ($items | ForEach-Object { '"' + $_ + '"' }) -join ','
}

$out = '{{"status":"{0}","details":{{"applicable":true,"checked":{1},"covered":[{2}],"uncovered_new":[{3}],"uncovered_legacy":[{4}]}},"briefing":"{5}"}}' -f `
    $status, $checked, (ToJsonArr $covered), (ToJsonArr $uncNew), (ToJsonArr $uncLegacy), ($briefing -replace '"', '\"')
Write-Output $out
if ($status -eq 'fail') { exit 1 }
exit 0
