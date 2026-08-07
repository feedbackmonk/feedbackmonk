#!/bin/bash
# rspd-elaboration-tree oracle self-test (Unix) -- RSPD-06 / RSPD-10.
# This validate IS the oracle's smoke (oracles ship validate harnesses).
# Covers the full derive logic against sandbox fixtures:
#   A. populated tree: charter (store present) / elaborated / missing
#      (CHARTER row -> missing store) / done (via recursion into a child store)
#   B. graceful absence: a spec with no delegated nodes -> applicable:false
#   C. no spec at all -> applicable:false, spec_root null
#   D. schema fields present + valid JSON
#   E. real-repo run -> applicable:false (no live delegated nodes yet)
#
# Usage: bash claude-template/oracles/rspd-elaboration-tree/validate.sh [--keep-sandbox]
# Exit: 0 all pass; 1 at least one assertion failed.

set -uo pipefail

KEEP=""
for a in "$@"; do [ "$a" = "--keep-sandbox" ] && KEEP=1; done

ORACLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN="$ORACLE_DIR/run.sh"
[ -f "$RUN" ] || { echo "FATAL: missing $RUN" >&2; exit 1; }

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); }
bad() { FAIL=$((FAIL+1)); echo "  FAIL: $1" >&2; }

assert_has()  { case "$2" in *"$3"*) ok ;; *) bad "$1 (missing '$3')" ;; esac; }
assert_not()  { case "$2" in *"$3"*) bad "$1 (unexpected '$3')" ;; *) ok ;; esac; }
# assert a node fragment: id ... elaboration (single JSON object, no nested braces)
assert_node() { # desc json id elab
    if printf '%s' "$2" | grep -qE "\"id\":\"$3\"[^}]*\"elaboration\":\"$4\""; then ok
    else bad "$1 (node $3 not classified $4)"; fi
}

SB="$(mktemp -d 2>/dev/null || echo "/tmp/rspd-oracle-$$")"
trap '[ -z "$KEEP" ] && rm -rf "$SB" 2>/dev/null || true' EXIT

# --- A. populated tree fixture ------------------------------------------------
mkdir -p "$SB/A/docs/specs/payments" "$SB/A/docs/specs/auth/session" "$SB/A/docs/specs/billing"
cat > "$SB/A/docs/specs/SPECIFICATION.md" <<'EOF'
# Root spec

#### PAY-DELEG: Payments sub-system [CHARTER]
**Priority**: Must
**Spec-Owner**: child (delegated) - `payments/SPECIFICATION.md`
**Contract**: `payments/CONTRACT.md`

#### AUTH-DELEG: Auth sub-system [ELABORATED]
**Priority**: Must
**Spec-Owner**: child (delegated) - `auth/SPECIFICATION.md`
**Contract**: `auth/CONTRACT.md`

#### BILL-DELEG: Billing sub-system [IN_PROGRESS]
**Priority**: Must
**Spec-Owner**: child (delegated) - `billing/SPECIFICATION.md`
**Contract**: `billing/CONTRACT.md`

#### RPT-DELEG: Reporting sub-system [CHARTER]
**Priority**: Should
**Spec-Owner**: child (delegated) - `reporting/SPECIFICATION.md`
**Contract**: `reporting/CONTRACT.md`

#### CORE-01: Core thing [DONE]
**Priority**: Must
**Implementation**: src/core.ts
EOF
# payments store EXISTS (CHARTER + present -> charter)
echo "# payments" > "$SB/A/docs/specs/payments/SPECIFICATION.md"
# billing store EXISTS (IN_PROGRESS -> in_progress)
echo "# billing" > "$SB/A/docs/specs/billing/SPECIFICATION.md"
# auth store EXISTS (ELABORATED -> elaborated) and itself delegates a child
cat > "$SB/A/docs/specs/auth/SPECIFICATION.md" <<'EOF'
# auth spec
#### SESS-DELEG: Session sub-system [DONE]
**Spec-Owner**: child (delegated) - `session/SPECIFICATION.md`
EOF
# auth's session store EXISTS (recursion: DONE -> done)
echo "# session" > "$SB/A/docs/specs/auth/session/SPECIFICATION.md"
# reporting store does NOT exist (CHARTER + absent -> missing)

