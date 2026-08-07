# harness-capabilities oracle -- self-test (Windows). HAL-01.
#
# Byte-parallel with validate.sh: runs run.ps1 under each golden fixture's
# pinned environment and exact-matches stdout against the fixture file. Each
# fixture runs twice (determinism). Test seams (HARNESS_CAPS_FORCE_VERSION /
# HARNESS_CAPS_NOW) keep the goldens hermetic -- the real `claude --version`
# and real clock are never consulted during validation.

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$scriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$oracleRun   = Join-Path $scriptDir 'run.ps1'
$fixturesDir = Join-Path $scriptDir 'test-fixtures'

if (-not (Test-Path -LiteralPath $fixturesDir -PathType Container)) {
    Write-Error "validate.ps1: no test-fixtures directory at $fixturesDir"
    exit 2
}

# Env-per-fixture mapping ($null = ensure unset). Mirrors validate.sh exactly.
$fixtureEnv = @{
    'claude-full-2.1.206' = @{
        CLAUDECODE = '1'; HARNESS_CAPS_FORCE_VERSION = '2.1.206 (Claude Code)'
        CLAUDE_CODE_DISABLE_WORKFLOWS = $null; CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = $null
    }
    'claude-teams-opt-in' = @{
        CLAUDECODE = '1'; HARNESS_CAPS_FORCE_VERSION = '2.1.206 (Claude Code)'
        CLAUDE_CODE_DISABLE_WORKFLOWS = $null; CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = '1'
    }
    'claude-workflows-disabled' = @{
        CLAUDECODE = '1'; HARNESS_CAPS_FORCE_VERSION = '2.1.206 (Claude Code)'
        CLAUDE_CODE_DISABLE_WORKFLOWS = '1'; CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = $null
    }
    'claude-old-2.1.150' = @{
        CLAUDECODE = '1'; HARNESS_CAPS_FORCE_VERSION = '2.1.150 (Claude Code)'
        CLAUDE_CODE_DISABLE_WORKFLOWS = $null; CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = $null
    }
    'claude-version-unknown' = @{
        CLAUDECODE = '1'; HARNESS_CAPS_FORCE_VERSION = 'garbage'
        CLAUDE_CODE_DISABLE_WORKFLOWS = $null; CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = $null
    }
    'non-claude-all-false' = @{
        CLAUDECODE = $null; CLAUDE_CODE_SESSION_ID = $null
        CLAUDE_CODE_DISABLE_WORKFLOWS = $null; CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = $null
    }
}

# All env vars any fixture touches (saved/restored around each run).
$touchedVars = @('CLAUDECODE', 'CLAUDE_CODE_SESSION_ID', 'HARNESS_CAPS_FORCE_VERSION',
                 'HARNESS_CAPS_NOW', 'CLAUDE_CODE_DISABLE_WORKFLOWS',
                 'CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS')

function Run-Fixture([hashtable]$envMap) {
    $saved = @{}
    foreach ($v in $touchedVars) { $saved[$v] = [Environment]::GetEnvironmentVariable($v) }
    try {
        foreach ($v in $touchedVars) { [Environment]::SetEnvironmentVariable($v, $null) }
        [Environment]::SetEnvironmentVariable('HARNESS_CAPS_NOW', '2026-07-10T00:00:00Z')
        foreach ($k in $envMap.Keys) { [Environment]::SetEnvironmentVariable($k, $envMap[$k]) }
        return (& powershell -NoProfile -ExecutionPolicy Bypass -File $oracleRun 2>&1) -join "`n"
    } finally {
        foreach ($v in $touchedVars) { [Environment]::SetEnvironmentVariable($v, $saved[$v]) }
    }
}

$pass = 0
$fail = 0
$failedNames = @()

foreach ($expectedFile in (Get-ChildItem -Path $fixturesDir -Filter '*.json' | Sort-Object Name)) {
    $name = $expectedFile.BaseName
    if (-not $fixtureEnv.ContainsKey($name)) {
        Write-Output "SKIP: $name (no env mapping in validate.ps1 -- add one)"
        continue
    }
    $actual  = (Run-Fixture $fixtureEnv[$name]) -replace "[`r`n]", ''
    $actual2 = (Run-Fixture $fixtureEnv[$name]) -replace "[`r`n]", ''
    $expected = (Get-Content -LiteralPath $expectedFile.FullName -Raw) -replace "[`r`n]", ''

    if ($actual -ne $actual2) {
        $fail++; $failedNames += "$name(non-deterministic)"
        Write-Output "FAIL: $name -- non-deterministic across two runs"
        continue
    }
    if ($actual -eq $expected) {
        $pass++; Write-Output "PASS: $name"
    } else {
        $fail++; $failedNames += $name
        Write-Output "FAIL: $name"
        Write-Output "  expected: $expected"
        Write-Output "  actual:   $actual"
    }
}

Write-Output "---"
Write-Output "validate.ps1: $pass passed, $fail failed"
if ($fail -gt 0) {
    Write-Output ("Failed fixtures: " + ($failedNames -join ' '))
    exit 1
}
exit 0
