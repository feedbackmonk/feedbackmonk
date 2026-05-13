#!/bin/bash
# workspace-shared-repos oracle self-test (Unix)
#
# Sandbox-builds eight scenarios and asserts the oracle output is correct:
#   T1. pnpm-workspace.yaml discovery (literal paths + glob expansion)
#   T2. Cargo.toml [workspace] members discovery
#   T3. package.json workspaces discovery (both array and object forms)
#   T4. .claude/config.json sharedRepos explicit-list discovery
#   T5. Multi-source dedup: same path declared in pnpm AND explicit -> explicit wins
#   T6. Skip non-git: declared path without .git/ is filtered out
#   T7. Skip self: declaration pointing back at the project itself is dropped
#   T8. Graceful empty: no declaration files -> {count:0, repos:[], discoveryMethod:"none"}

set -e
ORACLE_DIR="$(cd "$(dirname "$0")" 2>/dev/null && pwd -P)"

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

# -----------------------------------------------------------------------------
# Sandbox helpers
# -----------------------------------------------------------------------------
SANDBOX=""
cleanup() {
    [ -n "$SANDBOX" ] && [ -d "$SANDBOX" ] && rm -rf "$SANDBOX"
}
trap cleanup EXIT

mk_sandbox() {
    SANDBOX=$(mktemp -d 2>/dev/null || mktemp -d -t 'wsro')
    if [ -z "$SANDBOX" ] || [ ! -d "$SANDBOX" ]; then
        echo "FATAL: could not create sandbox" >&2
        exit 1
    fi
    mkdir -p "$SANDBOX/project/.claude/oracles/workspace-shared-repos"
    # Copy oracle scripts into the sandbox so relative-path lookups work as in production.
    cp "$ORACLE_DIR/run.sh"      "$SANDBOX/project/.claude/oracles/workspace-shared-repos/run.sh"
    cp "$ORACLE_DIR/oracle.json" "$SANDBOX/project/.claude/oracles/workspace-shared-repos/oracle.json"
    chmod +x "$SANDBOX/project/.claude/oracles/workspace-shared-repos/run.sh" 2>/dev/null || true
}

# Create a fake git repo at $1 (just enough to satisfy `[ -d "$path/.git" ]`).
mk_git_repo() {
    mkdir -p "$1/.git"
    : > "$1/.git/HEAD"
}

# Create a non-git directory (used for skip-non-git tests).
mk_plain_dir() {
    mkdir -p "$1"
    : > "$1/README.md"
}

# Run the oracle with CWD set to the sandbox project root.  Echo its stdout.
run_oracle() {
    (cd "$SANDBOX/project" && bash .claude/oracles/workspace-shared-repos/run.sh 2>&1)
}

# Helper: count occurrences of "path" key in JSON output containing each substring.
contains_path() {
    local output="$1"
    local needle="$2"
    case "$output" in *"\"path\":\""*"$needle"*) return 0 ;; esac
    return 1
}

# Helper: assert JSON validity via python.
assert_valid_json() {
    local json="$1"
    if [ -n "$PYBIN" ]; then
        echo "$json" | "$PYBIN" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null
        return $?
    fi
    return 0  # skip JSON validity check when no python
}

# -----------------------------------------------------------------------------
# T1: pnpm-workspace.yaml discovery
# -----------------------------------------------------------------------------
mk_sandbox
mk_git_repo "$SANDBOX/sibling-a"
mk_git_repo "$SANDBOX/sibling-b"
mk_plain_dir "$SANDBOX/not-a-git-checkout"
mk_git_repo "$SANDBOX/glob-foo"
cp "$ORACLE_DIR/test-fixtures/pnpm-workspace/pnpm-workspace.yaml" "$SANDBOX/project/pnpm-workspace.yaml"

T1=$(run_oracle)
echo "[T1 output]: $T1"
assert_valid_json "$T1" && pass "T1 valid JSON" || fail "T1 invalid JSON"
contains_path "$T1" "/sibling-a"   && pass "T1 sibling-a in repos[]"      || fail "T1 sibling-a missing"
contains_path "$T1" "/sibling-b"   && pass "T1 sibling-b in repos[]"      || fail "T1 sibling-b missing"
contains_path "$T1" "/glob-foo"    && pass "T1 glob-* expanded"           || fail "T1 glob-* did not expand"
if contains_path "$T1" "/not-a-git-checkout"; then
    fail "T1 not-a-git-checkout should NOT be in output"
