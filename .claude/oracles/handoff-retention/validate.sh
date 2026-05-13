#!/bin/bash
# handoff-retention oracle self-test (Unix)
#
# Phase 1: validate the read-only briefing path against the real handoff dir.
# Phase 2: validate --gc and --gc-cheap sweep semantics in a sandbox:
#   T1. --gc deletes briefs older than threshold (no .KEEP file).
#   T2. --gc does NOT delete briefs younger than threshold.
#   T3. Sibling <file>.KEEP exempts brief from sweep regardless of age.
#   T4. --gc is idempotent: re-running on post-sweep dir sweeps zero.
#   T5. --gc emits JSON summary with all expected fields.
#   T6. .claude/config.json handoffRetention.threshold is honored (numeric and PnD).
#   T7. --gc-cheap is silent on success.
#   T8. _summary.jsonl receives one valid JSON line per swept brief BEFORE delete (SWEEP-08).
#   T9. Malformed config falls back to default 30 days with threshold_source=default.

set -uo pipefail
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
# Phase 1 — briefing path against the real handoff dir (best-effort)
# =============================================================================

OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

if [ -n "$PYBIN" ]; then
    if ! echo "$OUTPUT" | "$PYBIN" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: briefing output is not valid JSON" >&2
        echo "Output: $OUTPUT" >&2
        exit 1
    fi
fi

for field in swept retained_keep_pinned retained_under_ttl threshold_days threshold_source briefing; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        fail "briefing: missing schema field '$field'"
    fi
done

THRESH=$(echo "$OUTPUT" | grep -oE '"threshold_days"[[:space:]]*:[[:space:]]*[0-9]+' | grep -oE '[0-9]+$' | head -1)
if [ "$THRESH" = "30" ]; then
    pass "briefing: threshold_days=30 (default)"
else
    fail "briefing: threshold_days != 30 (got '$THRESH')"
fi

# =============================================================================
# Phase 2 — sweep semantics in a sandbox
# =============================================================================

if [ -z "$PYBIN" ]; then
    echo "SKIP: Phase 2 (--gc tests) requires python for fixture build"
    if [ "$FAIL" -gt 0 ]; then exit 1; fi
    exit 0
fi

SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'handoff-ret')"
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

mkdir -p "$SANDBOX/.claude/handoff"
mkdir -p "$SANDBOX/.claude/oracles/handoff-retention"

cp "$ORACLE_DIR/run.sh" "$SANDBOX/.claude/oracles/handoff-retention/run.sh"
cp "$ORACLE_DIR/oracle.json" "$SANDBOX/.claude/oracles/handoff-retention/oracle.json"

HANDOFF="$SANDBOX/.claude/handoff"
SUMMARY="$HANDOFF/_summary.jsonl"

# ---- Build fixture handoff files (5 briefs) --------------------------------
# We use mtime to drive age. Set mtimes via touch -t.
#   handoff-aged-A.md      (60d old, no KEEP) -> sweepable (T1)
#   handoff-aged-B.md      (45d old, has KEEP) -> kept (T3)
#   handoff-recent-A.md    (5d old) -> too-young (T2)
#   handoff-recent-B.md    (1d old) -> too-young (T2)
#   handoff-aged-malformed (60d old, but no .md ext) -> ignored (defensive)

# Helper: epoch-to-touch-stamp ([[CC]YY]MMDDhhmm[.ss])
_mk_aged() {
    local f="$1"
    local days="$2"
    echo "# Aged handoff brief $days days" > "$f"
    echo "" >> "$f"
    echo "Read first: docs/specs/SPECIFICATION.md" >> "$f"
    # Set mtime to N days ago.
    local epoch
    epoch=$(("$($PYBIN -c 'import time; print(int(time.time()))')" - days * 86400))
    "$PYBIN" -c "import os, sys; os.utime(sys.argv[1], (int(sys.argv[2]), int(sys.argv[2])))" "$f" "$epoch"
}

_mk_aged "$HANDOFF/handoff-aged-A.md" 60
_mk_aged "$HANDOFF/handoff-aged-B.md" 45
_mk_aged "$HANDOFF/handoff-recent-A.md" 5
_mk_aged "$HANDOFF/handoff-recent-B.md" 1
# Malformed: no .md extension
_mk_aged "$HANDOFF/handoff-no-extension" 60

# Pin aged-B with KEEP sibling
echo "Keep because: T3 fixture" > "$HANDOFF/handoff-aged-B.md.KEEP"

# ---- T1+T2+T3+T5+T8: run --gc and inspect --------------------------------
GC_OUT=$(cd "$SANDBOX" && bash .claude/oracles/handoff-retention/run.sh --gc 2>&1)
echo "[--gc summary]: $GC_OUT"

# T5: summary shape
for f in swept before after threshold thresholdSource summarized; do
    if ! echo "$GC_OUT" | grep -q "\"$f\""; then
        fail "T5: --gc summary missing field '$f' (got: $GC_OUT)"
    fi
done

echo "$GC_OUT" | grep -q '"swept":1'      && pass "T1: --gc swept=1 (only handoff-aged-A.md)" || fail "T1: --gc swept != 1 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"before":4'     && pass "T5: --gc before=4 (handoff-*.md only, malformed excluded)" || fail "T5: --gc before != 4 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"after":3'      && pass "T5: --gc after=3" || fail "T5: --gc after != 3 (got: $GC_OUT)"
echo "$GC_OUT" | grep -q '"summarized":1' && pass "T5: --gc summarized=1" || fail "T5: --gc summarized != 1 (got: $GC_OUT)"

