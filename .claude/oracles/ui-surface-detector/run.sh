#!/bin/bash
# ui-surface-detector oracle (Unix)
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
# package.json/Cargo.toml/pubspec.yaml). ≤200ms bound: filesystem stat + small-file reads only.

set -e

CACHE_PATH=".claude/oracle-cache/ui-surface-detector.json"

# ---- Cache freshness check ----
if [ -f "$CACHE_PATH" ]; then
    cache_mtime=$(stat -c %Y "$CACHE_PATH" 2>/dev/null || stat -f %m "$CACHE_PATH" 2>/dev/null || echo 0)
    fresh="true"
    for src in package.json Cargo.toml pubspec.yaml; do
        if [ -f "$src" ]; then
            src_mtime=$(stat -c %Y "$src" 2>/dev/null || stat -f %m "$src" 2>/dev/null || echo 0)
            if [ "$src_mtime" -gt "$cache_mtime" ]; then
                fresh="false"
                break
            fi
        fi
    done
    if [ "$fresh" = "true" ]; then
        cat "$CACHE_PATH"
        exit 0
    fi
fi

# ---- Detection ----
candidates=""   # space-separated kinds detected
evidence=""     # newline-separated evidence strings (will be joined into JSON array)

add_evidence() {
    if [ -z "$evidence" ]; then
        evidence="$1"
    else
        evidence="$evidence
$1"
    fi
}

add_candidate() {
    case " $candidates " in
        *" $1 "*) ;;
        *) candidates="$candidates $1" ;;
    esac
}

# 1. tauri-desktop: src-tauri/ + Cargo.toml
if [ -d "src-tauri" ] && { [ -f "src-tauri/Cargo.toml" ] || [ -f "Cargo.toml" ]; }; then
    add_candidate "tauri-desktop"
    add_evidence "src-tauri/ directory present"
    [ -f "src-tauri/Cargo.toml" ] && add_evidence "src-tauri/Cargo.toml present"
fi

# Read package.json once if present
PKG_CONTENT=""
PKG_DEPS=""
if [ -f "package.json" ]; then
    PKG_CONTENT=$(cat package.json 2>/dev/null || echo "")
    # Extract dependency-like keys; tolerant of missing jq
    PKG_DEPS=$(printf '%s' "$PKG_CONTENT" | tr -d '\n\r' | grep -oE '"(dependencies|devDependencies|peerDependencies|optionalDependencies)"[[:space:]]*:[[:space:]]*\{[^}]*\}' || echo "")
fi

dep_present() {
    # Case-sensitive substring match within dep-key values
    printf '%s' "$PKG_DEPS" | grep -qE "\"$1\"[[:space:]]*:" 2>/dev/null
}

# 2. electron-desktop: package.json with electron dep
if dep_present "electron"; then
    add_candidate "electron-desktop"
    add_evidence "package.json declares 'electron' dependency"
fi

# 3. react-native: package.json with react-native or expo
if dep_present "react-native" || dep_present "expo"; then
    add_candidate "react-native"
    if dep_present "react-native"; then add_evidence "package.json declares 'react-native' dependency"; fi
    if dep_present "expo"; then add_evidence "package.json declares 'expo' dependency"; fi
fi

# 4. flutter: pubspec.yaml with Flutter SDK
if [ -f "pubspec.yaml" ] && grep -qE 'sdk:[[:space:]]*flutter' pubspec.yaml 2>/dev/null; then
    add_candidate "flutter"
    add_evidence "pubspec.yaml declares Flutter SDK"
fi

# 5. web-spa: package.json with UI framework dep AND index.html
HAS_UI_DEP="false"
UI_DEP_EVIDENCE=""
for ui_dep in react vue svelte "@angular/core" preact solid-js lit; do
    if dep_present "$ui_dep"; then
        HAS_UI_DEP="true"
        UI_DEP_EVIDENCE="package.json declares '$ui_dep' dependency"
        break
    fi
done
HAS_INDEX_HTML="false"
if [ -f "index.html" ] || [ -f "public/index.html" ] || [ -f "src/index.html" ]; then
    HAS_INDEX_HTML="true"
fi
if [ "$HAS_UI_DEP" = "true" ] && [ "$HAS_INDEX_HTML" = "true" ]; then
    add_candidate "web-spa"
    add_evidence "$UI_DEP_EVIDENCE"
    add_evidence "index.html present"
