# stranded-dirty-files oracle self-test (Windows)
#
# Sandbox-builds five scenarios and asserts the oracle output matches the
# FROZEN output schema (oracle.json):
#
#   T1. no-stranded                 -> count==0, briefing==""
#   T2. small-stranded              -> count>0, briefing references "no live owner"
#   T3. large-stranded              -> count>=50, briefing references "significant accumulation"
#   T4. detection-skipped-too-many  -> count==-1, briefing references "detection skipped"
#   T5. live-peer-owns-file         -> peer's claimed file excluded from sample, count<dirty
#
# Each test creates a fresh git sandbox under $env:TEMP, runs the oracle from
# the project root, and asserts the JSON output's shape + key fields.

$ErrorActionPreference = 'Continue'
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$Pass = 0
$Fail = 0
function Pass($msg) { $script:Pass++; Write-Host "PASS: $msg" }
function Fail($msg) { $script:Fail++; Write-Host "FAIL: $msg" -ForegroundColor Red }

$Sandbox = $null

function Cleanup {
    if ($script:Sandbox -and (Test-Path -LiteralPath $script:Sandbox)) {
        Remove-Item -Recurse -Force -LiteralPath $script:Sandbox -ErrorAction SilentlyContinue
    }
    $script:Sandbox = $null
}

function Mk-Sandbox {
    $rawSandbox = Join-Path $env:TEMP ("sdfix-" + [guid]::NewGuid().ToString("N").Substring(0,8))
    $proj = Join-Path $rawSandbox "project"
    $oracleSub = Join-Path $proj ".claude/oracles/stranded-dirty-files"
    New-Item -ItemType Directory -Path $oracleSub -Force | Out-Null
    # Canonicalize to long path form. $env:TEMP often resolves to a DOS 8.3
    # short name (e.g. C:\Users\SOMEUS~1\...) on Windows, but the spawned
    # oracle's (Get-Location).Path returns the long form (C:\Users\someuser\...).
    # If we don't canonicalize, T5's workDir comparison (registry vs. oracle's
    # Get-Location) silently mismatches. Push-Location + Get-Location resolves
    # the long form via the filesystem.
    Push-Location $rawSandbox
    $script:Sandbox = (Get-Location).Path
    Pop-Location
    Copy-Item (Join-Path $oracleDir "run.ps1")      (Join-Path $oracleSub "run.ps1")      -Force
    Copy-Item (Join-Path $oracleDir "oracle.json")  (Join-Path $oracleSub "oracle.json")  -Force

    Push-Location $proj
    try {
        & git init -q -b main 2>$null
        if ($LASTEXITCODE -ne 0) { & git init -q 2>$null | Out-Null }
        & git config user.email "test@stranded.local"
        & git config user.name  "stranded-validate"
        Set-Content -Path "seed.txt" -Value "seed" -Encoding UTF8 -NoNewline
        & git add seed.txt 2>$null | Out-Null
        $env:GIT_AUTHOR_DATE = "2026-04-01T00:00:00Z"
        $env:GIT_COMMITTER_DATE = "2026-04-01T00:00:00Z"
        & git commit -q -m "seed commit" 2>$null | Out-Null
        Remove-Item Env:GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
        Remove-Item Env:GIT_COMMITTER_DATE -ErrorAction SilentlyContinue
    } finally {
        Pop-Location
    }
}

