#!/bin/bash
# pods-state oracle self-test (Unix). Sandboxed fixtures; asserts the frozen
# output schema over: inactive (no registry / null podsSession), active with a
# mixed-status roster (IN_PROGRESS, DONE synonym, BLOCKED, PENDING-unspawned),
# and channel-derived open items (ACTIVE alert; OPEN msg to LEAD counted, OPEN
# msg to a peer excluded, RESOLVED excluded; OPEN decision). Cross-shell: when
# powershell.exe is present, asserts run.ps1 output parity on the active case.

set -uo pipefail

THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_SH="$THIS_DIR/run.sh"
RUN_PS="$THIS_DIR/run.ps1"

SANDBOX="$(cd "$(mktemp -d 2>/dev/null || mktemp -d -t pods-state)" && pwd -P)"
trap 'rm -rf "$SANDBOX"' EXIT

PASS=0; FAIL=0
ok()  { echo "PASS: $1"; PASS=$((PASS+1)); }
bad() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }

# ---- Case 1: no registry -> inactive ----------------------------------------
mkdir -p "$SANDBOX/p1"
out="$(cd "$SANDBOX/p1" && bash "$RUN_SH")"
case "$out" in
    *'"active":false'*) ok "no registry -> active:false" ;;
    *) bad "no registry (got: $out)" ;;
esac

# ---- Case 2: registry with null podsSession -> inactive ----------------------
mkdir -p "$SANDBOX/p2/.claude/collaboration"
printf '{"podsSession":null,"sessions":[]}\n' > "$SANDBOX/p2/.claude/collaboration/active-sessions.json"
out="$(cd "$SANDBOX/p2" && bash "$RUN_SH")"
case "$out" in
    *'"active":false'*) ok "null podsSession -> active:false" ;;
    *) bad "null podsSession (got: $out)" ;;
esac

# ---- Case 3: active session, mixed roster + open items -----------------------
P3="$SANDBOX/p3"
SID="collab-20990101-000000"
SD="$P3/.claude/collaboration/$SID"
mkdir -p "$SD/channels" "$SD/workers/CLAUDE-A" "$SD/file-tracking"
printf '{"podsSession":"%s","sessions":[]}\n' "$SID" > "$P3/.claude/collaboration/active-sessions.json"
touch "$SD/workers/CLAUDE-A/shell.pid"

cat > "$SD/channels/status.md" <<'EOF'
# Status

## LEAD | Lead Developer
**Status**: COORDINATING

## CLAUDE-A | Builder
**Updated**: 2026-07-02 00:00
**Status**: IN_PROGRESS
**Progress**: 40%

## CLAUDE-B | Reviewer
**Status:** DONE
**Progress**: 100%

## CLAUDE-C | Blocked One
**Status**: BLOCKED
**Progress**: 10%

## CLAUDE-D | Deferred
**Status**: PENDING
EOF

cat > "$SD/channels/alerts.md" <<'EOF'
# Alerts

## ALERT-001: Live one
**From**: CLAUDE-A
**Status**: ACTIVE

## ALERT-002: Old one
**From**: CLAUDE-B
**Status**: RESOLVED
EOF

cat > "$SD/channels/messages.md" <<'EOF'
# Messages

## MSG-001: For the LD
**From**: CLAUDE-A
**To**: LEAD
**Status**: OPEN

## MSG-002: Peer-to-peer
**From**: CLAUDE-A
**To**: CLAUDE-B
**Status**: OPEN

## MSG-003: Done one
**From**: CLAUDE-B
**To**: ALL
**Status**: RESOLVED
EOF

cat > "$SD/channels/decisions.md" <<'EOF'
# Decisions

## DEC-001: Open call
**Status**: OPEN

## DEC-002: Settled
**Status**: RESOLVED
EOF

out="$(cd "$P3" && bash "$RUN_SH")"