fi

# 6. backend-service: package.json with express/fastify/hono AND no UI dep
HAS_BACKEND_DEP="false"
BACKEND_EVIDENCE=""
for be in express fastify hono koa "@nestjs/core" restify; do
    if dep_present "$be"; then
        HAS_BACKEND_DEP="true"
        BACKEND_EVIDENCE="package.json declares '$be' dependency"
        break
    fi
done
if [ "$HAS_BACKEND_DEP" = "true" ] && [ "$HAS_UI_DEP" = "false" ]; then
    add_candidate "backend-service"
    add_evidence "$BACKEND_EVIDENCE"
    add_evidence "no UI framework dependency detected"
fi

# 7. cli-tool: bin/ or package.json bin field, no UI surface
HAS_BIN="false"
BIN_EVIDENCE=""
if [ -d "bin" ]; then
    HAS_BIN="true"
    BIN_EVIDENCE="bin/ directory present"
fi
if [ -n "$PKG_CONTENT" ] && printf '%s' "$PKG_CONTENT" | tr -d '\n\r' | grep -qE '"bin"[[:space:]]*:'; then
    HAS_BIN="true"
    if [ -z "$BIN_EVIDENCE" ]; then
        BIN_EVIDENCE="package.json declares 'bin' field"
    else
        BIN_EVIDENCE="$BIN_EVIDENCE; package.json 'bin' field"
    fi
fi
# CLI heuristic: also check Cargo.toml [[bin]] section (Rust CLI)
if [ -f "Cargo.toml" ] && grep -qE '^\[\[bin\]\]' Cargo.toml 2>/dev/null && [ -z "${candidates// /}" ]; then
    HAS_BIN="true"
    if [ -z "$BIN_EVIDENCE" ]; then BIN_EVIDENCE="Cargo.toml [[bin]] section"; else BIN_EVIDENCE="$BIN_EVIDENCE; Cargo.toml [[bin]]"; fi
fi
# Only classify as cli-tool if no UI/desktop/mobile/backend candidate was added
if [ "$HAS_BIN" = "true" ] && [ -z "${candidates// /}" ]; then
    add_candidate "cli-tool"
    add_evidence "$BIN_EVIDENCE"
fi

# ---- Resolve ----
# Count candidates
candidate_count=$(printf '%s\n' $candidates | sed '/^$/d' | wc -l | tr -d ' ')
[ -z "$candidate_count" ] && candidate_count=0

# Pick primary kind with priority: tauri > electron > react-native > flutter > web-spa > backend-service > cli-tool
SELECTED="none"
for kind in tauri-desktop electron-desktop react-native flutter web-spa backend-service cli-tool; do
    case " $candidates " in
        *" $kind "*) SELECTED="$kind"; break ;;
    esac
done

# Confidence
ev_count=$(printf '%s\n' "$evidence" | sed '/^$/d' | wc -l | tr -d ' ')
[ -z "$ev_count" ] && ev_count=0
if [ "$SELECTED" = "none" ]; then
    CONFIDENCE="high"
    add_evidence "no UI/runtime surface markers detected"
elif [ "$candidate_count" -gt 1 ]; then
    CONFIDENCE="low"
    add_evidence "multiple surface kinds matched: $(echo $candidates | sed 's/^ //')"
elif [ "$ev_count" -ge 2 ]; then
    CONFIDENCE="high"
else
    CONFIDENCE="medium"
fi

# ---- Emit JSON ----
emit_json() {
    # Build evidence array as JSON
    local ev_json="["
    local first=1
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        # Escape backslashes and quotes
        local esc=$(printf '%s' "$line" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')
        if [ "$first" -eq 1 ]; then
            ev_json="$ev_json\"$esc\""
            first=0
        else
            ev_json="$ev_json,\"$esc\""
        fi
    done <<EOF
$evidence
EOF
    ev_json="$ev_json]"

    printf '{"surface_kind":"%s","confidence":"%s","evidence":%s}' "$SELECTED" "$CONFIDENCE" "$ev_json"
}

OUTPUT=$(emit_json)

# ---- Cache write ----
mkdir -p "$(dirname "$CACHE_PATH")" 2>/dev/null || true
printf '%s' "$OUTPUT" > "$CACHE_PATH" 2>/dev/null || true

echo "$OUTPUT"
exit 0
