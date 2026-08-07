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
# The parse itself lives in scripts/lib/dev-port-registry.ps1 (QUIESCE-01,
# DEC-208) — one owner, one fix. It was previously inlined here and in the .sh
# twin with a regex that could not match a single real row (markdown emphasis +
# code spans), so `hasLiveDevServer` was structurally incapable of returning
# true from 2026-05-10 to 2026-07-29. Never re-inline it.
$currentProject = ""
try { $currentProject = (Split-Path -Leaf (Get-Location).Path) } catch {}

$dprLib = Join-Path $PSScriptRoot "../../scripts/lib/dev-port-registry.ps1"
if (Test-Path -LiteralPath $dprLib -PathType Leaf) {
    . $dprLib
    foreach ($row in (Get-DprPortsForProject -Name $currentProject)) {
        $devPortEntries += [pscustomobject]@{
            project = $row.project
            port    = $row.port
            source  = "MACHINE_CONFIG.md"
        }
        if (Test-PortBound -Port $row.port) {
            $hasLiveDevServer = $true
            $antiFitReasons += "port $($row.port) assigned to '$($row.project)' is currently bound (live dev server)"
        }
    }
} else {
    # Degraded, and say so: an absent parser must not read as "no ports assigned".
    $antiFitReasons += "dev-port-registry lib unavailable - Dev Port Registry not consulted (this is NO-DATA, not 'no assignments')"
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
