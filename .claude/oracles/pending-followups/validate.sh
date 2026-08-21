#!/bin/bash
# pending-followups oracle self-test (Unix)
#
# Two layers, and the second is the point.
#
# LAYER 1 (pre-existing): run the oracle against the live repo and assert the
# schema fields are present. This is a SHAPE check. It is what shipped from
# 2026-04 to 2026-08-19, and it is exactly why FOLLOWUP-01 survived: the
# validator never asserted a VERDICT, so a date branch that could not match any
# entry the sanctioned producer writes was indistinguishable from one that
# worked. overdue was a constant and every self-test passed.
#
# LAYER 2 (FOLLOWUP-01/02, DEC-385): sandboxed cells that write entries THROUGH
# the /0-uldf-schedule documented output format -- **After YYYY-MM-DD (title)**
# and **On YYYY-MM-DD (title)** -- with a past date, and assert the oracle
# reports them overdue. The producer was never in the loop before; the old
# fixtures were all written in the format the oracle already parsed.
#
# The CONTROL cells are half the design. A fix that simply made the oracle
# credulous -- any line with a date is a date trigger, or any line I cannot
# parse is unknown -- passes every must-STILL-fire cell here and fails V5/V6/V7.
set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
PASS=0; FAIL=0

ok()  { PASS=$((PASS+1)); echo "  ok   $1"; }
bad() { FAIL=$((FAIL+1)); echo "  FAIL $1" >&2; }

PYBIN=""
for c in python3 python; do command -v "$c" >/dev/null 2>&1 && { PYBIN="$c"; break; }; done

# ---------------------------------------------------------------- LAYER 1 ----
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if [ -n "$PYBIN" ]; then
    if ! echo "$OUTPUT" | "$PYBIN" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        exit 1
    fi
fi

for field in briefing has_followups_section total overdue overdue_evaluable unevaluable items; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

if [ -z "$PYBIN" ]; then
    echo "SKIP: no python on PATH; verdict cells unavailable"
    echo "PASS: pending-followups oracle validates (shape only)"
    exit 0
fi

# ---------------------------------------------------------------- LAYER 2 ----
SANDBOX="$(mktemp -d 2>/dev/null || echo "${TMPDIR:-/tmp}/pf-validate-$$")"
mkdir -p "$SANDBOX"
cleanup() { rm -rf "$SANDBOX" 2>/dev/null || true; }
trap cleanup EXIT

# A machine-global source would contaminate every project-scoped assertion.
FAKEHOME="$SANDBOX/home"; mkdir -p "$FAKEHOME/.claude"; : > "$FAKEHOME/.claude/MACHINE_CONFIG.md"

jget() { printf '%s' "$1" | "$PYBIN" -c "import sys,json; d=json.load(sys.stdin); print($2)" 2>/dev/null; }

# proj <name>  -- reads the section body from stdin, wraps it in a CLAUDE.md
proj() {
    rm -rf "$SANDBOX/$1"; mkdir -p "$SANDBOX/$1"
    { printf '## Pending Follow-Ups\n\n'; cat; printf '\n## Next\n'; } > "$SANDBOX/$1/CLAUDE.md"
}
runp() { ( cd "$SANDBOX/$1" && HOME="$FAKEHOME" bash "$ORACLE_DIR/run.sh" 2>/dev/null ); }

PAST="$(date -d '30 days ago' +%Y-%m-%d 2>/dev/null || echo '2020-01-01')"
FUTURE="$(date -d '3650 days' +%Y-%m-%d 2>/dev/null || echo '2099-12-31')"

# --- V1: the producer --after form. THE cell that would have caught it. ------
proj v1 <<EOF
- **After $PAST (Standing telemetry review)**: do the thing.

  Remove this entry once acted on.
EOF
out="$(runp v1)"
o="$(jget "$out" "d['overdue']")"; t="$(jget "$out" "d['total']")"
io="$(jget "$out" "d['items'][0]['overdue']")"; idue="$(jget "$out" "d['items'][0]['due']")"
if [ "$t" = "1" ] && [ "$o" = "1" ] && [ "$io" = "True" ]; then
    ok "V1 schedule --after form reports OVERDUE (total=1, overdue=1)"
