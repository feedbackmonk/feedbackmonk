#!/bin/bash
# workflow-position oracle (Unix)
# Answers: where is this project in the LDIS/LTADS workflow, and what is the next /0-uldf-proceed step?
#
# Position values:
#   NONE                  - No planning artifacts and no active LTADS
#   POST-IDEATE           - Ideate artifacts exist but no intake yet
#   POST-INTAKE           - Intake exists, newer than any plan, no active implementation
#   POST-SPEC             - Spec exists, plan is either missing or older than spec
#   POST-PLAN             - Plan exists and is the newest planning artifact
#   IN-EXECUTION          - LTADS session is ACTIVE
#   POST-IMPLEMENTATION   - LTADS session status is COMPLETED or equivalent and no new plan
#   UNKNOWN               - Artifacts in an unexpected combination

set -e

# -----------------------------------------------------------------------------
# Artifact resolution (newest-wins per /0-uldf-proceed Phase 1 Step 1.2)
# -----------------------------------------------------------------------------

newest_in_dir() {
    # prints the newest file by lexical filename sort (LDIS convention: YYYY-MM-DD-HHMMSS prefix)
    dir="$1"
    [ -d "$dir" ] || return 1
    ls -1 "$dir" 2>/dev/null | grep -vE '^(README\.md|\..*)$' | sort | tail -1
}

LATEST_INTAKE=""
if [ -d "docs/planning/intakes" ]; then
    name=$(newest_in_dir "docs/planning/intakes" || true)
    if [ -n "$name" ]; then LATEST_INTAKE="docs/planning/intakes/$name"; fi
fi
if [ -z "$LATEST_INTAKE" ] && [ -f "docs/planning/intake-assessment.md" ]; then
    LATEST_INTAKE="docs/planning/intake-assessment.md"
fi

LATEST_PLAN=""
if [ -d "docs/planning/plans" ]; then
    name=$(newest_in_dir "docs/planning/plans" || true)
    if [ -n "$name" ]; then LATEST_PLAN="docs/planning/plans/$name"; fi
fi
if [ -z "$LATEST_PLAN" ] && [ -f "docs/planning/execution-plan.md" ]; then
    LATEST_PLAN="docs/planning/execution-plan.md"
fi

SPEC_EXISTS=false
if [ -f "docs/specs/SPECIFICATION.md" ]; then SPEC_EXISTS=true; fi

IDEATE_EXISTS=false
if [ -d "docs/planning/ideations" ] && [ -n "$(ls -A docs/planning/ideations 2>/dev/null | grep -v README)" ]; then
    IDEATE_EXISTS=true
fi

# LTADS state
LTADS_ACTIVE=false
LTADS_STATUS=""
if [ -f "ltads/sessions/current-session.md" ]; then
    LTADS_STATUS=$(grep -oE '^(##[[:space:]]+|-[[:space:]]+\*\*|\*\*)Status(\*\*)?[[:space:]]*:[[:space:]]*[A-Z_]+' "ltads/sessions/current-session.md" 2>/dev/null | head -1 | grep -oE '[A-Z_]+$' || true)
    case "$LTADS_STATUS" in
        ACTIVE|IN_PROGRESS|STARTED) LTADS_ACTIVE=true ;;
    esac
fi

# -----------------------------------------------------------------------------
# Derive position via precedence (highest wins)
# -----------------------------------------------------------------------------

POSITION="NONE"
NEXT_CMD="null"
HINT=""

if [ "$LTADS_ACTIVE" = "true" ]; then
    POSITION="IN-EXECUTION"
    NEXT_CMD="Continue current work (or /0-uldf-finalize when implementation complete)"
    HINT="LTADS session is active. /0-uldf-proceed (IN-EXECUTION) will route to worker finalization or /0-uldf-finalize."
elif [ -n "$LTADS_STATUS" ] && { [ "$LTADS_STATUS" = "COMPLETED" ] || [ "$LTADS_STATUS" = "STOPPED" ] || [ "$LTADS_STATUS" = "FINALIZED" ]; }; then
    POSITION="POST-IMPLEMENTATION"
    NEXT_CMD="/0-uldf-finalize"
    HINT="Prior LTADS session is finalized. /0-uldf-proceed routes to /0-uldf-finalize (if not already run) or the next phase."
elif [ -n "$LATEST_PLAN" ] && { [ -z "$LATEST_INTAKE" ] || [ "$LATEST_PLAN" \> "$LATEST_INTAKE" ]; }; then
    POSITION="POST-PLAN"
    NEXT_CMD="/0-uldf-pods-parallelize or /0-uldf-ltads-start (per plan)"
    HINT="Plan is newest artifact. /0-uldf-proceed will read the plan's execution strategy and route to PODS or LTADS."
elif [ "$SPEC_EXISTS" = "true" ] && { [ -z "$LATEST_PLAN" ] || [ "docs/specs/SPECIFICATION.md" -nt "$LATEST_PLAN" ]; }; then
    POSITION="POST-SPEC"
    NEXT_CMD="/0-uldf-ldis-plan"
    HINT="Spec exists without a newer plan. /0-uldf-proceed routes to /0-uldf-ldis-plan."
elif [ -n "$LATEST_INTAKE" ]; then
    POSITION="POST-INTAKE"
    NEXT_CMD="/0-uldf-ldis-plan or /0-uldf-ldis-spec (per intake recommendation)"
    HINT="Intake is newest artifact. /0-uldf-proceed will honor the intake's RECOMMENDED NEXT STEPS."
elif [ "$IDEATE_EXISTS" = "true" ]; then
    POSITION="POST-IDEATE"
    NEXT_CMD="/0-uldf-ldis-intake"
    HINT="Ideation artifacts exist without intake. /0-uldf-proceed routes to /0-uldf-ldis-intake."
else
    POSITION="NONE"
    NEXT_CMD="null"
    HINT="No planning artifacts or active LTADS. Start with /0-uldf-ldis-ideate or /0-uldf-ldis-intake."
fi

# -----------------------------------------------------------------------------
# Emit JSON
# -----------------------------------------------------------------------------

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

jsonval() {
    if [ -z "$1" ]; then printf 'null'; else printf '"%s"' "$(esc "$1")"; fi
}

intake_json=$(jsonval "$LATEST_INTAKE")
plan_json=$(jsonval "$LATEST_PLAN")
ltads_status_json=$(jsonval "$LTADS_STATUS")
next_cmd_json=$(jsonval "$NEXT_CMD")
[ "$NEXT_CMD" = "null" ] && next_cmd_json="null"

cat <<EOF
{"position":"$POSITION","latest_intake":$intake_json,"latest_plan":$plan_json,"spec_exists":$SPEC_EXISTS,"ltads_active":$LTADS_ACTIVE,"ltads_session_status":$ltads_status_json,"suggested_next_command":$next_cmd_json,"proceed_hint":"$(esc "$HINT")"}
EOF
