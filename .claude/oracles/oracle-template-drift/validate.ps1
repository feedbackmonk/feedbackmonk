# oracle-template-drift oracle self-test (Windows PowerShell)
#
# Builds synthetic baseline/project oracle trees in a tmpdir and runs run.ps1
# against each, asserting the expected drift classification. Nine cases,
# parallel to validate.sh (8/9 = the PACK-02 missing-starter extension).

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$run = Join-Path $oracleDir "run.ps1"

$script:Pass = 0
$script:Fail = 0
function Write-Pass($m) { Write-Host "PASS: $m"; $script:Pass++ }
function Write-FailMsg($m) { Write-Host "FAIL: $m" -ForegroundColor Red; $script:Fail++ }

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("otd-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null

function New-Oracle([string]$Root, [string]$Name, [string]$Variant) {
    $od = Join-Path $Root $Name
    New-Item -ItemType Directory -Path $od -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $od "oracle.json") -Value ('{"name":"' + $Name + '"}') -NoNewline
    Set-Content -LiteralPath (Join-Path $od "run.sh")  -Value ("#run $Variant")   -NoNewline
    Set-Content -LiteralPath (Join-Path $od "run.ps1") -Value ("#runps $Variant") -NoNewline
}

function Invoke-Oracle([string]$Baseline, [string]$Project) {
    $env:CLAUDE_ORACLE_BASELINE_DIR = $Baseline
    $env:CLAUDE_ORACLE_PROJECT_DIR  = $Project
    $out = & powershell -NoProfile -File $run 2>&1
    Remove-Item Env:\CLAUDE_ORACLE_BASELINE_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:\CLAUDE_ORACLE_PROJECT_DIR  -ErrorAction SilentlyContinue
    return ($out | Out-String).Trim()
}

