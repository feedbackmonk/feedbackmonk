# change-coupling oracle self-test (Windows) — scrutiny A2, Arc 1.
# Asserts: valid JSON, schema fields, and the NO-DATA path on a non-git dir.
$ErrorActionPreference = "Stop"
$oracleDir = $PSScriptRoot

function Assert-Json([string]$text, [string]$label) {
    try { return ($text | ConvertFrom-Json) }
    catch { Write-Error "FAIL: $label is not valid JSON`n$text"; exit 1 }
}

# --- 1. Real run --------------------------------------------------------------
$out = & powershell.exe -NoProfile -File (Join-Path $oracleDir "run.ps1") 2>&1 | Out-String
$out = $out.Trim()
$doc = Assert-Json $out "output"

foreach ($f in @('status','window','filters','filePairs','modulePairs','crossBoundaryTop','truncated','cached','briefing')) {
    if ($null -eq $doc.PSObject.Properties[$f]) { Write-Error "FAIL: missing schema field '$f'"; exit 1 }
}
foreach ($f in @('sinceDays','maxCommits','commitsAnalyzed','qualifyingCommits')) {
    if ($null -eq $doc.window.PSObject.Properties[$f]) { Write-Error "FAIL: missing window field '$f'"; exit 1 }
}
foreach ($f in @('bulkCommitMax','minCoChanges','excludedCommits')) {
    if ($null -eq $doc.filters.PSObject.Properties[$f]) { Write-Error "FAIL: missing filter field '$f'"; exit 1 }
}
if ($doc.status -ne 'ok' -and $doc.status -ne 'no-data') {
    Write-Error "FAIL: status not ok|no-data ('$($doc.status)')"; exit 1
}

# --- 2. NO-DATA path on a non-git directory ----------------------------------
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("cc-validate-" + [System.Guid]::NewGuid().ToString("N").Substring(0,8))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    Push-Location $tmp
    $nd = & powershell.exe -NoProfile -File (Join-Path $oracleDir "run.ps1") 2>&1 | Out-String
    Pop-Location
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
$nd = $nd.Trim()
$ndDoc = Assert-Json $nd "non-git output"
if ($ndDoc.status -ne 'no-data') { Write-Error "FAIL: non-git dir did not yield status no-data (NO-DATA honesty floor)`n$nd"; exit 1 }
if ($null -eq $ndDoc.PSObject.Properties['reason']) { Write-Error "FAIL: no-data result missing reason"; exit 1 }

Write-Output "PASS: change-coupling oracle validates"
exit 0
