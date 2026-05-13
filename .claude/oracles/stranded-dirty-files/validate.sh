#!/bin/bash
# stranded-dirty-files oracle self-test (Unix)
#
# Sandbox-builds five scenarios and asserts the oracle output matches the
# FROZEN output schema (oracle.json):
#
#   T1. no-stranded                 -> count==0, briefing==""
#   T2. small-stranded              -> count>0, briefing references "no live owner"
#   T3. large-stranded              -> count>=50, briefing references "significant accumulation"
#   T4. detection-skipped-too-many  -> count==-1, briefing references "detection skipped"
#   T5. live-peer-owns-file         -> peer's claimed file is excluded from sample, count<dirty
#
# Each test creates a fresh git sandbox under a TMPDIR, runs the oracle from
# the project root, and asserts the JSON output's shape + key fields.

set +e
ORACLE_DIR="$(cd "$(dirname "$0")" 2>/dev/null && pwd -P)"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

# Probe-verify python (Microsoft Store stub on Windows exits non-zero silently).
PY=""
if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
    PY="python3"
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
    PY="python"
fi

SANDBOX=""
cleanup() { [ -n "$SANDBOX" ] && [ -d "$SANDBOX" ] && rm -rf "$SANDBOX"; }
trap cleanup EXIT

mk_sandbox() {
    SANDBOX=$(mktemp -d 2>/dev/null || mktemp -d -t 'sdfix')
    if [ -z "$SANDBOX" ] || [ ! -d "$SANDBOX" ]; then
        echo "FATAL: could not create sandbox" >&2
        exit 1
    fi
    mkdir -p "$SANDBOX/project/.claude/oracles/stranded-dirty-files"
    cp "$ORACLE_DIR/run.sh"      "$SANDBOX/project/.claude/oracles/stranded-dirty-files/run.sh"
    cp "$ORACLE_DIR/oracle.json" "$SANDBOX/project/.claude/oracles/stranded-dirty-files/oracle.json"
    chmod +x "$SANDBOX/project/.claude/oracles/stranded-dirty-files/run.sh" 2>/dev/null || true

    # Initialize a git repo with a single committed file. The oracle reads
    # `git log -1 --format=%aI HEAD` for the finalize boundary, so we need a
    # commit. We pin git config locally so the test does not depend on the
    # outer environment's user.name / user.email.
    (
        cd "$SANDBOX/project"
        git init -q -b main 2>/dev/null || git init -q
        git config user.email "test@stranded.local"
        git config user.name  "stranded-validate"
        echo "seed" > seed.txt
        git add seed.txt
        # Use --allow-empty-message-style: ensure the commit lands.
        GIT_AUTHOR_DATE="2026-04-01T00:00:00Z" GIT_COMMITTER_DATE="2026-04-01T00:00:00Z" \
            git commit -q -m "seed commit"
    ) || { echo "FATAL: sandbox git init failed" >&2; exit 1; }
}

# Touch a path with an mtime BEFORE the seed commit (2026-04-01).
mk_old_dirty() {
    local p="$1"
    local content="${2:-old}"
    mkdir -p "$(dirname "$SANDBOX/project/$p")"
    printf '%s\n' "$content" > "$SANDBOX/project/$p"
    # Force mtime to 2026-03-15 (well before the seed commit at 2026-04-01)
    touch -t 202603150000 "$SANDBOX/project/$p" 2>/dev/null || true
}

# Touch a path with an mtime AFTER the seed commit (current time = post-2026-04-01).
mk_new_dirty() {
    local p="$1"
    local content="${2:-new}"
    mkdir -p "$(dirname "$SANDBOX/project/$p")"
    printf '%s\n' "$content" > "$SANDBOX/project/$p"
    # Default mtime = now (after seed commit)
}

run_oracle() {
    (cd "$SANDBOX/project" && bash .claude/oracles/stranded-dirty-files/run.sh 2>&1)
}

