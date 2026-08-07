# code-graph oracle self-test (Windows) — scrutiny Arc 3 A1.
# Asserts: valid JSON, the frozen schema fields, a legal status, the MANDATORY
# coverage field, and the NO-DATA path on a non-git directory.
$ErrorActionPreference = "Stop"
$oracleDir = $PSScriptRoot

function Assert-Json([string]$text, [string]$label) {
    try { return ($text | ConvertFrom-Json) }
    catch { Write-Error "FAIL: $label is not valid JSON`n$text"; exit 1 }
}

# --- 1. Real run (default summary) -------------------------------------------
$out = & powershell.exe -NoProfile -File (Join-Path $oracleDir "run.ps1") --compact 2>&1 | Out-String
$out = $out.Trim()
$doc = Assert-Json $out "output"

foreach ($f in @('status','schemaVersion','query','result','coverage','coverageNote','truncated','briefing')) {
    if ($null -eq $doc.PSObject.Properties[$f]) { Write-Error "FAIL: missing schema field '$f'"; exit 1 }
}
foreach ($f in @('verb','target','transitive')) {
    if ($null -eq $doc.query.PSObject.Properties[$f]) { Write-Error "FAIL: missing query field '$f'"; exit 1 }
}
if ($doc.status -ne 'ok' -and $doc.status -ne 'no-data') {
    Write-Error "FAIL: status not ok|no-data ('$($doc.status)')"; exit 1
}
if ($doc.coverage -notin @('full','grep-only','none')) {
    Write-Error "FAIL: coverage not full|grep-only|none ('$($doc.coverage)')"; exit 1
}

# --- 2. --cycles verb answers with a legal coverage --------------------------
$cout = & powershell.exe -NoProfile -File (Join-Path $oracleDir "run.ps1") --cycles --compact 2>&1 | Out-String
$cdoc = Assert-Json ($cout.Trim()) "--cycles output"
if ($cdoc.coverage -notin @('full','grep-only','none')) {
    Write-Error "FAIL: --cycles missing legal coverage ('$($cdoc.coverage)')"; exit 1
}

# --- 3. NO-DATA path on a non-git directory ----------------------------------
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("cg-validate-" + [System.Guid]::NewGuid().ToString("N").Substring(0,8))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    Push-Location $tmp
    $nd = & powershell.exe -NoProfile -File (Join-Path $oracleDir "run.ps1") --compact 2>&1 | Out-String
    Pop-Location
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
$nd = $nd.Trim()
$ndDoc = Assert-Json $nd "non-git output"
if ($ndDoc.status -ne 'no-data') { Write-Error "FAIL: non-git dir did not yield status no-data (NO-DATA honesty floor)`n$nd"; exit 1 }
if ($null -eq $ndDoc.PSObject.Properties['reason']) { Write-Error "FAIL: no-data result missing reason"; exit 1 }

Write-Output "PASS: code-graph oracle validates"
exit 0
