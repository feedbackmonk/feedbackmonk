#!/bin/bash
# pending-ideas oracle self-test (Unix) -- INJECT-08
# Drives run.sh across the four contract cases in a temp sandbox:
#   1. no deferred/ dir          -> empty briefing, no_data:false (graceful absence)
#   2. one PROPOSED item          -> count 1, briefing lists it
#   3. one TRIAGED item           -> excluded, count 0, empty briefing
#   4. a DEFER-*.md w/o status    -> no_data:true (NO-DATA honesty, never silent none)
set -u

ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
RUN="$ORACLE_DIR/run.sh"
FAILS=0
pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1" >&2; FAILS=$((FAILS + 1)); }

SANDBOX="$(mktemp -d 2>/dev/null || echo "/tmp/pending-ideas-validate-$$")"
mkdir -p "$SANDBOX"
trap 'rm -rf "$SANDBOX"' EXIT

run_in() { ( cd "$1" && bash "$RUN" 2>/dev/null ); }
# field <json> <key> -> raw value (numbers/booleans/strings without quotes)
jget() { printf '%s' "$1" | grep -oE "\"$2\"[[:space:]]*:[[:space:]]*(\"[^\"]*\"|[a-z0-9]+)" | head -1 | sed -E "s/\"$2\"[[:space:]]*:[[:space:]]*//; s/^\"//; s/\"$//"; }

# --- Case 1: no deferred dir -------------------------------------------------
C1="$SANDBOX/c1"; mkdir -p "$C1"
OUT="$(run_in "$C1")"
[ "$(jget "$OUT" pending_count)" = "0" ] && [ "$(jget "$OUT" no_data)" = "false" ] \
  && printf '%s' "$OUT" | grep -qE '"briefing"[[:space:]]*:[[:space:]]*""' \
  && pass "case1 absent -> empty briefing, no_data:false" \
  || fail "case1 absent (got: $OUT)"

# --- Case 2: one PROPOSED item ----------------------------------------------
C2="$SANDBOX/c2"; mkdir -p "$C2/docs/planning/deferred"
cat > "$C2/docs/planning/deferred/DEFER-001_sample-idea.md" <<'EOF'
---
id: DEFER-001
title: Sample injected idea
status: PROPOSED
origin: inject
source-project: GitCellar
injected-at: 2026-07-08T21:00:00Z
autonomy-hint: collaborative
suggested-entry-point: spec
scope-estimate: needs-scoping
content-hash: abc123
---

# DEFER-001: Sample injected idea

## Idea
A sample.
EOF
OUT="$(run_in "$C2")"
[ "$(jget "$OUT" pending_count)" = "1" ] \
  && printf '%s' "$OUT" | grep -q 'DEFER-001' \
  && printf '%s' "$OUT" | grep -q 'GitCellar' \
  && ! printf '%s' "$OUT" | grep -qE '"briefing"[[:space:]]*:[[:space:]]*""' \
  && pass "case2 one PROPOSED -> count 1, briefing lists it" \
  || fail "case2 PROPOSED (got: $OUT)"

# --- Case 3: one TRIAGED item (excluded) ------------------------------------
C3="$SANDBOX/c3"; mkdir -p "$C3/docs/planning/deferred"
cat > "$C3/docs/planning/deferred/DEFER-002_triaged.md" <<'EOF'
---
id: DEFER-002
title: Already triaged
status: TRIAGED
origin: defer-local
---

# DEFER-002: Already triaged
EOF
OUT="$(run_in "$C3")"
[ "$(jget "$OUT" pending_count)" = "0" ] \
  && printf '%s' "$OUT" | grep -qE '"briefing"[[:space:]]*:[[:space:]]*""' \
  && pass "case3 TRIAGED -> excluded, empty briefing" \
  || fail "case3 TRIAGED (got: $OUT)"

# --- Case 4: DEFER file with no parseable status -> NO-DATA ------------------
C4="$SANDBOX/c4"; mkdir -p "$C4/docs/planning/deferred"
cat > "$C4/docs/planning/deferred/DEFER-003_broken.md" <<'EOF'
# DEFER-003: No status anywhere

