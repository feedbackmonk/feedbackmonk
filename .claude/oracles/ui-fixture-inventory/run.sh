#!/bin/bash
# ui-fixture-inventory oracle (Unix)
# Answers: what fixture/smoke-test infrastructure does this project have?
#
# Output (frozen schema, TGFP-02):
#   {
#     has_fixtures: bool,
#     patterns: [string, ...],
#     counts: { fixtures: int, smoke_specs: int, e2e_specs: int },
#     conventions: [string, ...],
#     briefing: string  (≤200 chars; empty string suppresses the [fixture-inventory] line)
#   }
#
# Briefing-line forms (TGFP-02 acceptance):
#   surface-with-no-fixtures: "fixture-inventory: UI surface detected; no fixture infrastructure. /0-uldf-ldis-plan can scaffold."
#   surface-with-fixtures:    "fixture-inventory: <count> fixtures, <count> smoke specs (conventions: <list>)"
#   no-surface:               "" (line suppressed by hook)
#
# Filesystem stat + glob only — never executes project scripts.

set -e

# ---- Surface detection (composes with ui-surface-detector) ----
SURFACE_KIND="none"
SURFACE_ORACLE_DIR="$(dirname "$0")/../ui-surface-detector"
if [ -f "$SURFACE_ORACLE_DIR/run.sh" ]; then
    SURFACE_OUT=$(bash "$SURFACE_ORACLE_DIR/run.sh" 2>/dev/null || echo '{"surface_kind":"none"}')
    SURFACE_KIND=$(printf '%s' "$SURFACE_OUT" | grep -oE '"surface_kind"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"' || echo "none")
fi
[ -z "$SURFACE_KIND" ] && SURFACE_KIND="none"

# Surface presence (UI surfaces qualify; backend/cli do not for fixture-inventory purposes)
SURFACE_PRESENT="false"
case "$SURFACE_KIND" in
    none|cli-tool|backend-service) SURFACE_PRESENT="false" ;;
    *) SURFACE_PRESENT="true" ;;
esac

# ---- No-surface short-circuit: emit empty briefing (hook will suppress) ----
if [ "$SURFACE_PRESENT" = "false" ]; then
    printf '{"has_fixtures":false,"patterns":[],"counts":{"fixtures":0,"smoke_specs":0,"e2e_specs":0},"conventions":[],"briefing":""}'
    exit 0
fi

# ---- Pattern detection ----
PATTERNS=()
CONVENTIONS=()
FIXTURE_COUNT=0
SMOKE_COUNT=0
E2E_COUNT=0

# Helper: count files matching a glob, depth-bounded to avoid pathological repos
count_matches() {
    local pattern="$1"
    local depth="${2:-6}"
    # Use find with maxdepth; -path matches the full pattern; redirect errors
    find . -maxdepth "$depth" -type f -path "$pattern" 2>/dev/null | wc -l | tr -d ' '
}

# Detect: tests/fixtures/*-smoke.{ts,js,py}
N=$(count_matches "*/tests/fixtures/*-smoke.ts" 6)
N2=$(count_matches "*/tests/fixtures/*-smoke.js" 6)
N3=$(count_matches "*/tests/fixtures/*-smoke.py" 6)
SUM=$((N + N2 + N3))
if [ "$SUM" -gt 0 ]; then
    PATTERNS+=("tests/fixtures/*-smoke.{ts,js,py}")
    CONVENTIONS+=("co-located smoke")
    FIXTURE_COUNT=$((FIXTURE_COUNT + SUM))
fi

# Detect: tests/smoke/*.spec.{ts,js,py}
N=$(count_matches "*/tests/smoke/*.spec.ts" 6)
N2=$(count_matches "*/tests/smoke/*.spec.js" 6)
N3=$(count_matches "*/tests/smoke/*.spec.py" 6)
SUM=$((N + N2 + N3))
if [ "$SUM" -gt 0 ]; then
    PATTERNS+=("tests/smoke/*.spec.{ts,js,py}")
    SMOKE_COUNT=$((SMOKE_COUNT + SUM))
fi

