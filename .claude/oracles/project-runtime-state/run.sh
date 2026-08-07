#!/bin/bash
# project-runtime-state oracle (Unix)
# Detects whether THIS project has dev-environment-state contributors that would
# conflict under PODS worktree isolation: live dev servers (port-bound),
# shared build artifacts (node_modules/target/.gradle/etc.), file watchers
# (vite/nodemon/webpack configs), and stateful runtimes (tauri/electron/expo/django).
#
# Output: single JSON object matching oracle.json schema (frozen v1).
# Freshness: always-fresh (each call re-probes ports and re-globs artifacts).
#
# Lineage: WT-05 (Arc 1 of PODS opt-in worktree mode, DEC-61, 2026-05-10).

set -e

# ---- Defaults ----
schema_version=1
has_live_dev_server="false"
stateful_runtime="null"

# Arrays
dev_port_entries=()
shared_build_artifacts=()
file_watchers=()
anti_fit_reasons=()

# ---- Helpers ----
esc() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

# JSON string array (emits [] for empty)
json_string_array() {
    if [ "$#" -eq 0 ]; then echo "[]"; return; fi
    local result="[" first=1 item
    for item in "$@"; do
        if [ "$first" -eq 1 ]; then first=0; else result+=","; fi
        result+="\"$(esc "$item")\""
    done
    result+="]"
    echo "$result"
}

# Cross-platform port liveness probe.
# Returns 0 if port is bound, 1 otherwise.
port_is_bound() {
    local port="$1"
    [ -n "$port" ] || return 1
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*)
            # Windows: Get-NetTCPConnection. Fall back to netstat.
            if powershell.exe -NoProfile -Command "if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1; then
                return 0
            fi
            return 1
            ;;
        *)
            # POSIX: prefer lsof; fall back to ss; fall back to netstat.
            if command -v lsof >/dev/null 2>&1; then
                if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then return 0; fi
                return 1
            fi
            if command -v ss >/dev/null 2>&1; then
                if ss -ltn 2>/dev/null | awk -v p=":$port" '$4 ~ p {found=1} END{exit !found}'; then return 0; fi
                return 1
            fi
            if command -v netstat >/dev/null 2>&1; then
                if netstat -ltn 2>/dev/null | awk -v p=":$port" '$4 ~ p {found=1} END{exit !found}'; then return 0; fi
                return 1
            fi
            return 1
            ;;
    esac
}

# ---- Step 1: Parse Dev Port Registry from MACHINE_CONFIG.md ----
# The parse itself lives in scripts/lib/dev-port-registry.sh (QUIESCE-01,
# DEC-208) — one owner, one fix. It was previously inlined here and in the .ps1
# twin with a regex that could not match a single real row (markdown emphasis +
# code spans), so `hasLiveDevServer` was structurally incapable of returning
# true from 2026-05-10 to 2026-07-29. Never re-inline it.
_prs_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
_dpr_lib="$_prs_dir/../../scripts/lib/dev-port-registry.sh"

current_project="$(basename "$(pwd)" 2>/dev/null || echo "")"

if [ -f "$_dpr_lib" ]; then
    # shellcheck source=/dev/null
    . "$_dpr_lib"
    while IFS=$'\t' read -r proj port role; do
        [ -n "$port" ] || continue
        dev_port_entries+=("{\"project\":\"$(esc "$proj")\",\"port\":$port,\"source\":\"MACHINE_CONFIG.md\"}")
        if port_is_bound "$port"; then
            has_live_dev_server="true"
            anti_fit_reasons+=("port $port assigned to '$proj' is currently bound (live dev server)")
        fi
    done < <(dpr_ports_for_project "$current_project" 2>/dev/null || true)
else
    # Degraded, and say so: an absent parser must not read as "no ports assigned".
    anti_fit_reasons+=("dev-port-registry lib unavailable - Dev Port Registry not consulted (this is NO-DATA, not 'no assignments')")
fi

# ---- Step 2: Glob shared build artifacts ----
for d in node_modules target .cargo .gradle vendor .venv .next .nuxt build dist; do
    if [ -d "$d" ]; then
        shared_build_artifacts+=("$d")
    fi
