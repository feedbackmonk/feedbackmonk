# env-preflight oracle (Windows) -- LTADS-ENV-01 (skill-corpus scrutiny 04 ADD-3 / DEC-124).
#
# PowerShell parity of run.sh -- see run.sh header for the full contract.
# READ-ONLY by contract (ORACULURGY_DESIGN.md Part 11 sec 11.3.4). Frozen
# output schema (README.md): {ok, checks:[{name,status,critical,detail,fix}],
# critical_failures, warnings, summary, briefing}. Exit code always 0.
#
# ASCII-only strings (PW-005 lineage).

$ErrorActionPreference = 'SilentlyContinue'

$script:Checks = New-Object System.Collections.ArrayList
$script:CritFails = 0
$script:Warnings = 0

function Add-Check {
    param([string]$Name, [string]$Status, [bool]$Critical, [string]$Detail, [string]$Fix)
    [void]$script:Checks.Add(@{
        name = $Name; status = $Status; critical = $Critical; detail = $Detail; fix = $Fix
    })
    if ($Status -eq 'unavailable' -and $Critical) { $script:CritFails++ }
    elseif ($Status -eq 'degraded' -or ($Status -eq 'unavailable' -and -not $Critical)) { $script:Warnings++ }
}

function Test-Cmd {
    param([string]$Name)
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

# ---- git (critical) ---------------------------------------------------------
if (Test-Cmd 'git') {
    git rev-parse --is-inside-work-tree *> $null
    if ($LASTEXITCODE -eq 0) {
        Add-Check 'git' 'available' $true 'git present; inside a work tree' ''
    } else {
        Add-Check 'git' 'degraded' $true 'git present but this directory is not a git work tree' 'git init (or run from the project root)'
    }
} else {
    Add-Check 'git' 'unavailable' $true 'git not found on PATH' 'install git'
}

# ---- working python probe (PW-005 Windows Store stub detection) -------------
$py = ''
foreach ($cand in @('python3', 'python')) {
    if (Test-Cmd $cand) {
        & $cand -c 'pass' *> $null
        if ($LASTEXITCODE -eq 0) { $py = $cand; break }
    }
}

# ---- json-parser (warn) ------------------------------------------------------
if (Test-Cmd 'jq') {
    Add-Check 'json-parser' 'available' $false 'jq present' ''
} elseif ($py) {
    Add-Check 'json-parser' 'available' $false "no jq; working $py used as JSON parser" ''
} else {
    Add-Check 'json-parser' 'degraded' $false 'no jq and no working python -- framework registry/oracle scripts degrade' 'install jq (preferred) or a real python'
}

# ---- python (warn) ------------------------------------------------------------
if ($py) {
    Add-Check 'python' 'available' $false "working python: $py" ''
} elseif ((Test-Cmd 'python3') -or (Test-Cmd 'python')) {
    Add-Check 'python' 'degraded' $false 'python binary on PATH is non-functional (Windows Store stub, PW-005)' 'install real python or disable the WindowsApps alias'
} else {
    Add-Check 'python' 'unavailable' $false 'no python on PATH' 'install python (optional; jq covers JSON parsing)'
}

# ---- node (critical only when package.json exists) --------------------------
if (Test-Path 'package.json') {
    if (Test-Cmd 'node') {
        $nv = (& node --version 2>$null | Select-Object -First 1)
        Add-Check 'node' 'available' $true "package.json present; node $nv found" ''
    } else {
        Add-Check 'node' 'unavailable' $true 'package.json present but node not on PATH -- build/test will fail' 'install Node.js'
    }
} else {
    Add-Check 'node' 'not-applicable' $false 'no package.json' ''
}

# ---- playwright (warn only when a playwright config exists) -----------------
$pwCfg = ''
foreach ($c in @('playwright.config.ts', 'playwright.config.js', 'playwright.config.mjs')) {
    if (Test-Path $c) { $pwCfg = $c; break }
}
if ($pwCfg) {
    if ((Test-Path 'node_modules/playwright') -or (Test-Path 'node_modules/@playwright')) {
        Add-Check 'playwright' 'available' $false "$pwCfg present; playwright installed in node_modules" ''
    } else {
        Add-Check 'playwright' 'degraded' $false "$pwCfg present but playwright not installed" 'npm install && npx playwright install'
    }
} else {
    Add-Check 'playwright' 'not-applicable' $false 'no playwright config' ''
}

# ---- shared Android/mobile marker detection ----------------------------------
# Widened for monorepo layouts (apps/*/android, packages/*/android, Expo app.json).
# (apps/*/android, packages/*/android) + Expo app.json -- shared by the adb,
# jdk, and mobile-mcp probes so all mobile checks gate consistently (PROV-01/03).
$android = (Test-Path 'android') -or (Test-Path 'AndroidManifest.xml') -or (Test-Path 'app/build.gradle') -or (Test-Path 'build.gradle')
if (-not $android) {
    foreach ($glob in @('apps/*/android', 'packages/*/android')) {
        if (@(Resolve-Path -Path $glob -ErrorAction SilentlyContinue).Count -gt 0) { $android = $true; break }
    }
}
if (-not $android -and (Test-Path 'app.json')) {
    $rawAppJson = Get-Content 'app.json' -Raw -ErrorAction SilentlyContinue
    if ($rawAppJson -and $rawAppJson.Contains('"expo"')) { $android = $true }
}
# RN/Expo mobile marker (mobile-mcp gate, MSG-001)
$mobile = $android
if (-not $mobile -and (Test-Path 'app.json') -and (Test-Path 'package.json')) {
    $rawPkg = Get-Content 'package.json' -Raw -ErrorAction SilentlyContinue
    if ($rawPkg -and ($rawPkg -match '"(react-native|expo)"')) { $mobile = $true }
}

# ---- adb (warn only when Android markers exist) ------------------------------
if ($android) {
    if (Test-Cmd 'adb') {
        # Bounded live probe (2s) -- `adb devices` may auto-start a server.
        $job = Start-Job -ScriptBlock { adb devices 2>$null }
        $deviceCount = $null
        if (Wait-Job $job -Timeout 2) {
            $out = Receive-Job $job
            $deviceCount = @($out | Select-Object -Skip 1 | Where-Object { $_ -match '\sdevice\s*$' }).Count
        }
        Remove-Job $job -Force *> $null
        if ($null -ne $deviceCount -and $deviceCount -gt 0) {
            Add-Check 'adb' 'available' $false "Android markers present; $deviceCount device(s)/emulator(s) connected" ''
        } elseif ($null -ne $deviceCount) {
            Add-Check 'adb' 'degraded' $false 'adb present but no device/emulator connected' 'start an emulator (emulator -avd <name>) or connect a device, then adb devices'
        } else {
            Add-Check 'adb' 'degraded' $false 'adb present; live device probe timed out' 'verify manually: adb devices'
        }
    } else {
        Add-Check 'adb' 'degraded' $false 'Android markers present but adb not on PATH' 'install Android SDK platform-tools'
    }
} else {
    Add-Check 'adb' 'not-applicable' $false 'no Android markers' ''
}

# ---- node-deps (warn only when package.json exists) -- PROV-01 wall 1 --------
# PROV-01 check-leg probes (node-deps,
# workspace-built, jdk, version-drift) + PROV-03 mobile-mcp probe (spec by
# CLAUDE-E, MSG-001). Parity with run.sh; additive, marker-gated, read-only.
if (Test-Path 'package.json') {
    $pm = 'npm'
    if (Test-Path 'pnpm-lock.yaml') { $pm = 'pnpm' }
    elseif (Test-Path 'yarn.lock') { $pm = 'yarn' }
    $nmPresent = $false
    if (Test-Path 'node_modules' -PathType Container) {
        if (@(Get-ChildItem 'node_modules' -Force -ErrorAction SilentlyContinue | Select-Object -First 1).Count -gt 0) { $nmPresent = $true }
    }
    if ($nmPresent) {
        Add-Check 'node-deps' 'available' $false "node_modules present ($pm project)" ''
    } else {
        Add-Check 'node-deps' 'degraded' $false 'node_modules absent -- build/dev/test will fail until installed' "run the provisioner: provision.ps1 provision deps (or $pm install)"
    }
} else {
    Add-Check 'node-deps' 'not-applicable' $false 'no package.json' ''
}

# ---- workspace-built (warn only when workspace markers exist) -- wall 4 -------
$workspace = Test-Path 'pnpm-workspace.yaml'
if (-not $workspace -and (Test-Path 'package.json')) {
    $rawPkgWs = Get-Content 'package.json' -Raw -ErrorAction SilentlyContinue
    if ($rawPkgWs -and $rawPkgWs.Contains('"workspaces"')) { $workspace = $true }
}
if ($workspace) {
    $unbuilt = @()
    foreach ($glob in @('packages/*', 'apps/*', 'libs/*')) {
        foreach ($dir in @(Resolve-Path -Path $glob -ErrorAction SilentlyContinue)) {
            $d = $dir.Path
            if (-not (Test-Path -LiteralPath $d -PathType Container)) { continue }
            $pkg = Join-Path $d 'package.json'
            if (-not (Test-Path -LiteralPath $pkg)) { continue }
            $raw = Get-Content -LiteralPath $pkg -Raw -ErrorAction SilentlyContinue
            if (-not ($raw -match '"build"\s*:')) { continue }
            $built = $false
            foreach ($outDir in @('dist', 'lib', 'build')) {
                if (Test-Path -LiteralPath (Join-Path $d $outDir) -PathType Container) { $built = $true; break }
            }
            if (-not $built) { $unbuilt += (Split-Path -Leaf (Split-Path -Parent $d)) + '/' + (Split-Path -Leaf $d) }
        }
    }
    if ($unbuilt.Count -eq 0) {
        Add-Check 'workspace-built' 'available' $false 'all workspace packages with build scripts have build output' ''
    } else {
        Add-Check 'workspace-built' 'degraded' $false "$($unbuilt.Count) unbuilt workspace package(s): $($unbuilt -join ' ') -- dev servers cannot resolve them (wall 4)" 'run the provisioner: provision.ps1 provision workspace'
    }
} else {
    Add-Check 'workspace-built' 'not-applicable' $false 'no workspace markers' ''
}

# ---- jdk (warn only when Android markers exist) -- PROV-08 wall 6 -------------
function Get-JdkRelMajor {
    param([string]$JdkDir)
    $rel = Join-Path $JdkDir 'release'
    if (-not (Test-Path -LiteralPath $rel)) { return '' }
    $line = Get-Content -LiteralPath $rel -ErrorAction SilentlyContinue | Where-Object { $_ -match '^JAVA_VERSION=' } | Select-Object -First 1
    if ($line -match 'JAVA_VERSION="?([0-9.]+)') {
        $parts = $Matches[1].Split('.')
        if ($parts[0] -eq '1' -and $parts.Count -gt 1) { return $parts[1] }
        return $parts[0]
    }
    return ''
}
function Get-JdkExecMajor {
    param([string]$JavaBin)
    $out = & $JavaBin -version 2>&1 | Select-Object -First 1
    if ($out -match 'version "?([0-9._]+)') {
        $parts = $Matches[1].Split('.')
        if ($parts[0] -eq '1' -and $parts.Count -gt 1) { return ($parts[1] -split '_')[0] }
        return $parts[0]
    }
    return ''
}
if ($android) {
    $jdkSel = ''
    $jdkSrc = ''
    $jdkPathV = ''
    if (Test-Cmd 'java') { $jdkPathV = Get-JdkExecMajor 'java' }
    $cands = @()
    if ($env:PROVISION_JBR_CANDIDATES) {
        $cands = @($env:PROVISION_JBR_CANDIDATES -split ';' | Where-Object { $_ })
    } else {
        if ($env:LOCALAPPDATA) { $cands += (Join-Path $env:LOCALAPPDATA 'Programs\Android Studio\jbr') }
        $cands += 'C:\Program Files\Android\Android Studio\jbr'
        if ($env:ProgramFiles) { $cands += (Join-Path $env:ProgramFiles 'Android\Android Studio\jbr') }
    }
    foreach ($c in $cands) {
        if (-not (Test-Path -LiteralPath $c -PathType Container)) { continue }
        $m = Get-JdkRelMajor $c
        if (-not $m) {
            $bin = Join-Path $c 'bin\java.exe'
            if (Test-Path -LiteralPath $bin) { $m = Get-JdkExecMajor $bin }
        }
        if ($m -and ([int]$m -ge 17)) { $jdkSel = "$m ($c)"; $jdkSrc = 'jbr'; break }
    }
    if (-not $jdkSel -and $env:JAVA_HOME -and (Test-Path -LiteralPath $env:JAVA_HOME -PathType Container)) {
        $m = Get-JdkRelMajor $env:JAVA_HOME
        if (-not $m) {
            $bin = Join-Path $env:JAVA_HOME 'bin\java.exe'
            if (Test-Path -LiteralPath $bin) { $m = Get-JdkExecMajor $bin }
        }
        if ($m -and ([int]$m -ge 17)) { $jdkSel = "$m ($($env:JAVA_HOME))"; $jdkSrc = 'java_home' }
    }
    if (-not $jdkSel -and $jdkPathV -and ([int]$jdkPathV -ge 17)) { $jdkSel = "$jdkPathV (PATH)"; $jdkSrc = 'path' }
    $pathVNote = 'none'
    if ($jdkPathV) { $pathVNote = $jdkPathV }
    if ($jdkSel) {
        Add-Check 'jdk' 'available' $false "JDK $jdkSel via $jdkSrc; PATH java: $pathVNote" ''
    } elseif ($jdkPathV) {
        Add-Check 'jdk' 'degraded' $false "only JDK $jdkPathV found -- Android Gradle needs 17+ (wall 6)" 'install Android Studio (bundles JBR 21) or a JDK 17+; the provisioner records the selection (provision toolchain)'
    } else {
        Add-Check 'jdk' 'degraded' $false 'no JDK found (JBR candidates, JAVA_HOME, PATH probed) -- Android builds will fail' 'install Android Studio (bundles JBR 21) or a JDK 17+'
    }
} else {
    Add-Check 'jdk' 'not-applicable' $false 'no Android markers' ''
}

# ---- version-drift (advisory; config-driven latest-known majors) -- wall 9 ----
# Table: .claude/config.json provision.latestKnownMajors. No table => NO-DATA.
$driftGate = $false
if ((Test-Path 'package.json') -and (Test-Path '.claude/config.json')) {
    $rawCfg = Get-Content '.claude/config.json' -Raw -ErrorAction SilentlyContinue
    if ($rawCfg -and $rawCfg.Contains('"latestKnownMajors"')) { $driftGate = $true }
}
if ($driftGate) {
    $table = $null
    try {
        $cfg = $rawCfg | ConvertFrom-Json
        if ($cfg.provision -and $cfg.provision.latestKnownMajors) { $table = $cfg.provision.latestKnownMajors }
    } catch { }
    if ($null -eq $table) {
        Add-Check 'version-drift' 'not-applicable' $false 'latestKnownMajors table empty or unparsable' ''
    } else {
        $drifts = @()
        foreach ($prop in $table.PSObject.Properties) {
            $pf = Join-Path (Join-Path 'node_modules' $prop.Name) 'package.json'
            if (-not (Test-Path -LiteralPath $pf)) { continue }
            $rawPf = Get-Content -LiteralPath $pf -Raw -ErrorAction SilentlyContinue
            if ($rawPf -match '"version"\s*:\s*"(\d+)') {
                $inst = [int]$Matches[1]
                if ($inst -lt [int]$prop.Value) { $drifts += "$($prop.Name)@$inst<$($prop.Value)" }
            }
        }
        if ($drifts.Count -eq 0) {
            Add-Check 'version-drift' 'available' $false 'no major-version drift vs configured latest-known majors' ''
        } else {
            Add-Check 'version-drift' 'degraded' $false "$($drifts.Count) package(s) behind latest-known major: $($drifts -join ' ') (advisory, wall 9)" 'review upgrade paths, or update provision.latestKnownMajors if the table is stale'
        }
    }
} else {
    Add-Check 'version-drift' 'not-applicable' $false 'no latestKnownMajors table configured (.claude/config.json provision.latestKnownMajors) -- drift check has NO-DATA' ''
}

# ---- mobile-mcp (warn only when mobile markers exist) -- PROV-03, MSG-001 -----
# Spec by CLAUDE-E (messages.md MSG-001). NO-DATA honesty: unparsable is never
# reported as unregistered. ConvertFrom-Json is always available in PS.
#
# Registration is necessary but NOT sufficient (DEFER-PROV-MCP-REACH): Claude
# Code does not start a project-scoped server until the user trusts it, and
# trust lives in ~/.claude.json -> projects[<root>]. Reporting only; the oracle
# stays read-only (Part 11 sec 11.3.4) and never actuates trust.

# Normalize a host path for key comparison: backslashes -> slashes, git-bash
# /c/ and WSL /mnt/c/ drive forms -> c:/, drop trailing slashes, lowercase.
function Get-McpNormPath {
    param([string]$Path)
    if (-not $Path) { return '' }
    $p = $Path -replace '\\', '/'
    $p = $p -replace '^/mnt/([a-zA-Z])/', '$1:/'
    $p = $p -replace '^/([a-zA-Z])/', '$1:/'
    $p = $p -replace '/+$', ''
    return $p.ToLowerInvariant()
}

# Canonicalize before normalizing: resolves Windows 8.3 short names (CARBON~1)
# and symlinks to the long form the stored keys use. Applied to the lookup
# TARGET only -- the stored keys are already canonical.
function Get-McpCanonPath {
    param([string]$Path)
    if (-not $Path) { return '' }
    $p = $Path
    try {
        $item = Get-Item -LiteralPath $p -ErrorAction Stop
        if ($item -and $item.FullName) { $p = $item.FullName }
    } catch { }
    return (Get-McpNormPath $p)
}

function Get-McpHomeDir {
    if ($env:HOME) { return $env:HOME }
    if ($env:USERPROFILE) { return $env:USERPROFILE }
    return $HOME
}

# -> 'trusted' | 'untrusted' | 'disabled' | 'nodata'
function Get-McpTrustState {
    param([string]$Server)
    if (-not $Server) { return 'nodata' }
    $cfg = Join-Path (Get-McpHomeDir) '.claude.json'
    if (-not (Test-Path -LiteralPath $cfg)) { return 'nodata' }
    try {
        $j = (Get-Content -LiteralPath $cfg -Raw -ErrorAction Stop) | ConvertFrom-Json
    } catch { return 'nodata' }
    if (-not $j -or -not $j.projects) { return 'nodata' }
    $target = Get-McpCanonPath (Get-Location).Path
    $entry = $null
    foreach ($prop in $j.projects.PSObject.Properties) {
        if ((Get-McpNormPath $prop.Name) -eq $target) { $entry = $prop.Value; break }
    }
    if ($null -eq $entry) { return 'nodata' }
    if (@($entry.disabledMcpjsonServers) -contains $Server) { return 'disabled' }
    if ($entry.enableAllProjectMcpServers -eq $true) { return 'trusted' }
    if (@($entry.enabledMcpjsonServers) -contains $Server) { return 'trusted' }
    return 'untrusted'
}

# Emit the trust-aware check for a resolvable, registered mobile MCP server.
function Add-McpCheckTrusted {
    param([string]$Server, [string]$Detail)
    switch (Get-McpTrustState $Server) {
        'trusted' {
            Add-Check 'mobile-mcp' 'available' $false "$Detail; trusted for this project" ''
        }
        'disabled' {
            Add-Check 'mobile-mcp' 'unavailable' $false "$Detail, but '$Server' is explicitly disabled for this project (~/.claude.json disabledMcpjsonServers) -- the server will not start" "remove '$Server' from projects[<root>].disabledMcpjsonServers in ~/.claude.json"
        }
        'untrusted' {
            Add-Check 'mobile-mcp' 'degraded' $false "registered-but-untrusted: $Detail, but the project has not trusted it (~/.claude.json enableAllProjectMcpServers not true, '$Server' absent from enabledMcpjsonServers) -- the server will never start and its tools will not load" 'accept the interactive trust prompt at session start, OR set projects[<root>].enableAllProjectMcpServers=true in ~/.claude.json, OR register the server at user scope (claude mcp add --scope user)'
        }
        default {
            Add-Check 'mobile-mcp' 'degraded' $false "$Detail; trust state is NO-DATA (~/.claude.json unreadable, unparsable, or no matching project key) -- registration alone does not mean the server will start" 'verify trust manually: claude mcp list (expect ''Connected''), or check projects[<root>] in ~/.claude.json'
        }
    }
}

if ($mobile) {
    $mcpKeywords = @('mobile-mcp', 'maestro', 'agent-device', 'appium-mcp')
    if (-not (Test-Path '.mcp.json')) {
        Add-Check 'mobile-mcp' 'degraded' $false 'mobile project markers present but no .mcp.json -- no mobile MCP registered' 'run the provisioner mobile-MCP step (PROV-03) or register a mobile MCP server project-scoped in .mcp.json'
    } else {
        $mcp = $null
        try { $mcp = (Get-Content '.mcp.json' -Raw -ErrorAction SilentlyContinue) | ConvertFrom-Json } catch { }
        if ($null -eq $mcp) {
            Add-Check 'mobile-mcp' 'degraded' $false 'cannot parse .mcp.json -- mobile-MCP registration state is NO-DATA, not absence' 'fix the .mcp.json syntax'
        } else {
            $matchName = ''
            $matchCmd = ''
            $matchArgs = @()
            $extraN = 0
            if ($mcp.mcpServers) {
                foreach ($prop in $mcp.mcpServers.PSObject.Properties) {
                    $e = $prop.Value
                    $cmdStr = [string]$e.command
                    $argStr = ''
                    if ($e.args) { $argStr = ($e.args -join ' ') }
                    $joined = ("$cmdStr $argStr").ToLower()
                    $hit = $false
                    foreach ($kw in $mcpKeywords) { if ($joined.Contains($kw)) { $hit = $true; break } }
                    if ($hit) {
                        if (-not $matchName) { $matchName = $prop.Name; $matchCmd = $cmdStr; $matchArgs = @($e.args) }
                        else { $extraN++ }
                    }
                }
            }
            $extraNote = ''
            if ($extraN -gt 0) { $extraNote = " (+$extraN other mobile MCP entries)" }
            if (-not $matchName) {
                Add-Check 'mobile-mcp' 'degraded' $false '.mcp.json present; no mobile MCP server registered' 'run the provisioner mobile-MCP step (PROV-03) or register a mobile MCP server project-scoped in .mcp.json'
            } elseif (@('npx', 'bunx', 'pnpm') -contains $matchCmd) {
                $pkgArg = ''
                foreach ($a in $matchArgs) {
                    $s = [string]$a
                    if ($s.StartsWith('-') -or $s -eq 'dlx' -or $s -eq 'exec') { continue }
                    $pkgArg = $s; break
                }
                $pkgInstalled = $false
                if ($pkgArg) {
                    if (Test-Path -LiteralPath (Join-Path 'node_modules' $pkgArg)) { $pkgInstalled = $true }
                    elseif (Test-Path -LiteralPath (Join-Path 'node_modules/.bin' (Split-Path -Leaf $pkgArg))) { $pkgInstalled = $true }
                }
                if ($pkgInstalled) {
                    Add-McpCheckTrusted $matchName "mobile MCP registered: $matchName (package installed locally)$extraNote"
                } else {
                    $pkgDisplay = '<pkg>'
                    if ($pkgArg) { $pkgDisplay = $pkgArg }
                    Add-Check 'mobile-mcp' 'degraded' $false "mobile MCP registered ($matchName) but package not installed locally -- will fetch at first use (non-deterministic offline)$extraNote" "pre-install project-scoped: npm i -D $pkgDisplay"
                }
            } else {
                if (Test-Cmd $matchCmd) {
                    Add-McpCheckTrusted $matchName "mobile MCP registered: $matchName ($matchCmd resolvable)$extraNote"
                } else {
                    Add-Check 'mobile-mcp' 'degraded' $false "mobile MCP registered ($matchName) but command not resolvable$extraNote" "install $matchCmd or fix the .mcp.json command path"
                }
            }
        }
    }
} else {
    Add-Check 'mobile-mcp' 'not-applicable' $false 'no mobile markers' ''
}

# ---- Compose verdict ----------------------------------------------------------
if ($script:CritFails -gt 0) {
    $ok = $false
    $summary = "$($script:CritFails) critical capability failure(s), $($script:Warnings) warning(s)"
    $briefing = "env-preflight: $($script:CritFails) critical capability failure(s) -- autonomous execution will fail mid-run; run /0-uldf-ltads-start --preflight for the fix list"
} else {
    $ok = $true
    $summary = "all critical capabilities available ($($script:Warnings) warning(s))"
    $briefing = ''
}

# NOTE: -replace substitution strings treat backslash as escape -- '\\' here is -replace
# substitution text is literal, so '\\\\' emitted FOUR backslashes per one.
# Latent until now (no detail string carried backslashes); the new jdk probe
# emits Windows paths, which made it live.
function Esc-Json { param([string]$s) return ($s -replace '\\', '\\' -replace '"', '\"') }

$checkParts = foreach ($c in $script:Checks) {
    $crit = if ($c.critical) { 'true' } else { 'false' }
    '{"name":"' + (Esc-Json $c.name) + '","status":"' + (Esc-Json $c.status) + '","critical":' + $crit + ',"detail":"' + (Esc-Json $c.detail) + '","fix":"' + (Esc-Json $c.fix) + '"}'
}
$okStr = if ($ok) { 'true' } else { 'false' }
$json = '{"ok":' + $okStr + ',"checks":[' + ($checkParts -join ',') + '],"critical_failures":' + $script:CritFails + ',"warnings":' + $script:Warnings + ',"summary":"' + (Esc-Json $summary) + '","briefing":"' + (Esc-Json $briefing) + '"}'

Write-Output $json
exit 0
