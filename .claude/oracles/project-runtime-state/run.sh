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
# Format expectation (tolerant): a "Dev Port Registry" section with lines of
# the rough shape "- <project>: <port>" or table rows "| <project> | <port> |".
# We extract (project, port) pairs and scope to the current workDir's project
# name when we can match it.
machine_config=""
if [ -n "${HOME:-}" ] && [ -f "$HOME/.claude/MACHINE_CONFIG.md" ]; then
    machine_config="$HOME/.claude/MACHINE_CONFIG.md"
elif [ -n "${USERPROFILE:-}" ] && [ -f "$USERPROFILE/.claude/MACHINE_CONFIG.md" ]; then
    machine_config="$USERPROFILE/.claude/MACHINE_CONFIG.md"
fi

current_project="$(basename "$(pwd)" 2>/dev/null || echo "")"

if [ -n "$machine_config" ] && [ -f "$machine_config" ]; then
    # Slice the section between "Dev Port Registry" heading and the next "## " heading.
    section="$(awk '
        /^##[[:space:]]+Dev Port Registry/ {in_section=1; next}
        /^##[[:space:]]/ && in_section {in_section=0}
        in_section {print}
    ' "$machine_config" 2>/dev/null)"

    # Extract candidate (project, port) pairs from list items / table rows.
    # Patterns we accept:
    #   - <name>: <port>
    #   - * <name>: <port>
    #   | <name> | <port> | ...
    # The port is any 4-5 digit integer in 1024..65535.
    while IFS= read -r raw; do
        [ -n "$raw" ] || continue
        # Trim leading list/table markers.
        line="$(echo "$raw" | sed -E 's/^[[:space:]]*[-*+|][[:space:]]*//')"
        # Match "<project>: <port>" form.
        if echo "$line" | grep -qE '^[A-Za-z0-9._/[:space:]-]+:[[:space:]]*[1-9][0-9]{3,4}([[:space:]]|$)'; then
            proj="$(echo "$line" | sed -E 's/^([A-Za-z0-9._/[:space:]-]+):[[:space:]]*([1-9][0-9]{3,4}).*$/\1/' | sed -E 's/[[:space:]]+$//')"
            port="$(echo "$line" | sed -E 's/^([A-Za-z0-9._/[:space:]-]+):[[:space:]]*([1-9][0-9]{3,4}).*$/\2/')"
        # Match table-row form "| <project> | <port> |".
        elif echo "$line" | grep -qE '^\|?[[:space:]]*[A-Za-z0-9._/-]+[[:space:]]*\|[[:space:]]*[1-9][0-9]{3,4}[[:space:]]*\|'; then
            proj="$(echo "$line" | sed -E 's/^\|?[[:space:]]*([A-Za-z0-9._/-]+)[[:space:]]*\|[[:space:]]*([1-9][0-9]{3,4}).*$/\1/')"
            port="$(echo "$line" | sed -E 's/^\|?[[:space:]]*([A-Za-z0-9._/-]+)[[:space:]]*\|[[:space:]]*([1-9][0-9]{3,4}).*$/\2/')"
        else
            continue
        fi
        # Guard: port must be 1024..65535
        if [ "$port" -ge 1024 ] 2>/dev/null && [ "$port" -le 65535 ] 2>/dev/null; then
            : # ok
        else
            continue
        fi
        # Scope to current project when name matches (case-insensitive substring).
        proj_lc="$(echo "$proj" | tr '[:upper:]' '[:lower:]')"
        cur_lc="$(echo "$current_project" | tr '[:upper:]' '[:lower:]')"
        if [ -z "$cur_lc" ] || [ "$proj_lc" = "$cur_lc" ] || echo "$proj_lc" | grep -q "$cur_lc" 2>/dev/null || echo "$cur_lc" | grep -q "$proj_lc" 2>/dev/null; then
            dev_port_entries+=("{\"project\":\"$(esc "$proj")\",\"port\":$port,\"source\":\"MACHINE_CONFIG.md\"}")
            if port_is_bound "$port"; then
                has_live_dev_server="true"
                anti_fit_reasons+=("port $port assigned to '$proj' is currently bound (live dev server)")
            fi
        fi
    done <<< "$section"
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