done

# ---- Step 3: Glob file-watcher configs ----
for pat in vite.config.js vite.config.ts vite.config.mjs vite.config.cjs nodemon.json webpack.config.js webpack.config.ts tsup.config.js tsup.config.ts rollup.config.js rollup.config.ts; do
    if [ -f "$pat" ]; then
        file_watchers+=("$pat")
    fi
done

# ---- Step 4: Detect stateful runtime ----
if [ -f package.json ]; then
    if grep -q '"@tauri-apps' package.json 2>/dev/null; then stateful_runtime="\"tauri\""; fi
    if [ "$stateful_runtime" = "null" ] && grep -q '"electron"' package.json 2>/dev/null; then stateful_runtime="\"electron\""; fi
    if [ "$stateful_runtime" = "null" ] && grep -q '"expo"' package.json 2>/dev/null; then stateful_runtime="\"expo\""; fi
    if [ "$stateful_runtime" = "null" ] && grep -q '"next"' package.json 2>/dev/null; then stateful_runtime="\"next.js-dev\""; fi
fi
if [ "$stateful_runtime" = "null" ] && [ -f Cargo.toml ]; then
    if grep -q '^tauri' Cargo.toml 2>/dev/null || grep -qE 'tauri[[:space:]]*=' Cargo.toml 2>/dev/null; then
        stateful_runtime="\"tauri\""
    fi
fi
if [ "$stateful_runtime" = "null" ] && [ -f manage.py ]; then
    stateful_runtime="\"django-runserver\""
fi
if [ "$stateful_runtime" = "null" ] && [ -f pyproject.toml ]; then
    if grep -q 'django' pyproject.toml 2>/dev/null; then stateful_runtime="\"django-runserver\""; fi
fi

# ---- Step 5: Compute antiFitScore + reasons ----
# Indicators (each contributes 1, capped at 5):
#   1. hasLiveDevServer == true
#   2. statefulRuntime != null
#   3. >= 1 file watcher present
#   4. >= 2 shared build artifacts present (single one is normal/cheap to recreate;
#      multiple suggests a heavy multi-language build that's expensive to duplicate)
#   5. >= 1 dev port registry entry (assigned to this project — even if not bound)
score=0
if [ "$has_live_dev_server" = "true" ]; then
    score=$((score + 1))
fi
if [ "$stateful_runtime" != "null" ]; then
    score=$((score + 1))
    rt_clean="$(echo "$stateful_runtime" | tr -d '"')"
    anti_fit_reasons+=("stateful runtime detected: $rt_clean")
fi
if [ "${#file_watchers[@]}" -ge 1 ]; then
    score=$((score + 1))
    anti_fit_reasons+=("file watcher config(s) present: ${file_watchers[*]}")
fi
if [ "${#shared_build_artifacts[@]}" -ge 2 ]; then
    score=$((score + 1))
    anti_fit_reasons+=("multiple shared build-artifact dirs present: ${shared_build_artifacts[*]}")
fi
if [ "${#dev_port_entries[@]}" -ge 1 ]; then
    score=$((score + 1))
    anti_fit_reasons+=("Dev Port Registry assignment(s) for this project: ${#dev_port_entries[@]}")
fi
if [ "$score" -gt 5 ]; then score=5; fi

# ---- Step 6: Emit JSON ----
# Build dev_port_entries JSON array (entries are pre-built JSON objects).
dev_ports_json="["
first=1
for entry in "${dev_port_entries[@]}"; do
    if [ "$first" -eq 1 ]; then first=0; else dev_ports_json+=","; fi
    dev_ports_json+="$entry"
done
dev_ports_json+="]"

cat <<EOF
{"schemaVersion":$schema_version,"hasLiveDevServer":$has_live_dev_server,"devPortRegistryEntries":$dev_ports_json,"sharedBuildArtifacts":$(json_string_array "${shared_build_artifacts[@]}"),"fileWatchers":$(json_string_array "${file_watchers[@]}"),"statefulRuntime":$stateful_runtime,"antiFitScore":$score,"antiFitReasons":$(json_string_array "${anti_fit_reasons[@]}")}
EOF
