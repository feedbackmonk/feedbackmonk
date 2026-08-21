#!/bin/bash
# pods-state oracle (Unix)
#
# Scrutiny 05 ADD-3: answers "is a PODS session active, who is on the roster,
# what is each worker's status, any open alerts/messages-to-LEAD/decisions?"
# in one deterministic call, replacing the 4-file agentic re-derivation the LD,
# converge Phase 2, and collab-sync each performed per consult.
#
# Sources (cwd-relative, like sibling oracles):
#   .claude/collaboration/active-sessions.json   -> top-level podsSession id
#   .claude/collaboration/<id>/channels/status.md    -> per-agent status
#   .claude/collaboration/<id>/channels/alerts.md    -> ACTIVE alerts
#   .claude/collaboration/<id>/channels/messages.md  -> OPEN msgs to LEAD/ALL
#   .claude/collaboration/<id>/channels/decisions.md -> OPEN decisions
#
# PARSING PARITY CONTRACT: the status.md agent parser and the channel scanners
# are behavior-locked to monitor-pods.sh parse_snapshot()/scan_channel_file()
# (heading regex '## CLAUDE-X | Role', tolerant '**Status**:'/'**Status:**'
# forms, upper-cased values, completion synonyms COMPLETE|COMPLETED|DONE|
# FINISHED, spawned = shell.pid OR status != PENDING, To: LEAD/ALL matching).
# A change to the monitor's parser changes this oracle in the same commit.
#
# Output: single-line JSON; frozen schema documented in README.md.
# READ-ONLY. Gracefully absent: no registry / no podsSession / no session dir
# -> {"active": false, ...}. No JSON parser required (sed/awk only).

set +e

REGISTRY=".claude/collaboration/active-sessions.json"
NOW="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

emit_inactive() {
    printf '{"active":false,"podsSession":null,"sessionDir":null,"agents":[],"counts":{"total":0,"complete":0,"blocked":0,"unspawned":0},"allComplete":false,"rosterMissing":[],"open":{"alerts":[],"messagesToLead":[],"decisions":[]},"monitor":{"pidFileExists":false,"pid":null,"live":false},"generatedAt":"%s"}\n' "$NOW"
    exit 0
}

[ -f "$REGISTRY" ] || emit_inactive