OUT_A="$(cd "$SB/A" && bash "$RUN" 2>&1)" || bad "A run.sh exited non-zero"
assert_has  "A1 valid-ish JSON object"          "$OUT_A" '{"status":"ok"'
assert_has  "A2 applicable true"                "$OUT_A" '"applicable":true'
assert_node "A3 PAY-DELEG charter (store present)"   "$OUT_A" "PAY-DELEG"  "charter"
assert_node "A4 AUTH-DELEG elaborated"               "$OUT_A" "AUTH-DELEG" "elaborated"
assert_node "A5 RPT-DELEG missing (store absent)"    "$OUT_A" "RPT-DELEG"  "missing"
assert_node "A6 SESS-DELEG done (recursion)"         "$OUT_A" "SESS-DELEG" "done"
assert_node "A6b BILL-DELEG in_progress"             "$OUT_A" "BILL-DELEG" "in_progress"
assert_has  "A7 summary totals"                 "$OUT_A" '"summary":{"total":5,"charter":1,"elaborated":1,"in_progress":1,"done":1,"missing":1}'
assert_has  "A8 missing surfaces in briefing"   "$OUT_A" 'MISSING their sub-spec store'
assert_not  "A9 non-delegated CORE-01 excluded" "$OUT_A" '"id":"CORE-01"'
assert_has  "A10 store_exists false on missing" "$OUT_A" '"id":"RPT-DELEG","domain":"reporting","status":"[CHARTER]"'
assert_has  "A11 domain derived for nested"     "$OUT_A" '"domain":"auth/session"'

# --- B. graceful absence: spec with no delegated nodes ------------------------
mkdir -p "$SB/B/docs/specs"
cat > "$SB/B/docs/specs/SPECIFICATION.md" <<'EOF'
# Root spec
#### CORE-01: Core thing [DONE]
**Implementation**: src/core.ts
#### CORE-02: Other thing [PLANNED]
EOF
OUT_B="$(cd "$SB/B" && bash "$RUN" 2>&1)" || bad "B run.sh exited non-zero"
assert_has "B1 applicable false (no delegated nodes)" "$OUT_B" '"applicable":false'
assert_has "B2 empty tree"                            "$OUT_B" '"tree":[]'
assert_has "B3 zero totals"                           "$OUT_B" '"total":0'
assert_has "B4 empty briefing"                        "$OUT_B" '"briefing":""'

# --- C. no spec at all -------------------------------------------------------
mkdir -p "$SB/C"
OUT_C="$(cd "$SB/C" && bash "$RUN" 2>&1)" || bad "C run.sh exited non-zero"
assert_has "C1 applicable false (no spec)" "$OUT_C" '"applicable":false'
assert_has "C2 spec_root null"             "$OUT_C" '"spec_root":null'

# --- D. schema fields present ------------------------------------------------
for field in '"status":' '"applicable":' '"spec_root":' '"tree":' '"summary":' '"briefing":'; do
    assert_has "D schema field $field" "$OUT_A" "$field"
done
for field in '"total":' '"charter":' '"elaborated":' '"in_progress":' '"done":' '"missing":'; do
    assert_has "D summary field $field" "$OUT_A" "$field"
done

# --- E. real repo: no live delegated nodes yet -> gracefully absent ----------
if [ -f "docs/specs/SPECIFICATION.md" ]; then
    OUT_E="$(bash "$RUN" 2>&1)" || bad "E run.sh exited non-zero on real repo"
    assert_has "E1 real repo applicable false (no live delegations)" "$OUT_E" '"applicable":false'
fi

echo ""
echo "================================================================"
echo "RSPD-ELABORATION-TREE ORACLE VALIDATE (bash): $PASS passed, $FAIL failed"
echo "Sandbox: $SB"
echo "================================================================"
[ "$FAIL" -gt 0 ] && exit 1
echo "PASS: rspd-elaboration-tree oracle validates"
exit 0
