#!/bin/bash
# stranded-dirty-files oracle self-test (Unix)
#
# Sandbox-builds each scenario and asserts the oracle output matches the
# FROZEN output schema (oracle.json):
#
#   T1.  no-stranded                 -> count==0, briefing==""
#   T2.  small-stranded              -> count>0, briefing references "no live owner"
#   T3.  large-stranded              -> count>=50, briefing references "significant accumulation"
#   T4.  detection-skipped-too-many  -> count==-1, briefing references "detection skipped"
#   T5.  live-peer-owns-file         -> peer's claimed file is excluded from sample, count<dirty
#   T6.  live silent peer            -> 'unavailable', sweep WITHHELD (+ two-form workDir match)
#   T7.  journal falsification pair  -> 'journal-partial'; T7b proves the filter did the work
#   T8.  STRAND-02 mtime proof       -> 'predates-peers', sweep RECOMMENDED
#   T8b. anti-vacuity                -> one unproved file holds the whole stage
#   T8c. fail-closed                 -> a peer with no spawnedAt disables the proof entirely
#   T8d. two peers                   -> the floor is the EARLIEST start, not the latest
#   T8e. anchored + anchorless peer  -> an anchored peer's floor is not used for an anchorless one
#   T9.  identity-keying caveat      -> a mis-keyed journal degrades toward withholding; T9b re-keys it
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

# ULDF_SDF_SKIP_BULK=1 skips ONLY T3 and T4, whose fixtures build 55 and 2001
# files and dominate this suite's wall clock. The mutation driver
# (scripts/smoke-tests/stranded-proof-mutations.sh) runs this suite eight times
# over mutated copies and none of its mutations can touch the bulk paths. The
# skip is loud (a SKIP line per cell) and never silent, and it cannot hide a
# failure in any other cell -- but a full run is still what a receipt cites.
SKIP_BULK="${ULDF_SDF_SKIP_BULK:-0}"

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
SCHEMA_FIELDS=(has_stranded count oldest_mtime sample live_peer_count provably_unowned attribution last_finalize_at briefing)

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
at=$(json_field "$out" "attribution")
if [ "$hs" = "True" ] && [ "$ct" = "3" ] && echo "$br" | grep -q "no live owner" \
   && [ "$at" = "no-peers" ] && echo "$br" | grep -q "for cleanup"; then
    pass "T2: small-stranded (no registry) -> count=3, attribution=no-peers, sweep still recommended"
else
    fail "T2: expected count=3 attribution=no-peers briefing matches 'no live owner'+'for cleanup'; got hs=$hs ct=$ct at=$at br='$br'"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T3. large-stranded (>=50 stranded files -> "significant accumulation" briefing)
# -----------------------------------------------------------------------------
if [ "$SKIP_BULK" = "1" ]; then
    echo "SKIP: T3 (55-file bulk fixture) -- ULDF_SDF_SKIP_BULK=1"
else
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
fi

# -----------------------------------------------------------------------------
# T4. detection-skipped-too-many (>2000 dirty files -> count==-1)
# -----------------------------------------------------------------------------
if [ "$SKIP_BULK" = "1" ]; then
    echo "SKIP: T4 (2001-file bulk fixture) -- ULDF_SDF_SKIP_BULK=1"
else
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
fi

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
at=$(json_field "$out" "attribution")
br=$(json_field "$out" "briefing")
# unclaimed.txt should be the sole strand; the published dirtyFiles array is the
# STRONG evidence source, so this is the anti-vacuity control: attribution is
# 'measured' and the sweep recommendation SURVIVES the hardening.
if [ "$hs" = "True" ] && [ "$ct" = "1" ] && [ "$lpc" = "1" ] && [ "$at" = "measured" ] \
   && echo "$out" | grep -q '"unclaimed.txt"' && ! echo "$out" | grep -q '"peer-claimed.txt"' \
   && echo "$br" | grep -q "for cleanup"; then
    pass "T5: live-peer-owns-file -> claimed file excluded; attribution=measured; sweep still recommended (anti-vacuity)"
