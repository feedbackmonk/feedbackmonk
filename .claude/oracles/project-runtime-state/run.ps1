# project-runtime-state oracle (Windows PowerShell)
# Detects whether THIS project has dev-environment-state contributors that would
# conflict under PODS worktree isolation.
#
# Output: single JSON object matching oracle.json schema (frozen v1).
# Lineage: WT-05 (Arc 1 of PODS opt-in worktree mode, DEC-61, 2026-05-10).

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$schemaVersion = 1
$hasLiveDevServer = $false
$statefulRuntime = $null
$devPortEntries = @()
$sharedBuildArtifacts = @()
$fileWatchers = @()
$antiFitReasons = @()

function Test-PortBound {
    param([int]$Port)
    if ($Port -le 0) { return $false }
    try {
        $conn = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
        if ($conn) { return $true }
    } catch {}
    return $false
}

# ---- Step 1: Parse Dev Port Registry from MACHINE_CONFIG.md ----
$machineConfig = $null
if ($env:USERPROFILE -and (Test-Path (Join-Path $env:USERPROFILE ".claude/MACHINE_CONFIG.md"))) {
    $machineConfig = Join-Path $env:USERPROFILE ".claude/MACHINE_CONFIG.md"
} elseif ($env:HOME -and (Test-Path (Join-Path $env:HOME ".claude/MACHINE_CONFIG.md"))) {
    $machineConfig = Join-Path $env:HOME ".claude/MACHINE_CONFIG.md"
}

$currentProject = ""
try { $currentProject = (Split-Path -Leaf (Get-Location).Path) } catch {}
$currentProjectLc = $currentProject.ToLowerInvariant()

if ($machineConfig -and (Test-Path $machineConfig)) {
    $lines = Get-Content -LiteralPath $machineConfig -Encoding UTF8 -ErrorAction SilentlyContinue
    $inSection = $false
    foreach ($raw in $lines) {
        if ($raw -match '^##\s+Dev Port Registry') { $inSection = $true; continue }
        if ($inSection -and $raw -match '^##\s') { $inSection = $false }
        if (-not $inSection) { continue }
        $line = $raw -replace '^\s*[-*+|]\s*', ''

        $proj = $null; $port = 0
        if ($line -match '^([A-Za-z0-9._/\-\s]+):\s*([1-9][0-9]{3,4})(\s|$)') {
            $proj = $matches[1].Trim()
            $port = [int]$matches[2]
        } elseif ($line -match '^\|?\s*([A-Za-z0-9._/\-]+)\s*\|\s*([1-9][0-9]{3,4})\s*\|') {
            $proj = $matches[1].Trim()
            $port = [int]$matches[2]
        } else {
            continue
        }
        if ($port -lt 1024 -or $port -gt 65535) { continue }

        $projLc = $proj.ToLowerInvariant()
        $matched = (-not $currentProjectLc) -or ($projLc -eq $currentProjectLc) -or `
                   ($currentProjectLc -and $projLc.Contains($currentProjectLc)) -or `
                   ($currentProjectLc -and $currentProjectLc.Contains($projLc))
        if ($matched) {
            $devPortEntries += [pscustomobject]@{
                project = $proj
                port    = $port
                source  = "MACHINE_CONFIG.md"
            }
            if (Test-PortBound -Port $port) {
                $hasLiveDevServer = $true
                $antiFitReasons += "port $port assigned to '$proj' is currently bound (live dev server)"
            }
        }
    }
}

# ---- Step 2: Glob shared build artifacts ----
foreach ($d in @("node_modules","target",".cargo",".gradle","vendor",".venv",".next",".nuxt","build","dist")) {
    if (Test-Path -LiteralPath $d -PathType Container) {
        $sharedBuildArtifacts += $d
    }
}

# ---- Step 3: Glob file-watcher configs ----
foreach ($pat in @("vite.config.js","vite.config.ts","vite.config.mjs","vite.config.cjs","nodemon.json","webpack.config.js","webpack.config.ts","tsup.config.js","tsup.config.ts","rollup.config.js","rollup.config.ts")) {
    if (Test-Path -LiteralPath $pat -PathType Leaf) {
        $fileWatchers += $pat
    }
}

# ---- Step 4: Detect stateful runtime ----
if (Test-Path -LiteralPath "package.json" -PathType Leaf) {
    $pkg = Get-Content -LiteralPath "package.json" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if ($pkg -match '"@tauri-apps') { $statefulRuntime = "tauri" }
    elseif ($pkg -match '"electron"') { $statefulRuntime = "electron" }
    elseif ($pkg -match '"expo"') { $statefulRuntime = "expo" }
    elseif ($pkg -match '"next"') { $statefulRuntime = "next.js-dev" }
}
if (-not $statefulRuntime -and (Test-Path -LiteralPath "Cargo.toml" -PathType Leaf)) {
    $cargo = Get-Content -LiteralPath "Cargo.toml" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if ($cargo -match '(?m)^tauri' -or $cargo -match 'tauri\s*=') {
        $statefulRuntime = "tauri"
    }
}
if (-not $statefulRuntime -and (Test-Path -LiteralPath "manage.py" -PathType Leaf)) {
    $statefulRuntime = "django-runserver"
}
if (-not $statefulRuntime -and (Test-Path -LiteralPath "pyproject.toml" -PathType Leaf)) {
    $py = Get-Content -LiteralPath "pyproject.toml" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if ($py -match 'django') { $statefulRuntime = "django-runserver" }
}

# ---- Step 5: Compute antiFitScore + reasons ----
$score = 0
if ($hasLiveDevServer) { $score++ }
if ($statefulRuntime) {
    $score++
    $antiFitReasons += "stateful runtime detected: $statefulRuntime"
}
if ($fileWatchers.Count -ge 1) {
    $score++
    $antiFitReasons += "file watcher config(s) present: $($fileWatchers -join ', ')"
}
if ($sharedBuildArtifacts.Count -ge 2) {
    $score++
    $antiFitReasons += "multiple shared build-artifact dirs present: $($sharedBuildArtifacts -join ', ')"
}
if ($devPortEntries.Count -ge 1) {
    $score++
    $antiFitReasons += "Dev Port Registry assignment(s) for this project: $($devPortEntries.Count)"
}
if ($score -gt 5) { $score = 5 }

# ---- Step 6: Emit JSON ----
$output = [ordered]@{
    schemaVersion = $schemaVersion
    hasLiveDevServer = $hasLiveDevServer
    devPortRegistryEntries = @($devPortEntries)
    sharedBuildArtifacts = @($sharedBuildArtifacts)
    fileWatchers = @($fileWatchers)
    statefulRuntime = $statefulRuntime
    antiFitScore = $score
    antiFitReasons = @($antiFitReasons)
}

$output | ConvertTo-Json -Compress -Depth 6
