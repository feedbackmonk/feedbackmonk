#!/usr/bin/env bash
# Run only the named tests, each with its own front end's runner. This is the
# `finalize.test_select` template's command (`.claude/config.json`): the landing
# check hands it the repo-relative test paths that observe a change, and it
# never runs a whole suite in their place.
#
#   widget/**/*.test.ts | admin-ui/**/*.test.ts(x)   -> vitest run <path>
#   widget/e2e/*.spec.ts | admin-ui/e2e/*.spec.ts     -> playwright test <path>
#   crates/<crate>/**                                 -> cargo test -p <crate>
#   <dir>/tests/test_*.py                             -> python -m unittest, run in <dir>/tests
#
# A path it cannot place is an error, not a skip: a test the landing check
# believed it ran must have run.
set -euo pipefail
cd "$(dirname "$0")/.."

declare -A vitest=() playwright=() crates=() pytests=()
for path in "$@"; do
  path="${path//\\//}"
  case "$path" in
    widget/e2e/*.spec.ts) playwright[widget]+=" ${path#widget/}" ;;
    admin-ui/e2e/*.spec.ts) playwright[admin-ui]+=" ${path#admin-ui/}" ;;
    widget/*.test.ts) vitest[widget]+=" ${path#widget/}" ;;
    admin-ui/*.test.ts | admin-ui/*.test.tsx) vitest[admin-ui]+=" ${path#admin-ui/}" ;;
    crates/*/*) crate="${path#crates/}"; crates[${crate%%/*}]=1 ;;
    */tests/test_*.py) pytests[${path%/*}]+=" $(basename "$path" .py)" ;;
    *) echo "run-selected-tests: no runner for '$path'" >&2; exit 2 ;;
  esac
done

status=0
for dir in "${!vitest[@]}"; do
  # widget's vitest needs the generated locale slices first (its `test` script does the same).
  if [ "$dir" = widget ]; then (cd widget && npm run --silent gen:locales); fi
  # shellcheck disable=SC2086
  (cd "$dir" && npx vitest run ${vitest[$dir]}) || status=1
done
for dir in "${!playwright[@]}"; do
  # shellcheck disable=SC2086
  (cd "$dir" && npx playwright test ${playwright[$dir]}) || status=1
done
for crate in "${!crates[@]}"; do
  cargo test -p "$crate" || status=1
done
for dir in "${!pytests[@]}"; do
  # shellcheck disable=SC2086
  (cd "$dir" && python -m unittest ${pytests[$dir]}) || status=1
done
exit $status