else
    fail "T5: expected count=1 live_peer_count=1 attribution=measured sample=[unclaimed.txt] + sweep advice; got hs=$hs ct=$ct lpc=$lpc at=$at out=$out"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T6. live peer, NO attribution published (no dirtyFiles field, no journal)
# -> attribution 'unavailable'; sweep recommendation WITHHELD (DEFER-039/190).
#
# STRAND-02 / DEC-372 fixture note: the peer's spawnedAt was 2026-05-07 when this
# cell was written, i.e. AFTER the dirty files' 2026-03-15 mtime -- a peer that
# could not possibly have written them. Under the mtime-ordering proof that is
# now correctly 'predates-peers' (sweep), which would have made T6/T7/T7b assert
# the OPPOSITE of what they mean. The date is moved to 2026-01-01 so the peer
# CAN own these files, which is the situation each of these three cells is about
# (a peer that could own them and says nothing). The propositions are unchanged;
# the case where a peer could NOT own them is asserted by the new T8/T8b cells.
# The registry workDir is written in the OTHER path form when one exists
# (pwd -W on Git Bash = Windows form vs the oracle's MSYS-form pwd), which is
# the exact mismatch that made this twin report 0 live peers against 8 on the
# ULDF repo -- so this cell also pins the two-form workDir match.
# -----------------------------------------------------------------------------
mk_sandbox
mk_old_dirty "inflight-a.txt"
mk_old_dirty "inflight-b.txt"
PROJ_ROOT_NORM="$(printf '%s' "$SANDBOX/project" | tr '\\' '/' | sed 's:/*$::')"
WD_ALT="$PROJ_ROOT_NORM"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        _alt="$( (cd "$SANDBOX/project" && pwd -W) 2>/dev/null | tr '\\' '/' | sed 's:/*$::')"
        [ -n "$_alt" ] && WD_ALT="$_alt"
        ;;
esac
LIVE_PID="$$"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        win_pid=$(powershell.exe -NoProfile -Command "(Get-Process -Name explorer -ErrorAction SilentlyContinue | Select-Object -First 1).Id" 2>/dev/null | tr -d '\r')
        [ -z "$win_pid" ] && win_pid=$(powershell.exe -NoProfile -Command "(Get-Process -Name lsass -ErrorAction SilentlyContinue | Select-Object -First 1).Id" 2>/dev/null | tr -d '\r')
        [ -n "$win_pid" ] && LIVE_PID="$win_pid"
        ;;
esac
mkdir -p "$SANDBOX/project/.claude/collaboration"
cat > "$SANDBOX/project/.claude/collaboration/active-sessions.json" <<EOF
{
  "registryVersion": 2,
  "sessions": [
    {
      "sessionId": "live-peer-silent",
      "status": "active",
      "claudeShellPid": $LIVE_PID,
      "workDir": "$WD_ALT",
      "spawnedAt": "2026-01-01T00:00:00Z"
    }
  ],
  "closed": []
}
EOF
out=$(run_oracle)
assert_valid_json "$out" "T6" || true
ct=$(json_field "$out" "count")
lpc=$(json_field "$out" "live_peer_count")
at=$(json_field "$out" "attribution")
br=$(json_field "$out" "briefing")
if [ "$ct" = "2" ] && [ "$lpc" = "1" ] && [ "$at" = "unavailable" ] \
   && echo "$br" | grep -q "OWNERSHIP UNKNOWN" && ! echo "$br" | grep -q "for cleanup"; then
    pass "T6: live silent peer (alt-form workDir) -> attribution=unavailable; sweep WITHHELD"
else
    fail "T6: expected count=2 live_peer_count=1 attribution=unavailable briefing=OWNERSHIP UNKNOWN w/o sweep advice; got ct=$ct lpc=$lpc at=$at br='$br'"
