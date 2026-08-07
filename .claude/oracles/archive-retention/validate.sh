#!/bin/bash
# archive-retention oracle self-test (Unix)
#
# Phase 1: validate the read-only briefing path against the real archived dir.
# Phase 2: validate --gc and --gc-cheap sweep semantics in a sandbox:
#   T1. Sweep deletes dirs older than threshold (with no KEEP file).
#   T2. Sweep does NOT delete dirs younger than threshold.
#   T3. KEEP file exempts a dir from sweep regardless of age.
#   T4. Sweep is idempotent: re-running on post-sweep dir sweeps zero.
#   T5. --gc emits JSON summary with all expected fields.
#   T6. .claude/config.json archiveRetention.threshold is honored.
#   T7. --gc-cheap is silent on success and performs the sweep.
#   T8. _summary.jsonl receives one JSON line per swept dir BEFORE delete.
#   T9. --gc-cheap never starves: every invocation sweeps >=1 candidate, so a
#       backlog drains to zero across repeated session-starts (DEFER-019).
#  T10. Time-invariance: the whole sandbox phase is green under a clock shifted
#       +1 year, so no cell can rot across its own threshold (DEFER-072).
#  T11. The ULDF_FAKE_NOW seam rejects non-numeric values (falls back to the
#       real clock) -- the seam moves a delete cutoff and must not fail open.

set -e
ORACLE_DIR="$(dirname "$0")"

PASS=0
FAIL=0
SKIP=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

# DEFER-070 / DEC-247: the --gc-cheap cells (T7, T9) are members of the
# timing-windowed class even though their assertions are about sweep COUNTS --
# the wall-clock dependency is one layer down, in the SUBJECT: --gc-cheap runs
# under a 1000ms budget, and under machine load the budget expires before the
# first candidate is swept. Measured live 2026-08-04 (five reds on an unchanged
# tree, machine-quiescence noisy: 18 listeners / 10 agent sessions / 13 builds;
# the runs immediately before and after were PASS=30 FAIL=0). Such a failure is
# a true statement about a starved run, not about sweep semantics -- so it is
# DEFERRED, never counted as a FAIL. The budget is NOT widened (OVALID-05).
_TG_LIB="$ORACLE_DIR/../../scripts/lib/timing-guard.sh"
[ -f "$_TG_LIB" ] || _TG_LIB="$ORACLE_DIR/../../../claude-template/scripts/lib/timing-guard.sh"
[ -f "$_TG_LIB" ] || _TG_LIB="$HOME/.claude/scripts/lib/timing-guard.sh"
if [ -f "$_TG_LIB" ]; then
    . "$_TG_LIB"
else
    # Graceful absence: grade normally (status quo ante), never silently skip.
    timing_guard_declare() { :; }; timing_guard_deferred() { return 1; }
    timing_guard_note() { :; }; timing_guard_summary() { :; }
fi
timing_guard_declare "archive-retention --gc-cheap 1000ms sweep budget (T7/T9)"

# fail_timed: for cells whose subject runs under a wall-clock budget. Defers
# (SKIP-with-note) instead of failing when the machine was not quiet.
fail_timed() {
    if timing_guard_deferred; then
        SKIP=$((SKIP+1))
        echo "SKIP: $1" >&2
        echo "      $(timing_guard_note)" >&2
    else
        fail "$1"
    fi
}

PYBIN=""
for _candidate in python3 python; do
    if command -v "$_candidate" >/dev/null 2>&1; then
        if "$_candidate" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
            PYBIN="$_candidate"
            break
        fi
    fi
done

# =============================================================================
# Fixture clock (DEFER-072)
# =============================================================================
# Fixture ages are computed from `now` at runtime, NEVER written as calendar
# literals. The properties under test are *older than threshold* and *younger
# than threshold* -- both relative by definition; a literal date additionally
# encodes the authoring day, which is not part of the contract. Fixture
# collab-20260420-130000 was "~10 days" old when authored (2026-04-30), crossed
# the P90D default on 2026-07-19, and left this harness red for 15 days before
# an unrelated finalize tripped over it.
#
# ULDF_FAKE_NOW (epoch seconds) shifts this validator's notion of `now`; run.sh
# honors the same seam, so validator and oracle shift together. Faking one alone
# would only measure the skew between them. T10 uses it; T11 guards it.
NOW_EPOCH_FIXTURE=""
case "${ULDF_FAKE_NOW:-}" in
    ''|*[!0-9]*) ;;
    *) NOW_EPOCH_FIXTURE="$ULDF_FAKE_NOW" ;;