Just prose, no front-matter status and no legacy Status line.
EOF
OUT="$(run_in "$C4")"
[ "$(jget "$OUT" no_data)" = "true" ] \
  && ! printf '%s' "$OUT" | grep -qE '"briefing"[[:space:]]*:[[:space:]]*""' \
  && pass "case4 unparseable -> no_data:true, NO-DATA briefing" \
  || fail "case4 NO-DATA (got: $OUT)"

# --- Case 5: PARKED surfaces (DEC-281 / DEFER-109) --------------------------
# The gap this closes is INVERTED: surfacing only PROPOSED made a brief that is
# genuinely still open but triaged invisible at every session start, forever.
C5="$SANDBOX/c5"; mkdir -p "$C5/docs/planning/deferred"
cat > "$C5/docs/planning/deferred/DEFER-003_parked-with-condition.md" <<'EOF'
---
id: DEFER-003
title: AOR-drive re-arm at 0 of 10
status: PARKED
re-arm: R2 reaches 10 tracked sessions
---

# DEFER-003: parked, open, not being built
EOF
OUT="$(run_in "$C5")"
[ "$(jget "$OUT" parked_count)" = "1" ] \
  && [ "$(jget "$OUT" pending_count)" = "0" ] \
  && printf '%s' "$OUT" | grep -q 'DEFER-003' \
  && printf '%s' "$OUT" | grep -q 're-arm: R2 reaches 10 tracked sessions' \
  && ! printf '%s' "$OUT" | grep -qE '"briefing"[[:space:]]*:[[:space:]]*""' \
  && pass "case5 PARKED -> surfaced at session start with its re-arm condition" \
  || fail "case5 PARKED (got: $OUT)"

# 5b. PARKED does NOT join pending_count. INJECT-08's frozen clause says `items`
#     is PROPOSED-only and un-triaged; a parked item is by definition triaged, so
#     folding it in would silently redefine a frozen field rather than add one.
printf '%s' "$OUT" | grep -qE '"items"[[:space:]]*:[[:space:]]*\[\]' \
  && pass "case5b PARKED stays out of the frozen items[] / pending_count" \
  || fail "case5b PARKED leaked into the frozen PROPOSED contract (got: $OUT)"

# 5c. A parked brief with NO declared re-arm is still surfaced, and said to have
#     none. Presence is checked; the condition is never evaluated (OVALID-02).
cat > "$C5/docs/planning/deferred/DEFER-017_parked-no-condition.md" <<'EOF'
---
id: DEFER-017
title: TUTOR Arc 3 centrepiece
status: PARKED
---
EOF
OUT="$(run_in "$C5")"
[ "$(jget "$OUT" parked_count)" = "2" ] \
  && printf '%s' "$OUT" | grep -q 'no re-arm condition declared' \
  && pass "case5c parked-without-a-condition is surfaced AND named as such" \
  || fail "case5c parked without re-arm (got: $OUT)"

# --- Case 6: THE RED-FIRST HALF -- a finished brief must stay hidden ---------
# This is the half that matters and the one the acceptance criterion singles
# out. A PARKED surface that also lights up for finished briefs is WORSE than no
# surface: it trains agents to ignore the lane (§ QUIESCE names this failure
# explicitly, and the corpus's own history is the evidence -- 27 briefs sat at
# TRIAGED and 21 had shipped long ago).
#
# Every spelling of "done" the corpus actually uses is seeded here, because the
# corpus has EIGHT of them and a cell that tested only RESOLVED would be green
# against a check that matched on "not PROPOSED".
C6="$SANDBOX/c6"; mkdir -p "$C6/docs/planning/deferred"
i=0
for st in RESOLVED IMPLEMENTED COMPLETED CLOSED APPLIED FOLDED PHASE-1-IMPLEMENTED RESOLVED-DECLINED TRIAGED DISMISSED IN-PROGRESS; do
    i=$((i + 1))
    printf -- '---\nid: DEFER-%03d\ntitle: finished (%s)\nstatus: %s\n---\n' "$i" "$st" "$st" \
        > "$C6/docs/planning/deferred/DEFER-$(printf '%03d' "$i")_done.md"