# Detect: e2e/**/*.spec.{ts,js,py}
N=$(find . -maxdepth 6 -type f \( -path "*/e2e/*.spec.ts" -o -path "*/e2e/*.spec.js" -o -path "*/e2e/*.spec.py" -o -path "*/e2e/**/*.spec.ts" -o -path "*/e2e/**/*.spec.js" \) 2>/dev/null | wc -l | tr -d ' ')
if [ "$N" -gt 0 ]; then
    PATTERNS+=("e2e/**/*.spec.{ts,js,py}")
    E2E_COUNT=$((E2E_COUNT + N))
fi

# Detect: __tests__/fixtures/**
N=$(find . -maxdepth 6 -type d -path "*/__tests__/fixtures*" 2>/dev/null | wc -l | tr -d ' ')
if [ "$N" -gt 0 ]; then
    PATTERNS+=("__tests__/fixtures/**")
    CONVENTIONS+=("jest fixtures")
fi

# Detect: tests/visual/**
N=$(find . -maxdepth 6 -type d -path "*/tests/visual*" 2>/dev/null | wc -l | tr -d ' ')
if [ "$N" -gt 0 ]; then
    PATTERNS+=("tests/visual/**")
    CONVENTIONS+=("visual regression")
fi

# Detect: playwright.config.{ts,js,mjs,cjs}
for ext in ts js mjs cjs; do
    if [ -f "playwright.config.$ext" ]; then
        PATTERNS+=("playwright.config.$ext")
        CONVENTIONS+=("playwright")
        break
    fi
done

# Detect: cypress/ + cypress.config.{ts,js}
if [ -d "cypress" ]; then
    PATTERNS+=("cypress/**")
    CONVENTIONS+=("cypress")
fi

# Detect: vitest fixtures (vitest.config.* + tests/fixtures/)
for ext in ts js mjs cjs; do
    if [ -f "vitest.config.$ext" ]; then
        if [ -d "tests/fixtures" ] || [ -d "src/__tests__/fixtures" ]; then
            CONVENTIONS+=("vitest fixtures")
        fi
        break
    fi
done

# ---- Compose has_fixtures ----
HAS_FIXTURES="false"
if [ "$FIXTURE_COUNT" -gt 0 ] || [ "$SMOKE_COUNT" -gt 0 ] || [ "$E2E_COUNT" -gt 0 ] || [ ${#PATTERNS[@]} -gt 0 ]; then
    HAS_FIXTURES="true"
fi

# ---- Compose briefing ----
BRIEFING=""
if [ "$HAS_FIXTURES" = "true" ]; then
    # Suppress when project is healthy — no nudge needed for fixtured projects
    BRIEFING=""
else
    BRIEFING="fixture-inventory: UI surface detected; no fixture infrastructure. /0-uldf-ldis-plan can scaffold."
fi

# Cap briefing at 200 chars (defensive)
if [ ${#BRIEFING} -gt 200 ]; then
    BRIEFING="${BRIEFING:0:197}..."
fi

# ---- Emit JSON ----
# Build patterns array
PATTERNS_JSON=""
for i in "${!PATTERNS[@]}"; do
    p=$(printf '%s' "${PATTERNS[$i]}" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')
    if [ -z "$PATTERNS_JSON" ]; then
        PATTERNS_JSON="\"$p\""
    else
        PATTERNS_JSON="$PATTERNS_JSON,\"$p\""
    fi
done

# Build conventions array (dedupe inline)
CONVENTIONS_JSON=""
SEEN=""
for c in "${CONVENTIONS[@]}"; do
    case " $SEEN " in
        *" $c "*) ;;
        *)
            SEEN="$SEEN $c"
            esc=$(printf '%s' "$c" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')
            if [ -z "$CONVENTIONS_JSON" ]; then
                CONVENTIONS_JSON="\"$esc\""
            else
                CONVENTIONS_JSON="$CONVENTIONS_JSON,\"$esc\""
            fi
            ;;
    esac
done

ESC_BRIEFING=$(printf '%s' "$BRIEFING" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')

printf '{"has_fixtures":%s,"patterns":[%s],"counts":{"fixtures":%d,"smoke_specs":%d,"e2e_specs":%d},"conventions":[%s],"briefing":"%s"}' \
    "$HAS_FIXTURES" \
    "$PATTERNS_JSON" \
    "$FIXTURE_COUNT" "$SMOKE_COUNT" "$E2E_COUNT" \
    "$CONVENTIONS_JSON" \
    "$ESC_BRIEFING"

exit 0