fi
# Keep this sandbox for T7 (same peer gains a write journal).

# -----------------------------------------------------------------------------
# T7. FALSIFICATION PAIR (port of GitCellar selftest.ps1, DEFER-039):
# the peer's write journal claims exactly ONE of the two old dirty files.
# Expect: that one and ONLY that one is filtered; attribution 'journal-partial';
# sweep still withheld. Then DELETE the journal and expect both files back and
# the grade to drop to 'unavailable' -- which proves the filter was doing the
# work rather than the arithmetic happening to agree.
# -----------------------------------------------------------------------------
mkdir -p "$SANDBOX/project/.claude/session-state/write-journal"
printf '{"ts":"2026-03-01T00:00:00Z","path":"%s/inflight-a.txt","op":"edit"}\n' "$PROJ_ROOT_NORM" \
    > "$SANDBOX/project/.claude/session-state/write-journal/live-peer-silent.jsonl"
out=$(run_oracle)
ct=$(json_field "$out" "count")
at=$(json_field "$out" "attribution")
br=$(json_field "$out" "briefing")
if [ "$ct" = "1" ] && [ "$at" = "journal-partial" ] \
   && ! echo "$out" | grep -q '"inflight-a.txt"' && echo "$out" | grep -q '"inflight-b.txt"' \
   && echo "$br" | grep -q "OWNERSHIP UNPROVEN" && ! echo "$br" | grep -q "for cleanup"; then
    pass "T7: journal HIT filters exactly its file; attribution=journal-partial; sweep still WITHHELD"
else
    fail "T7: expected count=1 (inflight-b only) attribution=journal-partial briefing=OWNERSHIP UNPROVEN; got ct=$ct at=$at out=$out"
fi
rm -f "$SANDBOX/project/.claude/session-state/write-journal/live-peer-silent.jsonl"
out=$(run_oracle)
ct=$(json_field "$out" "count")
at=$(json_field "$out" "attribution")
if [ "$ct" = "2" ] && [ "$at" = "unavailable" ]; then
    pass "T7b: journal deleted -> BOTH files stranded again + grade drops to unavailable (filter was doing the work)"
else
    fail "T7b: expected count=2 attribution=unavailable after journal deletion; got ct=$ct at=$at"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T8 group. STRAND-02 / DEC-372 -- the mtime-ordering proof.
#
# Shared timeline for the group (the seed commit at 2026-04-01 is the strand
# boundary, so every candidate must be older than that):
#   2026-01-01  a peer that COULD own the files
#   2026-03-15  inflight-a / inflight-b written
#   2026-03-20  a peer that could NOT have written them
#   2026-03-25  inflight-c written  (after the 03-20 peer's start)
#   2026-04-01  seed commit == strand boundary
# -----------------------------------------------------------------------------
mk_dirty_at() {   # mk_dirty_at <path> <touch -t stamp>
    local p="$1" stamp="$2"
    mkdir -p "$(dirname "$SANDBOX/project/$p")"
    printf 'x\n' > "$SANDBOX/project/$p"
    touch -t "$stamp" "$SANDBOX/project/$p" 2>/dev/null || true
}

sdf_live_pid() {
    local lp="$$"
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*)
            local wp
            wp=$(powershell.exe -NoProfile -Command "(Get-Process -Name explorer -ErrorAction SilentlyContinue | Select-Object -First 1).Id" 2>/dev/null | tr -d '\r')
            [ -z "$wp" ] && wp=$(powershell.exe -NoProfile -Command "(Get-Process -Name lsass -ErrorAction SilentlyContinue | Select-Object -First 1).Id" 2>/dev/null | tr -d '\r')
            [ -n "$wp" ] && lp="$wp"
            ;;
    esac
    printf '%s' "$lp"
}