done
OUT="$(run_in "$C6")"
[ "$(jget "$OUT" parked_count)" = "0" ] \
  && [ "$(jget "$OUT" pending_count)" = "0" ] \
  && [ "$(jget "$OUT" no_data)" = "false" ] \
  && printf '%s' "$OUT" | grep -qE '"briefing"[[:space:]]*:[[:space:]]*""' \
  && pass "case6 RED-FIRST: 11 finished/triaged spellings stay hidden, briefing empty" \
  || fail "case6 finished briefs leaked into the briefing (got: $OUT)"

# 6b. And TRIAGED specifically stays hidden. PARKED is a NEW label meaning
#     "triaged, open, deliberately unbuilt" -- it is not a rename of TRIAGED, and
#     nothing in this change relabels a single brief. If a future edit made
#     TRIAGED surface, every one of the 21 long-shipped briefs would light up.
printf -- '---\nid: DEFER-099\ntitle: triaged only\nstatus: TRIAGED\n---\n' \
    > "$C6/docs/planning/deferred/DEFER-099_triaged.md"
OUT="$(run_in "$C6")"
[ "$(jget "$OUT" parked_count)" = "0" ] \
  && printf '%s' "$OUT" | grep -qE '"briefing"[[:space:]]*:[[:space:]]*""' \
  && pass "case6b TRIAGED is not PARKED -- still hidden" \
  || fail "case6b TRIAGED leaked into the parked surface (got: $OUT)"

# --- Case 7: a parked item survives alongside pending + malformed ------------
# Composition, and the ordering rule: the NO-DATA note TRAILS what was surfaced,
# so it can never displace it.
C7="$SANDBOX/c7"; mkdir -p "$C7/docs/planning/deferred"
printf -- '---\nid: DEFER-050\ntitle: fresh idea\nstatus: PROPOSED\n---\n' > "$C7/docs/planning/deferred/DEFER-050_fresh.md"
printf -- '---\nid: DEFER-051\ntitle: parked thing\nstatus: PARKED\nre-arm: a second consumer appears\n---\n' > "$C7/docs/planning/deferred/DEFER-051_parked.md"
printf -- '# DEFER-052: no status anywhere\n' > "$C7/docs/planning/deferred/DEFER-052_broken.md"
OUT="$(run_in "$C7")"
[ "$(jget "$OUT" pending_count)" = "1" ] && [ "$(jget "$OUT" parked_count)" = "1" ] \
  && [ "$(jget "$OUT" no_data)" = "true" ] \
  && printf '%s' "$OUT" | grep -q 'DEFER-050' \
  && printf '%s' "$OUT" | grep -q 'DEFER-051' \
  && printf '%s' "$OUT" | grep -q 'unparseable' \
  && pass "case7 pending + parked + NO-DATA compose, none displacing another" \
  || fail "case7 composition (got: $OUT)"

# 7b. The briefing must stay double-quote-free -- the session-start fan-out
#     extracts it with a "[^\"]*" regex, so one stray quote truncates the line.
#     A parked title or re-arm condition is new attacker-controlled text on that
#     path, so this is checked where the new text is, not just in principle.
printf -- '---\nid: DEFER-053\ntitle: a "quoted" title\nstatus: PARKED\nre-arm: when "x" happens\n---\n' \
    > "$C7/docs/planning/deferred/DEFER-053_quoted.md"
OUT="$(run_in "$C7")"
BR="$(printf '%s' "$OUT" | sed -E 's/.*"briefing"[[:space:]]*:[[:space:]]*"//; s/"\}$//')"
if printf '%s' "$BR" | grep -q '\\"'; then
    fail "case7b a quote reached the briefing string (would truncate the fan-out line)"
else
    pass "case7b parked titles and re-arm text stay double-quote-free"
fi