# ---- podsSession id extraction (DEFER-096) ----------------------------------
# CANONICAL shape is the STRING `"podsSession": "collab-..."` -- what
# segments/-pods/parallelize_session.md prescribes and what ten of this field's
# eleven readers parse. The DICT shape `{"sessionId": ..., "agents": [...]}` is
# tolerated ADDITIVELY and is never narrowed away: registries are machine-local
# and are never synced or committed, so dict-form files written before this fix
# persist on other machines indefinitely.
#
# Bounded by construction. The id is read ONLY from inside the podsSession
# value: the walk below tracks brace depth (skipping string literals so a brace
# inside a workDir or role can't unbalance it) and stops at the object's own
# closing brace. That bound is load-bearing, not decoration -- a sessions[]
# entry carries `sessionId`, `siblingGroup` and `podsWindow` values that name
# REAL collab ids, so an unbounded scan would happily report a *different*
# live session's collab as this project's active one (validate.{sh,ps1} cell 7c).
# Whole-file read (not line-scoped) because a live registry is pretty-printed:
# the key and its `sessionId` sit on different lines.
#
# Preference order inside the object: `sessionId`, then `id` -- the same alias
# pair csi_arc_conclude_eligibility accepts. The `id` fallback is taken only
# from the region BEFORE any `agents` key, so it can never pick up an agent's
# own `id`. Still sed/awk only: no JSON-parser dependency (both twins).
PODS_ID="$(awk '
    { buf = buf $0 "\n" }
    END {
        key = "\"podsSession\""
        klen = length(key)
        pos = 1
        # First occurrence of the KEY that is actually followed by a colon --
        # a value string may legitimately contain the bare word podsSession.
        while (1) {
            n = index(substr(buf, pos), key)
            if (n == 0) exit 0
            n = pos + n - 1
            rest = substr(buf, n + klen)
            if (match(rest, /^[ \t\r\n]*:/)) break
            pos = n + klen
        }
        sub(/^[ \t\r\n]*:[ \t\r\n]*/, "", rest)
        first = substr(rest, 1, 1)

        # --- canonical: string form ---
        if (first == "\"") {
            if (match(rest, /^"[^"]*"/)) print substr(rest, 2, RLENGTH - 2)
            exit 0
        }
        # null / number / array -> no id (inactive), same as before.
        if (first != "{") exit 0

        # --- legacy: dict form. Isolate the object by brace depth. ---
        L = length(rest); depth = 0; i = 1
        while (i <= L) {
            ch = substr(rest, i, 1)
            if (ch == "\"") {                 # skip a string literal wholesale
                j = i + 1
                while (j <= L) {
                    cj = substr(rest, j, 1)
                    if (cj == "\\") { j = j + 2; continue }
                    if (cj == "\"") break
                    j = j + 1
                }
                i = j + 1
                continue
            }
            if (ch == "{") depth = depth + 1
            else if (ch == "}") {
                depth = depth - 1
                if (depth == 0) { i = i + 1; break }
            }
            i = i + 1
        }
        obj = substr(rest, 1, i - 1)

        if (match(obj, /"sessionId"[ \t\r\n]*:[ \t\r\n]*"[^"]*"/)) {
            v = substr(obj, RSTART, RLENGTH)
            if (match(v, /"[^"]*"$/)) { print substr(v, RSTART + 1, RLENGTH - 2); exit 0 }
        }
        head = obj
        a = index(obj, "\"agents\"")
        if (a > 0) head = substr(obj, 1, a - 1)
        if (match(head, /"id"[ \t\r\n]*:[ \t\r\n]*"[^"]*"/)) {
            v = substr(head, RSTART, RLENGTH)
            if (match(v, /"[^"]*"$/)) { print substr(v, RSTART + 1, RLENGTH - 2); exit 0 }
        }
    }
' "$REGISTRY" 2>/dev/null | head -n 1)"
[ -n "$PODS_ID" ] || emit_inactive

SESSION_DIR=".claude/collaboration/$PODS_ID"
[ -d "$SESSION_DIR" ] || emit_inactive
STATUS_FILE="$SESSION_DIR/channels/status.md"
WORKERS_DIR="$SESSION_DIR/workers"

# ---- Per-agent rows: <id>\t<status>\t<spawned 0|1>\t<progress>\t<role> ------
# monitor-pods.sh parse_snapshot parity (+ a Progress capture the monitor does
# not need). Missing status file -> empty roster (still active session).
# Empty fields carry a \001 sentinel: bash `read` with tab IFS collapses
# consecutive tabs (IFS whitespace), which would shift the columns.
parse_agents() {
    [ -f "$STATUS_FILE" ] || return 0
    awk -v wd="$WORKERS_DIR" -v OFS='\t' '
      function nz(v) { return (v == "" ? "\001" : v) }
      function flush() {
        if (id != "") print id, nz(status), spawned, nz(progress), nz(role)
      }
      # LEAD FIRST, AND THIS ORDERING IS NOW LOAD-BEARING (it was not before).
      # The widened id rule below matches `## LEAD | Coordinator`, which the old
      # CLAUDE--anchored rule could not -- so the LEAD exclusion has to run first
      # or the coordinator joins the completable worker set and allComplete is
      # false forever. monitor-pods.sh carries the identical rule in the identical
      # position, for the identical reason (DISC-MON-09).
      /^## LEAD([ \t]|\|)/{ flush(); id=""; next }
      # DISC-MON-09/DISC-MON-10 PARITY (DEC-306). This used to be
      # /^## CLAUDE-[A-Z0-9_-]+ \|/ while monitor-pods.sh had already been widened
      # to any bounded id token -- so this oracle was BLIND to every PODS session
      # whose roster is not CLAUDE-*, which is every wave-6..9 roster (W9-A, W9-B,
      # ...). Measured 2026-08-06 with a control (NO APOSTROPHES IN THIS AWK BODY:
      # they terminate the single-quoted program -- monitor-pods.sh carries the
      # same warning and this comment still tripped over it once):
      #     heading "## W9-B -- <ts>"           -> agents:[]  (no " | role" either)
      #     heading "## W9-B | Rest claims"     -> agents:[]  <-- CLAUDE- prefix alone
      #     heading "## CLAUDE-A | Rest claims" -> agents:[{...}]  <-- control passes
      # and live on this tree: agents:[], total:0, with four workers alive.
      # The README for this oracle says the two parsers are behavior-locked and
      # must not drift. They had drifted; this is the re-lock, so keep the two
      # regexes identical when either moves. LEAD is excluded by NAME above,
      # never by id shape.
      /^## [A-Za-z][A-Za-z0-9_-]* \|/{
        flush()
        match($0, /[A-Za-z][A-Za-z0-9_-]*/); id=substr($0, RSTART, RLENGTH)
        role=$0; sub(/^## [A-Za-z][A-Za-z0-9_-]* \| /, "", role); sub(/[ \t\r]+$/, "", role)
        status=""; progress=""; spawned=0
        cmd="test -f \"" wd "/" id "/shell.pid\" && echo 1 || echo 0"
        cmd | getline has_pid; close(cmd)
        if (has_pid=="1") spawned=1
        next
      }
      id && /^\*\*Status(\*\*:|:\*\*)/{
        s=$0; sub(/.*\*\*Status(\*\*:|:\*\*)[ \t]*/, "", s); sub(/[ \t\r].*/, "", s)
        status=toupper(s)
        if (status != "PENDING") spawned=1
      }
      id && /^\*\*Progress(\*\*:|:\*\*)/{
        s=$0; sub(/.*\*\*Progress(\*\*:|:\*\*)[ \t]*/, "", s); sub(/[ \t\r]+$/, "", s)
        progress=s
      }
      END { flush() }
    ' "$STATUS_FILE"
}

# ---- Channel scan: one id per line (monitor-pods.sh scan_channel_file parity)
#   $1=file  $2=heading prefix (ALERT|MSG|DEC)  $3=firing status  $4=require To LEAD/ALL
scan_channel() {
    local f="$1" hp="$2" fire="$3" need_to="$4"
    [ -f "$f" ] || return 0
    awk -v hp="$hp" -v fire="$fire" -v need_to="$need_to" '
      function flush() {
        if (id == "") return
        if (toupper(st) != fire) { id=""; return }
        if (need_to == "1") {
          t = toupper(to)
          if (t !~ /LEAD/ && t !~ /(^|[^A-Z])ALL([^A-Z]|$)/) { id=""; return }
        }
        print id
        id=""
      }
      $0 ~ "^## " hp "-[A-Za-z0-9_-]+:" {
        flush()
        match($0, hp "-[A-Za-z0-9_-]+"); id=substr($0, RSTART, RLENGTH)
        to=""; st=""
        next
      }
      /^## /{ flush(); next }
      id != "" && /^\*\*To(\*\*:|:\*\*)/{
        s=$0; sub(/.*\*\*To(\*\*:|:\*\*)[ \t]*/, "", s); sub(/[ \t\r]+$/, "", s); to=s
      }
      id != "" && /^\*\*Status(\*\*:|:\*\*)/{
        s=$0; sub(/.*\*\*Status(\*\*:|:\*\*)[ \t]*/, "", s); sub(/[ \t\r].*/, "", s); st=s
      }
      END { flush() }
    ' "$f"
}

ids_to_json() { # newline-separated ids -> "a","b" (empty -> empty string)
    local out="" line
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        if [ -z "$out" ]; then out="\"$(esc "$line")\""; else out="$out,\"$(esc "$line")\""; fi
    done
    printf '%s' "$out"
}

# ---- Aggregate ---------------------------------------------------------------
TOTAL=0; COMPLETE=0; BLOCKED=0; UNSPAWNED=0
AGENTS_JSON=""
PARSED_IDS=""
while IFS=$'\t' read -r id status spawned progress role; do
    [ -n "$id" ] || continue
    PARSED_IDS="$PARSED_IDS $id "
    [ "$status" = $'\001' ] && status=""
    [ "$progress" = $'\001' ] && progress=""
    [ "$role" = $'\001' ] && role=""
    TOTAL=$((TOTAL + 1))
    is_complete="false"
    case "$status" in COMPLETE|COMPLETED|DONE|FINISHED) is_complete="true"; COMPLETE=$((COMPLETE + 1)) ;; esac
    [ "$status" = "BLOCKED" ] && BLOCKED=$((BLOCKED + 1))
    sp="false"; [ "$spawned" = "1" ] && sp="true"
    [ "$spawned" = "1" ] || UNSPAWNED=$((UNSPAWNED + 1))
    st_json="null"; [ -n "$status" ] && st_json="\"$(esc "$status")\""
    pg_json="null"; [ -n "$progress" ] && pg_json="\"$(esc "$progress")\""
    rec="{\"id\":\"$(esc "$id")\",\"role\":\"$(esc "$role")\",\"status\":$st_json,\"progress\":$pg_json,\"isComplete\":$is_complete,\"spawned\":$sp}"
    if [ -z "$AGENTS_JSON" ]; then AGENTS_JSON="$rec"; else AGENTS_JSON="$AGENTS_JSON,$rec"; fi
done <<EOF
$(parse_agents)
EOF

# ---- PODSSTATUS-08 (DEC-348): roster provenance -----------------------------
# `allComplete` was `COMPLETE == TOTAL`, both from the status parser above -- the
# same shared-source defect DEC-348 fixes in monitor-pods, in the sibling surface
# this oracle's own provenance note declares parity-locked to it. A parser that
# sees fewer agents than exist shrinks the denominator with the numerator and
# still reports unanimity.
#
# workers/<AGENT>/ is the independent enumeration (the spawn surface's own
# artifact). LEAD is excluded BY NAME, never by pattern -- it is COORDINATING,
# never COMPLETE (DISC-MON-09), and counting it would make allComplete false
# forever.
#
# `rosterMissing` is emitted ALWAYS and is informative, not an alarm: before a
# worker's first status publish it legitimately names who has not checked in yet
# (since DEC-306 status.md is DERIVED and starts empty). It gates allComplete
# only in the one state where the wrong answer is acted on.
ROSTER_MISSING_JSON=""
if [ -d "$WORKERS_DIR" ]; then
    for _wd in "$WORKERS_DIR"/*/; do
        [ -d "$_wd" ] || continue
        _wid="${_wd%/}"; _wid="${_wid##*/}"
        [ "$_wid" = "LEAD" ] && continue
        case "$PARSED_IDS" in *" $_wid "*) continue ;; esac
        if [ -z "$ROSTER_MISSING_JSON" ]; then ROSTER_MISSING_JSON="\"$(esc "$_wid")\""
        else ROSTER_MISSING_JSON="$ROSTER_MISSING_JSON,\"$(esc "$_wid")\""; fi
    done