esac
[ -n "$NOW_EPOCH_FIXTURE" ] || NOW_EPOCH_FIXTURE=$(date -u +%s)

# collab-YYYYMMDD-HHMMSS basename for (now - $1 days).
# $2 is a fixed time-of-day used purely as a GROUP DISCRIMINATOR, so a cell can
# match its own fixtures without a date literal (T9). It is not part of the age:
# it leaves each fixture's true age within +/-1 day of the requested offset, and
# every margin here is >= 5 days, so that slack is immaterial.
_fixture_name() {
    local days_ago="$1" hms="$2" epoch ymd
    epoch=$((NOW_EPOCH_FIXTURE - days_ago * 86400))
    ymd=$(date -u -d "@$epoch" +%Y%m%d 2>/dev/null) || ymd=""
    if [ -z "$ymd" ]; then ymd=$(date -u -r "$epoch" +%Y%m%d 2>/dev/null) || ymd=""; fi
    if [ -z "$ymd" ] && [ -n "$PYBIN" ]; then
        ymd=$("$PYBIN" -c "
import sys
from datetime import datetime, timezone
print(datetime.fromtimestamp(int(sys.argv[1]), tz=timezone.utc).strftime('%Y%m%d'))
" "$epoch" 2>/dev/null) || ymd=""
    fi
    if [ -z "$ymd" ]; then
        echo "FAIL: fixture clock: cannot format epoch $epoch (no usable date/python)" >&2
        exit 1
    fi
    echo "collab-${ymd}-${hms}"
}

# =============================================================================
# Phase 1 — briefing path against the real archived dir (best-effort)
# =============================================================================

# T10's inner run skips Phase 1: it reads the REAL repo (the dominant cost --
# ~53s over 21 archived dirs, measured 2026-08-04) and contains no fixtures to
# age, so it has nothing to say about time-invariance.
if [ -n "${ULDF_AR_INNER:-}" ]; then
    echo "SKIP: Phase 1 (inner time-invariance run -- no fixtures on this path)"
else

OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if [ -n "$PYBIN" ]; then
    if ! echo "$OUTPUT" | "$PYBIN" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: briefing output is not valid JSON" >&2
        echo "Output: $OUTPUT" >&2
        exit 1
    fi
fi

for field in count dirs threshold thresholdSource summary; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        fail "briefing: missing schema field '$field'"
    fi
done

COUNT=$(echo "$OUTPUT" | grep -oE '"count"[[:space:]]*:[[:space:]]*[0-9]+' | grep -oE '[0-9]+$' | head -1)
if [ -z "$COUNT" ]; then
    fail "briefing: 'count' is not a non-negative integer"
else
    pass "briefing: count=$COUNT"
fi

fi

# =============================================================================
# Phase 2 — sweep semantics in a sandbox
# =============================================================================

if [ -z "$PYBIN" ]; then
    echo "SKIP: Phase 2 (--gc tests) requires python for fixture build"
    if [ "$FAIL" -gt 0 ]; then exit 1; fi
    exit 0
fi

SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'retention')"
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

mkdir -p "$SANDBOX/.claude/collaboration/archived"
mkdir -p "$SANDBOX/.claude/oracles/archive-retention"

cp "$ORACLE_DIR/run.sh" "$SANDBOX/.claude/oracles/archive-retention/run.sh"
cp "$ORACLE_DIR/oracle.json" "$SANDBOX/.claude/oracles/archive-retention/oracle.json"

# ---- Build fixture archive dirs --------------------------------------------
# 4 dirs, every date computed from `now` (see "Fixture clock" above):
#   FX_AGED    -- now-200d, no KEEP  -> sweepable at P90D           (T1)
#   FX_KEPT    -- now-180d, has KEEP -> kept regardless of age      (T3)
#   FX_RECENT  -- now-10d            -> too-young at P90D           (T2)
#                                    -> AND sweepable at the P5D of (T6)
#   FX_BOGUS   -- unparseable name   -> never swept (failure-open)
# Offsets are chosen for margin, not precision: the nearest boundary is 5 days
# away, so the +/-1 day of time-of-day slack cannot flip any verdict.
ARCHIVE="$SANDBOX/.claude/collaboration/archived"