# --- Case 8: THE RED-FIRST CELL for DEC-317 (DEFER-151) ---------------------
# The status WORD is the first token; a rationale after it is house style, not
# a different status. Before this fix the whole line was uppercased with ALL
# whitespace stripped and compared exactly, so `status: PARKED 2026-08-06 -- the
# fix is SHIPPED (...); RE-ARM -- ...` collapsed to one ~400-char token that
# matched nothing and the brief vanished in silence. Measured live on
# DEFER-146/DEFER-147; removing the rationale and changing nothing else surfaced
# them.
#
# This cell REDDENS against the pre-fix run.sh -- that is the point of it.
C8="$SANDBOX/c8"; mkdir -p "$C8/docs/planning/deferred"
cat > "$C8/docs/planning/deferred/DEFER-146_parked-with-rationale.md" <<'EOF'
---
id: DEFER-146
title: parked with a rationale on the status line
status: PARKED 2026-08-06 -- the behavior change is SHIPPED (ef0c93fb); what remains is the spec reconciliation. RE-ARM -- next session finding docs/specs clean
---

# DEFER-146: parked, open, rationale on the status line
EOF
# ...and a DONE word with a rationale must STAY hidden. Without this control a
# "match on the leading token" fix and a "match on any prefix" fix are
# indistinguishable, and the second one lights up 105 finished briefs.
cat > "$C8/docs/planning/deferred/DEFER-147_resolved-with-rationale.md" <<'EOF'
---
id: DEFER-147
title: resolved with a rationale on the status line
status: RESOLVED 2026-08-06 (DEC-999) -- shipped, and here is a long rationale that mentions PROPOSED and PARKED in passing
---

# DEFER-147: done
EOF
OUT="$(run_in "$C8")"
[ "$(jget "$OUT" parked_count)" = "1" ] \
  && [ "$(jget "$OUT" pending_count)" = "0" ] \
  && [ "$(jget "$OUT" unknown_status_count)" = "0" ] \
  && printf '%s' "$OUT" | grep -q 'DEFER-146' \
  && ! printf '%s' "$OUT" | grep -q 'DEFER-147' \
  && pass "case8 the status WORD is parsed out of the line (rationale after it does not hide the brief)" \
  || fail "case8 status-word parse (got: $OUT)"

# 8b. The re-arm condition written INTO the status rationale is reported.
#     Presence only, never evaluated (OVALID-02). This exists so the oracle
#     stops SAYING 'no re-arm condition declared' about a brief that declared
#     one -- a wrong statement, not a missing one.
printf '%s' "$OUT" | grep -q 're-arm: next session finding docs/specs clean' \
  && pass "case8b re-arm declared inside the status rationale is surfaced" \
  || fail "case8b re-arm from status rationale (got: $OUT)"

# 8c. Hyphenated words survive the token cut whole. RESOLVED-DECLINED must not
#     become RESOLVED (harmless here) NOR be reported as unrecognized (noise).
C8C="$SANDBOX/c8c"; mkdir -p "$C8C/docs/planning/deferred"
cat > "$C8C/docs/planning/deferred/DEFER-149_hyphen.md" <<'EOF'
---
id: DEFER-149
title: hyphenated done-word
status: RESOLVED-DECLINED 2026-08-06 -- retro-demoted to a ledger line
---

# DEFER-149: declined
EOF
OUT="$(run_in "$C8C")"
[ "$(jget "$OUT" unknown_status_count)" = "0" ] \
  && [ "$(jget "$OUT" pending_count)" = "0" ] \
  && printf '%s' "$OUT" | grep -qE '"briefing"[[:space:]]*:[[:space:]]*""' \
  && pass "case8c hyphenated done-words survive the token cut and stay silent" \
  || fail "case8c hyphenated done-word (got: $OUT)"

# --- Case 9: a word outside the set is REPORTED, not dropped (DEC-317) ------
# The trigger instance: a live cross-project injection carrying
# `**Status**: OPEN` -- the framework's OWN word for an open DISCOVERIES.md
# entry -- was invisible here AND unaudited by `dec-alloc-guard --check-defer`.
# A file this oracle silently drops is indistinguishable from a file that is
# not there (ORACLE-COST-09/10, DEC-292, one door over).
C9="$SANDBOX/c9"; mkdir -p "$C9/docs/planning/deferred"
cat > "$C9/docs/planning/deferred/DEFER-table-20260806_stale-grant.md" <<'EOF'
# A grant request outlives its subject

