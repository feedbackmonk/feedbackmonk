#!/bin/bash
# module-tree-map oracle self-test (Unix)
# Asserts: T1 single-module, T2 multi-module hierarchical, T3 missing-synopsis,
#          T4 graceful empty-project, T5 file-index extraction, T6 cross-platform parity
# (T6 is asserted by the validate.ps1 counterpart producing identical JSON.)

set -e

ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
RUN_SH="$ORACLE_DIR/run.sh"

# Use python for JSON parsing (already used by sibling oracle validators).
# On Windows, `python3` may be a Microsoft Store stub that exits non-zero
# without doing anything — probe each candidate by actually running it.
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
    echo "FAIL: python required for JSON validation (tried python3, python, py)" >&2
    exit 1
fi

# ---- Build a tmp test project ----
TMPROOT=$(mktemp -d)
trap 'rm -rf "$TMPROOT"' EXIT

# ---- T4: empty project ----
T4DIR="$TMPROOT/t4-empty"
mkdir -p "$T4DIR"
cd "$T4DIR"
out=$(bash "$RUN_SH" 2>/dev/null)
echo "$out" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['root']['path'] == '.', 'T4 root path'
assert d['root']['synopsis'] is None, 'T4 root synopsis null'
assert d['root']['children'] == [], 'T4 root children empty'
assert d['stats']['total_modules'] == 0, 'T4 total_modules 0'
assert d['stats']['synopsized'] == 0, 'T4 synopsized 0'
assert d['stats']['missing_synopsis'] == [], 'T4 missing_synopsis empty'
assert d.get('briefing', None) == '', f'T4 briefing expected empty (graceful absence) got {d.get(\"briefing\")!r}'
print('PASS T4: graceful empty-project (briefing empty -> gracefully absent)')
" || { echo "FAIL T4" >&2; exit 1; }

# ---- T1: single-module project (root README only with Synopsis) ----
T1DIR="$TMPROOT/t1-single"
mkdir -p "$T1DIR"
cd "$T1DIR"
cat > README.md <<'EOF'
# Project

## Synopsis

Single-module project for testing. Come here for triage tests.

## Purpose & Responsibilities

Test fixture.
EOF
out=$(bash "$RUN_SH" 2>/dev/null)
echo "$out" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['root']['synopsis'] is not None, 'T1 root has synopsis'
assert 'Single-module' in d['root']['synopsis'], 'T1 synopsis content'
assert d['stats']['total_modules'] == 1, 'T1 total 1'
assert d['stats']['synopsized'] == 1, 'T1 synopsized 1'
assert d['stats']['missing_synopsis'] == [], 'T1 missing empty'
b = d.get('briefing', '')
assert '1 module' in b and '1/1 with Synopsis' in b and '/0-uldf-oracle module-tree-map' in b, f'T1 briefing format wrong: {b!r}'
print('PASS T1: single-module (briefing populated per HCT-05 format)')
" || { echo "FAIL T1" >&2; exit 1; }

# ---- T2: multi-module hierarchical + T3 missing + T5 file-index ----
T2DIR="$TMPROOT/t2-hierarchical"
mkdir -p "$T2DIR/src/auth/session"
mkdir -p "$T2DIR/src/auth"
mkdir -p "$T2DIR/src/billing"
mkdir -p "$T2DIR/src/legacy"
cd "$T2DIR"
cat > README.md <<'EOF'
# Root

## Synopsis

Root.

## File Index

- [src/](./src/): Application source
EOF
cat > src/auth/README.md <<'EOF'
# Auth

## Synopsis

Auth module. Come here for token issuance.

## File Index

- [tokens.ts](./tokens.ts): Token generation
- [session.ts](./session.ts): Session lifecycle
EOF
cat > src/auth/session/README.md <<'EOF'
# Session

## Synopsis

Session lifecycle helpers.
EOF
cat > src/billing/README.md <<'EOF'
# Billing

## Synopsis

Billing.
EOF
# src/legacy intentionally has README without Synopsis → T3
cat > src/legacy/README.md <<'EOF'
# Legacy

## Purpose & Responsibilities

Old code.
EOF
out=$(bash "$RUN_SH" 2>/dev/null)
echo "$out" | "$PYBIN" -c "
import sys, json
d = json.loads(sys.stdin.read())
assert d['stats']['total_modules'] == 5, f'T2 total_modules expected 5 got {d[\"stats\"][\"total_modules\"]}'
assert d['stats']['synopsized'] == 4, f'T2 synopsized expected 4 got {d[\"stats\"][\"synopsized\"]}'
assert 'src/legacy' in d['stats']['missing_synopsis'], 'T3 src/legacy in missing'
# T2 hierarchy: root.children includes src/auth, src/billing, src/legacy
root_child_paths = sorted(c['path'] for c in d['root']['children'])
assert root_child_paths == ['src/auth', 'src/billing', 'src/legacy'], f'T2 root children paths: {root_child_paths}'
# src/auth has src/auth/session as child
auth = next(c for c in d['root']['children'] if c['path'] == 'src/auth')
assert len(auth['children']) == 1, 'T2 src/auth has 1 child'
assert auth['children'][0]['path'] == 'src/auth/session', 'T2 child is session'
# T5 file-index extracted on src/auth
assert 'file_index' in auth, 'T5 src/auth has file_index'
assert len(auth['file_index']) == 2, 'T5 src/auth file_index has 2 entries'
names = sorted(e['name'] for e in auth['file_index'])
assert names == ['session.ts', 'tokens.ts'], f'T5 file_index names: {names}'
# T5 root file_index extraction
assert 'file_index' in d['root'], 'T5 root has file_index'
print('PASS T2 + T3 + T5: multi-module hierarchical, missing-synopsis, file-index extraction')
" || { echo "FAIL T2/T3/T5" >&2; exit 1; }

echo "PASS: module-tree-map oracle validates (T1, T2, T3, T4, T5)"
exit 0
