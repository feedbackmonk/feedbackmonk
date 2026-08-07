#!/bin/bash
# markdown-link-validity oracle self-test (Unix)
#
# Builds hermetic sandbox doc trees (no dependence on this repo's own docs) and
# asserts the detector's real behaviour against the FROZEN output schema
# (oracle.json):
#
#   T1. clean-tree        -> status=pass, broken_count=0, only internal links counted
#   T2. one-broken        -> status=fail, broken_count=1, the BROKEN link is named
#                            and the GOOD link is NOT (anti-silent-breakage: a
#                            detector that reports everything, or nothing, fails)
#   T3. exclusions-only   -> http/https/mailto/tel/#anchor links are not checked
#   T4. relative-nesting  -> ../ resolution from a nested dir, good vs broken
#   T5. empty-scope       -> no scanned dirs at all -> pass, scanned_files=0
#   T6. foreign-uncommitted-deletion (OVALID-05, DEC-220) -> a target TRACKED and
#                            present in HEAD, removed only by an uncommitted
#                            deletion, is informational and does NOT fail; a
#                            genuinely absent target in the SAME run still fails
#                            (not a blanket amnesty); and committing the deletion
#                            makes it fail for real (deferred, not weakened)
#
# Each test creates a fresh sandbox under a TMPDIR, copies run.sh into the
# sandbox's .claude/oracles/ path, runs the oracle from the sandbox root, and
# asserts on the compact JSON output.

set +e
ORACLE_DIR="$(cd "$(dirname "$0")" 2>/dev/null && pwd -P)"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

SANDBOX=""
cleanup() { [ -n "$SANDBOX" ] && [ -d "$SANDBOX" ] && rm -rf "$SANDBOX"; }
trap cleanup EXIT

mk_sandbox() {
    SANDBOX=$(mktemp -d 2>/dev/null || mktemp -d -t 'mlvix')
    if [ -z "$SANDBOX" ] || [ ! -d "$SANDBOX" ]; then
        echo "FATAL: could not create sandbox" >&2
        exit 1
    fi
    mkdir -p "$SANDBOX/project/.claude/oracles/markdown-link-validity"
    cp "$ORACLE_DIR/run.sh"      "$SANDBOX/project/.claude/oracles/markdown-link-validity/run.sh"
    cp "$ORACLE_DIR/oracle.json" "$SANDBOX/project/.claude/oracles/markdown-link-validity/oracle.json"
    chmod +x "$SANDBOX/project/.claude/oracles/markdown-link-validity/run.sh" 2>/dev/null || true
}

run_oracle() {
    (cd "$SANDBOX/project" && bash .claude/oracles/markdown-link-validity/run.sh 2>&1)
}

# Schema fields the oracle MUST emit on every run.
SCHEMA_FIELDS=(status checked broken_count scanned_files scan_duration_ms broken uncommitted_deletion_count uncommitted_deletions)

assert_schema_fields() {
    local out="$1"
    local label="$2"
    local f
    for f in "${SCHEMA_FIELDS[@]}"; do
        if ! printf '%s' "$out" | grep -q "\"$f\""; then
            fail "$label: missing schema field '$f' in: $out"
            return 1
        fi
    done
    return 0
}

# Compact-JSON field probe: emits the raw value text following "<field>":
json_num() {
    printf '%s' "$1" | sed -n "s/.*\"$2\":\([-0-9][0-9]*\).*/\1/p" | head -n1
}
json_status() {
    printf '%s' "$1" | sed -n 's/.*"status":"\([a-z]*\)".*/\1/p' | head -n1
}