assert_valid_json() {
    local out="$1"
    local label="$2"
    [ -n "$PY" ] || return 0   # skip if no python
    if ! echo "$out" | "$PY" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        fail "$label: output is not valid JSON: $out"
        return 1
    fi
    return 0
}

# Extract a JSON field via python (cheap shape-check). Echoes value or empty.
json_field() {
    local out="$1"
    local field="$2"
    [ -n "$PY" ] || { echo ""; return; }
    echo "$out" | "$PY" -c "
import sys, json
d = json.load(sys.stdin)
v = d.get('$field')
if v is None: print('')
else: print(v)
" 2>/dev/null
}

# Schema fields the oracle MUST emit on every run.
SCHEMA_FIELDS=(has_stranded count oldest_mtime sample live_peer_count last_finalize_at briefing)

assert_schema_fields() {
    local out="$1"
    local label="$2"
    for f in "${SCHEMA_FIELDS[@]}"; do
        if ! echo "$out" | grep -q "\"$f\""; then
            fail "$label: missing schema field '$f' in: $out"
            return 1
        fi
    done
    return 0
}

# -----------------------------------------------------------------------------
# T1. no-stranded
# -----------------------------------------------------------------------------
mk_sandbox
mk_new_dirty "post-commit-mod.txt" "fresh"   # dirty but mtime > finalize boundary
out=$(run_oracle)
assert_valid_json "$out" "T1" || true
assert_schema_fields "$out" "T1" || true
hs=$(json_field "$out" "has_stranded")
ct=$(json_field "$out" "count")
br=$(json_field "$out" "briefing")
if [ "$hs" = "False" ] && [ "$ct" = "0" ] && [ -z "$br" ]; then
    pass "T1: no-stranded -> has_stranded=false count=0 briefing=\"\""
else
    fail "T1: expected has_stranded=False count=0 briefing=\"\"; got hs=$hs ct=$ct br='$br'"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T2. small-stranded
# -----------------------------------------------------------------------------
mk_sandbox
mk_old_dirty "stranded-1.txt"
mk_old_dirty "stranded-2.txt"
mk_old_dirty "stranded-3.txt"
out=$(run_oracle)
assert_valid_json "$out" "T2" || true
assert_schema_fields "$out" "T2" || true
hs=$(json_field "$out" "has_stranded")
ct=$(json_field "$out" "count")
br=$(json_field "$out" "briefing")
if [ "$hs" = "True" ] && [ "$ct" = "3" ] && echo "$br" | grep -q "no live owner"; then
    pass "T2: small-stranded -> count=3 briefing references 'no live owner'"
else
    fail "T2: expected has_stranded=True count=3 briefing matches 'no live owner'; got hs=$hs ct=$ct br='$br'"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T3. large-stranded (>=50 stranded files -> "significant accumulation" briefing)
# -----------------------------------------------------------------------------
mk_sandbox
i=1
while [ "$i" -le 55 ]; do
    mk_old_dirty "stranded-$i.txt"
    i=$((i + 1))
done
out=$(run_oracle)
assert_valid_json "$out" "T3" || true
assert_schema_fields "$out" "T3" || true
hs=$(json_field "$out" "has_stranded")
ct=$(json_field "$out" "count")
br=$(json_field "$out" "briefing")
if [ "$hs" = "True" ] && [ "$ct" = "55" ] && echo "$br" | grep -q "significant accumulation"; then
    pass "T3: large-stranded -> count=55 briefing references 'significant accumulation'"
else
    fail "T3: expected has_stranded=True count=55 briefing references 'significant accumulation'; got hs=$hs ct=$ct br='$br'"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T4. detection-skipped-too-many (>2000 dirty files -> count==-1)