case "$out" in *'"active":true'*) ok "active:true" ;; *) bad "active flag (got: $out)" ;; esac
case "$out" in *"\"podsSession\":\"$SID\""*) ok "podsSession id" ;; *) bad "podsSession id" ;; esac
case "$out" in *'"total":4'*) ok "total=4 (LEAD excluded)" ;; *) bad "total (got: $out)" ;; esac
case "$out" in *'"complete":1'*) ok "complete=1 (DONE synonym counted)" ;; *) bad "complete count" ;; esac
case "$out" in *'"blocked":1'*) ok "blocked=1" ;; *) bad "blocked count" ;; esac
case "$out" in *'"unspawned":1'*) ok "unspawned=1 (PENDING, no shell.pid)" ;; *) bad "unspawned count" ;; esac
case "$out" in *'"allComplete":false'*) ok "allComplete=false" ;; *) bad "allComplete" ;; esac
case "$out" in *'"id":"CLAUDE-B","role":"Reviewer","status":"DONE"'*) ok "colon-inside-bold Status form parsed" ;; *) bad "Status:** form (got: $out)" ;; esac
case "$out" in *'"progress":"40%"'*) ok "progress captured" ;; *) bad "progress" ;; esac
case "$out" in *'"alerts":["ALERT-001"]'*) ok "ACTIVE alert only" ;; *) bad "alerts (got: $out)" ;; esac
case "$out" in *'"messagesToLead":["MSG-001"]'*) ok "OPEN-to-LEAD msg only (peer + resolved excluded)" ;; *) bad "messagesToLead (got: $out)" ;; esac
case "$out" in *'"decisions":["DEC-001"]'*) ok "OPEN decision only" ;; *) bad "decisions (got: $out)" ;; esac
case "$out" in *'"monitor":{"pidFileExists":false,"pid":null,"live":false}'*) ok "no monitor.pid -> monitor absent shape" ;; *) bad "monitor absent shape (got: $out)" ;; esac

# ---- Case 4: all complete -----------------------------------------------------
sed -i.bak -e 's/^\*\*Status\*\*: IN_PROGRESS/**Status**: COMPLETE/' \
           -e 's/^\*\*Status\*\*: BLOCKED/**Status**: COMPLETED/' \
           -e 's/^\*\*Status\*\*: PENDING/**Status**: FINISHED/' "$SD/channels/status.md"
out="$(cd "$P3" && bash "$RUN_SH")"
case "$out" in *'"allComplete":true'*) ok "completion-synonym set -> allComplete:true" ;; *) bad "allComplete synonyms (got: $out)" ;; esac

# ---- Case 5: PS parity on the active mixed case -------------------------------
if command -v powershell.exe >/dev/null 2>&1; then
    mv "$SD/channels/status.md.bak" "$SD/channels/status.md"   # restore mixed roster
    out_sh="$(cd "$P3" && bash "$RUN_SH" | sed 's/"generatedAt":"[^"]*"/"generatedAt":"X"/')"
    out_ps="$(cd "$P3" && powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$RUN_PS" 2>/dev/null | tr -d '\r' | sed 's/"generatedAt":"[^"]*"/"generatedAt":"X"/')"
    if [ "$out_sh" = "$out_ps" ]; then
        ok "run.ps1 parity (byte-identical modulo generatedAt)"
    else
        bad "PS parity (sh: $out_sh | ps: $out_ps)"
    fi
else
    echo "SKIP: PS parity (no powershell.exe)"
fi

# ---- Case 6: monitor.pid visibility (DEFER-005) -------------------------------
# 6a: dead PID -> pidFileExists:true, pid captured, live:false.
( : ) & DEAD_PID=$!; wait "$DEAD_PID" 2>/dev/null
printf '%s\n' "$DEAD_PID" > "$SD/monitor.pid"
out="$(cd "$P3" && bash "$RUN_SH")"
case "$out" in
    *"\"monitor\":{\"pidFileExists\":true,\"pid\":$DEAD_PID,\"live\":false}"*) ok "dead monitor.pid -> live:false" ;;
    *) bad "dead monitor.pid (got: $out)" ;;
esac
# 6b: live PID (this shell) -> live:true (sh-namespace check; sh leg only).
printf '%s\n' "$$" > "$SD/monitor.pid"
out="$(cd "$P3" && bash "$RUN_SH")"
case "$out" in
    *"\"monitor\":{\"pidFileExists\":true,\"pid\":$$,\"live\":true}"*) ok "live monitor.pid -> live:true" ;;
    *) bad "live monitor.pid (got: $out)" ;;
esac
# 6c: garbage content -> pidFileExists:true, pid:null, live:false.
printf 'not-a-pid\n' > "$SD/monitor.pid"
out="$(cd "$P3" && bash "$RUN_SH")"
case "$out" in
    *'"monitor":{"pidFileExists":true,"pid":null,"live":false}'*) ok "garbage monitor.pid -> pid:null" ;;
    *) bad "garbage monitor.pid (got: $out)" ;;
esac
rm -f "$SD/monitor.pid"

# ---- Case 7: dict-form podsSession (DEFER-096 / DEC-2xx) ----------------------
# Every fixture above uses the STRING form, which is why the string-only
# extractor's blindness to the dict form was invisible for the life of this
# oracle (observed live: converge collab-20260804-230108 Phase 2 read
# podsSession:null against a registry that provably held a live session).
# The canonical shape is the STRING; the dict is tolerated ADDITIVELY and is
# never narrowed away, because registries are machine-local and never synced,
# so dict-form files persist on other machines indefinitely.
P7="$SANDBOX/p7"
SID7="collab-20990202-000000"
SD7="$P7/.claude/collaboration/$SID7"
mkdir -p "$SD7/channels" "$SD7/workers/CLAUDE-A"
touch "$SD7/workers/CLAUDE-A/shell.pid"
cat > "$SD7/channels/status.md" <<'EOF'
# Status