FX_AGED=$(_fixture_name 200 100000)
FX_KEPT=$(_fixture_name 180 120000)
FX_RECENT=$(_fixture_name 10 130000)
FX_BOGUS="collab-bogus-name"
FX_CHEAP=$(_fixture_name 190 110000)   # built later, for T7
BACKLOG_HMS=190000                     # T9's group discriminator; unique to T9

for d in "$FX_AGED" "$FX_KEPT" "$FX_RECENT" "$FX_BOGUS"; do
    mkdir -p "$ARCHIVE/$d/workers/CLAUDE-A"
    mkdir -p "$ARCHIVE/$d/tasks"
    mkdir -p "$ARCHIVE/$d/channels"
    mkdir -p "$ARCHIVE/$d/file-tracking"
    echo "# Test session $d" > "$ARCHIVE/$d/GUIDE.md"
    echo "## Worker A notes" >> "$ARCHIVE/$d/GUIDE.md"
    echo "fixture-content" > "$ARCHIVE/$d/workers/CLAUDE-A/status.md"
    touch "$ARCHIVE/$d/tasks/task-1.md"
    touch "$ARCHIVE/$d/tasks/task-2.md"
done

# Pin the second one
echo "Keep because: T3 fixture" > "$ARCHIVE/$FX_KEPT/KEEP"

# ---- T1+T2+T3+T5+T8: run --gc and inspect --------------------------------
GC_OUT=$(cd "$SANDBOX" && bash .claude/oracles/archive-retention/run.sh --gc 2>&1)
echo "[--gc summary]: $GC_OUT"

# T5: summary shape
for f in swept before after threshold thresholdSource summarized; do
    if ! echo "$GC_OUT" | grep -q "\"$f\""; then
        fail "T5: --gc summary missing field '$f' (got: $GC_OUT)"
    fi
done

echo "$GC_OUT" | grep -q '"swept":1'      && pass "T1: --gc swept=1 (only $FX_AGED)"               || fail "T1: --gc swept != 1 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"before":3'     && pass "T5: --gc before=3 (excludes bogus-name)"        || fail "T5: --gc before != 3 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"after":2'      && pass "T5: --gc after=2"                                || fail "T5: --gc after != 2 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"summarized":1' && pass "T5: --gc summarized=1"                            || fail "T5: --gc summarized != 1 (got: $GC_OUT)"

# T2: the now-10d dir survives a P90D sweep
[ -d "$ARCHIVE/$FX_RECENT" ] && pass "T2: recent dir survived sweep" || fail "T2: recent dir was deleted"

# T3: the KEEP-pinned dir survives
[ -d "$ARCHIVE/$FX_KEPT" ] && pass "T3: KEEP-pinned dir survived sweep" || fail "T3: KEEP-pinned dir was deleted"

# T1: the now-200d dir is deleted
[ ! -d "$ARCHIVE/$FX_AGED" ] && pass "T1: aged dir deleted" || fail "T1: aged dir was NOT deleted"

# Bogus name preserved
[ -d "$ARCHIVE/$FX_BOGUS" ] && pass "Defensive: bogus-name dir preserved (failure-open)" || fail "Defensive: bogus-name dir was deleted"

