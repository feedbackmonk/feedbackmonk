# env-preflight oracle self-test (Windows) -- LTADS-ENV-01.
# Confirms run.ps1 emits valid JSON carrying the frozen schema fields and the
# universal check entries. ASCII-only strings (PW-005 lineage).
$ErrorActionPreference = 'Stop'

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$output = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir 'run.ps1') 2>&1 | Out-String

try {
    $parsed = $output | ConvertFrom-Json
} catch {
    Write-Host "FAIL: output is not valid JSON"
    exit 1
}

foreach ($field in @('ok', 'checks', 'critical_failures', 'warnings', 'summary', 'briefing')) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Host "FAIL: missing schema field '$field'"
        exit 1
    }
}

$names = @($parsed.checks | ForEach-Object { $_.name })
# PROV-01/03/08 probes covered below.
foreach ($check in @('git', 'json-parser', 'python', 'node', 'playwright', 'adb', 'node-deps', 'workspace-built', 'jdk', 'version-drift', 'mobile-mcp')) {
    if ($names -notcontains $check) {
        Write-Host "FAIL: missing check entry '$check'"
        exit 1
    }
}

$gitCheck = $parsed.checks | Where-Object { $_.name -eq 'git' } | Select-Object -First 1
if ($gitCheck.status -ne 'available') {
    Write-Host "FAIL: git check not 'available' in a git work tree"
    exit 1
}

# --- mobile-mcp trust reporting (DEFER-PROV-MCP-REACH) ----------------------
# Registration in .mcp.json is necessary but NOT sufficient -- Claude Code does
# not start a project-scoped server the project has not trusted. Assert the
# probe never reports 'available' on registration alone.
$tdir = Join-Path ([System.IO.Path]::GetTempPath()) ("env-preflight-mcp-" + [System.Guid]::NewGuid().ToString('N').Substring(0, 8))
$proj = Join-Path $tdir 'proj'
$fakeHome = Join-Path $tdir 'home'
New-Item -ItemType Directory -Path (Join-Path $proj 'android') -Force | Out-Null
New-Item -ItemType Directory -Path $fakeHome -Force | Out-Null
Set-Content -LiteralPath (Join-Path $proj 'package.json') -Value '{"name":"x"}' -Encoding ASCII
Set-Content -LiteralPath (Join-Path $proj '.mcp.json') -Value '{"mcpServers":{"maestro":{"command":"bash","args":["-c","mobile-mcp"]}}}' -Encoding ASCII
$projKey = ((Get-Item -LiteralPath $proj).FullName -replace '\\', '/')

$savedHome = $env:HOME
try {
    function Get-McpStatus {
        param([string]$TrustJson)
        $cfg = Join-Path $fakeHome '.claude.json'
        if ($TrustJson -eq '-') {
            Remove-Item -LiteralPath $cfg -Force -ErrorAction SilentlyContinue
        } else {
            # multi-key to match the .sh fixture shape (jq-CRLF blind spot;
            # a single-key config is the one shape that cannot catch it)
            Set-Content -LiteralPath $cfg -Value ('{"projects":{"a:/decoy/before":{},"' + $projKey + '":' + $TrustJson + ',"z:/decoy/after":{}}}') -Encoding ASCII
        }
        $env:HOME = $fakeHome
        Push-Location $proj
        $raw = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $oracleDir 'run.ps1') 2>$null | Out-String
        Pop-Location
        $p = $null
        try { $p = $raw | ConvertFrom-Json } catch { return '' }
        $c = $p.checks | Where-Object { $_.name -eq 'mobile-mcp' } | Select-Object -First 1
        if ($null -eq $c) { return '' }
        return [string]$c.status
    }

    $cases = @(
        @{ Expect = 'degraded';    Trust = '{"enabledMcpjsonServers":[],"disabledMcpjsonServers":[]}'; Label = "registered-but-untrusted must not be 'available'" },
        @{ Expect = 'available';   Trust = '{"enableAllProjectMcpServers":true}';                      Label = 'enableAllProjectMcpServers=true is trusted' },
        @{ Expect = 'available';   Trust = '{"enabledMcpjsonServers":["maestro"]}';                    Label = 'server named in enabledMcpjsonServers is trusted' },
        @{ Expect = 'unavailable'; Trust = '{"disabledMcpjsonServers":["maestro"]}';                   Label = 'explicitly-disabled server' },
        @{ Expect = 'degraded';    Trust = '{}';                                                        Label = 'empty project entry is untrusted' },
        @{ Expect = 'degraded';    Trust = '-';                                                         Label = 'absent ~/.claude.json is NO-DATA, not a pass' }
    )
    foreach ($case in $cases) {
        $got = Get-McpStatus $case.Trust
        if ($got -ne $case.Expect) {
            Write-Host ("FAIL: mobile-mcp " + $case.Label + ": expected '" + $case.Expect + "', got '" + $got + "'")
            exit 1
        }
    }
} finally {
    $env:HOME = $savedHome
    Remove-Item -LiteralPath $tdir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "PASS: env-preflight oracle validates"
exit 0
