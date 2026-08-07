#!/bin/bash
# synopsis-coverage Verification Oracle self-test (Unix)
# Asserts: T1 all-conformant -> 100%, T2 missing -> correct missing[],
#          T3 over-length -> correct over_length[], T4 graceful empty,
#          T5 cross-platform parity (asserted by validate.ps1 producing identical JSON)

set -e

ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
RUN_SH="$ORACLE_DIR/run.sh"

PYBIN=""
for candidate in python3 python py; do
    if command -v "$candidate" >/dev/null 2>&1; then
        if echo "{}" | "$candidate" -c "import sys,json; json.loads(sys.stdin.read())" >/dev/null 2>&1; then
            PYBIN="$candidate"
            break
        fi
    fi
done
if [ -z "$PYBIN" ]; then
    echo "FAIL: python required for JSON validation" >&2
    exit 1
fi

TMPROOT=$(mktemp -d)
trap 'rm -rf "$TMPROOT"' EXIT

# Cache isolation: run.sh keeps its trigger-invalidate cache in a cache/ dir
# NEXT TO ITSELF — copy the script into the sandbox so validation never
# touches the production cache (twin rationale: module-tree-map/validate.sh).
mkdir -p "$TMPROOT/oracle"
cp "$RUN_SH" "$TMPROOT/oracle/run.sh"
RUN_SH="$TMPROOT/oracle/run.sh"

# T4: empty project
T4DIR="$TMPROOT/t4-empty"
mkdir -p "$T4DIR"
cd "$T4DIR"
out=$(bash "$RUN_SH" 2>/dev/null)
echo "$out" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['coverage_pct'] == 100, f'T4 coverage_pct expected 100 got {d[\"coverage_pct\"]}'
assert d['total_modules'] == 0, 'T4 total 0'
assert d['conformant_count'] == 0, 'T4 conformant 0'
assert d['missing'] == [], 'T4 missing empty'
assert d['over_length'] == [], 'T4 over_length empty'
assert d['briefing'] == '', 'T4 briefing empty (graceful absence)'
print('PASS T4: graceful empty-project (100% by convention, no briefing)')
" || { echo "FAIL T4" >&2; exit 1; }

# T1: all conformant -> 100% coverage, empty briefing
T1DIR="$TMPROOT/t1-all-conformant"
mkdir -p "$T1DIR/src/auth"
cd "$T1DIR"
cat > README.md <<'EOF'
# Root

## Synopsis

Root module.
EOF
cat > src/auth/README.md <<'EOF'
# Auth

## Synopsis

Auth module.
Comes here for tokens.
EOF
out=$(bash "$RUN_SH" 2>/dev/null)
echo "$out" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['coverage_pct'] == 100, f'T1 coverage_pct expected 100 got {d[\"coverage_pct\"]}'
assert d['total_modules'] == 2, f'T1 total_modules expected 2 got {d[\"total_modules\"]}'
assert d['conformant_count'] == 2, f'T1 conformant expected 2 got {d[\"conformant_count\"]}'
assert d['missing'] == [], 'T1 missing empty'
assert d['over_length'] == [], 'T1 over_length empty'
assert d['briefing'] == '', 'T1 briefing empty (gracefully absent at 100%)'
print('PASS T1: all conformant -> 100%, no briefing')
" || { echo "FAIL T1" >&2; exit 1; }

# T2: some missing
T2DIR="$TMPROOT/t2-some-missing"
mkdir -p "$T2DIR/src/auth"
mkdir -p "$T2DIR/src/billing"
mkdir -p "$T2DIR/src/legacy"
cd "$T2DIR"
cat > README.md <<'EOF'
# Root

## Synopsis

Root.
EOF
cat > src/auth/README.md <<'EOF'
# Auth

## Synopsis

Auth.
EOF
cat > src/billing/README.md <<'EOF'
# Billing

## Synopsis

Billing.
EOF
# legacy: README without Synopsis
cat > src/legacy/README.md <<'EOF'
# Legacy

## Purpose & Responsibilities

Old.
EOF
out=$(bash "$RUN_SH" 2>/dev/null)
echo "$out" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['total_modules'] == 4, f'T2 total expected 4 got {d[\"total_modules\"]}'
assert d['conformant_count'] == 3, f'T2 conformant expected 3 got {d[\"conformant_count\"]}'
assert d['coverage_pct'] == 75, f'T2 coverage_pct expected 75 got {d[\"coverage_pct\"]}'
assert 'src/legacy' in d['missing'], f'T2 src/legacy expected in missing got {d[\"missing\"]}'
assert d['over_length'] == [], 'T2 over_length empty'
assert d['briefing'].startswith('75%'), f'T2 briefing expected to start with 75% got {d[\"briefing\"]!r}'
assert '1 missing' in d['briefing'], f'T2 briefing should report 1 missing got {d[\"briefing\"]!r}'
print('PASS T2: missing surfaced in missing[] and briefing')
" || { echo "FAIL T2" >&2; exit 1; }