write_peer_registry() {  # write_peer_registry <sessionId> <pid> <workDir> <spawnedAt-or-OMIT>
    mkdir -p "$SANDBOX/project/.claude/collaboration"
    if [ "$4" = "OMIT" ]; then
        cat > "$SANDBOX/project/.claude/collaboration/active-sessions.json" <<EOF
{"registryVersion":2,"sessions":[{"sessionId":"$1","status":"active","claudeShellPid":$2,"workDir":"$3"}],"closed":[]}
EOF
    else
        cat > "$SANDBOX/project/.claude/collaboration/active-sessions.json" <<EOF
{"registryVersion":2,"sessions":[{"sessionId":"$1","status":"active","claudeShellPid":$2,"workDir":"$3","spawnedAt":"$4"}],"closed":[]}
EOF
    fi
}

mk_sandbox
mk_dirty_at "inflight-a.txt" 202603150000
mk_dirty_at "inflight-b.txt" 202603150000
PROJ_ROOT_NORM="$(printf '%s' "$SANDBOX/project" | tr '\\' '/' | sed 's:/*$::')"
WD_ALT="$PROJ_ROOT_NORM"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        _alt="$( (cd "$SANDBOX/project" && pwd -W) 2>/dev/null | tr '\\' '/' | sed 's:/*$::')"
        [ -n "$_alt" ] && WD_ALT="$_alt"
        ;;
esac
LIVE_PID="$(sdf_live_pid)"

# T8: the live peer started AFTER both files were written, publishes nothing and
# has no journal -- exactly T6's fixture but for the one fact that decides it.
# Pre-STRAND-02 this graded 'unavailable' and withheld forever; it is the
# permanently-unreachable success state DEFER-193 was filed about.
write_peer_registry "live-peer-late" "$LIVE_PID" "$WD_ALT" "2026-03-20T00:00:00Z"
out=$(run_oracle)
assert_valid_json "$out" "T8" || true
assert_schema_fields "$out" "T8" || true
ct=$(json_field "$out" "count"); at=$(json_field "$out" "attribution")
lpc=$(json_field "$out" "live_peer_count"); pu=$(json_field "$out" "provably_unowned")
br=$(json_field "$out" "briefing")
if [ "$ct" = "2" ] && [ "$lpc" = "1" ] && [ "$at" = "predates-peers" ] && [ "$pu" = "2" ] \
   && echo "$br" | grep -q "for cleanup"; then
    pass "T8: live peer started AFTER the files -> attribution=predates-peers, provably_unowned=2, sweep RECOMMENDED"
else
    fail "T8: expected count=2 live_peer_count=1 attribution=predates-peers provably_unowned=2 + sweep advice; got ct=$ct lpc=$lpc at=$at pu=$pu br='$br'"
fi

# T8b ANTI-VACUITY + caveat (a), the tool-calls-only floor: the same live peer
# writes a third file by a route the write journal cannot see (a shell redirect
# leaves no {ts,path,op} record). Its mtime lands AFTER the peer's start, so the
# proof declines it -- and one unproved file must hold the WHOLE stage, because
# the sweep it gates is all-or-nothing. Without this cell, "always grade
# predates-peers" passes T8 perfectly.
mk_dirty_at "inflight-c.txt" 202603250000
out=$(run_oracle)
ct=$(json_field "$out" "count"); at=$(json_field "$out" "attribution")
pu=$(json_field "$out" "provably_unowned"); br=$(json_field "$out" "briefing")
if [ "$ct" = "3" ] && [ "$at" = "unavailable" ] && [ "$pu" = "2" ] \
   && ! echo "$br" | grep -q "for cleanup" && echo "$br" | grep -q "2 of 3"; then
    pass "T8b: one journal-invisible file written after the peer's start -> grade drops to unavailable, sweep WITHHELD, 2 of 3 still reported as proved"
else
    fail "T8b: expected count=3 attribution=unavailable provably_unowned=2 + '2 of 3' clause + no sweep advice; got ct=$ct at=$at pu=$pu br='$br'"
