#!/bin/bash
# project-type oracle (Unix)
# Detects languages, frameworks, build systems, and run/test commands from manifest files.
# Output: single JSON object matching oracle.json schema.
# Freshness: always-fresh (reads live files on every invocation).

set -e

# Arrays to collect findings
languages=()
frameworks=()
build_systems=()
package_managers=()
test_command=""
dev_command=""

# ----- JavaScript / TypeScript / Node -----
if [ -f package.json ]; then
    languages+=("javascript")
    package_managers+=("npm")
    # TypeScript presence
    if [ -f tsconfig.json ] || grep -q '"typescript"' package.json 2>/dev/null; then
        languages+=("typescript")
    fi
    # Framework detection
    if grep -q '"react"' package.json 2>/dev/null; then frameworks+=("react"); fi
    if grep -q '"vue"' package.json 2>/dev/null; then frameworks+=("vue"); fi
    if grep -q '"svelte"' package.json 2>/dev/null; then frameworks+=("svelte"); fi
    if grep -q '"next"' package.json 2>/dev/null; then frameworks+=("next.js"); fi
    if grep -q '"@tauri-apps' package.json 2>/dev/null; then frameworks+=("tauri"); fi
    if grep -q '"electron"' package.json 2>/dev/null; then frameworks+=("electron"); fi
    if grep -q '"express"' package.json 2>/dev/null; then frameworks+=("express"); fi
    if grep -q '"fastify"' package.json 2>/dev/null; then frameworks+=("fastify"); fi
    if grep -q '"vite"' package.json 2>/dev/null; then build_systems+=("vite"); fi
    if grep -q '"webpack"' package.json 2>/dev/null; then build_systems+=("webpack"); fi
    # Alt package managers
    if [ -f yarn.lock ]; then package_managers+=("yarn"); fi
    if [ -f pnpm-lock.yaml ]; then package_managers+=("pnpm"); fi
    if [ -f bun.lockb ]; then package_managers+=("bun"); fi
    # Extract test/dev scripts
    if command -v jq &> /dev/null; then
        test_command=$(jq -r '.scripts.test // empty' package.json 2>/dev/null || echo "")
        dev_command=$(jq -r '.scripts.dev // .scripts.start // empty' package.json 2>/dev/null || echo "")
    else
        test_command=$(grep -oE '"test"[[:space:]]*:[[:space:]]*"[^"]+"' package.json 2>/dev/null | head -1 | sed -E 's/"test"[[:space:]]*:[[:space:]]*"([^"]+)"/\1/')
        dev_command=$(grep -oE '"dev"[[:space:]]*:[[:space:]]*"[^"]+"' package.json 2>/dev/null | head -1 | sed -E 's/"dev"[[:space:]]*:[[:space:]]*"([^"]+)"/\1/')
    fi
fi

# ----- Rust -----
if [ -f Cargo.toml ]; then
    languages+=("rust")
    build_systems+=("cargo")
    package_managers+=("cargo")
    if grep -q 'tauri' Cargo.toml 2>/dev/null; then frameworks+=("tauri"); fi
    if grep -q 'axum\|actix\|rocket\|warp' Cargo.toml 2>/dev/null; then
        if grep -q 'axum' Cargo.toml 2>/dev/null; then frameworks+=("axum"); fi
        if grep -q 'actix' Cargo.toml 2>/dev/null; then frameworks+=("actix"); fi
        if grep -q 'rocket' Cargo.toml 2>/dev/null; then frameworks+=("rocket"); fi
    fi
    if [ -z "$test_command" ]; then test_command="cargo test"; fi
fi

# ----- Python -----
if [ -f pyproject.toml ]; then
    languages+=("python")
    if grep -q 'poetry' pyproject.toml 2>/dev/null; then package_managers+=("poetry"); fi
    if grep -q 'hatch' pyproject.toml 2>/dev/null; then package_managers+=("hatch"); fi
    if grep -q 'fastapi' pyproject.toml 2>/dev/null; then frameworks+=("fastapi"); fi
    if grep -q 'django' pyproject.toml 2>/dev/null; then frameworks+=("django"); fi
    if grep -q 'flask' pyproject.toml 2>/dev/null; then frameworks+=("flask"); fi
elif [ -f requirements.txt ] || [ -f setup.py ]; then
    languages+=("python")
    package_managers+=("pip")
fi

# ----- Go -----
if [ -f go.mod ]; then
    languages+=("go")
    build_systems+=("go")
    if [ -z "$test_command" ]; then test_command="go test ./..."; fi
fi

# ----- .NET -----
if ls *.csproj 2>/dev/null | grep -q . || ls *.sln 2>/dev/null | grep -q .; then
    languages+=("csharp")
    build_systems+=("dotnet")
    if [ -z "$test_command" ]; then test_command="dotnet test"; fi
fi

# ----- Java / Kotlin -----
if [ -f build.gradle ] || [ -f build.gradle.kts ]; then
    build_systems+=("gradle")
    if [ -f build.gradle.kts ]; then languages+=("kotlin"); else languages+=("java"); fi
    if [ -z "$test_command" ]; then test_command="./gradlew test"; fi
fi
if [ -f pom.xml ]; then
    languages+=("java")
    build_systems+=("maven")
    if [ -z "$test_command" ]; then test_command="mvn test"; fi
fi

# ----- Deduplicate arrays (in-place, no subshell piping) -----
dedupe_inplace() {
    # Usage: dedupe_inplace varname
    local __var="$1"
    local -a __in
    eval "__in=(\"\${${__var}[@]}\")"
    local -a __out=()
    local __item __existing __found
    for __item in "${__in[@]}"; do
        if [ -z "$__item" ]; then continue; fi
        __found=0
        for __existing in "${__out[@]}"; do
            if [ "$__existing" = "$__item" ]; then __found=1; break; fi
        done
        if [ "$__found" -eq 0 ]; then __out+=("$__item"); fi
    done
    eval "$__var=(\"\${__out[@]}\")"
}

# JSON array builder (correctly emits [] for empty)
json_array() {
    if [ "$#" -eq 0 ]; then
        echo "[]"
        return
    fi
    local result="["
    local first=1
    local item
    for item in "$@"; do
        if [ "$first" -eq 1 ]; then first=0; else result+=","; fi
        result+="\"$item\""
    done
    result+="]"
    echo "$result"
}

# Deduplicate in place
dedupe_inplace languages
dedupe_inplace frameworks
dedupe_inplace build_systems
dedupe_inplace package_managers

# JSON-escape helper for scalar strings
esc() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

# Build final JSON
test_json="null"
if [ -n "$test_command" ]; then test_json="\"$(esc "$test_command")\""; fi
dev_json="null"
if [ -n "$dev_command" ]; then dev_json="\"$(esc "$dev_command")\""; fi

cat <<EOF
{"languages":$(json_array "${languages[@]}"),"frameworks":$(json_array "${frameworks[@]}"),"build_systems":$(json_array "${build_systems[@]}"),"test_command":$test_json,"dev_command":$dev_json,"package_managers":$(json_array "${package_managers[@]}")}
EOF
