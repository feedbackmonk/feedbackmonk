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

set -e
ORACLE_DIR="$(dirname "$0")"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

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
# Phase 1 — briefing path against the real archived dir (best-effort)
# =============================================================================

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
# 4 dirs:
#   collab-20260101-100000  -- AGED (> 90 days), no KEEP -> sweepable (T1)
#   collab-20260201-120000  -- AGED, has KEEP             -> kept (T3)
#   collab-20260420-130000  -- recent (~10 days)          -> too-young (T2)
#   collab-bogus-name       -- unparseable                -> never sweep
ARCHIVE="$SANDBOX/.claude/collaboration/archived"

for d in collab-20260101-100000 collab-20260201-120000 collab-20260420-130000 collab-bogus-name; do
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
echo "Keep because: T3 fixture" > "$ARCHIVE/collab-20260201-120000/KEEP"

# ---- T1+T2+T3+T5+T8: run --gc and inspect --------------------------------
GC_OUT=$(cd "$SANDBOX" && bash .claude/oracles/archive-retention/run.sh --gc 2>&1)
echo "[--gc summary]: $GC_OUT"

# T5: summary shape
for f in swept before after threshold thresholdSource summarized; do
    if ! echo "$GC_OUT" | grep -q "\"$f\""; then
        fail "T5: --gc summary missing field '$f' (got: $GC_OUT)"
    fi
done

echo "$GC_OUT" | grep -q '"swept":1'      && pass "T1: --gc swept=1 (only collab-20260101-100000)" || fail "T1: --gc swept != 1 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"before":3'     && pass "T5: --gc before=3 (excludes bogus-name)"        || fail "T5: --gc before != 3 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"after":2'      && pass "T5: --gc after=2"                                || fail "T5: --gc after != 2 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"summarized":1' && pass "T5: --gc summarized=1"                            || fail "T5: --gc summarized != 1 (got: $GC_OUT)"

# T2: collab-20260420-130000 (recent) survives
[ -d "$ARCHIVE/collab-20260420-130000" ] && pass "T2: recent dir survived sweep" || fail "T2: recent dir was deleted"

# T3: collab-20260201-120000 (KEEP) survives
[ -d "$ARCHIVE/collab-20260201-120000" ] && pass "T3: KEEP-pinned dir survived sweep" || fail "T3: KEEP-pinned dir was deleted"

# T1: collab-20260101-100000 deleted
[ ! -d "$ARCHIVE/collab-20260101-100000" ] && pass "T1: aged dir deleted" || fail "T1: aged dir was NOT deleted"

# Bogus name preserved
[ -d "$ARCHIVE/collab-bogus-name" ] && pass "Defensive: bogus-name dir preserved (failure-open)" || fail "Defensive: bogus-name dir was deleted"

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
    echo "$SUMMARY_LINE" | grep -q '"sessionId":"collab-20260101-100000"' && pass "T8: _summary.jsonl sessionId matches" || fail "T8: _summary.jsonl sessionId mismatch"
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
# Set threshold to 5 days; rebuild fixture so collab-20260420-130000 is now sweepable.
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
mkdir -p "$ARCHIVE/collab-20260102-100000/workers"
mkdir -p "$ARCHIVE/collab-20260102-100000/tasks"
echo "# fixture" > "$ARCHIVE/collab-20260102-100000/GUIDE.md"

CHEAP_OUT=$(cd "$SANDBOX" && bash .claude/oracles/archive-retention/run.sh --gc-cheap 2>&1)
if [ -z "$(echo "$CHEAP_OUT" | tr -d '[:space:]')" ]; then
    pass "T7: --gc-cheap silent on success"
else
    fail "T7: --gc-cheap emitted output (should be silent): $CHEAP_OUT"
fi

[ ! -d "$ARCHIVE/collab-20260102-100000" ] && pass "T7: --gc-cheap performed the sweep" || fail "T7: --gc-cheap did not sweep aged dir"

# Final summary file should have 3 lines now
LINES3=$(wc -l < "$SUMMARY" | tr -d ' ')
if [ "$LINES3" = "3" ]; then
    pass "T7: _summary.jsonl reached 3 lines (cumulative across 3 sweeps)"
else
    fail "T7: _summary.jsonl has $LINES3 lines after --gc-cheap, expected 3"
fi

# =============================================================================
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
if [ "$FAIL" -gt 0 ]; then exit 1; fi
exit 0