## CLAUDE-A | Builder
**Status**: IN_PROGRESS
**Progress**: 50%
EOF

# 7a: dict form, PRETTY-PRINTED across lines exactly as a live LD-written
# registry carries it (the value's id sits on a DIFFERENT line from the key, so
# any line-scoped extractor misses it even after the pattern is widened).
cat > "$P7/.claude/collaboration/active-sessions.json" <<EOF
{
    "sessions":  [],
    "podsSession":  {
                        "sessionId":  "$SID7",
                        "agents":  [
                                       {
                                           "id":  "CLAUDE-A",
                                           "role":  "Builder"
                                       }
                                   ]
                    },
    "lastUpdated":  ""
}
EOF
out="$(cd "$P7" && bash "$RUN_SH")"
case "$out" in *'"active":true'*) ok "7a dict-form podsSession -> active:true" ;; *) bad "7a dict-form active (got: $out)" ;; esac
case "$out" in *"\"podsSession\":\"$SID7\""*) ok "7a dict-form sessionId extracted" ;; *) bad "7a dict-form id (got: $out)" ;; esac

# 7b: dict form using the `id` alias csi_arc_conclude_eligibility also accepts.
cat > "$P7/.claude/collaboration/active-sessions.json" <<EOF
{"sessions":[],"podsSession":{"id":"$SID7","agents":[{"id":"CLAUDE-A","role":"Builder"}]}}
EOF
out="$(cd "$P7" && bash "$RUN_SH")"
case "$out" in *"\"podsSession\":\"$SID7\""*) ok "7b dict-form \`id\` alias extracted" ;; *) bad "7b id alias (got: $out)" ;; esac

# 7c: ANTI-VACUITY CONTROL. podsSession is null, but the registry is full of
# decoys a whole-file scan would happily pick up: a sessions[] entry carrying
# the very same `sessionId`, plus `siblingGroup`/`podsWindow` values that name
# a REAL collab directory. A widened matcher that is not bounded to the
# podsSession value itself reports active:true here -- so this cell is what
# keeps 7a/7b from passing for the wrong reason.
cat > "$P7/.claude/collaboration/active-sessions.json" <<EOF
{"sessions":[{"id":"CLAUDE-A","sessionId":"$SID7","siblingGroup":"$SID7","podsWindow":"pods-$SID7","status":"active"}],"podsSession":null}
EOF
out="$(cd "$P7" && bash "$RUN_SH")"
case "$out" in
    *'"active":false'*) ok "7c null podsSession + sessions[] decoys -> still active:false" ;;
    *) bad "7c decoy leaked into podsSession (got: $out)" ;;
esac

# 7d: dict form naming a session dir that does not exist -> inactive (the
# existing sessionDir guard still governs; tolerance widens extraction, not trust).
cat > "$P7/.claude/collaboration/active-sessions.json" <<'EOF'
{"sessions":[],"podsSession":{"sessionId":"collab-19700101-000000","agents":[]}}
EOF
out="$(cd "$P7" && bash "$RUN_SH")"
case "$out" in
    *'"active":false'*) ok "7d dict-form id with no session dir -> active:false" ;;
    *) bad "7d missing session dir (got: $out)" ;;
esac

# 7e: PS parity on the pretty-printed dict form (TWIN-01).
if command -v powershell.exe >/dev/null 2>&1; then
    cat > "$P7/.claude/collaboration/active-sessions.json" <<EOF
{
    "sessions":  [],
    "podsSession":  {
                        "sessionId":  "$SID7",
                        "agents":  [
                                       {
                                           "id":  "CLAUDE-A",
                                           "role":  "Builder"
                                       }
                                   ]
                    },
    "lastUpdated":  ""
}
EOF
    out_sh="$(cd "$P7" && bash "$RUN_SH" | sed 's/"generatedAt":"[^"]*"/"generatedAt":"X"/')"
    out_ps="$(cd "$P7" && powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$RUN_PS" 2>/dev/null | tr -d '\r' | sed 's/"generatedAt":"[^"]*"/"generatedAt":"X"/')"
    if [ "$out_sh" = "$out_ps" ]; then
        ok "7e run.ps1 parity on the dict form"
    else
        bad "7e PS parity on dict form (sh: $out_sh | ps: $out_ps)"
    fi
else
    echo "SKIP: 7e PS parity on dict form (no powershell.exe)"
fi

echo
echo "pods-state validate: $PASS pass / $FAIL fail"
[ "$FAIL" = "0" ] && exit 0 || exit 1
