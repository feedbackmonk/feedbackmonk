# gitignore-template-drift oracle self-test (Windows PowerShell)
#
# Runs run.ps1 against each fixture in test-fixtures/ and compares output
# against the expected drift classification. Six cases (parallel to validate.sh).

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$fixtures = Join-Path $oracleDir "test-fixtures"

$script:Pass = 0
$script:Fail = 0

function Write-Pass($msg) { Write-Host "PASS: $msg"; $script:Pass++ }
function Write-FailMsg($msg) { Write-Host "FAIL: $msg" -ForegroundColor Red; $script:Fail++ }

function Invoke-Case {
    param(
        [string]$Name,
        [string]$Baseline,
        [string]$Project,
        [bool]$ExpDrifted,
        [int]$ExpMissing,
        [bool]$ExpBriefingNonEmpty
    )

    $env:CLAUDE_GITIGNORE_BASELINE = $Baseline
    $env:CLAUDE_GITIGNORE_PROJECT = $Project

    $runScript = Join-Path $oracleDir "run.ps1"
    $output = & powershell -NoProfile -File $runScript 2>&1
    $rc = $LASTEXITCODE

    Remove-Item Env:\CLAUDE_GITIGNORE_BASELINE -ErrorAction SilentlyContinue
    Remove-Item Env:\CLAUDE_GITIGNORE_PROJECT -ErrorAction SilentlyContinue

    if ($rc -ne 0) {
        Write-FailMsg "${Name}: run.ps1 exited $rc (output: $output)"
        return
    }

    $outputStr = ($output | Out-String).Trim()

    # Schema check — output must contain all 5 fields
    foreach ($field in @("drifted","missing_patterns","baseline_patterns","project_patterns","briefing")) {
        if ($outputStr -notmatch "`"$field`"") {
            Write-FailMsg "${Name}: missing schema field '$field' (out=$outputStr)"
            return
        }
    }

    try {
        $parsed = $outputStr | ConvertFrom-Json
    } catch {
        Write-FailMsg "${Name}: output is not valid JSON (out=$outputStr)"
        return
    }

    $gotDrifted = [bool]$parsed.drifted
    # missing_patterns may deserialize as array or single string in PS5; normalize.
    $missingArr = @($parsed.missing_patterns)
    if ($missingArr.Count -eq 1 -and ($null -eq $missingArr[0])) { $missingArr = @() }
    $gotMissing = $missingArr.Count
    $gotBriefing = if ($null -ne $parsed.briefing) { [string]$parsed.briefing } else { "" }

    if ($gotDrifted -ne $ExpDrifted) {
        Write-FailMsg "${Name}: drifted got='$gotDrifted' want='$ExpDrifted' (out=$outputStr)"
        return
    }
    if ($gotMissing -ne $ExpMissing) {
        Write-FailMsg "${Name}: missing_patterns count got=$gotMissing want=$ExpMissing (out=$outputStr)"
        return
    }
    if ($ExpBriefingNonEmpty) {
        if ([string]::IsNullOrEmpty($gotBriefing)) {
            Write-FailMsg "${Name}: briefing expected non-empty, got empty (out=$outputStr)"
            return
        }
        if ($gotBriefing -notmatch "/0-uldf-migrate-hygiene") {
            Write-FailMsg "${Name}: briefing missing /0-uldf-migrate-hygiene reference: '$gotBriefing'"
            return
        }
    } else {
        if (-not [string]::IsNullOrEmpty($gotBriefing)) {
            Write-FailMsg "${Name}: briefing expected empty, got '$gotBriefing' (out=$outputStr)"
            return
        }
    }

    Write-Pass "${Name} (drifted=$gotDrifted missing=$gotMissing)"
}

# ---- Run all 6 fixtures -----------------------------------------------------

Invoke-Case -Name "no-drift" `
    -Baseline (Join-Path $fixtures "no-drift\baseline.gitignore") `
    -Project  (Join-Path $fixtures "no-drift\project.gitignore") `
    -ExpDrifted $false -ExpMissing 0 -ExpBriefingNonEmpty $false

Invoke-Case -Name "1-pattern-missing" `
    -Baseline (Join-Path $fixtures "1-pattern-missing\baseline.gitignore") `
    -Project  (Join-Path $fixtures "1-pattern-missing\project.gitignore") `
    -ExpDrifted $true -ExpMissing 1 -ExpBriefingNonEmpty $true

Invoke-Case -Name "5-patterns-missing" `
    -Baseline (Join-Path $fixtures "5-patterns-missing\baseline.gitignore") `
    -Project  (Join-Path $fixtures "5-patterns-missing\project.gitignore") `
    -ExpDrifted $true -ExpMissing 5 -ExpBriefingNonEmpty $true

Invoke-Case -Name "project-has-extra-patterns" `
    -Baseline (Join-Path $fixtures "project-has-extra-patterns\baseline.gitignore") `
    -Project  (Join-Path $fixtures "project-has-extra-patterns\project.gitignore") `
    -ExpDrifted $false -ExpMissing 0 -ExpBriefingNonEmpty $false

Invoke-Case -Name "no-baseline-found" `
    -Baseline (Join-Path $fixtures "no-baseline-found\__nonexistent_baseline__.gitignore") `
    -Project  (Join-Path $fixtures "no-baseline-found\project.gitignore") `
    -ExpDrifted $false -ExpMissing 0 -ExpBriefingNonEmpty $false

Invoke-Case -Name "project-no-gitignore" `
    -Baseline (Join-Path $fixtures "project-no-gitignore\baseline.gitignore") `
    -Project  (Join-Path $fixtures "project-no-gitignore\__nonexistent_project__.gitignore") `
    -ExpDrifted $true -ExpMissing 13 -ExpBriefingNonEmpty $true

# ---- Summary ----------------------------------------------------------------
Write-Host "----"
Write-Host "Total: PASS=$($script:Pass)  FAIL=$($script:Fail)"
if ($script:Fail -gt 0) { exit 1 }
exit 0