**Status**: OPEN
**Injected-From**: Table
EOF
OUT="$(run_in "$C9")"
[ "$(jget "$OUT" unknown_status_count)" = "1" ] \
  && printf '%s' "$OUT" | grep -q 'DEFER-table-20260806_stale-grant' \
  && printf '%s' "$OUT" | grep -q 'unrecognized status word' \
  && pass "case9 an unrecognized status word is reported rather than silently dropped" \
  || fail "case9 unknown-status reporting (got: $OUT)"

# 9b. AND IT IS NOT PROMOTED. The filter is correct and is NOT widened here:
#     INJECT-08 freezes items[] as PROPOSED-only and DEC-281 deliberately kept
#     PARKED out of pending_count. A fix that made the filter permissive would
#     pass 9 and is exactly what the brief said must not survive.
[ "$(jget "$OUT" pending_count)" = "0" ] \
  && [ "$(jget "$OUT" parked_count)" = "0" ] \
  && ! printf '%s' "$OUT" | grep -qE '"items"[[:space:]]*:[[:space:]]*\[\{' \
  && pass "case9b an unrecognized word does NOT join pending_count/items[]" \
  || fail "case9b unknown must not be promoted (got: $OUT)"

# 9c. Anti-noise control: the 105 RESOLVED / 28 IMPLEMENTED briefs in the real
#     corpus must NOT reach this clause. A signal that fires 130 times is
#     ignored, which is how a real unknown word hides.
[ "$(jget "$(run_in "$C6")" unknown_status_count)" = "0" ] \
  && pass "case9c every spelling of done stays out of the unrecognized clause" \
  || fail "case9c done-words leaked into the unrecognized clause (got: $(run_in "$C6"))"

# --- Case 10: no empty string in the frozen items[].id (DEC-317) ------------
# A slug-form brief with no front-matter `id:` used to emit "" into INJECT-08's
# frozen id field -- an empty required key a consumer joining on id gets
# silently, which is worse than exclusion -- and rendered a double space in the
# briefing. The PARKED branch always had the basename fallback; PROPOSED did not.
C10="$SANDBOX/c10"; mkdir -p "$C10/docs/planning/deferred"
cat > "$C10/docs/planning/deferred/DEFER-slugform_no-numeric-id.md" <<'EOF'
# A slug-form brief with no front matter at all

**Status**: PROPOSED
EOF
OUT="$(run_in "$C10")"
[ "$(jget "$OUT" pending_count)" = "1" ] \
  && ! printf '%s' "$OUT" | grep -q '"id":""' \
  && printf '%s' "$OUT" | grep -q 'DEFER-slugform_no-numeric-id' \
  && ! printf '%s' "$OUT" | grep -q 'ideas:  ' \
  && ! printf '%s' "$OUT" | grep -q 'idea:  ' \
  && pass "case10 a slug-form brief gets a non-empty id and no double space" \
  || fail "case10 empty id fallback (got: $OUT)"

# --- Schema field presence (on the case-2 output) ---------------------------
OUT="$(run_in "$C2")"
for field in pending_count items parked_count parked unknown_status_count unknown_status no_data briefing; do
    printf '%s' "$OUT" | grep -q "\"$field\"" || fail "missing schema field '$field'"
done
# The new fields must be present on EVERY emission path, including the two
# early-exit emitters -- a consumer that reads parked_count would otherwise
# crash on a graceful-absence run.
for c in "$C1" "$C4" "$C6"; do
    OUT="$(run_in "$c")"
    printf '%s' "$OUT" | grep -q '"parked_count"' \
        || fail "parked_count missing from an early-exit emission ($c)"
    printf '%s' "$OUT" | grep -q '"unknown_status_count"' \
        || fail "unknown_status_count missing from an early-exit emission ($c)"
done

if [ "$FAILS" -eq 0 ]; then
    echo "PASS: pending-ideas oracle validates (18 checks + schema on every path)"
    exit 0
else
    echo "FAIL: pending-ideas oracle -- $FAILS check(s) failed" >&2
    exit 1
fi
