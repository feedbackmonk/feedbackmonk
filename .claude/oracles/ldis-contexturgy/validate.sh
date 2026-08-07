#!/bin/bash
# ldis-contexturgy oracle self-test (Unix). Sandboxed fixtures over the
# CTXY-01 contract:
#   1. no ledger                 -> pass + NO-DATA note
#   2. gap (stale artifact home) -> warn + {skill,at,expected} + briefing
#   3. crystallized (fresh file) -> pass, checked=1
#   4. other-project record      -> ignored (checked=0)
#   5. out-of-window record      -> ignored (checked=0)
#   6. never-fail invariant      -> warn case status != fail
# Cross-shell: when powershell.exe present, parity leg on cases 2+3.

set -uo pipefail

THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_SH="$THIS_DIR/run.sh"
RUN_PS="$THIS_DIR/run.ps1"

SANDBOX="$(cd "$(mktemp -d 2>/dev/null || mktemp -d -t ldis-ctxy)" && pwd -P)"
trap 'rm -rf "$SANDBOX"' EXIT

PASS=0; FAIL=0
ok()  { echo "PASS: $1"; PASS=$((PASS+1)); }
bad() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }

PROJ="$SANDBOX/projx"
LEDGER="$SANDBOX/ledger"
mkdir -p "$PROJ/docs/planning/intakes" "$PROJ/docs/specs" "$LEDGER"

AT_RECENT="$(date -d '1 hour ago' +%Y-%m-%dT%H:%M:%S)"
AT_OLD="$(date -d '3 days ago' +%Y-%m-%dT%H:%M:%S)"

write_ledger() { # $1 = at, $2 = project, $3 = cmd
    printf '{"at":"%s","cmd":"%s","project":"%s","type":"skill","machine":"TEST","session":"s1"}\n' \
        "$1" "$3" "$2" > "$LEDGER/test.jsonl"
}

run_o() { ( cd "$PROJ" && CLAUDE_USAGE_LEDGER_DIR="$LEDGER" bash "$RUN_SH" ); }

# ---- 1. no ledger -> pass + NO-DATA ------------------------------------------
out="$(cd "$PROJ" && CLAUDE_USAGE_LEDGER_DIR="$SANDBOX/__none__" bash "$RUN_SH")"
case "$out" in
    *'"status":"pass"'*NO-DATA*) ok "no ledger -> pass + NO-DATA note" ;;
    *) bad "no-ledger (got: $out)" ;;
esac

# ---- 2. gap: intake invoked, intakes/ has only an older file -----------------
touch -d '2 days ago' "$PROJ/docs/planning/intakes/old.md"
write_ledger "$AT_RECENT" "projx" "0-uldf-ldis-intake"
out="$(run_o)"
case "$out" in
    *'"status":"warn"'*) ok "gap -> warn" ;;
    *) bad "gap verdict (got: $out)" ;;
esac
echo "$out" | grep -q '"skill":"0-uldf-ldis-intake"' && ok "gap entry names the skill" || bad "gap skill missing: $out"
echo "$out" | grep -q '"expected":"docs/planning/intakes"' && ok "gap entry names the expected home" || bad "gap expected missing: $out"
echo "$out" | grep -qi "Ephemeral" && ok "briefing present on warn" || bad "briefing missing: $out"
case "$out" in *'"status":"fail"'*) bad "NEVER-FAIL invariant broken" ;; *) ok "never-fail invariant holds" ;; esac

# ---- 3. crystallized: fresh artifact after the invocation --------------------
printf 'x' > "$PROJ/docs/planning/intakes/fresh-intake.md"   # mtime now > AT_RECENT
out="$(run_o)"
case "$out" in
    *'"status":"pass"'*'"checked":1'*) ok "crystallized -> pass (checked=1)" ;;
    *) bad "crystallized (got: $out)" ;;
esac

# ---- 4. other-project record ignored -----------------------------------------
write_ledger "$AT_RECENT" "some-other-project" "0-uldf-ldis-intake"
out="$(run_o)"
case "$out" in
    *'"checked":0'*'"status":"pass"'*|*'"status":"pass"'*'"checked":0'*) ok "other-project record ignored" ;;
    *) bad "other-project (got: $out)" ;;
esac

# ---- 5. out-of-window record ignored ------------------------------------------
write_ledger "$AT_OLD" "projx" "0-uldf-ldis-intake"
out="$(run_o)"
case "$out" in
    *'"checked":0'*|*'"status":"pass"'*) ok "out-of-window record ignored" ;;
    *) bad "out-of-window (got: $out)" ;;
esac

# ---- 6. ldis-spec maps to docs/specs mtime ------------------------------------
write_ledger "$AT_RECENT" "projx" "0-uldf-ldis-spec"
out="$(run_o)"
case "$out" in
    *'"status":"warn"'*'"expected":"docs/specs"'*) ok "spec gap -> warn on docs/specs" ;;
    *) bad "spec-gap (got: $out)" ;;
esac
printf 'x' > "$PROJ/docs/specs/SPECIFICATION.md"
out="$(run_o)"
case "$out" in
    *'"status":"pass"'*) ok "spec mtime bump -> pass" ;;
    *) bad "spec-bump (got: $out)" ;;
esac

# ---- PS parity (focused) -------------------------------------------------------
if command -v powershell.exe >/dev/null 2>&1 && command -v cygpath >/dev/null 2>&1; then
    LEDGER_WIN="$(cygpath -m "$LEDGER")"
    run_ps() { ( cd "$PROJ" && CLAUDE_USAGE_LEDGER_DIR="$LEDGER_WIN" powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$RUN_PS" 2>/dev/null | tr -d '\r' ); }

    rm -f "$PROJ/docs/planning/intakes/fresh-intake.md"
    write_ledger "$AT_RECENT" "projx" "0-uldf-ldis-intake"
    out="$(run_ps)"
    case "$out" in
        *'"status":"warn"'*'"skill":"0-uldf-ldis-intake"'*) ok "PS parity: gap -> warn" ;;
        *) bad "PS gap (got: $out)" ;;
    esac
    printf 'x' > "$PROJ/docs/planning/intakes/fresh2.md"
    out="$(run_ps)"
    case "$out" in
        *'"status":"pass"'*'"checked":1'*) ok "PS parity: crystallized -> pass" ;;
        *) bad "PS crystallized (got: $out)" ;;
    esac
else
    echo "SKIP: PS parity (no powershell.exe/cygpath)"
fi

echo
echo "ldis-contexturgy validate: $PASS pass / $FAIL fail"
[ "$FAIL" = "0" ] && exit 0 || exit 1
