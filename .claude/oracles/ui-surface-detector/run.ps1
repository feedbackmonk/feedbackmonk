# ui-surface-detector oracle (Windows PowerShell)
# Answers: does this project have a UI / runtime surface that ARIA could instrument?
#
# Reads project manifests (package.json, Cargo.toml, pubspec.yaml) and a small
# set of marker paths (src-tauri/, index.html, bin/) and emits:
#   { surface_kind, confidence, evidence: [string] }
#
# Detection rules (ARIA-02 acceptance #1):
#   src-tauri/ + Cargo.toml                          -> tauri-desktop (high)
#   package.json with `electron` dep                  -> electron-desktop (high)
#   package.json with `react-native`/`expo` dep       -> react-native (high)
#   pubspec.yaml with Flutter SDK                     -> flutter (high)
#   package.json with framework UI deps + index.html  -> web-spa (high)
#   package.json with express/fastify/hono, no UI dep -> backend-service (medium)
#   bin/ or package.json bin field, no UI surface     -> cli-tool (medium)
#   else                                              -> none
#
# Confidence: high when >=2 evidence items align; medium when 1; low when ambiguous.
#
# Cache: .claude/oracle-cache/ui-surface-detector.json (mtime-based freshness against
# package.json/Cargo.toml/pubspec.yaml). <=200ms bound: filesystem stat + small-file reads only.

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$cachePath = ".claude/oracle-cache/ui-surface-detector.json"

# ---- Cache freshness check ----
if (Test-Path $cachePath) {
    try {
        $cacheMtime = (Get-Item $cachePath).LastWriteTimeUtc
        $fresh = $true
        foreach ($src in @("package.json", "Cargo.toml", "pubspec.yaml")) {
            if (Test-Path $src) {
                $srcMtime = (Get-Item $src).LastWriteTimeUtc
                if ($srcMtime -gt $cacheMtime) { $fresh = $false; break }
            }
        }
        if ($fresh) {
            $cached = Get-Content $cachePath -Raw -Encoding UTF8 -ErrorAction Stop
            Write-Output $cached.Trim()
            exit 0
        }
    } catch {
        # Fall through to fresh computation
    }
}

# ---- Detection ----
$candidates = New-Object System.Collections.ArrayList
$evidence = New-Object System.Collections.ArrayList

function Add-Candidate { param([string]$Kind) if (-not $candidates.Contains($Kind)) { [void]$candidates.Add($Kind) } }
function Add-Evidence { param([string]$Item) [void]$evidence.Add($Item) }

# 1. tauri-desktop: src-tauri/ + Cargo.toml
if ((Test-Path "src-tauri" -PathType Container) -and ((Test-Path "src-tauri/Cargo.toml") -or (Test-Path "Cargo.toml"))) {
    Add-Candidate "tauri-desktop"
    Add-Evidence "src-tauri/ directory present"
    if (Test-Path "src-tauri/Cargo.toml") { Add-Evidence "src-tauri/Cargo.toml present" }
}

# Read package.json once if present
$pkgContent = ""
$pkgDeps = ""
if (Test-Path "package.json") {
    try {
        $pkgContent = Get-Content "package.json" -Raw -Encoding UTF8 -ErrorAction Stop
        # Match dep blocks
        $depMatches = [regex]::Matches($pkgContent, '"(dependencies|devDependencies|peerDependencies|optionalDependencies)"\s*:\s*\{[^}]*\}')
        foreach ($m in $depMatches) { $pkgDeps += $m.Value }
    } catch {
        $pkgContent = ""
    }
}

function Test-Dep { param([string]$Name)
    if (-not $pkgDeps) { return $false }
    return $pkgDeps -match ('"' + [regex]::Escape($Name) + '"\s*:')
}

# 2. electron-desktop
if (Test-Dep "electron") {
    Add-Candidate "electron-desktop"
    Add-Evidence "package.json declares 'electron' dependency"
}

# 3. react-native: react-native or expo
if ((Test-Dep "react-native") -or (Test-Dep "expo")) {
    Add-Candidate "react-native"
    if (Test-Dep "react-native") { Add-Evidence "package.json declares 'react-native' dependency" }
    if (Test-Dep "expo") { Add-Evidence "package.json declares 'expo' dependency" }
}