else
    bad "V1 producer --after form not overdue: total=$t overdue=$o item.overdue=$io -- the FOLLOWUP-01 defect is back"
fi
if [ "$idue" = "$PAST" ]; then ok "V1 due carries the date, not the whole bold prefix"
else bad "V1 expected due=$PAST, got $idue"; fi

# --- V2: the producer --on form (absent from the pre-fix alternation) --------
proj v2 <<EOF
- **On $PAST (A dated review)**: do the thing.
EOF
out="$(runp v2)"; o="$(jget "$out" "d['overdue']")"
if [ "$o" = "1" ]; then ok "V2 schedule --on form reports OVERDUE"
else bad "V2 producer --on form not overdue (overdue=$o)"; fi

# --- V3: a future date is a real measurement, not an unknown -----------------
proj v3 <<EOF
- **After $FUTURE (Not yet due)**: later.
EOF
out="$(runp v3)"; o="$(jget "$out" "d['overdue']")"; u="$(jget "$out" "d['unevaluable']")"
io="$(jget "$out" "d['items'][0]['overdue']")"
if [ "$o" = "0" ] && [ "$u" = "0" ] && [ "$io" = "False" ]; then
    ok "V3 future producer entry: overdue=false as a MEASUREMENT (not null, not overdue)"
else bad "V3 expected overdue=0/unevaluable=0/item=False, got $o/$u/$io"; fi

# --- V4 CONTROL: a trigger entry has no date and is NOT unevaluable ----------
proj v4 <<'EOF'
- **Critic verdict telemetry (trigger: when the first PODS session converges)**: capture it.
EOF
out="$(runp v4)"; t="$(jget "$out" "d['total']")"; u="$(jget "$out" "d['unevaluable']")"
io="$(jget "$out" "d['items'][0]['overdue']")"
if [ "$t" = "1" ] && [ "$u" = "0" ] && [ "$io" = "False" ]; then
    ok "V4 CONTROL trigger-based entry stays a clean label (not an unknown)"
else bad "V4 CONTROL expected total=1/unevaluable=0/overdue=False, got $t/$u/$io -- DEC-379: an unknown may only be emitted where a real question existed"; fi

# --- V5 CONTROL: a date MENTIONED mid-prefix is a label, not a trigger -------
# 59 of the 61 dated-looking prefixes measured machine-wide are this shape.
proj v5 <<'EOF'
- **In-App Agent (TUTOR) -- Phase 3 SHIPPED 2020-01-01 (58da696)**: shipped, kept for provenance.
- **[ACTIVE, added 2020-02-02]**: an ongoing note.
EOF
out="$(runp v5)"; t="$(jget "$out" "d['total']")"; o="$(jget "$out" "d['overdue']")"; u="$(jget "$out" "d['unevaluable']")"
if [ "$t" = "2" ] && [ "$o" = "0" ] && [ "$u" = "0" ]; then
    ok "V5 CONTROL a date mentioned mid-prefix is neither overdue nor unknown"
else bad "V5 CONTROL expected total=2/overdue=0/unevaluable=0, got $t/$o/$u -- contains-a-date would misjudge 59 live entries"; fi

# --- V6: DECLARED but unreadable => null, never false -----------------------
proj v6 <<'EOF'
- **After 2026-02-30 (An impossible date)**: this date cannot exist.
EOF
out="$(runp v6)"; u="$(jget "$out" "d['unevaluable']")"; e="$(jget "$out" "d['overdue_evaluable']")"
io="$(jget "$out" "d['items'][0]['overdue']")"; idays="$(jget "$out" "d['items'][0]['days_overdue']")"
b="$(jget "$out" "d['briefing']")"
if [ "$u" = "1" ] && [ "$e" = "False" ] && [ "$io" = "None" ] && [ "$idays" = "None" ]; then
    ok "V6 declared-but-unreadable date => overdue:null + overdue_evaluable:false (CSI-36/DEC-342)"
else bad "V6 expected unevaluable=1/evaluable=False/item null, got $u/$e/$io/$idays"; fi
case "$b" in
    *FLOOR*) ok "V6 the briefing SAYS the count is a floor (silence is half the defect -- OVALID-09)" ;;
    *) bad "V6 briefing does not disclose the unevaluable entry: $b" ;;
