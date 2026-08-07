#!/bin/bash
# retirement-candidates oracle self-test (Unix) -- CTXY-04
#
# Cases:
#   T1. exit-condition-satisfied   past ISO date in a "Remove ... once ..." line
#   T2. exit-condition-declared    non-evaluable condition -> surfaced[], NOT candidates[]
#   T3. done-marker                uppercase DONE/RESOLVED/... token in a section
#   T4. correction-strata          >=2 CORRECTION/UPDATE layers under one head
#   T5. provisional-no-exit        FILE-level: fires with no exit condition, silent with one
#   T6. self-supersession          head banner + long body
#   T7. no-inbound-refs            basename mentioned nowhere else
#   T8. clean corpus               a trap note + a rejected-alternative note => ZERO candidates
#   T9. code-fence immunity        markers inside ``` fences never fire
#   T10. TWIN PARITY               run.ps1 returns the identical candidate set (TWIN-01..03)
#
# T8 is the load-bearing one. A retirement detector that fires on guardrails is
# worse than no detector: it teaches the agent to delete exactly the text the
# discipline exists to protect.

set -u
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
PASS=0; FAIL=0
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }

# `command -v python3` succeeds on Windows even when it is only the Microsoft
# Store app-execution-alias stub, which prints an install nag and exits nonzero.
# Probe by actually running it (same guard as planning-doc-staleness/validate.sh).
PYBIN=""
for c in python3 python; do
  if command -v "$c" >/dev/null 2>&1 && "$c" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
    PYBIN="$c"; break
  fi
done
[ -z "$PYBIN" ] && { echo "SKIP: validate requires python for JSON inspection"; exit 0; }

SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'retcand')"
cleanup() { rm -rf "$SANDBOX"; }
trap cleanup EXIT

mkdir -p "$SANDBOX/.claude/oracles/retirement-candidates"
cp "$ORACLE_DIR/run.sh"  "$SANDBOX/.claude/oracles/retirement-candidates/run.sh"
cp "$ORACLE_DIR/run.ps1" "$SANDBOX/.claude/oracles/retirement-candidates/run.ps1" 2>/dev/null || true
mkdir -p "$SANDBOX/docs/pending" "$SANDBOX/docs/reviews" "$SANDBOX/docs/planning/deferred"
(cd "$SANDBOX" && git init -q && git config user.email t@t && git config user.name t)

# ---------------------------------------------------------------- fixtures
cat > "$SANDBOX/docs/pending/t1-satisfied.md" <<'EOF'
# T1

## Waiting on the installer
Remove this entry once the installer ships, after 2020-01-01.
Body.
EOF

cat > "$SANDBOX/docs/pending/t2-declared.md" <<'EOF'
# T2

## Waiting on a vendor
Remove this entry once the vendor confirms the SLA.
Body.
EOF

cat > "$SANDBOX/docs/reviews/t3-done.md" <<'EOF'
# T3

## The migration entry
Delete when the shim is gone.
Status: RESOLVED -- landed in the 3.2 release.
EOF

cat > "$SANDBOX/docs/reviews/t4-strata.md" <<'EOF'
# T4

## The original claim
Delete when the parser is replaced.
The parser mis-handles CRLF.
**CORRECTION**: it was the tokenizer, not the parser.
**UPDATE**: reverted; the tokenizer was fine.
EOF

cat > "$SANDBOX/docs/planning/deferred/t5-noexit.md" <<'EOF'
# T5 provisional, no exit condition

## The idea
Nothing here declares what would make this brief deletable.
EOF

cat > "$SANDBOX/docs/planning/deferred/t5-hasexit.md" <<'EOF'
# T5b provisional WITH an exit condition

## The idea
Remove this brief once the CLI flag lands.
EOF

{
  echo "# T6"
  echo ""
  echo "> SUPERSEDED by the new design; kept for provenance."
  echo ""
  echo "## Body that is still sitting here being read"
  i=0; while [ $i -lt 60 ]; do echo "Line of stale body text number $i."; i=$((i+1)); done
} > "$SANDBOX/docs/reviews/t6-supersede.md"

# T11/T12 regression: prose ABOUT exit conditions, and a table row documenting
# one, must NOT be reported as satisfied. Both fired as false positives during
# dogfooding; DEFER-044 records the identical defect in governing-doc-consistency
# ("a stale date appears anywhere on the line"), where the prescribed remedy
# would have written a lie into the docs.
cat > "$SANDBOX/docs/reviews/t11-meta.md" <<'EOF'
# T11 prose about exit conditions