# T8: _summary.jsonl received the entry
SUMMARY="$ARCHIVE/_summary.jsonl"
if [ -f "$SUMMARY" ]; then
    LINES=$(wc -l < "$SUMMARY" | tr -d ' ')
    if [ "$LINES" = "1" ]; then
        pass "T8: _summary.jsonl has exactly 1 line"
    else
        fail "T8: _summary.jsonl has $LINES lines, expected 1"
    fi

    SUMMARY_LINE=$(cat "$SUMMARY")
    echo "$SUMMARY_LINE" | "$PYBIN" -c "import sys, json; json.loads(sys.stdin.read())" 2>/dev/null \
        && pass "T8: _summary.jsonl line is valid JSON" \
        || fail "T8: _summary.jsonl line is not valid JSON: $SUMMARY_LINE"

    for f in sessionId sweptAt createdAt ageDays sizeBytes workerCount taskCount criticVerdict hasOverrideVeto guideHeadline; do
        if ! echo "$SUMMARY_LINE" | grep -q "\"$f\""; then
            fail "T8: _summary.jsonl missing field '$f'"
        fi
    done
    echo "$SUMMARY_LINE" | grep -q "\"sessionId\":\"$FX_AGED\"" && pass "T8: _summary.jsonl sessionId matches" || fail "T8: _summary.jsonl sessionId mismatch"
    echo "$SUMMARY_LINE" | grep -q '"workerCount":1' && pass "T8: _summary.jsonl workerCount=1" || fail "T8: _summary.jsonl workerCount mismatch"
    echo "$SUMMARY_LINE" | grep -q '"taskCount":2' && pass "T8: _summary.jsonl taskCount=2" || fail "T8: _summary.jsonl taskCount mismatch"
else
    fail "T8: _summary.jsonl was not created"
fi

# ---- T4: idempotence ---------------------------------------------------------
GC_OUT2=$(cd "$SANDBOX" && bash .claude/oracles/archive-retention/run.sh --gc 2>&1)
echo "[second --gc]: $GC_OUT2"
echo "$GC_OUT2" | grep -q '"swept":0' && pass "T4: idempotence (second --gc swept=0)" || fail "T4: idempotence violated (got: $GC_OUT2)"

# ---- T6: config.json threshold honored ---------------------------------------
# Set threshold to 5 days; the now-10d fixture ($FX_RECENT) becomes sweepable.
mkdir -p "$SANDBOX/.claude"
echo '{"archiveRetention":{"threshold":5}}' > "$SANDBOX/.claude/config.json"

GC_OUT3=$(cd "$SANDBOX" && bash .claude/oracles/archive-retention/run.sh --gc 2>&1)
echo "[--gc with config 5d]: $GC_OUT3"
echo "$GC_OUT3" | grep -q '"swept":1' && pass "T6: 5d threshold sweeps the recent dir" || fail "T6: 5d threshold did not sweep (got: $GC_OUT3)"
echo "$GC_OUT3" | grep -q '"thresholdSource":"config"' && pass "T6: thresholdSource=config" || fail "T6: thresholdSource != config (got: $GC_OUT3)"
echo "$GC_OUT3" | grep -q '"threshold":"P5D"' && pass "T6: threshold=P5D" || fail "T6: threshold != P5D (got: $GC_OUT3)"

# Verify summary file now has 2 lines
LINES2=$(wc -l < "$SUMMARY" | tr -d ' ')
if [ "$LINES2" = "2" ]; then
    pass "T8: _summary.jsonl has 2 lines after second sweep"
else
    fail "T8: _summary.jsonl has $LINES2 lines after second sweep, expected 2"
fi

# ---- T7: --gc-cheap silent on success and performs sweep --------------------
# Rebuild a fixture: aged dir, default 90d threshold (drop config), expect sweep.
rm -f "$SANDBOX/.claude/config.json"
mkdir -p "$ARCHIVE/$FX_CHEAP/workers"
mkdir -p "$ARCHIVE/$FX_CHEAP/tasks"
echo "# fixture" > "$ARCHIVE/$FX_CHEAP/GUIDE.md"

CHEAP_OUT=$(cd "$SANDBOX" && bash .claude/oracles/archive-retention/run.sh --gc-cheap 2>&1)
if [ -z "$(echo "$CHEAP_OUT" | tr -d '[:space:]')" ]; then
    pass "T7: --gc-cheap silent on success"
else
    fail "T7: --gc-cheap emitted output (should be silent): $CHEAP_OUT"
fi

[ ! -d "$ARCHIVE/$FX_CHEAP" ] && pass "T7: --gc-cheap performed the sweep" || fail_timed "T7: --gc-cheap did not sweep aged dir"

# Final summary file should have 3 lines now
LINES3=$(wc -l < "$SUMMARY" | tr -d ' ')
if [ "$LINES3" = "3" ]; then
    pass "T7: _summary.jsonl reached 3 lines (cumulative across 3 sweeps)"