esac

# --- V7 CONTROL: the legacy bare form must not have narrowed ----------------
proj v7 <<'EOF'
- **After 2020-01-01**: the pre-fix fixture shape.
- **2020-02-02**: the bare pre-fix fixture shape.
EOF
out="$(runp v7)"; o="$(jget "$out" "d['overdue']")"
if [ "$o" = "2" ]; then ok "V7 CONTROL legacy bare forms still parse (the fix widened, never narrowed)"
else bad "V7 CONTROL expected overdue=2, got $o -- the fix NARROWED"; fi

# --- V8: the briefing must survive the session-start extractor --------------
# session-start.sh uses: grep -oE  "briefing"[[:space:]]*:[[:space:]]*"[^"]*"
# so a stray double quote anywhere in the field truncates the whole line.
proj v8 <<EOF
- **After $PAST (An entry whose title carries a "quoted phrase" and a comma)**: body.
EOF
out="$(runp v8)"
extracted=$(printf '%s' "$out" | tr -d '\n\r' \
    | grep -oE '"briefing"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 \
    | sed -E 's/^"briefing"[[:space:]]*:[[:space:]]*"(.*)"$/\1/')
full="$(jget "$out" "d['briefing']")"
if [ -n "$extracted" ] && [ "$extracted" = "$full" ]; then
    ok "V8 briefing round-trips through the consumer extractor even with quotes in a title"
else bad "V8 extractor got [$extracted] but the field is [$full] -- the briefing line would be truncated"; fi
if [ "${#full}" -le 300 ]; then ok "V8 briefing is ${#full} chars (session-start caps the whole briefing at 3000)"
else bad "V8 briefing is ${#full} chars -- too long for the shared cap"; fi

# --- V9 CONTROL: no section at all => empty briefing (line suppressed) ------
rm -rf "$SANDBOX/v9"; mkdir -p "$SANDBOX/v9"
printf '# P\n\nnothing here\n' > "$SANDBOX/v9/CLAUDE.md"
out="$(runp v9)"; b="$(jget "$out" "d['briefing']")"; h="$(jget "$out" "d['has_followups_section']")"
if [ "$h" = "False" ] && [ -z "$b" ]; then ok "V9 CONTROL no section => empty briefing (graceful absence)"
else bad "V9 CONTROL expected has_section=False + empty briefing, got $h/$b"; fi

# --- V10 CONTROL: an entry due TODAY is not yet overdue ---------------------
# TWIN-01 boundary. run.sh compares midnight epochs with `>`; run.ps1 must
# normalise Get-Date to .Date or the same entry reads overdue there and not
# here, for every session after 00:00.
TODAY="$(date +%Y-%m-%d 2>/dev/null || echo '')"
if [ -n "$TODAY" ]; then
proj v10 <<EOF
- **After $TODAY (Due today, not yet past)**: today is not overdue.
EOF
out="$(runp v10)"; o="$(jget "$out" "d['overdue']")"; u="$(jget "$out" "d['unevaluable']")"
if [ "$o" = "0" ] && [ "$u" = "0" ]; then ok "V10 CONTROL an entry dated TODAY is not overdue (midnight boundary)"
else bad "V10 CONTROL expected overdue=0/unevaluable=0, got $o/$u"; fi
fi

# --- V11: days_overdue is exact, and floors ---------------------------------
proj v11 <<EOF
- **After $PAST (Thirty days past)**: exactly thirty.
EOF
out="$(runp v11)"; dd="$(jget "$out" "d['items'][0]['days_overdue']")"
if [ "$dd" = "30" ]; then ok "V11 days_overdue is exactly 30 (integer division / floor, not rounding)"
else bad "V11 expected days_overdue=30, got $dd -- twins disagree on rounding"; fi

echo
echo "PASS=$PASS FAIL=$FAIL"
if [ "$FAIL" -gt 0 ]; then echo "FAIL: pending-followups oracle self-test" >&2; exit 1; fi
echo "PASS: pending-followups oracle validates"
exit 0