function Mk-OldDirty([string]$rel, [string]$content = "old") {
    $abs = Join-Path (Join-Path $script:Sandbox "project") $rel
    $dir = Split-Path -Parent $abs
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    Set-Content -Path $abs -Value $content -Encoding UTF8 -NoNewline
    # Force mtime to 2026-03-15 (before seed commit at 2026-04-01)
    $oldDate = [DateTime]::ParseExact("2026-03-15T00:00:00Z", "yyyy-MM-ddTHH:mm:ssZ", $null, [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal)
    (Get-Item -LiteralPath $abs).LastWriteTimeUtc = $oldDate
}

function Mk-NewDirty([string]$rel, [string]$content = "new") {
    $abs = Join-Path (Join-Path $script:Sandbox "project") $rel
    $dir = Split-Path -Parent $abs
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    Set-Content -Path $abs -Value $content -Encoding UTF8 -NoNewline
}

function Run-Oracle {
    # Spawned powershell.exe inherits the OS cwd ([Environment]::CurrentDirectory),
    # NOT the parent's Push-Location state. Set both before launching so the
    # oracle's (Get-Location).Path matches the sandbox project root.
    $proj = Join-Path $script:Sandbox "project"
    Push-Location $proj
    $prevEnvCwd = [Environment]::CurrentDirectory
    [Environment]::CurrentDirectory = $proj
    try {
        $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".claude/oracles/stranded-dirty-files/run.ps1" 2>&1
        return ($out | Out-String).Trim()
    } finally {
        [Environment]::CurrentDirectory = $prevEnvCwd
        Pop-Location
    }
}

function Assert-ValidJson($out, $label) {
    try {
        $null = $out | ConvertFrom-Json -ErrorAction Stop
        return $true
    } catch {
        Fail "${label}: output is not valid JSON: $out"
        return $false
    }
}

$schemaFields = @('has_stranded','count','oldest_mtime','sample','live_peer_count','last_finalize_at','briefing')

function Assert-SchemaFields($out, $label) {
    foreach ($f in $schemaFields) {
        if ($out -notmatch "`"$f`"") {
            Fail "${label}: missing schema field '$f' in: $out"
            return $false
        }
    }
    return $true
}

# -----------------------------------------------------------------------------
# T1. no-stranded
# -----------------------------------------------------------------------------
Mk-Sandbox
Mk-NewDirty "post-commit-mod.txt" "fresh"
$out = Run-Oracle
[void](Assert-ValidJson $out "T1")
[void](Assert-SchemaFields $out "T1")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.has_stranded -eq $false -and $obj.count -eq 0 -and [string]::IsNullOrEmpty($obj.briefing)) {
    Pass 'T1: no-stranded -> has_stranded=false count=0 briefing=""'
} else {
    Fail "T1: expected has_stranded=false count=0 briefing=''; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T2. small-stranded
# -----------------------------------------------------------------------------
Mk-Sandbox
Mk-OldDirty "stranded-1.txt"
Mk-OldDirty "stranded-2.txt"
Mk-OldDirty "stranded-3.txt"
$out = Run-Oracle
[void](Assert-ValidJson $out "T2")
[void](Assert-SchemaFields $out "T2")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.has_stranded -eq $true -and $obj.count -eq 3 -and $obj.briefing -match 'no live owner') {
    Pass "T2: small-stranded -> count=3 briefing references 'no live owner'"
} else {
    Fail "T2: expected has_stranded=true count=3 briefing matches 'no live owner'; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T3. large-stranded
# -----------------------------------------------------------------------------
Mk-Sandbox
1..55 | ForEach-Object { Mk-OldDirty ("stranded-$_.txt") }
$out = Run-Oracle
[void](Assert-ValidJson $out "T3")
[void](Assert-SchemaFields $out "T3")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.has_stranded -eq $true -and $obj.count -eq 55 -and $obj.briefing -match 'significant accumulation') {
    Pass "T3: large-stranded -> count=55 briefing references 'significant accumulation'"
} else {
    Fail "T3: expected has_stranded=true count=55 briefing references 'significant accumulation'; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T4. detection-skipped-too-many
# -----------------------------------------------------------------------------
Mk-Sandbox
$projRoot = Join-Path $script:Sandbox "project"
1..2001 | ForEach-Object {
    Set-Content -Path (Join-Path $projRoot ("f-$_.txt")) -Value "x" -Encoding UTF8 -NoNewline
}
$out = Run-Oracle
[void](Assert-ValidJson $out "T4")
[void](Assert-SchemaFields $out "T4")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
if ($null -ne $obj -and $obj.has_stranded -eq $false -and $obj.count -eq -1 -and $obj.briefing -match 'detection skipped') {
    Pass "T4: detection-skipped -> count=-1 briefing references 'detection skipped'"
} else {
    Fail "T4: expected has_stranded=false count=-1 briefing references 'detection skipped'; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# T5. live-peer-owns-file
# -----------------------------------------------------------------------------
Mk-Sandbox
Mk-OldDirty "peer-claimed.txt"
Mk-OldDirty "unclaimed.txt"
$projRoot = Join-Path $script:Sandbox "project"
$projRootNorm = ($projRoot -replace '\\', '/').TrimEnd('/')
$livePid = $PID
$collabDir = Join-Path $projRoot ".claude/collaboration"
New-Item -ItemType Directory -Path $collabDir -Force | Out-Null
$registry = @"
{
  "registryVersion": 2,
  "sessions": [
    {
      "id": "test-peer-1",
      "status": "active",
      "claudeShellPid": $livePid,
      "workDir": "$projRootNorm",
      "spawnedAt": "2026-05-07T00:00:00Z",
      "dirtyFiles": ["peer-claimed.txt"]
    }
  ],
  "closed": []
}
"@
Set-Content -Path (Join-Path $collabDir "active-sessions.json") -Value $registry -Encoding UTF8

$out = Run-Oracle
[void](Assert-ValidJson $out "T5")
[void](Assert-SchemaFields $out "T5")
$obj = $null; try { $obj = $out | ConvertFrom-Json } catch { }
$samplePaths = @()
if ($null -ne $obj -and $obj.sample) { $samplePaths = @($obj.sample | ForEach-Object { $_.path }) }
if ($null -ne $obj -and $obj.has_stranded -eq $true -and $obj.count -eq 1 -and $obj.live_peer_count -eq 1 -and ($samplePaths -contains 'unclaimed.txt') -and (-not ($samplePaths -contains 'peer-claimed.txt'))) {
    Pass "T5: live-peer-owns-file -> peer's claimed file excluded; count=1; live_peer_count=1"
} else {
    Fail "T5: expected count=1 live_peer_count=1 sample=[unclaimed.txt]; got $out"
}
Cleanup

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
Write-Host ""
Write-Host "================================================================"
Write-Host "  stranded-dirty-files validate: $Pass PASS / $Fail FAIL"
Write-Host "================================================================"

if ($Fail -gt 0) { exit 1 }
exit 0
