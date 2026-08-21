#!/bin/bash
# workflow-position oracle self-test (Unix)
set -e
ORACLE_DIR="$(dirname "$0")"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if ! echo "$OUTPUT" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
    if ! echo "$OUTPUT" | python -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        exit 1
    fi
fi

for field in position latest_intake latest_plan latest_start_analysis spec_exists ltads_active ltads_session_status suggested_next_command proceed_hint; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

# position must be one of the declared enums
pos=$(echo "$OUTPUT" | grep -oE '"position"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
case "$pos" in
    NONE|POST-IDEATE|POST-INTAKE|POST-SPEC|POST-PLAN|IN-EXECUTION|POST-IMPLEMENTATION|UNKNOWN) ;;
    *) echo "FAIL: position '$pos' is not in the declared enum" >&2; exit 1 ;;
esac

# PLANKIND-01 (DEC-327): `latest_plan` must name the PLAN OF RECORD, never a
# start-analysis stub that merely sorts later. Driven over a sandbox fixture
# built to the shape that was actually witnessed shadowing a program plan.
# The two must-still-hold controls are as load-bearing as the subject: a fix
# that simply stopped reporting plans would pass the subject perfectly.
_wp_sb="$(mktemp -d)"
_wp_fail=0
# ORACLE_DIR is relative ($(dirname "$0")); these cells cd into a sandbox, so
# resolve it absolutely first or run.sh silently is not found.
_wp_oracle_abs="$(cd "$ORACLE_DIR" && pwd)"
(
  mkdir -p "$_wp_sb/docs/planning/plans" "$_wp_sb/docs/specs"
  cd "$_wp_sb" || exit 1
  printf '# program plan\n\n## Testability Gate Findings\n' > docs/planning/plans/20260803T144837-program-plan.md
  printf '# stub\n' > docs/planning/plans/20260803T190500-stage0-start-analysis.md
  printf -- '---\nkind: start-analysis\n---\n\n# stub by front matter only\n' > docs/planning/plans/20260804T010000-zzz-renamed.md
  : > docs/specs/SPECIFICATION.md
  out="$(bash "$_wp_oracle_abs/run.sh" 2>/dev/null)"
  echo "$out" | grep -q '"latest_plan":"docs/planning/plans/20260803T144837-program-plan.md"' \
    || { echo "FAIL: latest_plan is shadowed by a start-analysis (PLANKIND-01)" >&2; exit 1; }
  # front-matter arm: the NEWEST file is a start-analysis by front matter only
  echo "$out" | grep -q '"latest_start_analysis":"docs/planning/plans/20260804T010000-zzz-renamed.md"' \
    || { echo "FAIL: front-matter kind: start-analysis not detected" >&2; exit 1; }
  # control: POSITION must not regress when only analyses remain
  rm -f docs/planning/plans/20260803T144837-program-plan.md
  out2="$(bash "$_wp_oracle_abs/run.sh" 2>/dev/null)"
  echo "$out2" | grep -q '"position":"POST-PLAN"' \
    || { echo "FAIL: analyses-only project regressed out of POST-PLAN" >&2; exit 1; }
) || _wp_fail=1
rm -rf "$_wp_sb"
[ "$_wp_fail" = "0" ] || exit 1

echo "PASS: workflow-position oracle validates (position=$pos; PLANKIND-01 plan-of-record cells green)"
exit 0