## What the monitor did
None of its four exit conditions (ALL_COMPLETE | TIMEOUT | ERROR) had fired
during the 2026-07-01 session, so the LD never woke.

## Table documenting a remove-when rule
| oracle | asserts | measures |
| --- | --- | --- |
| `governing-doc-consistency` | this remove-when deadline has passed | a stale date anywhere on the line (2026-07-13 review) |
EOF

cat > "$SANDBOX/docs/reviews/t8-clean.md" <<'EOF'
# T8 clean

## A trap with no mechanical guard
Calling flush() before the lock is taken corrupts the index. No regression test
covers this yet, so this paragraph is the guard.

## Why we rejected the queue approach
It serialized the writer, which is the entire point of the module.
EOF

cat > "$SANDBOX/docs/reviews/t9-fenced.md" <<'EOF'
# T9 fenced

## Example output, not a claim
The tool prints:

```
Status: DONE
Remove this entry once acted on, after 2020-01-01.
CORRECTION: ignored
UPDATE: ignored
```

That block is sample output, not an assertion about this document.
EOF

# T13/T14 regression: the SILENT FALSE PASS found by dogfooding on a second
# project, 2026-08-02. An 82 KB `docs/pending-followups.md` -- 41 entries, the
# exact artifact class this oracle exists for -- reported `pass` for two
# independent reasons: its only heading is level 1 (block boundaries were 2-4,
# so nothing was ever scanned) and its filename is neither the literal
# `docs/PENDING_FOLLOW_UPS.md` in the corpus nor a match for the provisional
# regex. Either alone is enough to silence the whole file.
cat > "$SANDBOX/docs/reviews/t13-h1only.md" <<'EOF'
# T13 entries under a single level-one heading

Status: RESOLVED -- the shim shipped.
Remove this entry once the shim is gone, after 2020-01-01.
EOF

cat > "$SANDBOX/docs/pending-followups.md" <<'EOF'
# Pending Follow-Ups

- **A kebab-cased follow-ups file.** Nothing here declares what would make it
  removable. Provisional by construction; spelling must not decide visibility.
EOF

# T7 needs an inbound reference for everything EXCEPT the orphan, so give the
# non-orphans a referrer and commit so `git grep` can see them.
cat > "$SANDBOX/docs/index.md" <<'EOF'
# Index
See t1-satisfied.md, t2-declared.md, t3-done.md, t4-strata.md, t11-meta.md,
t5-noexit.md, t5-hasexit.md, t6-supersede.md, t8-clean.md, t9-fenced.md,
t13-h1only.md, pending-followups.md.
EOF
cat > "$SANDBOX/docs/reviews/t7-orphan.md" <<'EOF'
# T7 orphan

## Nothing references this file
Remove this entry once the audit closes.
EOF
(cd "$SANDBOX" && git add -A >/dev/null 2>&1 && git commit -q -m "fixtures" >/dev/null 2>&1)

CORPUS='docs/pending/*.md:docs/reviews/*.md:docs/planning/deferred/*.md'

run_sh() { (cd "$SANDBOX" && CLAUDE_RETIREMENT_CORPUS="$CORPUS" bash .claude/oracles/retirement-candidates/run.sh); }

OUT="$(run_sh)"

q() { printf '%s' "$OUT" | "$PYBIN" -c "import json,sys;d=json.load(sys.stdin);$1" 2>/dev/null || echo "<<error>>"; }
check() { # expr expected label
  a="$(q "$1")"
  if [ "$a" = "$2" ]; then pass "$3"; else fail "$3 (expected '$2', got '$a')"; fi
}
HAS='has=lambda p,s:any(c["path"].endswith(p) and c["signal"]==s for c in d["candidates"]);'
SURF='surf=lambda p,s:any(c["path"].endswith(p) and c["signal"]==s for c in d["surfaced"]);'