fi

# T8c FAIL-CLOSED: the same all-predating fixture as T8, but the peer's registry
# entry carries NO spawnedAt. The proof has no floor, so it must be unavailable
# for the whole run -- NOT silently skipped for that peer, which would raise the
# floor and prove MORE.
rm -f "$SANDBOX/project/inflight-c.txt"
write_peer_registry "live-peer-anchorless" "$LIVE_PID" "$WD_ALT" "OMIT"
out=$(run_oracle)
ct=$(json_field "$out" "count"); at=$(json_field "$out" "attribution")
pu=$(json_field "$out" "provably_unowned"); br=$(json_field "$out" "briefing")
if [ "$ct" = "2" ] && [ "$at" = "unavailable" ] && [ "$pu" = "0" ] \
   && ! echo "$br" | grep -q "for cleanup" && ! echo "$br" | grep -q "predate every live peer"; then
    pass "T8c: live peer with NO spawnedAt -> proof unavailable for the whole run, sweep WITHHELD, no proved-count clause"
else
    fail "T8c: expected count=2 attribution=unavailable provably_unowned=0 and no proved-count clause; got ct=$ct at=$at pu=$pu br='$br'"
fi

# T8d: TWO live peers, one that COULD have written the files (started 2026-01-01)
# and one that could not (2026-03-20). The floor must be the EARLIEST start, so
# the proof does not apply and the sweep is withheld. Without this cell a
# max()-over-peers implementation is indistinguishable from min() -- every other
# cell in the group has exactly one peer.
mkdir -p "$SANDBOX/project/.claude/collaboration"
cat > "$SANDBOX/project/.claude/collaboration/active-sessions.json" <<EOF
{"registryVersion":2,"sessions":[
 {"sessionId":"peer-early","status":"active","claudeShellPid":$LIVE_PID,"workDir":"$WD_ALT","spawnedAt":"2026-01-01T00:00:00Z"},
 {"sessionId":"peer-late","status":"active","claudeShellPid":$LIVE_PID,"workDir":"$WD_ALT","spawnedAt":"2026-03-20T00:00:00Z"}
],"closed":[]}
EOF
out=$(run_oracle)
ct=$(json_field "$out" "count"); at=$(json_field "$out" "attribution")
lpc=$(json_field "$out" "live_peer_count"); pu=$(json_field "$out" "provably_unowned")
if [ "$ct" = "2" ] && [ "$lpc" = "2" ] && [ "$at" = "unavailable" ] && [ "$pu" = "0" ]; then
    pass "T8d: two live peers, floor is the EARLIEST start -> proof does not apply, sweep WITHHELD (a max()-over-peers floor would sweep here)"
else
    fail "T8d: expected count=2 live_peer_count=2 attribution=unavailable provably_unowned=0; got ct=$ct lpc=$lpc at=$at pu=$pu"
fi

# T8e: the fail-closed cell T8c CANNOT be. Found by running mutation M3, not by
# reading the code: M3 (an anchorless peer silently skipped instead of disabling
# the proof) SURVIVED T8c, because with ONE anchorless peer the earliest-start
# floor is empty either way and the `-n "$EARLIEST_PEER_START"` guard withholds
# regardless. The mutation is only observable when a peer WITH an anchor sets a
# floor that an anchorless peer would then be measured against -- so the fixture
# needs both kinds at once.
cat > "$SANDBOX/project/.claude/collaboration/active-sessions.json" <<EOF
{"registryVersion":2,"sessions":[
 {"sessionId":"peer-anchored","status":"active","claudeShellPid":$LIVE_PID,"workDir":"$WD_ALT","spawnedAt":"2026-03-20T00:00:00Z"},
 {"sessionId":"peer-anchorless","status":"active","claudeShellPid":$LIVE_PID,"workDir":"$WD_ALT"}
],"closed":[]}
EOF
out=$(run_oracle)
ct=$(json_field "$out" "count"); at=$(json_field "$out" "attribution")
lpc=$(json_field "$out" "live_peer_count"); pu=$(json_field "$out" "provably_unowned")
if [ "$ct" = "2" ] && [ "$lpc" = "2" ] && [ "$at" = "unavailable" ] && [ "$pu" = "0" ]; then
    pass "T8e: one anchored + one ANCHORLESS live peer -> the anchored peer's floor must NOT be used; whole run withholds (kills the silent-skip mutation T8c cannot see)"
