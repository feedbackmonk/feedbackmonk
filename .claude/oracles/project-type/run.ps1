# project-type oracle (Windows PowerShell)
# Detects languages, frameworks, build systems, and run/test commands from manifest files.
# Output: single JSON object matching oracle.json schema.
# Freshness: always-fresh (reads live files on every invocation).

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$languages = @()
$frameworks = @()
$buildSystems = @()
$packageManagers = @()
$testCommand = $null
$devCommand = $null

# ----- JavaScript / TypeScript / Node -----
if (Test-Path "package.json") {
    $languages += "javascript"
    $packageManagers += "npm"
    $pkgContent = Get-Content "package.json" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (Test-Path "tsconfig.json") { $languages += "typescript" }
    elseif ($pkgContent -match '"typescript"') { $languages += "typescript" }
    if ($pkgContent -match '"react"') { $frameworks += "react" }
    if ($pkgContent -match '"vue"') { $frameworks += "vue" }
    if ($pkgContent -match '"svelte"') { $frameworks += "svelte" }
    if ($pkgContent -match '"next"') { $frameworks += "next.js" }
    if ($pkgContent -match '"@tauri-apps') { $frameworks += "tauri" }
    if ($pkgContent -match '"electron"') { $frameworks += "electron" }
    if ($pkgContent -match '"express"') { $frameworks += "express" }
    if ($pkgContent -match '"fastify"') { $frameworks += "fastify" }
    if ($pkgContent -match '"vite"') { $buildSystems += "vite" }
    if ($pkgContent -match '"webpack"') { $buildSystems += "webpack" }
    if (Test-Path "yarn.lock") { $packageManagers += "yarn" }
    if (Test-Path "pnpm-lock.yaml") { $packageManagers += "pnpm" }
    if (Test-Path "bun.lockb") { $packageManagers += "bun" }
    # Extract test/dev scripts
    try {
        $pkg = $pkgContent | ConvertFrom-Json
        if ($pkg.scripts -and $pkg.scripts.test) { $testCommand = $pkg.scripts.test }
        if ($pkg.scripts -and $pkg.scripts.dev) { $devCommand = $pkg.scripts.dev }
        elseif ($pkg.scripts -and $pkg.scripts.start) { $devCommand = $pkg.scripts.start }
    } catch {}
}

# ----- Rust -----
if (Test-Path "Cargo.toml") {
    $languages += "rust"
    $buildSystems += "cargo"
    $packageManagers += "cargo"
    $cargoContent = Get-Content "Cargo.toml" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if ($cargoContent -match 'tauri') { $frameworks += "tauri" }
    if ($cargoContent -match 'axum') { $frameworks += "axum" }
    if ($cargoContent -match 'actix') { $frameworks += "actix" }
    if ($cargoContent -match 'rocket') { $frameworks += "rocket" }
    if (-not $testCommand) { $testCommand = "cargo test" }
}

# ----- Python -----
if (Test-Path "pyproject.toml") {
    $languages += "python"
    $pyContent = Get-Content "pyproject.toml" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if ($pyContent -match 'poetry') { $packageManagers += "poetry" }
    if ($pyContent -match 'hatch') { $packageManagers += "hatch" }
    if ($pyContent -match 'fastapi') { $frameworks += "fastapi" }
    if ($pyContent -match 'django') { $frameworks += "django" }
    if ($pyContent -match 'flask') { $frameworks += "flask" }
} elseif ((Test-Path "requirements.txt") -or (Test-Path "setup.py")) {
    $languages += "python"
    $packageManagers += "pip"
}

# ----- Go -----
if (Test-Path "go.mod") {
    $languages += "go"
    $buildSystems += "go"
    if (-not $testCommand) { $testCommand = "go test ./..." }
}

# ----- .NET -----
if ((Get-ChildItem -Filter "*.csproj" -ErrorAction SilentlyContinue) -or (Get-ChildItem -Filter "*.sln" -ErrorAction SilentlyContinue)) {
    $languages += "csharp"
    $buildSystems += "dotnet"
    if (-not $testCommand) { $testCommand = "dotnet test" }
}

# ----- Java / Kotlin -----
if ((Test-Path "build.gradle") -or (Test-Path "build.gradle.kts")) {
    $buildSystems += "gradle"
    if (Test-Path "build.gradle.kts") { $languages += "kotlin" } else { $languages += "java" }
    if (-not $testCommand) { $testCommand = "./gradlew test" }
}
if (Test-Path "pom.xml") {
    $languages += "java"
    $buildSystems += "maven"
    if (-not $testCommand) { $testCommand = "mvn test" }
}

# Deduplicate while preserving order
$languages = $languages | Select-Object -Unique
$frameworks = $frameworks | Select-Object -Unique
$buildSystems = $buildSystems | Select-Object -Unique
$packageManagers = $packageManagers | Select-Object -Unique

# Coerce to arrays for JSON output (Select-Object -Unique can unwrap single-element arrays)
$languagesArr = @($languages)
$frameworksArr = @($frameworks)
$buildSystemsArr = @($buildSystems)
$packageManagersArr = @($packageManagers)

# Build output object
$output = [ordered]@{
    languages = $languagesArr
    frameworks = $frameworksArr
    build_systems = $buildSystemsArr
    test_command = $testCommand
    dev_command = $devCommand
    package_managers = $packageManagersArr
}

# Emit as single-line JSON (Compress avoids PowerShell's default pretty-printing)
$output | ConvertTo-Json -Compress -Depth 4