# T2: recent briefs survived
[ -f "$HANDOFF/handoff-recent-A.md" ] && pass "T2: handoff-recent-A.md survived sweep" || fail "T2: handoff-recent-A.md was deleted"
[ -f "$HANDOFF/handoff-recent-B.md" ] && pass "T2: handoff-recent-B.md survived sweep" || fail "T2: handoff-recent-B.md was deleted"

# T3: KEEP-pinned brief survived
[ -f "$HANDOFF/handoff-aged-B.md" ] && pass "T3: KEEP-pinned brief survived sweep" || fail "T3: KEEP-pinned brief was deleted"

# T1: aged-A deleted
[ ! -f "$HANDOFF/handoff-aged-A.md" ] && pass "T1: aged-A.md deleted" || fail "T1: aged-A.md was NOT deleted"

# Malformed (no .md) preserved
[ -f "$HANDOFF/handoff-no-extension" ] && pass "Defensive: handoff-no-extension preserved (failure-open)" || fail "Defensive: handoff-no-extension was deleted"

# T8: _summary.jsonl received the entry BEFORE delete (we can't directly verify
# ordering without log timing; we verify the line exists and has fields).
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

    for f in file swept_at age_days brief_first_line; do
        if ! echo "$SUMMARY_LINE" | grep -q "\"$f\""; then
            fail "T8: _summary.jsonl missing field '$f'"
        fi
    done
    echo "$SUMMARY_LINE" | grep -q 'handoff-aged-A.md' && pass "T8: _summary.jsonl file path matches" || fail "T8: _summary.jsonl file mismatch"
else
    fail "T8: _summary.jsonl was not created"
fi

# ---- T4: idempotence ---------------------------------------------------------
GC_OUT2=$(cd "$SANDBOX" && bash .claude/oracles/handoff-retention/run.sh --gc 2>&1)
echo "[second --gc]: $GC_OUT2"
echo "$GC_OUT2" | grep -q '"swept":0' && pass "T4: idempotence (second --gc swept=0)" || fail "T4: idempotence violated (got: $GC_OUT2)"

# ---- T6: config.json threshold honored (numeric form) ----------------------
echo '{"handoffRetention":{"threshold":3}}' > "$SANDBOX/.claude/config.json"

GC_OUT3=$(cd "$SANDBOX" && bash .claude/oracles/handoff-retention/run.sh --gc 2>&1)
echo "[--gc with config 3d]: $GC_OUT3"
echo "$GC_OUT3" | grep -q '"swept":1' && pass "T6: 3d threshold sweeps the recent-A brief" || fail "T6: 3d threshold did not sweep (got: $GC_OUT3)"
echo "$GC_OUT3" | grep -q '"thresholdSource":"config"' && pass "T6: thresholdSource=config" || fail "T6: thresholdSource != config (got: $GC_OUT3)"
echo "$GC_OUT3" | grep -q '"threshold":"P3D"' && pass "T6: threshold=P3D" || fail "T6: threshold != P3D (got: $GC_OUT3)"

# T6 PnD form
echo '{"handoffRetention":{"threshold":"P1D"}}' > "$SANDBOX/.claude/config.json"
GC_OUT3b=$(cd "$SANDBOX" && bash .claude/oracles/handoff-retention/run.sh 2>&1)
echo "[default with PnD config 1d]: $GC_OUT3b"
echo "$GC_OUT3b" | grep -q '"threshold_days":1' && pass "T6: PnD form parses (threshold_days=1)" || fail "T6: PnD form did not parse (got: $GC_OUT3b)"
echo "$GC_OUT3b" | grep -q '"threshold_source":"config"' && pass "T6: PnD form sets source=config" || fail "T6: PnD form source mismatch"

# ---- T7: --gc-cheap silent on success --------------------------------------
CHEAP_OUT=$(cd "$SANDBOX" && bash .claude/oracles/handoff-retention/run.sh --gc-cheap 2>&1)
if [ -z "$(echo "$CHEAP_OUT" | tr -d '[:space:]')" ]; then
    pass "T7: --gc-cheap silent on success"
else
    fail "T7: --gc-cheap emitted output (should be silent): $CHEAP_OUT"
fi

# ---- T9: malformed config falls back to default ----------------------------
echo 'this is not json {{{' > "$SANDBOX/.claude/config.json"
GC_OUT_BAD=$(cd "$SANDBOX" && bash .claude/oracles/handoff-retention/run.sh 2>&1)
printf '[default with malformed config snippet]: %.200s\n' "$GC_OUT_BAD"
echo "$GC_OUT_BAD" | grep -q '"threshold_days":30' && pass "T9: malformed config -> default 30 days" || fail "T9: malformed config did not fall back to 30d (got snippet): $(echo "$GC_OUT_BAD" | head -c 200)"
echo "$GC_OUT_BAD" | grep -q '"threshold_source":"default"' && pass "T9: malformed config -> threshold_source=default" || fail "T9: malformed config source mismatch"

# =============================================================================
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
if [ "$FAIL" -gt 0 ]; then exit 1; fi
exit 0