else
    fail "T8e: expected count=2 live_peer_count=2 attribution=unavailable provably_unowned=0; got ct=$ct lpc=$lpc at=$at pu=$pu"
fi
cleanup; SANDBOX=""

# -----------------------------------------------------------------------------
# T9 group. Caveat (b) -- the identity-keying asymmetry (FINALIZE-SCOPE-11 /
# DEC-337: a session's journal can be keyed differently from the id a reader
# looks it up by, which produced 12 false "foreign co-writer" flags when it bit).
# The property asserted here is the FAILURE DIRECTION: a journal this oracle
# cannot find degrades the verdict toward withholding, never toward the sweep.
# -----------------------------------------------------------------------------
mk_sandbox
mk_dirty_at "inflight-a.txt" 202603150000
mk_dirty_at "inflight-b.txt" 202603150000
PROJ_ROOT_NORM="$(printf '%s' "$SANDBOX/project" | tr '\\' '/' | sed 's:/*$::')"
WD_ALT="$PROJ_ROOT_NORM"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        _alt="$( (cd "$SANDBOX/project" && pwd -W) 2>/dev/null | tr '\\' '/' | sed 's:/*$::')"
        [ -n "$_alt" ] && WD_ALT="$_alt"
        ;;
esac
LIVE_PID="$(sdf_live_pid)"
# Peer started 2026-01-01: it COULD have written both files, so the mtime proof
# does not apply and the journal is the only possible evidence.
write_peer_registry "registry-id" "$LIVE_PID" "$WD_ALT" "2026-01-01T00:00:00Z"
mkdir -p "$SANDBOX/project/.claude/session-state/write-journal"
printf '{"ts":"2026-03-01T00:00:00Z","path":"%s/inflight-a.txt","op":"edit"}\n' "$PROJ_ROOT_NORM" \
    > "$SANDBOX/project/.claude/session-state/write-journal/interactive-shadow-id.jsonl"
out=$(run_oracle)
ct=$(json_field "$out" "count"); at=$(json_field "$out" "attribution")
if [ "$ct" = "2" ] && [ "$at" = "unavailable" ]; then
    pass "T9: journal keyed under a DIFFERENT id than the registry sessionId -> lookup misses, grade degrades to unavailable (withhold), never to a sweep"
else
    fail "T9: expected count=2 attribution=unavailable with a mis-keyed journal; got ct=$ct at=$at out=$out"
fi
# Same bytes, correct key: the file is filtered and the grade becomes
# journal-partial -- which proves T9 measured the KEY and not the journal's
# absence.
mv "$SANDBOX/project/.claude/session-state/write-journal/interactive-shadow-id.jsonl" \
   "$SANDBOX/project/.claude/session-state/write-journal/registry-id.jsonl" 2>/dev/null
out=$(run_oracle)
ct=$(json_field "$out" "count"); at=$(json_field "$out" "attribution")
if [ "$ct" = "1" ] && [ "$at" = "journal-partial" ] && ! echo "$out" | grep -q '"inflight-a.txt"'; then
    pass "T9b: identical journal under the registry's own id -> file filtered, grade journal-partial (T9 measured the key, not the file)"
else
    fail "T9b: expected count=1 attribution=journal-partial after re-keying the journal; got ct=$ct at=$at out=$out"
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