# 4. flutter: pubspec.yaml with Flutter SDK
if (Test-Path "pubspec.yaml") {
    try {
        $pubspec = Get-Content "pubspec.yaml" -Raw -Encoding UTF8 -ErrorAction Stop
        if ($pubspec -match '(?m)sdk:\s*flutter') {
            Add-Candidate "flutter"
            Add-Evidence "pubspec.yaml declares Flutter SDK"
        }
    } catch {}
}

# 5. web-spa: package.json UI framework dep + index.html
$hasUiDep = $false
$uiDepEvidence = ""
foreach ($uiDep in @("react", "vue", "svelte", "@angular/core", "preact", "solid-js", "lit")) {
    if (Test-Dep $uiDep) {
        $hasUiDep = $true
        $uiDepEvidence = "package.json declares '$uiDep' dependency"
        break
    }
}
$hasIndexHtml = (Test-Path "index.html") -or (Test-Path "public/index.html") -or (Test-Path "src/index.html")
if ($hasUiDep -and $hasIndexHtml) {
    Add-Candidate "web-spa"
    Add-Evidence $uiDepEvidence
    Add-Evidence "index.html present"
}

# 6. backend-service: express/fastify/hono and no UI dep
$hasBackendDep = $false
$backendEvidence = ""
foreach ($be in @("express", "fastify", "hono", "koa", "@nestjs/core", "restify")) {
    if (Test-Dep $be) {
        $hasBackendDep = $true
        $backendEvidence = "package.json declares '$be' dependency"
        break
    }
}
if ($hasBackendDep -and (-not $hasUiDep)) {
    Add-Candidate "backend-service"
    Add-Evidence $backendEvidence
    Add-Evidence "no UI framework dependency detected"
}

# 7. cli-tool: bin/ or package.json bin field, no UI surface
$hasBin = $false
$binEvidenceParts = @()
if (Test-Path "bin" -PathType Container) {
    $hasBin = $true
    $binEvidenceParts += "bin/ directory present"
}
if ($pkgContent -and ($pkgContent -match '"bin"\s*:')) {
    $hasBin = $true
    $binEvidenceParts += "package.json declares 'bin' field"
}
if ((Test-Path "Cargo.toml") -and $candidates.Count -eq 0) {
    try {
        $cargo = Get-Content "Cargo.toml" -Raw -Encoding UTF8 -ErrorAction Stop
        if ($cargo -match '(?m)^\[\[bin\]\]') {
            $hasBin = $true
            $binEvidenceParts += "Cargo.toml [[bin]] section"
        }
    } catch {}
}
if ($hasBin -and $candidates.Count -eq 0) {
    Add-Candidate "cli-tool"
    Add-Evidence ($binEvidenceParts -join "; ")
}

# ---- Resolve ----
$selected = "none"
foreach ($kind in @("tauri-desktop", "electron-desktop", "react-native", "flutter", "web-spa", "backend-service", "cli-tool")) {
    if ($candidates.Contains($kind)) { $selected = $kind; break }
}

# Confidence
if ($selected -eq "none") {
    $confidence = "high"
    Add-Evidence "no UI/runtime surface markers detected"
} elseif ($candidates.Count -gt 1) {
    $confidence = "low"
    Add-Evidence ("multiple surface kinds matched: " + ($candidates -join ", "))
} elseif ($evidence.Count -ge 2) {
    $confidence = "high"
} else {
    $confidence = "medium"
}

# ---- Emit JSON ----
$result = [ordered]@{
    surface_kind = $selected
    confidence   = $confidence
    evidence     = @($evidence)
}
$json = $result | ConvertTo-Json -Compress -Depth 5

# ---- Cache write ----
try {
    $cacheDir = Split-Path -Parent $cachePath
    if (-not (Test-Path $cacheDir)) { New-Item -Path $cacheDir -ItemType Directory -Force | Out-Null }
    Set-Content -Path $cachePath -Value $json -Encoding UTF8 -NoNewline -ErrorAction SilentlyContinue
} catch {}

Write-Output $json
exit 0