check "$HAS print(has('t1-satisfied.md','exit-condition-satisfied'))" "True"  "T1: past-dated exit condition -> candidate"
check "$SURF print(surf('t2-declared.md','exit-condition-declared'))" "True"  "T2: non-evaluable condition -> surfaced"
check "$HAS print(has('t2-declared.md','exit-condition-declared'))"  "False"  "T2: non-evaluable condition is NOT a candidate"
check "$HAS print(has('t3-done.md','done-marker'))"                  "True"   "T3: uppercase RESOLVED -> done-marker"
check "$HAS print(has('t4-strata.md','correction-strata'))"          "True"   "T4: two correction layers -> correction-strata"
check "$HAS print(has('t5-noexit.md','provisional-no-exit'))"        "True"   "T5: provisional file without exit condition fires"
check "$HAS print(has('t5-hasexit.md','provisional-no-exit'))"       "False"  "T5: provisional file WITH exit condition is silent"
check "$HAS print(has('t6-supersede.md','self-supersession'))"       "True"   "T6: head banner over long body -> self-supersession"
check "$HAS print(has('t7-orphan.md','no-inbound-refs'))"            "True"   "T7: unreferenced file -> no-inbound-refs"
check "$HAS print(has('t8-clean.md','done-marker'))"                 "False"  "T8: trap note produces no done-marker"
check "$HAS print(has('t8-clean.md','no-inbound-refs'))"             "False"  "T8: referenced trap note is not an orphan"
check "print(sum(1 for c in d['candidates'] if c['path'].endswith('t8-clean.md')))" "0" "T8: clean guardrail doc yields ZERO candidates"
check "print(sum(1 for c in d['candidates'] if c['path'].endswith('t9-fenced.md')))" "0" "T9: fenced sample output never fires"
check "$HAS print(has('t11-meta.md','exit-condition-satisfied'))"   "False" "T11: prose ABOUT exit conditions never reads as satisfied"
check "$SURF print(surf('t11-meta.md','exit-condition-declared'))" "True"  "T11: it is surfaced as a mention instead"
check "$HAS print(has('t11-meta.md','done-marker'))"                "False" "T12: a remove-when table row is not a satisfied condition"
check "$HAS print(has('t13-h1only.md','done-marker'))"              "True"  "T13: a level-1-only file is scanned (done-marker)"
check "$HAS print(has('t13-h1only.md','exit-condition-satisfied'))" "True"  "T13: a level-1-only file is scanned (satisfied exit condition)"
check "print(d['status'])" "warn" "status is warn when candidates exist"
check "print('WORKLIST' in d['briefing'])" "True" "briefing states the worklist contract"

# T14 runs the DEFAULT corpus (no CLAUDE_RETIREMENT_CORPUS) -- the point is
# whether the shipped globs and provisional regex see a kebab-cased follow-ups
# file at all. Asserting it via the override would test nothing.
OUT_DEF="$( (cd "$SANDBOX" && bash .claude/oracles/retirement-candidates/run.sh) )"
qd() { printf '%s' "$OUT_DEF" | "$PYBIN" -c "import json,sys;d=json.load(sys.stdin);$1" 2>/dev/null || echo "<<error>>"; }
a="$(qd "$HAS print(has('pending-followups.md','provisional-no-exit'))")"
if [ "$a" = "True" ]; then pass "T14: default corpus + provisional regex see docs/pending-followups.md"
else fail "T14: default corpus missed docs/pending-followups.md (expected 'True', got '$a')"; fi

# --------------------------------------------------------- T10: twin parity
PSBIN=""
for c in pwsh powershell; do command -v "$c" >/dev/null 2>&1 && { PSBIN="$c"; break; }; done
if [ -z "$PSBIN" ]; then
  echo "SKIP: T10 twin parity (no PowerShell on PATH) -- NOT a pass"
else
  PSOUT="$( cd "$SANDBOX" && CLAUDE_RETIREMENT_CORPUS="$CORPUS" "$PSBIN" -NoProfile -File .claude/oracles/retirement-candidates/run.ps1 2>/dev/null )"
  printf '%s' "$OUT"   > "$SANDBOX/_sh.json"
  printf '%s' "$PSOUT" > "$SANDBOX/_ps.json"
  same="$("$PYBIN" - "$SANDBOX" <<'PYEOF'
import json,sys,os
sb=sys.argv[1]
def L(n):
    with open(os.path.join(sb,n),encoding='utf-8-sig',errors='replace') as f: return json.load(f)
try:
    a=L("_sh.json"); b=L("_ps.json")
except Exception as e:
    print("parse-error:%s"%e); raise SystemExit
k=lambda c:(c["path"],c["line_start"],c["signal"])
sa={k(c) for c in a["candidates"]}; sb_=({k(c) for c in b["candidates"]})
print("True" if sa==sb_ else "MISMATCH sh-only=%s ps-only=%s"%(sorted(sa-sb_),sorted(sb_-sa)))
PYEOF
)"
  if [ "$same" = "True" ]; then pass "T10: bash/PowerShell twins agree on the candidate set"
  else fail "T10: TWIN PARITY BROKEN -- $same"; fi
fi

echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -gt 0 ] && exit 1
exit 0
