#!/bin/bash
# jig-demand --self-test (Unix)
#
# ⚠ A PASSING SELF-TEST IS NOT A TRUST VERDICT (OVALID-03). Every cell below is
# written in the vocabulary of what the oracle MEASURES (lexically-similar
# records across distinct occasions), and is structurally blind to the gap
# between that and what it ASSERTS (the same capability, still unbuilt). Read
# oracle.json `assertion.known_gaps` before believing a green run.
#
# The two load-bearing cells are the brief's own falsifiability requirement:
#   T2  N candidates for ONE capability across N occasions  MUST escalate
#   T3  N DISTINCT capabilities, one occasion each          MUST NOT escalate  <- anti-vacuity
# T3 is the control that separates this oracle from one that simply fires on any
# non-empty log. Without it T2 passes for a detector that always says "signal".

set -u
PASS=0; FAIL=0
THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
RUN="$THIS_DIR/run.sh"
SB="$(mktemp -d 2>/dev/null || echo "/tmp/jigd-$$")"
trap 'rm -rf "$SB" 2>/dev/null' EXIT

ok()   { PASS=$((PASS+1)); echo "  PASS  $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL  $1"; }
chk()  { if [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1 (expected '$3', got '$2')"; fi; }

_py=""
for _c in python python3 py; do
    if command -v "$_c" >/dev/null 2>&1 && "$_c" -c 'import json,sys' >/dev/null 2>&1; then _py="$_c"; break; fi
done

field() { printf '%s' "$1" | "$_py" -c "
import json,sys
d=json.load(sys.stdin)
k=sys.argv[1]
v=d
for p in k.split('.'):
    v = v[p] if isinstance(v,dict) else v
print(v if not isinstance(v,(list,dict)) else len(v))
" "$2"; }

mklog() { : > "$1"; }
add() { # add <log> <ts> <sessionId|-> <question> [capability]
    local log="$1" ts="$2" sid="$3" q="$4" cap="${5:-}"
    if [ "$sid" = "-" ]; then
        printf '{"schemaVersion":"1","ts":"%s","sessionId":null,"category":"other","question":"%s","capability":"%s","aria_could_answer":true,"surface_present":false}\n' \
            "$ts" "$q" "$cap" >> "$log"
    else
        printf '{"schemaVersion":"1","ts":"%s","sessionId":"%s","category":"other","question":"%s","capability":"%s","aria_could_answer":true,"surface_present":false}\n' \
            "$ts" "$sid" "$q" "$cap" >> "$log"
    fi
}
runin() { # runin <dir>  -> stdout json
    ( cd "$1" && bash "$RUN" )
}
newproj() { mkdir -p "$SB/$1/.claude/session-state"; printf '%s' "$SB/$1"; }

echo "jig-demand self-test"

# ---------------------------------------------------------------------------
# T1  Absent log -> ok, empty briefing, clustered:true (measured, found nothing)
# ---------------------------------------------------------------------------
P="$(newproj p1)"
OUT="$(runin "$P")"
chk "T1a absent log -> status ok"        "$(field "$OUT" status)"    "ok"
chk "T1b absent log -> empty briefing"   "$(field "$OUT" briefing)"  ""
chk "T1c absent log -> clustered true"   "$(field "$OUT" clustered)" "True"

# ---------------------------------------------------------------------------
# T2  LOAD-BEARING: one capability, 4 candidates, 4 distinct sessions -> SIGNAL
# ---------------------------------------------------------------------------
P="$(newproj p2)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T10:00:00Z" "sess-a" "Drive the settings screen and screenshot which panel rendered" "fixture-ish"
add "$L" "2026-07-08T10:00:00Z" "sess-b" "Drive the settings screen to screenshot the rendered panel state" "fixture-ish"
add "$L" "2026-07-15T10:00:00Z" "sess-c" "Screenshot the settings screen panel after driving it, rendered state" "fixture-ish"
add "$L" "2026-07-22T10:00:00Z" "sess-d" "Panel screenshot: drive settings screen, capture what rendered" "fixture-ish"
OUT="$(runin "$P")"
chk "T2a repeated capability -> status signal"  "$(field "$OUT" status)"   "signal"
chk "T2b one cluster reported"                  "$(field "$OUT" clusters)" "1"
chk "T2c briefing is non-empty"                 "$([ -n "$(field "$OUT" briefing)" ] && echo yes || echo no)" "yes"

# ---------------------------------------------------------------------------
# T3  ANTI-VACUITY CONTROL: 6 DISTINCT capabilities, one occasion each -> ok
#     Same corpus SIZE as T2 and then some. If this fires, the oracle is
#     detecting "a log exists", not recurrence, and T2 proves nothing.
#
#     Every record deliberately SHARES the log's house vocabulary ("verify",
#     "the running app", "without a human"), because that is how the real corpus
#     reads -- 19 of 39 quiqpic questions contain "verify". An earlier version of
#     this cell used token-DISJOINT questions, and mutation M3 (similarity forced
#     to 0.0) then left it green: single-link merging never considers a pair with
#     an empty intersection, so the cell was refusing the corpus for the wrong
#     reason and proved nothing about the THRESHOLD. Shared incidental
#     vocabulary is what makes this a control.
# ---------------------------------------------------------------------------
P="$(newproj p3)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T10:00:00Z" "s1" "Verify audio-video sync drift in the running app without a human"       "probe:avsync"
add "$L" "2026-07-02T10:00:00Z" "s2" "Verify the exported archive uses fragmented atoms without a human"      "probe:mp4"
add "$L" "2026-07-03T10:00:00Z" "s3" "Verify database migration history in the running app without a human"   "state-dump:migrations"
add "$L" "2026-07-04T10:00:00Z" "s4" "Verify a one-time passcode arrives in the mail inbox without a human"   "companion:mail"
add "$L" "2026-07-05T10:00:00Z" "s5" "Verify which localisation strings lack translations without a human"    "oracle:i18n"
add "$L" "2026-07-06T10:00:00Z" "s6" "Verify the sandbox resets between benchmark iterations without a human" "environment-resetter"
OUT="$(runin "$P")"
chk "T3a distinct singletons -> status ok"      "$(field "$OUT" status)"   "ok"
chk "T3b no clusters"                           "$(field "$OUT" clusters)" "0"
chk "T3c empty briefing (quiet-path invariant)" "$(field "$OUT" briefing)" ""

# ---------------------------------------------------------------------------
# T4  THE DISCRIMINATOR: same 4 similar candidates, but ALL from ONE session.
#     A single session enumerating its own gaps is one ask, not recurrence.
#     Measured instance: RecoveryJourney's 7- and 6-member one-session clusters.
# ---------------------------------------------------------------------------
P="$(newproj p4)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T10:00:00Z" "only-one" "Drive the settings screen and screenshot which panel rendered" "x"
add "$L" "2026-07-01T10:05:00Z" "only-one" "Drive the settings screen to screenshot the rendered panel state" "x"
add "$L" "2026-07-01T10:10:00Z" "only-one" "Screenshot the settings screen panel after driving it, rendered state" "x"
add "$L" "2026-07-01T10:15:00Z" "only-one" "Panel screenshot: drive settings screen, capture what rendered" "x"
OUT="$(runin "$P")"
chk "T4a one-session enumeration -> status ok"  "$(field "$OUT" status)"   "ok"
chk "T4b withheld, no clusters"                 "$(field "$OUT" clusters)" "0"

# ---------------------------------------------------------------------------
# T5  DRAIN: dispositioning T2's cluster silences it, and the totals say so.
#     This is the conversion side proving it terminates -- without it the oracle
#     is a new collection point with no drain, i.e. the defect one layer up.
# ---------------------------------------------------------------------------
P="$(newproj p5)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T10:00:00Z" "sess-a" "Drive the settings screen and screenshot which panel rendered" "x"
add "$L" "2026-07-08T10:00:00Z" "sess-b" "Drive the settings screen to screenshot the rendered panel state" "x"
add "$L" "2026-07-15T10:00:00Z" "sess-c" "Screenshot the settings screen panel after driving it, rendered state" "x"
add "$L" "2026-07-22T10:00:00Z" "sess-d" "Panel screenshot: drive settings screen, capture what rendered" "x"
OUT="$(runin "$P")"
chk "T5a pre-drain -> signal"                   "$(field "$OUT" status)"   "signal"
KEY="$(printf '%s' "$OUT" | "$_py" -c 'import json,sys; print(json.load(sys.stdin)["clusters"][0]["clusterKey"])')"
TRIAGE="$THIS_DIR/../../scripts/aria/jig-demand-triage.sh"
if [ -f "$TRIAGE" ]; then
    ( cd "$P" && bash "$TRIAGE" --cluster "$KEY" --verdict built --reason "self-test drain" >/dev/null 2>&1 )
    OUT="$(runin "$P")"
    chk "T5b post-drain -> status ok"           "$(field "$OUT" status)"    "ok"
    chk "T5c post-drain -> 4 dispositioned"     "$(field "$OUT" totals.dispositioned)" "4"
    chk "T5d post-drain -> 0 undispositioned"   "$(field "$OUT" totals.undispositioned)" "0"
else
    echo "  SKIP  T5b-d (jig-demand-triage.sh not found beside this oracle -- not silently passed)"
fi

# ---------------------------------------------------------------------------
# T6  BACKWARD COMPAT: an in-record `triage` object counts as dispositioned.
#     One project drained 24 candidates this way before support existed; reading
#     the shape is why that work is not orphaned.
# ---------------------------------------------------------------------------
P="$(newproj p6)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T10:00:00Z" "sess-a" "Drive the settings screen and screenshot which panel rendered" "x"
add "$L" "2026-07-08T10:00:00Z" "sess-b" "Drive the settings screen to screenshot the rendered panel state" "x"
printf '{"schemaVersion":"1","ts":"2026-07-15T10:00:00Z","sessionId":"sess-c","category":"other","question":"Screenshot the settings screen panel after driving it, rendered state","capability":"x","triage":{"by":"someone","verdict":"built","reason":"legacy hand-drain"},"aria_could_answer":true,"surface_present":false}\n' >> "$L"
printf '{"schemaVersion":"1","ts":"2026-07-22T10:00:00Z","sessionId":"sess-d","category":"other","question":"Panel screenshot: drive settings screen, capture what rendered","capability":"x","triage":{"by":"someone","verdict":"built","reason":"legacy hand-drain"},"aria_could_answer":true,"surface_present":false}\n' >> "$L"
OUT="$(runin "$P")"
chk "T6a legacy triage counted as dispositioned" "$(field "$OUT" totals.dispositioned)" "2"
chk "T6b remaining 2 below minCandidates -> ok"  "$(field "$OUT" status)" "ok"

# ---------------------------------------------------------------------------
# T7  NULL sessionId falls back to the ts DATE as the occasion key -- and four
#     candidates on FOUR days still escalate, while four on ONE day do not.
# ---------------------------------------------------------------------------
P="$(newproj p7)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T10:00:00Z" "-" "Drive the settings screen and screenshot which panel rendered" "x"
add "$L" "2026-07-02T10:00:00Z" "-" "Drive the settings screen to screenshot the rendered panel state" "x"
add "$L" "2026-07-03T10:00:00Z" "-" "Screenshot the settings screen panel after driving it, rendered state" "x"
OUT="$(runin "$P")"
chk "T7a null ids, 3 distinct DAYS -> signal"    "$(field "$OUT" status)" "signal"
P="$(newproj p7b)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T10:00:00Z" "-" "Drive the settings screen and screenshot which panel rendered" "x"
add "$L" "2026-07-01T11:00:00Z" "-" "Drive the settings screen to screenshot the rendered panel state" "x"
add "$L" "2026-07-01T12:00:00Z" "-" "Screenshot the settings screen panel after driving it, rendered state" "x"
OUT="$(runin "$P")"
chk "T7b null ids, ONE day -> ok (conservative)" "$(field "$OUT" status)" "ok"

# ---------------------------------------------------------------------------
# T8  Malformed lines are skipped, not fatal, and do not inflate the totals.
# ---------------------------------------------------------------------------
P="$(newproj p8)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
printf 'not json at all\n' >> "$L"
printf '{"unterminated": \n' >> "$L"
add "$L" "2026-07-01T10:00:00Z" "s1" "Only well-formed record here" "x"
OUT="$(runin "$P")"
chk "T8a malformed skipped -> 1 candidate"       "$(field "$OUT" totals.candidates)" "1"
chk "T8b malformed -> status ok, no crash"       "$(field "$OUT" status)" "ok"

# ---------------------------------------------------------------------------
# T9  THE REJECTED DESIGN, pinned in executable form. The brief's leading
#     remedy was capability-KEYED dedup: group records whose `capability` string
#     matches. Measured over 223 real records in 12 projects, that yields THREE
#     duplicate groups (1.3%) -- free-text asks never repeat verbatim. Here the
#     same capability is asked on three occasions in three different
#     vocabularies, exactly as the real corpus does it (all 14 members of
#     quiqpic's screenshot cluster carry distinct capability strings).
#     An exact-key implementation passes every other cell in this file and
#     reds ONLY here.
#
#     THE DISTRACTORS ARE LOAD-BEARING and were added after the first version of
#     this cell failed against the SHIPPED code. IDF weighting is corpus-size
#     dependent: in a 3-record corpus every shared term appears in all 3
#     documents, so its weight collapses to log(4/3.5)=0.13 and nothing can
#     reach threshold. The fixture was wrong, not the instrument -- but a cell
#     whose fixture cannot exhibit the property it names would have been
#     "fixed" by loosening the real detector, which measurement showed
#     over-merges a 51-record corpus into an 18-member blob. The corpus-size
#     floor is declared in oracle.json known_gaps instead of tuned away.
# ---------------------------------------------------------------------------
P="$(newproj p9)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T10:00:00Z" "s1" "Verify the settings screen renders the right panel after driving it" "introspect:ui-snapshot"
add "$L" "2026-07-09T10:00:00Z" "s2" "Screenshot the settings screen to confirm the panel rendered correctly"  "state-dump:screenshot"
add "$L" "2026-07-19T10:00:00Z" "s3" "Drive settings, screenshot the panel, confirm what rendered"             "ui-driver jig"
add "$L" "2026-07-20T10:00:00Z" "d1" "Measure audio-video sync drift in a finished recording"                  "probe:avsync"
add "$L" "2026-07-21T10:00:00Z" "d2" "Confirm the exported archive uses fragmented container atoms"            "probe:mp4"
add "$L" "2026-07-22T10:00:00Z" "d3" "Read database migration history from the running server"                 "state-dump:migrations"
add "$L" "2026-07-23T10:00:00Z" "d4" "Fetch a one-time passcode from the mail inbox"                           "companion:mail"
add "$L" "2026-07-24T10:00:00Z" "d5" "Enumerate which localisation strings lack translations"                  "oracle:i18n"
add "$L" "2026-07-25T10:00:00Z" "d6" "Reset the sandbox environment between benchmark iterations"              "environment-resetter"
add "$L" "2026-07-26T10:00:00Z" "d7" "Assert the retry backoff schedule matches the documented curve"          "probe:backoff"
add "$L" "2026-07-27T10:00:00Z" "d8" "Report which feature flags are enabled in the deployed build"            "state-dump:flags"
OUT="$(runin "$P")"
chk "T9a same ask, THREE vocabularies -> signal" "$(field "$OUT" status)"   "signal"
chk "T9b exactly ONE cluster (8 distractors withheld)" "$(field "$OUT" clusters)" "1"

# ---------------------------------------------------------------------------
# T10 sessionId IS consulted, not merely correlated with the date. Three
#     distinct sessions, all on the SAME calendar day -> must still escalate.
#     Added after mutation M7 (sessionId ignored, dates only) left T4 green:
#     T4's four records share one session AND one date, so it could not tell the
#     two keys apart. A cell that cannot distinguish the property from its
#     neighbour endorses the defect under a true-sounding name.
# ---------------------------------------------------------------------------
P="$(newproj p10)"; L="$P/.claude/session-state/aria-probe-candidates.jsonl"; mklog "$L"
add "$L" "2026-07-01T09:00:00Z" "alpha" "Drive the settings screen and screenshot which panel rendered" "x"
add "$L" "2026-07-01T13:00:00Z" "beta"  "Drive the settings screen to screenshot the rendered panel state" "x"
add "$L" "2026-07-01T18:00:00Z" "gamma" "Screenshot the settings screen panel after driving it, rendered state" "x"
OUT="$(runin "$P")"
chk "T10a 3 sessions, ONE day -> signal" "$(field "$OUT" status)" "signal"

echo
echo "jig-demand self-test: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
