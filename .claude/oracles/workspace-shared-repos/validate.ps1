# workspace-shared-repos oracle self-test (Windows PowerShell)
#
# Sandbox-builds eight scenarios and asserts the oracle output is correct:
#   T1. pnpm-workspace.yaml discovery (literal paths + glob expansion)
#   T2. Cargo.toml [workspace] members discovery
#   T3. package.json workspaces discovery (both array and object forms)
#   T4. .claude/config.json sharedRepos explicit-list discovery
#   T5. Multi-source dedup: same path declared in pnpm AND explicit -> explicit wins
#   T6. Skip non-git: declared path without .git/ is filtered out
#   T7. Skip self: declaration pointing back at the project itself is dropped
#   T8. Graceful empty: no declaration files -> {count:0, repos:[], discoveryMethod:"none"}

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$ORACLE_DIR = Split-Path -Parent $PSCommandPath
$script:PASS = 0
$script:FAIL = 0
$script:Sandbox = $null

function Assert-Pass {
    param([string]$Name)
    Write-Host "PASS: $Name"
    $script:PASS++
}
function Assert-Fail {
    param([string]$Name)
    Write-Host "FAIL: $Name" -ForegroundColor Red
    $script:FAIL++
}

# Cleanup any sandbox even on early exit.
trap {
    if ($null -ne $script:Sandbox -and (Test-Path $script:Sandbox)) {
        Remove-Item -Path $script:Sandbox -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function New-Sandbox {
    $tmpRoot = [System.IO.Path]::GetTempPath()
    $sb = Join-Path $tmpRoot ("wsro-" + [Guid]::NewGuid().ToString("N").Substring(0, 12))
    New-Item -ItemType Directory -Path $sb -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $sb "project/.claude/oracles/workspace-shared-repos") -Force | Out-Null
    Copy-Item -Path (Join-Path $ORACLE_DIR "run.ps1")      -Destination (Join-Path $sb "project/.claude/oracles/workspace-shared-repos/run.ps1") -Force
    Copy-Item -Path (Join-Path $ORACLE_DIR "oracle.json")  -Destination (Join-Path $sb "project/.claude/oracles/workspace-shared-repos/oracle.json") -Force
    $script:Sandbox = $sb
    return $sb
}

function Remove-Sandbox {
    if ($null -ne $script:Sandbox -and (Test-Path $script:Sandbox)) {
        Remove-Item -Path $script:Sandbox -Recurse -Force -ErrorAction SilentlyContinue
    }
    $script:Sandbox = $null
}

function New-FakeGitRepo {
    param([string]$Path)
    New-Item -ItemType Directory -Path (Join-Path $Path ".git") -Force | Out-Null
    Set-Content -Path (Join-Path $Path ".git/HEAD") -Value "" -Encoding UTF8
}

function New-PlainDir {
    param([string]$Path)
    New-Item -ItemType Directory -Path $Path -Force | Out-Null
    Set-Content -Path (Join-Path $Path "README.md") -Value "non-git dir" -Encoding UTF8
}

function Invoke-Oracle {
    $proj = Join-Path $script:Sandbox "project"
    Push-Location $proj
    try {
        # Capture stdout only; redirect stderr to a separate stream that we discard.
        # Out-String preserves whitespace; Trim() strips final newline.
        $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".claude/oracles/workspace-shared-repos/run.ps1" 2>$null | Out-String
        return $out.Trim()
    } finally {
        Pop-Location
    }
}

function Test-PathInOutput {
    param([string]$Output, [string]$Needle)
    return ($Output -match ('"path":"[^"]*' + [Regex]::Escape($Needle)))
}

function Test-ValidJson {
    param([string]$Json)
    try {
        $null = $Json | ConvertFrom-Json -ErrorAction Stop
        return $true
    } catch {
        return $false
    }
}

# -----------------------------------------------------------------------------
# T1: pnpm-workspace.yaml discovery
# -----------------------------------------------------------------------------
$sb = New-Sandbox
New-FakeGitRepo (Join-Path $sb "sibling-a")
New-FakeGitRepo (Join-Path $sb "sibling-b")
New-PlainDir   (Join-Path $sb "not-a-git-checkout")
New-FakeGitRepo (Join-Path $sb "glob-foo")
Copy-Item -Path (Join-Path $ORACLE_DIR "test-fixtures/pnpm-workspace/pnpm-workspace.yaml") `
          -Destination (Join-Path $sb "project/pnpm-workspace.yaml") -Force

$T1 = Invoke-Oracle
Write-Host "[T1 output]: $T1"
if (Test-ValidJson $T1) { Assert-Pass "T1 valid JSON" } else { Assert-Fail "T1 invalid JSON" }
if (Test-PathInOutput $T1 "/sibling-a") { Assert-Pass "T1 sibling-a in repos[]" } else { Assert-Fail "T1 sibling-a missing" }
if (Test-PathInOutput $T1 "/sibling-b") { Assert-Pass "T1 sibling-b in repos[]" } else { Assert-Fail "T1 sibling-b missing" }
if (Test-PathInOutput $T1 "/glob-foo")  { Assert-Pass "T1 glob-* expanded"      } else { Assert-Fail "T1 glob-* did not expand" }
if (Test-PathInOutput $T1 "/not-a-git-checkout") { Assert-Fail "T1 not-a-git-checkout should NOT be in output" } else { Assert-Pass "T1 not-a-git-checkout filtered out" }
if ($T1 -match '"discoveryMethod":"pnpm"')         { Assert-Pass "T1 discoveryMethod=pnpm" } else { Assert-Fail "T1 discoveryMethod wrong" }
if ($T1 -match '"declarationSource":"pnpm"')       { Assert-Pass "T1 declarationSource=pnpm" } else { Assert-Fail "T1 declarationSource wrong" }
Remove-Sandbox

# -----------------------------------------------------------------------------
# T2: Cargo.toml [workspace] members discovery
# -----------------------------------------------------------------------------
$sb = New-Sandbox
New-FakeGitRepo (Join-Path $sb "sibling-a")
New-FakeGitRepo (Join-Path $sb "sibling-b")
New-PlainDir   (Join-Path $sb "not-a-git-checkout")
Copy-Item -Path (Join-Path $ORACLE_DIR "test-fixtures/cargo-workspace/Cargo.toml") `
          -Destination (Join-Path $sb "project/Cargo.toml") -Force

$T2 = Invoke-Oracle
Write-Host "[T2 output]: $T2"
if (Test-ValidJson $T2) { Assert-Pass "T2 valid JSON" } else { Assert-Fail "T2 invalid JSON" }
if (Test-PathInOutput $T2 "/sibling-a") { Assert-Pass "T2 sibling-a in repos[]" } else { Assert-Fail "T2 sibling-a missing" }
if (Test-PathInOutput $T2 "/sibling-b") { Assert-Pass "T2 sibling-b in repos[]" } else { Assert-Fail "T2 sibling-b missing" }
if ($T2 -match '"discoveryMethod":"cargo"') { Assert-Pass "T2 discoveryMethod=cargo" } else { Assert-Fail "T2 discoveryMethod wrong" }
Remove-Sandbox

# -----------------------------------------------------------------------------
# T3: package.json workspaces (array form via fixture; object form inline)
# -----------------------------------------------------------------------------
$sb = New-Sandbox
New-FakeGitRepo (Join-Path $sb "sibling-a")
New-FakeGitRepo (Join-Path $sb "sibling-b")
New-PlainDir   (Join-Path $sb "not-a-git-checkout")
Copy-Item -Path (Join-Path $ORACLE_DIR "test-fixtures/npm-workspace/package.json") `
          -Destination (Join-Path $sb "project/package.json") -Force

$T3a = Invoke-Oracle
Write-Host "[T3a output]: $T3a"
if (Test-ValidJson $T3a) { Assert-Pass "T3a valid JSON" } else { Assert-Fail "T3a invalid JSON" }
if (Test-PathInOutput $T3a "/sibling-a") { Assert-Pass "T3a sibling-a in repos[]" } else { Assert-Fail "T3a sibling-a missing" }
if (Test-PathInOutput $T3a "/sibling-b") { Assert-Pass "T3a sibling-b in repos[]" } else { Assert-Fail "T3a sibling-b missing" }
if ($T3a -match '"discoveryMethod":"npm"') { Assert-Pass "T3a discoveryMethod=npm" } else { Assert-Fail "T3a discoveryMethod wrong" }

# Object form
$objForm = '{"name":"fixture-root","private":true,"workspaces":{"packages":["../sibling-a","../sibling-b"]}}'
Set-Content -Path (Join-Path $sb "project/package.json") -Value $objForm -Encoding UTF8

$T3b = Invoke-Oracle
Write-Host "[T3b output]: $T3b"
if (Test-PathInOutput $T3b "/sibling-a") { Assert-Pass "T3b object-form sibling-a in repos[]" } else { Assert-Fail "T3b object-form sibling-a missing" }
if (Test-PathInOutput $T3b "/sibling-b") { Assert-Pass "T3b object-form sibling-b in repos[]" } else { Assert-Fail "T3b object-form sibling-b missing" }
Remove-Sandbox

# -----------------------------------------------------------------------------
# T4: .claude/config.json sharedRepos discovery
# -----------------------------------------------------------------------------
$sb = New-Sandbox
New-FakeGitRepo (Join-Path $sb "sibling-a")
New-FakeGitRepo (Join-Path $sb "sibling-b")
New-PlainDir   (Join-Path $sb "not-a-git-checkout")
New-Item -ItemType Directory -Path (Join-Path $sb "project/.claude") -Force | Out-Null
Copy-Item -Path (Join-Path $ORACLE_DIR "test-fixtures/explicit-list/config.json") `
          -Destination (Join-Path $sb "project/.claude/config.json") -Force

$T4 = Invoke-Oracle
Write-Host "[T4 output]: $T4"
if (Test-ValidJson $T4) { Assert-Pass "T4 valid JSON" } else { Assert-Fail "T4 invalid JSON" }
if (Test-PathInOutput $T4 "/sibling-a") { Assert-Pass "T4 sibling-a in repos[]" } else { Assert-Fail "T4 sibling-a missing" }
if (Test-PathInOutput $T4 "/sibling-b") { Assert-Pass "T4 sibling-b in repos[]" } else { Assert-Fail "T4 sibling-b missing" }
if ($T4 -match '"discoveryMethod":"explicit"')   { Assert-Pass "T4 discoveryMethod=explicit" } else { Assert-Fail "T4 discoveryMethod wrong" }
if ($T4 -match '"declarationSource":"explicit"') { Assert-Pass "T4 declarationSource=explicit" } else { Assert-Fail "T4 declarationSource wrong" }
Remove-Sandbox

# -----------------------------------------------------------------------------
# T5: multi-source dedup -- same path in pnpm AND explicit; explicit wins.
# -----------------------------------------------------------------------------
$sb = New-Sandbox
New-FakeGitRepo (Join-Path $sb "sibling-a")
New-Item -ItemType Directory -Path (Join-Path $sb "project/.claude") -Force | Out-Null

Set-Content -Path (Join-Path $sb "project/pnpm-workspace.yaml") -Encoding UTF8 -Value @"
packages:
  - "../sibling-a"
"@
Set-Content -Path (Join-Path $sb "project/.claude/config.json") -Encoding UTF8 -Value '{"sharedRepos":[{"path":"../sibling-a"}]}'

$T5 = Invoke-Oracle
Write-Host "[T5 output]: $T5"
if (Test-ValidJson $T5) { Assert-Pass "T5 valid JSON" } else { Assert-Fail "T5 invalid JSON" }
if ($T5 -match '"count":1')                       { Assert-Pass "T5 dedup count=1" } else { Assert-Fail "T5 dedup count != 1" }
if ($T5 -match '"declarationSource":"explicit"')  { Assert-Pass "T5 explicit wins (declarationSource)" } else { Assert-Fail "T5 declarationSource wrong" }
if ($T5 -match '"discoveryMethod":"explicit"')    { Assert-Pass "T5 discoveryMethod=explicit" } else { Assert-Fail "T5 discoveryMethod wrong" }
Remove-Sandbox

# -----------------------------------------------------------------------------
# T6: skip non-git -- declared path WITHOUT .git/ is filtered
# -----------------------------------------------------------------------------
$sb = New-Sandbox
New-FakeGitRepo (Join-Path $sb "sibling-a")
New-PlainDir   (Join-Path $sb "no-git-here")

Set-Content -Path (Join-Path $sb "project/pnpm-workspace.yaml") -Encoding UTF8 -Value @"
packages:
  - "../sibling-a"
  - "../no-git-here"
"@

$T6 = Invoke-Oracle
Write-Host "[T6 output]: $T6"
if (Test-PathInOutput $T6 "/sibling-a")        { Assert-Pass "T6 sibling-a kept" }       else { Assert-Fail "T6 sibling-a missing" }
if (Test-PathInOutput $T6 "/no-git-here")      { Assert-Fail "T6 no-git-here should be filtered out" } else { Assert-Pass "T6 no-git-here filtered out" }
if ($T6 -match '"count":1')                    { Assert-Pass "T6 count=1" }              else { Assert-Fail "T6 count != 1" }
Remove-Sandbox

# -----------------------------------------------------------------------------
# T7: skip self -- declaration pointing back at the project itself is dropped
# -----------------------------------------------------------------------------
$sb = New-Sandbox
New-FakeGitRepo (Join-Path $sb "project")
New-FakeGitRepo (Join-Path $sb "sibling-a")
New-Item -ItemType Directory -Path (Join-Path $sb "project/.claude") -Force | Out-Null
Set-Content -Path (Join-Path $sb "project/.claude/config.json") -Encoding UTF8 -Value '{"sharedRepos":[{"path":"."},{"path":"../sibling-a"}]}'

$T7 = Invoke-Oracle
Write-Host "[T7 output]: $T7"
if (Test-PathInOutput $T7 "/sibling-a") { Assert-Pass "T7 sibling-a kept" } else { Assert-Fail "T7 sibling-a missing" }
if ($T7 -match '"count":1')             { Assert-Pass "T7 self-reference dropped (count=1)" } else { Assert-Fail "T7 self-reference not dropped" }
Remove-Sandbox

# -----------------------------------------------------------------------------
# T8: graceful empty
# -----------------------------------------------------------------------------
$sb = New-Sandbox
$T8 = Invoke-Oracle
Write-Host "[T8 output]: $T8"
if (Test-ValidJson $T8) { Assert-Pass "T8 valid JSON" } else { Assert-Fail "T8 invalid JSON" }
if ($T8 -match '"count":0')                  { Assert-Pass "T8 count=0" }              else { Assert-Fail "T8 count != 0" }
if ($T8 -match '"repos":\[\]')               { Assert-Pass "T8 repos[] empty" }        else { Assert-Fail "T8 repos[] not empty" }
if ($T8 -match '"discoveryMethod":"none"')   { Assert-Pass "T8 discoveryMethod=none" } else { Assert-Fail "T8 discoveryMethod wrong" }
if ($T8 -match '"_meta"')                    { Assert-Pass "T8 _meta block present" }  else { Assert-Fail "T8 _meta block missing" }
if ($T8 -match '"schemaVersion":1')          { Assert-Pass "T8 schemaVersion=1" }      else { Assert-Fail "T8 schemaVersion wrong" }
Remove-Sandbox

# -----------------------------------------------------------------------------
Write-Host "----"
Write-Host "Total: PASS=$($script:PASS)  FAIL=$($script:FAIL)"
if ($script:FAIL -gt 0) { exit 1 }
exit 0