fi

ALL_COMPLETE="false"
[ "$TOTAL" -gt 0 ] && [ "$COMPLETE" -eq "$TOTAL" ] && [ -z "$ROSTER_MISSING_JSON" ] && ALL_COMPLETE="true"

ALERTS_JSON="$(scan_channel "$SESSION_DIR/channels/alerts.md"    "ALERT" "ACTIVE" ""  | ids_to_json)"
MSGS_JSON="$(scan_channel   "$SESSION_DIR/channels/messages.md"  "MSG"   "OPEN"   "1" | ids_to_json)"
DECS_JSON="$(scan_channel   "$SESSION_DIR/channels/decisions.md" "DEC"   "OPEN"   ""  | ids_to_json)"

# ---- Monitor visibility (DEFER-005): <session>/monitor.pid singleton record --
# live is best-effort process-existence (advisory; the authoritative duplicate
# defense is the monitor's own startup singleton check). On Windows the pid is
# a native PID written by monitor-pods.ps1 — run.ps1 is the primary reader.
MON_PID_FILE="$SESSION_DIR/monitor.pid"
MON_EXISTS="false"; MON_PID="null"; MON_LIVE="false"
if [ -f "$MON_PID_FILE" ]; then
    MON_EXISTS="true"
    mp="$(head -n1 "$MON_PID_FILE" 2>/dev/null | tr -d '[:space:]')"
    case "$mp" in
        ''|*[!0-9]*) : ;;
        *) MON_PID="$mp"
           kill -0 "$mp" 2>/dev/null && MON_LIVE="true" ;;
    esac
fi

printf '{"active":true,"podsSession":"%s","sessionDir":"%s","agents":[%s],"counts":{"total":%d,"complete":%d,"blocked":%d,"unspawned":%d},"allComplete":%s,"rosterMissing":[%s],"open":{"alerts":[%s],"messagesToLead":[%s],"decisions":[%s]},"monitor":{"pidFileExists":%s,"pid":%s,"live":%s},"generatedAt":"%s"}\n' \
    "$(esc "$PODS_ID")" "$(esc "$SESSION_DIR")" "$AGENTS_JSON" \
    "$TOTAL" "$COMPLETE" "$BLOCKED" "$UNSPAWNED" "$ALL_COMPLETE" "$ROSTER_MISSING_JSON" \
    "$ALERTS_JSON" "$MSGS_JSON" "$DECS_JSON" \
    "$MON_EXISTS" "$MON_PID" "$MON_LIVE" "$NOW"
exit 0
