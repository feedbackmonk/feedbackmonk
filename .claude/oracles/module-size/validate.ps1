# module-size oracle self-test (Windows). Asserts run.ps1 emits valid JSON with
# the frozen schema fields, a legal status, and the advisory contract.
$ErrorActionPreference = "Stop"
$oracleDir = $PSScriptRoot
$output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir "run.ps1") 2>&1 | Out-String
$output = $output.Trim()

try { $doc = $output | ConvertFrom-Json } catch {
    Write-Error "FAIL: output is not valid JSON`n$output"; exit 1
}

foreach ($field in @('status','modules_scanned','over_band','over_band_total','no_readme','no_data','band','enum_mode','briefing')) {
    if ($null -eq $doc.PSObject.Properties[$field]) {
        Write-Error "FAIL: missing schema field '$field'"; exit 1
    }
}
if ($null -eq $doc.band.PSObject.Properties['softTokens']) {
    Write-Error "FAIL: missing band.softTokens"; exit 1
}
if ($doc.status -notin @('pass','warn','no-data')) {
    Write-Error "FAIL: illegal status value '$($doc.status)'"; exit 1
}
# A clean pass MUST carry an empty briefing (suppressed-line convention).
if ($doc.status -eq 'pass' -and $doc.briefing -ne '') {
    Write-Error "FAIL: status=pass but briefing is non-empty (quiet-path invariant)"; exit 1
}

Write-Output "PASS: module-size oracle validates"
exit 0