function Test-Schema([string]$Label, [string]$Json) {
    foreach ($f in @("drifted","drifted_oracles","drift_count","compared_count","missing_starter","missing_count","briefing")) {
        if ($Json -notmatch "`"$f`"") { Write-FailMsg "${Label}: missing schema field '$f' (out=$Json)"; return $false }
    }
    return $true
}

# ---- Case 1: no-drift -------------------------------------------------------
$b = Join-Path $tmp "c1\b"; $p = Join-Path $tmp "c1\p"
New-Oracle $b "alpha" "v1"; New-Oracle $p "alpha" "v1"
$o = Invoke-Oracle $b $p
if (Test-Schema "no-drift" $o) {
    $j = $o | ConvertFrom-Json
    if ((-not $j.drifted) -and $j.drift_count -eq 0 -and $j.compared_count -eq 1 -and [string]::IsNullOrEmpty($j.briefing)) {
        Write-Pass "no-drift" } else { Write-FailMsg "no-drift: out=$o" }
}

# ---- Case 2: one-file-drift -------------------------------------------------
$b = Join-Path $tmp "c2\b"; $p = Join-Path $tmp "c2\p"
New-Oracle $b "alpha" "v1"; New-Oracle $p "alpha" "v1"
Set-Content -LiteralPath (Join-Path $p "alpha\run.sh") -Value "#run v2" -NoNewline
$o = Invoke-Oracle $b $p
if (Test-Schema "one-file-drift" $o) {
    $j = $o | ConvertFrom-Json
    if ($j.drifted -and $j.drift_count -eq 1 -and (@($j.drifted_oracles[0].files) -contains "run.sh") -and ($o -match "/0-uldf-migrate-oracles")) {
        Write-Pass "one-file-drift" } else { Write-FailMsg "one-file-drift: out=$o" }
}

# ---- Case 3: multi-oracle-drift ---------------------------------------------
$b = Join-Path $tmp "c3\b"; $p = Join-Path $tmp "c3\p"
New-Oracle $b "alpha" "v1"; New-Oracle $b "beta" "v1"
New-Oracle $p "alpha" "v1"; New-Oracle $p "beta" "v1"
Set-Content -LiteralPath (Join-Path $p "alpha\run.sh")  -Value "#run v2"   -NoNewline
Set-Content -LiteralPath (Join-Path $p "beta\run.ps1")  -Value "#runps v2" -NoNewline
$o = Invoke-Oracle $b $p
$j = $o | ConvertFrom-Json
if ($j.drift_count -eq 2 -and $j.compared_count -eq 2) { Write-Pass "multi-oracle-drift" } else { Write-FailMsg "multi-oracle-drift: out=$o" }

# ---- Case 4: pinned-skip ----------------------------------------------------
$b = Join-Path $tmp "c4\b"; $p = Join-Path $tmp "c4\p"
New-Oracle $b "alpha" "v1"; New-Oracle $p "alpha" "v1"
Set-Content -LiteralPath (Join-Path $p "alpha\run.sh") -Value "#run v2" -NoNewline
New-Item -ItemType File -Path (Join-Path $p "alpha\.local-customized") -Force | Out-Null
$o = Invoke-Oracle $b $p
$j = $o | ConvertFrom-Json
if ((-not $j.drifted) -and $j.drift_count -eq 0 -and $j.compared_count -eq 0) { Write-Pass "pinned-skip" } else { Write-FailMsg "pinned-skip: out=$o" }

# ---- Case 5: project-only-oracle --------------------------------------------
$b = Join-Path $tmp "c5\b"; $p = Join-Path $tmp "c5\p"
New-Item -ItemType Directory -Path $b -Force | Out-Null
New-Oracle $p "alpha" "v1"
$o = Invoke-Oracle $b $p
$j = $o | ConvertFrom-Json
if ((-not $j.drifted) -and $j.compared_count -eq 0) { Write-Pass "project-only-oracle" } else { Write-FailMsg "project-only-oracle: out=$o" }

# ---- Case 6: project-missing-file -------------------------------------------
$b = Join-Path $tmp "c6\b"; $p = Join-Path $tmp "c6\p"
New-Oracle $b "alpha" "v1"
New-Item -ItemType Directory -Path (Join-Path $p "alpha") -Force | Out-Null
Set-Content -LiteralPath (Join-Path $p "alpha\oracle.json") -Value '{"name":"alpha"}' -NoNewline
Set-Content -LiteralPath (Join-Path $p "alpha\run.sh")      -Value "#run v1"          -NoNewline
# project deliberately lacks run.ps1
$o = Invoke-Oracle $b $p
$j = $o | ConvertFrom-Json
if ($j.drifted -and $j.drift_count -eq 1 -and (@($j.drifted_oracles[0].files) -contains "run.ps1")) {
    Write-Pass "project-missing-file" } else { Write-FailMsg "project-missing-file: out=$o" }

# ---- Case 7: graceful-absent ------------------------------------------------
$p = Join-Path $tmp "c7\p"
New-Oracle $p "alpha" "v1"
$o = Invoke-Oracle (Join-Path $tmp "c7\__nonexistent_baseline__") $p
$j = $o | ConvertFrom-Json
if ((-not $j.drifted) -and [string]::IsNullOrEmpty($j.briefing)) { Write-Pass "graceful-absent" } else { Write-FailMsg "graceful-absent: out=$o" }

# ---- Case 8: missing-starter (PACK-02) ----------------------------------------
$b = Join-Path $tmp "c8\b"; $p = Join-Path $tmp "c8\p"
New-Oracle $b "alpha" "v1"; New-Oracle $b "beta" "v1"
New-Oracle $p "alpha" "v1"    # project lacks beta
$manifest = @(
    '{',
    '  "schemaVersion": 1,',
    '  "packs": {',
    '    "starter": [',
    '      "alpha",',
    '      "beta"',
    '    ],',
    '    "framework-internal": [',
    '    ]',
    '  }',
    '}'
) -join "`n"
Set-Content -LiteralPath (Join-Path $b "PACK_MANIFEST.json") -Value $manifest -NoNewline
$o = Invoke-Oracle $b $p
if (Test-Schema "missing-starter" $o) {
    $j = $o | ConvertFrom-Json
    if ((-not $j.drifted) -and $j.missing_count -eq 1 -and (@($j.missing_starter) -contains "beta") -and ($o -match "not installed")) {
        Write-Pass "missing-starter" } else { Write-FailMsg "missing-starter: out=$o" }
}

# ---- Case 9: no-manifest-graceful (pre-PACK-01 baseline) ----------------------
$b = Join-Path $tmp "c9\b"; $p = Join-Path $tmp "c9\p"
New-Oracle $b "alpha" "v1"; New-Oracle $b "beta" "v1"
New-Oracle $p "alpha" "v1"
$o = Invoke-Oracle $b $p
$j = $o | ConvertFrom-Json
if ($j.missing_count -eq 0 -and [string]::IsNullOrEmpty($j.briefing)) { Write-Pass "no-manifest-graceful" } else { Write-FailMsg "no-manifest-graceful: out=$o" }

Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue

Write-Host "----"
Write-Host "Total: PASS=$($script:Pass)  FAIL=$($script:Fail)"
if ($script:Fail -gt 0) { exit 1 }
exit 0