else
    pass "T1 not-a-git-checkout filtered out"
fi
echo "$T1" | grep -q '"discoveryMethod":"pnpm"' && pass "T1 discoveryMethod=pnpm" || fail "T1 discoveryMethod wrong"
echo "$T1" | grep -q '"declarationSource":"pnpm"' && pass "T1 declarationSource=pnpm" || fail "T1 declarationSource wrong"
cleanup

# -----------------------------------------------------------------------------
# T2: Cargo.toml [workspace] members discovery
# -----------------------------------------------------------------------------
mk_sandbox
mk_git_repo "$SANDBOX/sibling-a"
mk_git_repo "$SANDBOX/sibling-b"
mk_plain_dir "$SANDBOX/not-a-git-checkout"
cp "$ORACLE_DIR/test-fixtures/cargo-workspace/Cargo.toml" "$SANDBOX/project/Cargo.toml"

T2=$(run_oracle)
echo "[T2 output]: $T2"
assert_valid_json "$T2" && pass "T2 valid JSON" || fail "T2 invalid JSON"
contains_path "$T2" "/sibling-a" && pass "T2 sibling-a in repos[]" || fail "T2 sibling-a missing"
contains_path "$T2" "/sibling-b" && pass "T2 sibling-b in repos[]" || fail "T2 sibling-b missing"
echo "$T2" | grep -q '"discoveryMethod":"cargo"' && pass "T2 discoveryMethod=cargo" || fail "T2 discoveryMethod wrong"
cleanup

# -----------------------------------------------------------------------------
# T3: package.json workspaces discovery (array form via fixture; object form inline)
# -----------------------------------------------------------------------------
mk_sandbox
mk_git_repo "$SANDBOX/sibling-a"
mk_git_repo "$SANDBOX/sibling-b"
mk_plain_dir "$SANDBOX/not-a-git-checkout"
cp "$ORACLE_DIR/test-fixtures/npm-workspace/package.json" "$SANDBOX/project/package.json"

T3a=$(run_oracle)
echo "[T3a (array form) output]: $T3a"
assert_valid_json "$T3a" && pass "T3a valid JSON" || fail "T3a invalid JSON"
contains_path "$T3a" "/sibling-a" && pass "T3a sibling-a in repos[]" || fail "T3a sibling-a missing"
contains_path "$T3a" "/sibling-b" && pass "T3a sibling-b in repos[]" || fail "T3a sibling-b missing"
echo "$T3a" | grep -q '"discoveryMethod":"npm"' && pass "T3a discoveryMethod=npm" || fail "T3a discoveryMethod wrong"

# Object form: rewrite package.json to use {workspaces: {packages: [...]}}.
cat > "$SANDBOX/project/package.json" <<'EOF'
{
  "name": "fixture-root",
  "private": true,
  "workspaces": {"packages": ["../sibling-a", "../sibling-b"]}
}
EOF
T3b=$(run_oracle)
echo "[T3b (object form) output]: $T3b"
contains_path "$T3b" "/sibling-a" && pass "T3b object-form sibling-a in repos[]" || fail "T3b object-form sibling-a missing"
contains_path "$T3b" "/sibling-b" && pass "T3b object-form sibling-b in repos[]" || fail "T3b object-form sibling-b missing"
cleanup

# -----------------------------------------------------------------------------
# T4: .claude/config.json sharedRepos discovery
# -----------------------------------------------------------------------------
mk_sandbox
mk_git_repo "$SANDBOX/sibling-a"
mk_git_repo "$SANDBOX/sibling-b"
mk_plain_dir "$SANDBOX/not-a-git-checkout"
mkdir -p "$SANDBOX/project/.claude"
cp "$ORACLE_DIR/test-fixtures/explicit-list/config.json" "$SANDBOX/project/.claude/config.json"

T4=$(run_oracle)
echo "[T4 output]: $T4"
assert_valid_json "$T4" && pass "T4 valid JSON" || fail "T4 invalid JSON"
contains_path "$T4" "/sibling-a" && pass "T4 sibling-a in repos[]" || fail "T4 sibling-a missing"
contains_path "$T4" "/sibling-b" && pass "T4 sibling-b in repos[]" || fail "T4 sibling-b missing"
echo "$T4" | grep -q '"discoveryMethod":"explicit"' && pass "T4 discoveryMethod=explicit" || fail "T4 discoveryMethod wrong"
echo "$T4" | grep -q '"declarationSource":"explicit"' && pass "T4 declarationSource=explicit" || fail "T4 declarationSource wrong"
cleanup