else
    fail_timed "T7: _summary.jsonl has $LINES3 lines after --gc-cheap, expected 3"
fi

# ---- T9: forward progress + backlog convergence (DEFER-019 regression) -------
# The budget must never starve the sweep: each --gc-cheap invocation sweeps at
# least one candidate, so a backlog drains to zero across repeated session-starts.
# Regression guard for the class where setup cost consumed the whole budget
# before the first candidate was examined and --gc-cheap swept nothing, forever.
# The backlog is identified by its own time-of-day discriminator ($BACKLOG_HMS),
# not by a date prefix: the old 'collab-2026010?-120000' glob was itself a
# calendar literal, so relative dates alone would have left this cell counting
# zero dirs and passing vacuously.
BACKLOG=5
for i in 1 2 3 4 5; do
    d="$ARCHIVE/$(_fixture_name $((300 + i)) "$BACKLOG_HMS")"
    mkdir -p "$d/workers" "$d/tasks"
    echo "# fixture" > "$d/GUIDE.md"
done
LINES_BEFORE=$(wc -l < "$SUMMARY" | tr -d ' ')

# Bounded loop: must converge well inside BACKLOG invocations even at 1/pass.
REMAIN=$BACKLOG
PASSES=0
PROGRESS_STALLED=""
while [ "$PASSES" -lt "$BACKLOG" ]; do
    PREV=$REMAIN
    (cd "$SANDBOX" && bash .claude/oracles/archive-retention/run.sh --gc-cheap >/dev/null 2>&1)
    PASSES=$((PASSES + 1))
    REMAIN=$(find "$ARCHIVE" -mindepth 1 -maxdepth 1 -type d -name "collab-*-$BACKLOG_HMS" 2>/dev/null | wc -l | tr -d ' ')
    [ "$REMAIN" = "0" ] && break
    if [ "$REMAIN" = "$PREV" ]; then PROGRESS_STALLED=1; break; fi
done

if [ -z "$PROGRESS_STALLED" ]; then
    pass "T9: --gc-cheap makes forward progress every invocation (no budget starvation)"
else
    fail_timed "T9: --gc-cheap swept nothing on an invocation with $REMAIN candidates pending (budget starvation)"
fi

if [ "$REMAIN" = "0" ]; then
    pass "T9: backlog of $BACKLOG drained to zero in $PASSES --gc-cheap invocation(s)"
else
    fail_timed "T9: backlog did not converge -- $REMAIN of $BACKLOG dirs remain after $PASSES invocations"
fi

# Audit-trail invariant: exactly one summary line per swept dir, none lost.
LINES_AFTER=$(wc -l < "$SUMMARY" | tr -d ' ')
SWEPT_LINES=$((LINES_AFTER - LINES_BEFORE))
if [ "$SWEPT_LINES" = "$BACKLOG" ]; then
    pass "T9: audit trail gained exactly $BACKLOG lines (one per swept dir)"
else
    fail_timed "T9: audit trail gained $SWEPT_LINES lines for $BACKLOG swept dirs (expected $BACKLOG)"
fi

# ---- T11: the ULDF_FAKE_NOW seam rejects non-numeric values -----------------
# The seam moves a delete cutoff, so it must fail SAFE (fall back to the real
# clock) rather than fail open on garbage. Proven observationally: under a
# non-numeric value the oracle must behave exactly as it does with the seam
# unset, and must NOT advertise clockSource:fake.
# A dedicated probe dir whose VERDICT flips with the clock: now-10d is
# too-young under the real clock and 375d old under a +1yr shift. Without it the
# comparison would be vacuous (the post-T9 archive holds only a KEEP-pinned and
# an unparsable dir, whose verdicts no clock can change). --dry-run mutates
# nothing, so all three probes see the same tree.
FX_SEAM=$(_fixture_name 10 140000)
mkdir -p "$ARCHIVE/$FX_SEAM/workers" "$ARCHIVE/$FX_SEAM/tasks"
echo "# fixture" > "$ARCHIVE/$FX_SEAM/GUIDE.md"
SEAM_FUTURE=$((NOW_EPOCH_FIXTURE + 31536000))

