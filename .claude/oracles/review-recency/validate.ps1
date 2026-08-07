# review-recency oracle self-test (Windows) — RECENCY-03/04.
$ErrorActionPreference = "Stop"
$oracleDir = $PSScriptRoot
$output = & powershell.exe -NoProfile -File (Join-Path $oracleDir "run.ps1") 2>&1 | Out-String
$output = $output.Trim()

try { $doc = $output | ConvertFrom-Json } catch {
    Write-Error "FAIL: output is not valid JSON`n$output"; exit 1
}

foreach ($field in @('recent', 'details', 'briefing')) {
    if ($null -eq $doc.PSObject.Properties[$field]) {
        Write-Error "FAIL: missing schema field '$field'"; exit 1
    }
}
foreach ($field in @('project', 'recentDays', 'recentSkills')) {
    if ($null -eq $doc.details.PSObject.Properties[$field]) {
        Write-Error "FAIL: missing details field '$field'"; exit 1
    }
}

# recent=false MUST carry an empty briefing (quiet-path invariant).
if (-not $doc.recent -and $doc.briefing -ne "") {
    Write-Error "FAIL: recent=false but briefing is non-empty (quiet-path invariant)`n$output"; exit 1
}

Write-Output "PASS: review-recency oracle validates"
exit 0