# -----------------------------------------------------------------------------
# T5: multi-source dedup with priority -- same path in BOTH pnpm AND explicit;
#     explicit wins, declarationSource:"explicit", discoveryMethod:"explicit"
#     (pnpm contributes no surviving entry, so it's not in discoveryMethod).
# -----------------------------------------------------------------------------
mk_sandbox
mk_git_repo "$SANDBOX/sibling-a"
mkdir -p "$SANDBOX/project/.claude"

cat > "$SANDBOX/project/pnpm-workspace.yaml" <<'EOF'
packages:
  - "../sibling-a"
EOF
cat > "$SANDBOX/project/.claude/config.json" <<'EOF'
{"sharedRepos":[{"path":"../sibling-a"}]}
EOF

T5=$(run_oracle)
echo "[T5 output]: $T5"
assert_valid_json "$T5" && pass "T5 valid JSON" || fail "T5 invalid JSON"
echo "$T5" | grep -q '"count":1'                       && pass "T5 dedup count=1"             || fail "T5 dedup count != 1"
echo "$T5" | grep -q '"declarationSource":"explicit"'  && pass "T5 explicit wins (declarationSource)" || fail "T5 declarationSource wrong"
echo "$T5" | grep -q '"discoveryMethod":"explicit"'    && pass "T5 discoveryMethod=explicit" || fail "T5 discoveryMethod wrong (pnpm should not appear post-dedup)"
cleanup

# -----------------------------------------------------------------------------
# T6: skip non-git -- declared path WITHOUT .git/ is filtered out
# -----------------------------------------------------------------------------
mk_sandbox
mk_git_repo "$SANDBOX/sibling-a"
mk_plain_dir "$SANDBOX/no-git-here"

cat > "$SANDBOX/project/pnpm-workspace.yaml" <<'EOF'
packages:
  - "../sibling-a"
  - "../no-git-here"
EOF

T6=$(run_oracle)
echo "[T6 output]: $T6"
contains_path "$T6" "/sibling-a"  && pass "T6 sibling-a kept" || fail "T6 sibling-a missing"
if contains_path "$T6" "/no-git-here"; then
    fail "T6 no-git-here should be filtered out"
else
    pass "T6 no-git-here filtered out"
fi
echo "$T6" | grep -q '"count":1' && pass "T6 count=1 (only sibling-a)" || fail "T6 count != 1"
cleanup

# -----------------------------------------------------------------------------
# T7: skip self -- declaration pointing back at the project itself is dropped
# -----------------------------------------------------------------------------
mk_sandbox
mk_git_repo "$SANDBOX/project"      # make project itself a git repo so .git exists
mk_git_repo "$SANDBOX/sibling-a"
mkdir -p "$SANDBOX/project/.claude"

cat > "$SANDBOX/project/.claude/config.json" <<'EOF'
{"sharedRepos":[{"path":"."},{"path":"../sibling-a"}]}
EOF

T7=$(run_oracle)
echo "[T7 output]: $T7"
contains_path "$T7" "/sibling-a" && pass "T7 sibling-a kept" || fail "T7 sibling-a missing"
echo "$T7" | grep -q '"count":1' && pass "T7 self-reference dropped (count=1)" || fail "T7 self-reference not dropped (count != 1)"
cleanup

# -----------------------------------------------------------------------------
# T8: graceful empty -- no declaration files at all
# -----------------------------------------------------------------------------
mk_sandbox
T8=$(run_oracle)
echo "[T8 output]: $T8"
assert_valid_json "$T8" && pass "T8 valid JSON" || fail "T8 invalid JSON"
echo "$T8" | grep -q '"count":0'                  && pass "T8 count=0"                || fail "T8 count != 0"
echo "$T8" | grep -q '"repos":\[\]'               && pass "T8 repos[] empty"          || fail "T8 repos[] not empty"
echo "$T8" | grep -q '"discoveryMethod":"none"'   && pass "T8 discoveryMethod=none"   || fail "T8 discoveryMethod wrong"
echo "$T8" | grep -q '"_meta"'                    && pass "T8 _meta block present"    || fail "T8 _meta block missing"
echo "$T8" | grep -q '"schemaVersion":1'          && pass "T8 schemaVersion=1"        || fail "T8 schemaVersion wrong"
cleanup

# -----------------------------------------------------------------------------
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -gt 0 ] && exit 1
exit 0