# Trailing garbage on an otherwise-valid epoch: the shape a lenient parse would
# silently coerce (bash arithmetic, PowerShell's -as [int]) into a real shift.
SEAM_BOGUS_OUT=$(cd "$SANDBOX" && ULDF_FAKE_NOW="${SEAM_FUTURE}x" bash .claude/oracles/archive-retention/run.sh --dry-run 2>&1)
# `env -u`, not a bare call: under T10's inner run this validator's OWN
# environment already carries ULDF_FAKE_NOW, so a bare invocation would inherit
# it and the "unset" baseline would silently be a faked one.
SEAM_UNSET_OUT=$(cd "$SANDBOX" && env -u ULDF_FAKE_NOW bash .claude/oracles/archive-retention/run.sh --dry-run 2>&1)
SEAM_NUM_OUT=$(cd "$SANDBOX" && ULDF_FAKE_NOW="$SEAM_FUTURE" bash .claude/oracles/archive-retention/run.sh --dry-run 2>&1)

if echo "$SEAM_BOGUS_OUT" | grep -q '"clockSource":"fake"'; then
    fail "T11: non-numeric ULDF_FAKE_NOW was honored (got: $SEAM_BOGUS_OUT)"
else
    pass "T11: non-numeric ULDF_FAKE_NOW ignored (no clockSource:fake)"
fi
# Assert about the PROBE DIR specifically, never the total count: a preceding
# cell that failed to sweep (e.g. T7/T9 under a starved --gc-cheap budget on a
# loaded machine) leaves other aged dirs in the archive, and a count assertion
# would report that as a seam defect.
if echo "$SEAM_BOGUS_OUT" | grep -q "$FX_SEAM"; then
    fail "T11: non-numeric seam moved the cutoff -- probe dir became sweepable (got: $SEAM_BOGUS_OUT)"
else
    pass "T11: non-numeric seam left the probe dir too-young (failed safe)"
fi
if [ "$SEAM_BOGUS_OUT" = "$SEAM_UNSET_OUT" ]; then
    pass "T11: non-numeric seam is byte-identical to unset"
else
    fail "T11: non-numeric seam diverged from unset (bogus: $SEAM_BOGUS_OUT / unset: $SEAM_UNSET_OUT)"
fi
# The positive half -- without it T11 would pass just as well on a seam that
# never works at all.
if echo "$SEAM_NUM_OUT" | grep -q "$FX_SEAM" && echo "$SEAM_NUM_OUT" | grep -q '"clockSource":"fake"'; then
    pass "T11: numeric ULDF_FAKE_NOW IS honored (probe dir becomes sweepable, declares clockSource:fake)"
else
    fail "T11: numeric ULDF_FAKE_NOW was not honored -- seam is dead (got: $SEAM_NUM_OUT)"
fi
rm -rf "$ARCHIVE/$FX_SEAM"

# ---- T10: time-invariance (DEFER-072's decisive cell) -----------------------
# Re-run the sandbox phase with `now` shifted +1 year. A relative-date fixture
# is time-invariant by construction; a calendar literal is not, and that
# difference is exactly what must be proven rather than assumed -- the fixtures
# this cell guards sat red for 15 days precisely because nothing asserted it.
# ULDF_AR_INNER bounds the recursion to one level and skips Phase 1.
if [ -z "${ULDF_AR_INNER:-}" ]; then
    FUTURE_EPOCH=$((NOW_EPOCH_FIXTURE + 31536000))
    INNER_OUT=$(ULDF_AR_INNER=1 ULDF_FAKE_NOW="$FUTURE_EPOCH" bash "$0" 2>&1) && INNER_RC=0 || INNER_RC=$?
    if [ "$INNER_RC" = "0" ]; then
        pass "T10: sandbox phase green under a +1yr faked clock (time-invariant)"
    else
        fail "T10: NOT time-invariant -- +1yr faked-clock run exited $INNER_RC"
        echo "$INNER_OUT" | grep '^FAIL' >&2 || true
    fi
fi

# =============================================================================
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL  DEFERRED=$SKIP"
timing_guard_summary
if [ "$FAIL" -gt 0 ]; then exit 1; fi
exit 0