# T3: over-length
T3DIR="$TMPROOT/t3-over-length"
mkdir -p "$T3DIR/src/big"
cd "$T3DIR"
cat > README.md <<'EOF'
# Root

## Synopsis

Root.
EOF
cat > src/big/README.md <<'EOF'
# Big

## Synopsis

Line 1.
Line 2.
Line 3.
Line 4.
Line 5.
Line 6.
Line 7.
EOF
out=$(bash "$RUN_SH" 2>/dev/null)
echo "$out" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['total_modules'] == 2, f'T3 total expected 2 got {d[\"total_modules\"]}'
assert d['conformant_count'] == 1, f'T3 conformant expected 1 got {d[\"conformant_count\"]}'
assert 'src/big' in d['over_length'], f'T3 src/big expected in over_length got {d[\"over_length\"]}'
assert d['missing'] == [], f'T3 missing expected empty got {d[\"missing\"]}'
assert d['coverage_pct'] == 50, f'T3 coverage_pct expected 50 got {d[\"coverage_pct\"]}'
assert '1 over-length' in d['briefing'], f'T3 briefing should report over-length got {d[\"briefing\"]!r}'
print('PASS T3: over-length surfaced in over_length[] and briefing')
" || { echo "FAIL T3" >&2; exit 1; }

# ---- T7: cache warm-hit (freshness contract — serve identical bytes) ----
[ -f "$TMPROOT/oracle/cache/latest.json" ] || { echo "FAIL T7: cache not stored after compute" >&2; exit 1; }
out2=$(bash "$RUN_SH" 2>/dev/null)
[ "$out2" = "$out" ] || { echo "FAIL T7: warm-hit output differs from computed output" >&2; exit 1; }
echo "PASS T7: cache warm-hit serves byte-identical output"

# ---- T8: invalidation — edit / add / delete all recompute (set-level digest) ----
sleep 1
touch src/big/README.md
out3=$(bash "$RUN_SH" 2>/dev/null)
echo "$out3" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['total_modules'] == 2, 'T8a recompute total'
print('PASS T8a: mtime edit invalidates')
" || { echo "FAIL T8a" >&2; exit 1; }
mkdir -p src/newmod
printf '# New\n\n## Synopsis\n\nNew module.\n' > src/newmod/README.md
out4=$(bash "$RUN_SH" 2>/dev/null)
echo "$out4" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['total_modules'] == 3, f'T8b add not seen: {d[\"total_modules\"]}'
print('PASS T8b: added README invalidates')
" || { echo "FAIL T8b" >&2; exit 1; }
rm -f src/newmod/README.md
out5=$(bash "$RUN_SH" 2>/dev/null)
echo "$out5" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['total_modules'] == 2, f'T8c delete not seen: {d[\"total_modules\"]}'
print('PASS T8c: deleted README invalidates')
" || { echo "FAIL T8c" >&2; exit 1; }

# ---- T9: briefing-context cold cache — graceful absence + detached warm ----
rm -rf "$TMPROOT/oracle/cache"
out6=$(ULDF_BRIEFING=1 bash "$RUN_SH" 2>/dev/null)
echo "$out6" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d.get('briefing') == '', 'T9 cold briefing must be empty (graceful absence)'
assert d.get('cache') == 'cold-refreshing', 'T9 cache marker'
print('PASS T9: briefing-context cold cache gracefully absents (no stale serve)')
" || { echo "FAIL T9" >&2; exit 1; }
for _i in 1 2 3 4 5 6 7 8 9 10; do
    [ -f "$TMPROOT/oracle/cache/latest.json" ] && break
    sleep 1
done
[ -f "$TMPROOT/oracle/cache/latest.json" ] || { echo "FAIL T9b: detached refresh did not warm the cache" >&2; exit 1; }
out7=$(ULDF_BRIEFING=1 bash "$RUN_SH" 2>/dev/null)
echo "$out7" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['total_modules'] == 2, 'T9b warmed briefing serve'
assert d.get('cache') != 'cold-refreshing', 'T9b not cold anymore'
print('PASS T9b: detached refresh warmed the cache; next briefing serves it')
" || { echo "FAIL T9b" >&2; exit 1; }

echo "PASS: synopsis-coverage oracle validates (T1, T2, T3, T4, T7, T8a-c, T9)"
exit 0