# -----------------------------------------------------------------------------
mk_sandbox
# Create 2001 dirty files quickly via shell loop. Use an inner subshell + cd so
# we don't pay a cd per iteration.
(
    cd "$SANDBOX/project"
    i=1
    while [ "$i" -le 2001 ]; do
        printf 'x' > "f-$i.txt"
        i=$((i + 1))
    done
)
out=$(run_oracle)
assert_valid_json "$out" "T4" || true
assert_schema_fields "$out" "T4" || true
hs=$(json_field "$out" "has_stranded")
ct=$(json_field "$out" "count")
br=$(json_field "$out" "briefing")
if [ "$hs" = "False" ] && [ "$ct" = "-1" ] && echo "$br" | grep -q "detection skipped"; then
    pass "T4: detection-skipped -> count=-1 briefing references 'detection skipped'"
else
    fail "T4: expected has_stranded=False count=-1 briefing references 'detection skipped'; got hs=$hs ct=$ct br='$br'"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T5. live-peer-owns-file
#
# Sandbox has TWO old-dirty files: peer-claimed.txt (a live peer claims it via
# dirtyFiles[]) and unclaimed.txt (no peer claims it). Expect: peer-claimed.txt
# is excluded from sample; only unclaimed.txt counts.
#
# To simulate a live peer we register an entry pointing at THIS process's PID
# (always alive; portable across kill -0 and Get-Process).
# -----------------------------------------------------------------------------
mk_sandbox
mk_old_dirty "peer-claimed.txt"
mk_old_dirty "unclaimed.txt"

# Build registry. workDir must equal sandbox project root. Use forward slashes
# (oracle normalizes both sides).
PROJ_ROOT_NORM="$(printf '%s' "$SANDBOX/project" | tr '\\' '/' | sed 's:/*$::')"

# Pick a PID the oracle's liveness probe can see. On native Unix, $$ works
# (kill -0 sees bash's own PID). On Git Bash on Windows, the oracle probes
# liveness via powershell.exe Get-Process which does NOT see MSYS-fake bash
# PIDs -- ask powershell for a known-live Windows PID instead (explorer.exe is
# always running on a desktop session; falls back to lsass for headless).
LIVE_PID="$$"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        win_pid=$(powershell.exe -NoProfile -Command "(Get-Process -Name explorer -ErrorAction SilentlyContinue | Select-Object -First 1).Id" 2>/dev/null | tr -d '\r')
        if [ -z "$win_pid" ] || [ "$win_pid" = "" ]; then
            win_pid=$(powershell.exe -NoProfile -Command "(Get-Process -Name lsass -ErrorAction SilentlyContinue | Select-Object -First 1).Id" 2>/dev/null | tr -d '\r')
        fi
        if [ -n "$win_pid" ]; then LIVE_PID="$win_pid"; fi
        ;;
esac
mkdir -p "$SANDBOX/project/.claude/collaboration"
cat > "$SANDBOX/project/.claude/collaboration/active-sessions.json" <<EOF
{
  "registryVersion": 2,
  "sessions": [
    {
      "id": "test-peer-1",
      "status": "active",
      "claudeShellPid": $LIVE_PID,
      "workDir": "$PROJ_ROOT_NORM",
      "spawnedAt": "2026-05-07T00:00:00Z",
      "dirtyFiles": ["peer-claimed.txt"]
    }
  ],
  "closed": []
}
EOF

out=$(run_oracle)
assert_valid_json "$out" "T5" || true
assert_schema_fields "$out" "T5" || true
hs=$(json_field "$out" "has_stranded")
ct=$(json_field "$out" "count")
lpc=$(json_field "$out" "live_peer_count")
# unclaimed.txt should be the sole strand
if [ "$hs" = "True" ] && [ "$ct" = "1" ] && [ "$lpc" = "1" ] && echo "$out" | grep -q '"unclaimed.txt"' && ! echo "$out" | grep -q '"peer-claimed.txt"'; then
    pass "T5: live-peer-owns-file -> peer's claimed file excluded; count=1; live_peer_count=1"
else
    fail "T5: expected count=1 live_peer_count=1 sample=[unclaimed.txt]; got hs=$hs ct=$ct lpc=$lpc out=$out"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
echo
echo "================================================================"
echo "  stranded-dirty-files validate: $PASS PASS / $FAIL FAIL"
echo "================================================================"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
exit 0