# -----------------------------------------------------------------------------
# T1. clean-tree -- every internal link resolves
# -----------------------------------------------------------------------------
mk_sandbox
mkdir -p "$SANDBOX/project/docs"
printf '# Target\n' > "$SANDBOX/project/docs/good-target.md"
cat > "$SANDBOX/project/docs/index.md" <<'MD'
# Index
A [good link](good-target.md) here.
An [anchored good link](good-target.md#section) here.
An [external](https://example.com/page) link.
MD
out=$(run_oracle)
assert_schema_fields "$out" "T1" || true
st=$(json_status "$out"); ck=$(json_num "$out" "checked"); bc=$(json_num "$out" "broken_count"); sf=$(json_num "$out" "scanned_files")
if [ "$st" = "pass" ] && [ "$bc" = "0" ] && [ "$ck" = "2" ] && [ "$sf" = "2" ] && printf '%s' "$out" | grep -q '"broken":\[\]'; then
    pass "T1: clean-tree -> pass, checked=2 (external excluded), broken_count=0, broken=[]"
else
    fail "T1: expected pass/checked=2/broken_count=0/scanned_files=2; got st=$st ck=$ck bc=$bc sf=$sf out=$out"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T2. one-broken -- THE core anti-silent-breakage assertion
#     The broken link must be reported (with correct source/line/resolved_path)
#     and the good link must NOT appear in the broken list.
# -----------------------------------------------------------------------------
mk_sandbox
mkdir -p "$SANDBOX/project/docs"
printf '# Target\n' > "$SANDBOX/project/docs/good-target.md"
cat > "$SANDBOX/project/docs/index.md" <<'MD'
# Index
A [good link](good-target.md) here.
A [broken link](nope-missing.md) here.
MD
out=$(run_oracle)
assert_schema_fields "$out" "T2" || true
st=$(json_status "$out"); ck=$(json_num "$out" "checked"); bc=$(json_num "$out" "broken_count")
ok=1
[ "$st" = "fail" ] || ok=0
[ "$bc" = "1" ] || ok=0
[ "$ck" = "2" ] || ok=0
printf '%s' "$out" | grep -q '"source":"docs/index.md"'                 || ok=0
printf '%s' "$out" | grep -q '"line":3'                                 || ok=0
printf '%s' "$out" | grep -q '"link":"nope-missing.md"'                 || ok=0
printf '%s' "$out" | grep -q '"resolved_path":"docs/nope-missing.md"'   || ok=0
# The good link must NOT be reported broken.
printf '%s' "$out" | grep -q 'good-target'                              && ok=0
if [ "$ok" = "1" ]; then
    pass "T2: one-broken -> fail, broken_count=1, names nope-missing.md at docs/index.md:3, good link NOT reported"
else
    fail "T2: expected fail/broken_count=1 naming only the broken link; got st=$st ck=$ck bc=$bc out=$out"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T3. exclusions-only -- external schemes and bare anchors are never checked
# -----------------------------------------------------------------------------
mk_sandbox
mkdir -p "$SANDBOX/project/docs"
cat > "$SANDBOX/project/docs/index.md" <<'MD'
# Index
[http](http://example.com/a)
[https](https://example.com/b)
[ftp](ftp://example.com/c)
[mail](mailto:someone@example.com)
[tel](tel:+15551234567)
[anchor](#same-page-section)
MD
out=$(run_oracle)
assert_schema_fields "$out" "T3" || true
st=$(json_status "$out"); ck=$(json_num "$out" "checked"); bc=$(json_num "$out" "broken_count")
if [ "$st" = "pass" ] && [ "$ck" = "0" ] && [ "$bc" = "0" ]; then
    pass "T3: exclusions-only -> pass, checked=0 (http/https/ftp/mailto/tel/#anchor all skipped)"
else
    fail "T3: expected pass/checked=0/broken_count=0; got st=$st ck=$ck bc=$bc out=$out"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T4. relative-nesting -- ../ resolution from a nested source directory
# -----------------------------------------------------------------------------
mk_sandbox
mkdir -p "$SANDBOX/project/docs" "$SANDBOX/project/FOUNDATIONS/deep"
printf '# Target\n' > "$SANDBOX/project/docs/good-target.md"
cat > "$SANDBOX/project/FOUNDATIONS/deep/nested.md" <<'MD'
# Nested
Up-and-over [good](../../docs/good-target.md).
Up-one [broken](../absent-sibling.md).
MD
out=$(run_oracle)
assert_schema_fields "$out" "T4" || true
st=$(json_status "$out"); ck=$(json_num "$out" "checked"); bc=$(json_num "$out" "broken_count")
ok=1
[ "$st" = "fail" ] || ok=0
[ "$bc" = "1" ] || ok=0
[ "$ck" = "2" ] || ok=0
printf '%s' "$out" | grep -q '"link":"../absent-sibling.md"'                              || ok=0
printf '%s' "$out" | grep -q '"resolved_path":"FOUNDATIONS/deep/../absent-sibling.md"'    || ok=0
printf '%s' "$out" | grep -q 'good-target'                                                && ok=0
if [ "$ok" = "1" ]; then
    pass "T4: relative-nesting -> ../../ good link resolves, ../ broken link reported once"
else
    fail "T4: expected fail/checked=2/broken_count=1 naming only ../absent-sibling.md; got st=$st ck=$ck bc=$bc out=$out"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T5. empty-scope -- graceful absence when no scanned dirs/files exist
# -----------------------------------------------------------------------------
mk_sandbox
out=$(run_oracle)
assert_schema_fields "$out" "T5" || true
st=$(json_status "$out"); bc=$(json_num "$out" "broken_count"); sf=$(json_num "$out" "scanned_files")
if [ "$st" = "pass" ] && [ "$bc" = "0" ] && [ "$sf" = "0" ]; then
    pass "T5: empty-scope -> pass, scanned_files=0, broken_count=0"
else
    fail "T5: expected pass/scanned_files=0/broken_count=0; got st=$st bc=$bc sf=$sf out=$out"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T6. foreign-uncommitted-deletion (OVALID-05, DEC-220)
#
#     The oracle asserts "the citation resolves" but measures "a path exists in
#     the working tree" -- a tree shared with live sibling sessions. This test
#     pins all three halves of the remedy, and it is invertible in the direction
#     that matters: if the suppression ever becomes a blanket amnesty, T6b fails.
#
#       T6a  uncommitted deletion of a HEAD-present target -> informational, pass
#       T6b  a genuinely absent target in the SAME run     -> still fails
#       T6c  commit the deletion                           -> now fails for real
#
#     T6a deliberately includes a PARENT-RELATIVE citation (`../FOUNDATIONS/...`),
#     because the resolved path then carries a `..` segment while
#     `git ls-files --deleted` never does. Without the canonicalization in
#     run.sh the membership test silently never matches for that whole class --
#     a proxy bug inside the fix for a proxy bug. Neuter norm_path and this leg
#     goes red.
# -----------------------------------------------------------------------------
if command -v git >/dev/null 2>&1; then
    mk_sandbox
    mkdir -p "$SANDBOX/project/docs" "$SANDBOX/project/FOUNDATIONS"
    printf '# Target\n' > "$SANDBOX/project/docs/tracked-target.md"
    printf '# Upstairs\n' > "$SANDBOX/project/FOUNDATIONS/tracked-upstairs.md"
    cat > "$SANDBOX/project/docs/index.md" <<'MD'
# Index
A [tracked link](tracked-target.md) here.
A [genuinely broken link](never-existed.md) here.
A [parent-relative tracked link](../FOUNDATIONS/tracked-upstairs.md) here.
MD
    (
        cd "$SANDBOX/project" || exit 1
        git init -q . >/dev/null 2>&1
        git config user.email "selftest@localhost" >/dev/null 2>&1
        git config user.name "mlv selftest" >/dev/null 2>&1
        git add -A >/dev/null 2>&1
        git -c commit.gpgsign=false commit -qm "selftest baseline" >/dev/null 2>&1
    )

    # T6a + T6b: delete BOTH tracked targets WITHOUT committing (one same-dir,
    # one parent-relative so the canonicalization is exercised).
    rm -f "$SANDBOX/project/docs/tracked-target.md" "$SANDBOX/project/FOUNDATIONS/tracked-upstairs.md"
    out=$(run_oracle)
    assert_schema_fields "$out" "T6" || true
    st=$(json_status "$out"); bc=$(json_num "$out" "broken_count"); uc=$(json_num "$out" "uncommitted_deletion_count")
    broken_only=$(printf '%s' "$out" | sed -n 's/.*"broken":\(\[.*\]\),"uncommitted_deletion_count".*/\1/p')
    ok=1
    # T6b -- the genuinely-absent target still fails, in the same run.
    [ "$st" = "fail" ] || ok=0
    [ "$bc" = "1" ] || ok=0
    printf '%s' "$out" | grep -q '"link":"never-existed.md"' || ok=0
    # T6a -- BOTH uncommitted deletions are informational, not broken. The count
    # of 2 is what pins the canonicalization: without it the parent-relative one
    # lands in broken[] instead and this drops to 1.
    [ "$uc" = "2" ] || ok=0
    printf '%s' "$out" | grep -q 'tracked-upstairs' || ok=0
    # Neither may also appear in broken[] (double-counting would defeat it).
    printf '%s' "$broken_only" | grep -q 'tracked-target'   && ok=0
    printf '%s' "$broken_only" | grep -q 'tracked-upstairs' && ok=0
    if [ "$ok" = "1" ]; then
        pass "T6a/T6b: both uncommitted deletions informational (count=2, incl. the parent-relative path) while a genuinely absent target still fails in the same run -- not a blanket amnesty"
    else
        fail "T6a/T6b: expected fail/broken_count=1(never-existed)/uncommitted_deletion_count=2(tracked-target + ../tracked-upstairs); got st=$st bc=$bc uc=$uc out=$out"
    fi

    # T6c: commit the deletion -- the suppression must LAPSE.
    (
        cd "$SANDBOX/project" || exit 1
        git add -A >/dev/null 2>&1
        git -c commit.gpgsign=false commit -qm "selftest: commit the deletion" >/dev/null 2>&1
    )
    out=$(run_oracle)
    st=$(json_status "$out"); bc=$(json_num "$out" "broken_count"); uc=$(json_num "$out" "uncommitted_deletion_count")
    broken_only=$(printf '%s' "$out" | sed -n 's/.*"broken":\(\[.*\]\),"uncommitted_deletion_count".*/\1/p')
    ok=1
    [ "$st" = "fail" ] || ok=0
    [ "$bc" = "3" ] || ok=0
    [ "$uc" = "0" ] || ok=0
    printf '%s' "$broken_only" | grep -q 'tracked-target'   || ok=0
    printf '%s' "$broken_only" | grep -q 'tracked-upstairs' || ok=0
    if [ "$ok" = "1" ]; then
        pass "T6c: committing the deletions restores the failures (broken_count=3, uncommitted_deletion_count=0) -- the gate is deferred, not weakened"
    else
        fail "T6c: expected fail/broken_count=3/uncommitted_deletion_count=0 after committing the deletions; got st=$st bc=$bc uc=$uc out=$out"
    fi
    cleanup; SANDBOX=""
else
    echo "SKIP: T6 (OVALID-05) -- git not on PATH; the informational bucket needs git ls-files --deleted"
fi

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
echo
echo "================================================================"
echo "  markdown-link-validity validate: $PASS PASS / $FAIL FAIL"
echo "================================================================"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
exit 0
