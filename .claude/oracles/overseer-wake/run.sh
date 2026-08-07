#!/bin/bash
# overseer-wake oracle (Unix) -- CSO Phase 1, component C1 (the cheap mechanic).
#
# Question: is there anything in the airspace worth waking the LLM Overseer for?
#
# This is the cost defense of CSO (CSO-09 / Q-CSO-01): a PURE-SHELL deterministic
# aggregator over the four already-deterministic detectors. It does the routine
# watching at ~0 LLM tokens so the expensive LLM Overseer is woken ONLY when this
# oracle raises wake:true -- never on a hot always-on poll (the 15x multi-agent
# cost trap, Anthropic). NO LLM is in this path.
#
# READ-ONLY by contract: never writes. Aggregates four sources, each gracefully
# absent (a missing source is NO-DATA -- omitted -- NEVER a fabricated all-clear;
# wake fires only on positive evidence):
#   (d) stall          : active worker entries in the CSI registry whose
#                        claudeShellPid is DEAD (one-shot version of the DEC-69
#                        pods-monitor PID-dead/terminal signal)
#   (a) concurrent-mut : the sibling concurrent-mutation oracle's external_mutation
#   (b) shared-foreign : another live session's sharedClaim on a shared-repo file
#   (c) touches-screen : a PODS touches.json path claimed by >=2 distinct sessions
#                        (a cheap pre-screen; the FULL line-overlap classification
#                        is the Overseer-run C8 pass, Q-CSO-04)
#
# Output: single-line JSON, CSO domain schema (mirrors the briefing-oracle
# convention -- domain fields + a briefing field, not the status/details envelope):
#   {wake:bool, signals:[{detectorId,severity,sessions:[...],summary}], summary, briefing}
# briefing is empty when wake=false (the session-start hook then emits no line --
# gracefully absent, same convention as concurrent-mutation / stale-ltads-state).

set +e

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# ---- Field-safe TSV split (DEFER-078) ---------------------------------------
# `IFS=$'\t' read -r a b c` DROPS an empty NON-FINAL field: tab is IFS
# *whitespace*, so a run of tabs collapses to one delimiter and every later
# field shifts left. Both row producers in this file default `id` (and one of
# them `claudeShellPid`) to "" -- announcing that absence is expected -- and the
# collapsing read defeated that: an entry with no `id` silently vanished from
# the stall-monitor scan, and in the shared-claim leg the liveness probe was
# handed a FILE COUNT instead of a pid. Splitting on literal tabs preserves
# empties. bash-3.2 safe (no arrays / readarray -d / namerefs).
# `${r#*<tab>}` returns the whole string when no tab remains, so strip only
# while one does. Sets OW_F1..OW_F3, empty for absent fields, as `read` would.
_ow_split_tsv() {
    local _r="$1" _t
    _t=$(printf '\t')
    case "$_r" in (*"$_t"*) OW_F1="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) OW_F1="$_r"; _r="" ;; esac
    case "$_r" in (*"$_t"*) OW_F2="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) OW_F2="$_r"; _r="" ;; esac
    case "$_r" in (*"$_t"*) OW_F3="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) OW_F3="$_r"; _r="" ;; esac
    OW_F4="$_r"
}

# Bound a sibling-oracle invocation so a slow child cannot drag this aggregator
# past the fast-lane budget on the session-start briefing path: a source that
# exceeds the bound degrades to NO-DATA (graceful), never blocks. Uses coreutils
# `timeout` when present (Git Bash on win32 -- the slow-probe env -- ships it);
# falls back to a direct call where absent (macOS without gtimeout etc.).
_TIMEOUT=""
if command -v timeout >/dev/null 2>&1; then _TIMEOUT="timeout";
elif command -v gtimeout >/dev/null 2>&1; then _TIMEOUT="gtimeout"; fi
bounded() {  # bounded <seconds> <cmd...>
    local secs="$1"; shift
    if [ -n "$_TIMEOUT" ]; then "$_TIMEOUT" "${secs}s" "$@" 2>/dev/null; else "$@" 2>/dev/null; fi
}

_OW_PWD="$(pwd)"
REGISTRY=""
# (legacy ltads/sessions/active-sessions.json fallback retired -- dead per DEC-124/Arc 1 DEC-198)
if [ -f ".claude/collaboration/active-sessions.json" ]; then
    REGISTRY="$_OW_PWD/.claude/collaboration/active-sessions.json"
fi

# ---- JSON parser (jq preferred, python fallback) ---------------------------
PARSER=""
if command -v jq >/dev/null 2>&1; then
    PARSER="jq"
else
    for _cand in python3 python; do
        if command -v "$_cand" >/dev/null 2>&1 && "$_cand" -c "import sys" >/dev/null 2>&1; then
            PARSER="$_cand"; break
        fi
    done
fi

# ---- PID liveness (Git Bash on Windows falls back to powershell Get-Process) -
PID_PROBE="kill"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*) PID_PROBE="powershell" ;;
esac
pid_alive() {
    local pid="$1"
    [ -n "$pid" ] || return 1
    case "$pid" in (*[!0-9]*) return 1 ;; esac
    [ "$pid" -gt 0 ] || return 1
    if [ "$PID_PROBE" = "powershell" ]; then
        powershell.exe -NoProfile -Command "if (Get-Process -Id $pid -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1 </dev/null
    else
        kill -0 "$pid" 2>/dev/null </dev/null
    fi
}

# SWEEP-10 / DEFER-095: identity-aware liveness via lib/pid-liveness.sh. A
# recycled registry pid (live process, started after the entry's own
# claudeShellPidWrittenAt anchor) made a DEAD session read alive: never
# stall-flagged, its shared claims honoured, its touches claims live. Refuse on
# POSITIVE evidence only -- absent/unparseable anchor or lib unavailable is
# byte-identical to the existence-only probe, which is load-bearing in the
# OTHER direction here: a stall detector must never page the Overseer about a
# LIVE session because its anchor is unreadable. Anchor-only, no name glob
# (DEC-257). Path is REPORTABLE (QUIESCE-08 W4).
OW_PID_IDENTITY="fallback"
_OW_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
for _ow_pl in \
    "$_OW_LIB_DIR/../../scripts/lib/pid-liveness.sh" \
    "$_OW_LIB_DIR/../../../claude-template/scripts/lib/pid-liveness.sh" \
    "$HOME/.claude/scripts/lib/pid-liveness.sh"; do
    if [ -f "$_ow_pl" ]; then
        # shellcheck source=/dev/null
        . "$_ow_pl" 2>/dev/null || true
        break
    fi
done
command -v pid_is_alive_as >/dev/null 2>&1 && OW_PID_IDENTITY="lib"
if [ "${ULDF_OW_REPORT_PID_IDENTITY:-}" = "1" ]; then
    printf '%s\n' "$OW_PID_IDENTITY"
    exit 0
fi

# ow_pid_alive_as <pid> <anchor> -- identity-aware when the lib resolved.
ow_pid_alive_as() {
    if [ "$OW_PID_IDENTITY" = "lib" ]; then
        pid_is_alive_as "$1" "$2"
    else
        pid_alive "$1"
    fi
}

# Accumulators
SIGNALS=""          # JSON array body (comma-joined objects)
SIG_COUNT=0
SOURCES_SEEN=""     # which detector sources had readable data
SOURCES_ABSENT=""   # which were NO-DATA

add_signal() {
    # detectorId severity sessions_json summary
    local entry
    entry="{\"detectorId\":\"$(esc "$1")\",\"severity\":\"$(esc "$2")\",\"sessions\":$3,\"summary\":\"$(esc "$4")\"}"
    if [ -z "$SIGNALS" ]; then SIGNALS="$entry"; else SIGNALS="$SIGNALS,$entry"; fi
    SIG_COUNT=$((SIG_COUNT + 1))
}
mark_seen()   { SOURCES_SEEN="$SOURCES_SEEN $1"; }
mark_absent() { SOURCES_ABSENT="$SOURCES_ABSENT $1"; }

# =============================================================================
# (d) stall: active registry entries with a dead claudeShellPid
# =============================================================================
# A one-shot version of the pods-monitor PID-dead/terminal signal: a worker the
# registry still lists as active whose shell process has exited. Emits one signal
# per dead-PID active entry (id + role).
if [ -n "$REGISTRY" ] && [ -n "$PARSER" ]; then
    mark_seen "stall-monitor"
    # TSV rows: id <TAB> pid <TAB> role  for active entries with a numeric pid
    if [ "$PARSER" = "jq" ]; then
        STALL_ROWS=$(jq -r '
            (.sessions // [])
            | map(select((type=="object") and ((.status // "")=="active") and ((.claudeShellPid // null) != null)))
            | .[] | "\(.id // "")\t\(.claudeShellPid)\t\(.sessionRole // .role // "")\t\(.claudeShellPidWrittenAt // "-")"
        ' "$REGISTRY" 2>/dev/null | tr -d '\r')
    else
        STALL_ROWS=$("$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
for s in (d.get("sessions") or []):
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    pid = s.get("claudeShellPid")
    if pid is None: continue
    wa = s.get("claudeShellPidWrittenAt") or "-"
    print("\t".join([s.get("id") or "", str(pid), s.get("sessionRole") or s.get("role") or "", str(wa)]))
PY
        )
    fi
    if [ -n "$STALL_ROWS" ]; then
        while IFS= read -r _ow_rec; do
            [ -n "$_ow_rec" ] || continue
            _ow_split_tsv "$_ow_rec"
            sid="$OW_F1"; pid="$OW_F2"; role="$OW_F3"; wa="$OW_F4"
            [ "$wa" = "-" ] && wa=""
            [ -n "$sid" ] || continue
            case "$pid" in (''|*[!0-9]*) continue ;; esac
            if ! ow_pid_alive_as "$pid" "$wa"; then
                add_signal "stall-monitor" "critical" "[\"$(esc "$sid")\"]" \
                    "active ${role:-worker} $sid claudeShellPid $pid is dead (terminal/stall)"
            fi
        done <<EOF
$STALL_ROWS
EOF
    fi
else
    mark_absent "stall-monitor"
fi

# =============================================================================
# (a) concurrent-mutation: reuse the sibling CSI-10 oracle's verdict
# =============================================================================
_OW_THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
CM_RUN=""
for _cand in \
    "$_OW_PWD/.claude/oracles/concurrent-mutation/run.sh" \
    "$_OW_THIS_DIR/../concurrent-mutation/run.sh"; do
    if [ -f "$_cand" ]; then CM_RUN="$_cand"; break; fi
done
if [ -n "$CM_RUN" ]; then
    mark_seen "concurrent-mutation"
    CM_OUT=$(bounded 1.5 bash "$CM_RUN")
    if [ -n "$CM_OUT" ]; then
        CM_EXT=""
        if [ "$PARSER" = "jq" ]; then
            CM_EXT=$(printf '%s' "$CM_OUT" | jq -r '.external_mutation // false' 2>/dev/null | tr -d '\r')
        elif [ -n "$PARSER" ]; then
            CM_EXT=$(printf '%s' "$CM_OUT" | "$PARSER" -c 'import json,sys; print(str(json.load(sys.stdin).get("external_mutation", False)).lower())' 2>/dev/null)
        fi
        if [ "$CM_EXT" = "true" ]; then
            CM_SUM=""
            if [ "$PARSER" = "jq" ]; then
                CM_SUM=$(printf '%s' "$CM_OUT" | jq -r '.summary // "external LTADS mutation detected"' 2>/dev/null | tr -d '\r')
            elif [ -n "$PARSER" ]; then
                CM_SUM=$(printf '%s' "$CM_OUT" | "$PARSER" -c 'import json,sys; print(json.load(sys.stdin).get("summary") or "external LTADS mutation detected")' 2>/dev/null)
            fi
            [ -n "$CM_SUM" ] || CM_SUM="external LTADS mutation detected"
            add_signal "concurrent-mutation" "warn" "[]" "$CM_SUM"
        fi
    fi
else
    mark_absent "concurrent-mutation"
fi

# =============================================================================
# (b) shared-foreign: another live session's sharedClaim on a shared-repo file
# =============================================================================
# Discover shared repos via the workspace-shared-repos oracle (if present); for
# each repo, scan its active-sessions.json for active non-self entries holding a
# non-empty live sharedClaim. A lean foreign-claim PRESENCE pre-screen (the full
# per-file foreign-claim resolution is csi_shared_foreign_claims, used downstream).
WSR_RUN=""
for _cand in \
    "$_OW_PWD/.claude/oracles/workspace-shared-repos/run.sh" \
    "$_OW_THIS_DIR/../workspace-shared-repos/run.sh"; do
    if [ -f "$_cand" ]; then WSR_RUN="$_cand"; break; fi
done
if [ -n "$WSR_RUN" ] && [ -n "$PARSER" ]; then
    WSR_OUT=$(bounded 1.0 bash "$WSR_RUN")
    SHARED_PATHS=""
    if [ -n "$WSR_OUT" ]; then
        if [ "$PARSER" = "jq" ]; then
            SHARED_PATHS=$(printf '%s' "$WSR_OUT" | jq -r '.repos[]?.path // empty' 2>/dev/null | tr -d '\r')
        else
            SHARED_PATHS=$(printf '%s' "$WSR_OUT" | "$PARSER" -c '
import json,sys
try: d=json.load(sys.stdin)
except Exception: sys.exit(0)
for r in (d.get("repos") or []):
    if isinstance(r,dict) and r.get("path"): print(r["path"])' 2>/dev/null)
        fi
    fi
    if [ -n "$SHARED_PATHS" ]; then
        mark_seen "shared-foreign-claim"
        # Resolve own sessionId so we exclude self.
        MY_SID="${CLAUDE_SESSION_ID:-}"
        NOW_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        while IFS= read -r repo; do
            [ -n "$repo" ] || continue
            SREG="$repo/.claude/collaboration/active-sessions.json"
            [ -f "$SREG" ] || continue
            # rows: id <TAB> pid <TAB> nfiles  for active non-self entries with a live, unexpired sharedClaim
            if [ "$PARSER" = "jq" ]; then
                FROWS=$(jq -r --arg sid "$MY_SID" --arg now "$NOW_ISO" '
                    (.sessions // [])
                    | map(select((type=="object") and ((.id // "")!=$sid) and ((.status // "")=="active")
                          and (.sharedClaim != null) and (((.sharedClaim.files // []) | length) > 0)
                          and (((.sharedClaim.boundUntil // "")=="") or (.sharedClaim.boundUntil >= $now))))
                    | .[] | "\(.id // "")\t\(.claudeShellPid // "")\t\((.sharedClaim.files // []) | length)\t\(.claudeShellPidWrittenAt // "-")"
                ' "$SREG" 2>/dev/null | tr -d '\r')
            else
                FROWS=$(SID="$MY_SID" NOW="$NOW_ISO" "$PARSER" - "$SREG" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
sid=os.environ.get("SID",""); now=os.environ.get("NOW","")
for s in (d.get("sessions") or []):
    if not isinstance(s, dict): continue
    if (s.get("id") or "")==sid: continue
    if s.get("status")!="active": continue
    c=s.get("sharedClaim")
    if not isinstance(c, dict): continue
    files=c.get("files") or []
    if not files: continue
    bu=c.get("boundUntil") or ""
    if bu and bu < now: continue
    pid=s.get("claudeShellPid")
    wa=s.get("claudeShellPidWrittenAt") or "-"
    print("\t".join([s.get("id") or "", "" if pid is None else str(pid), str(len(files)), str(wa)]))
PY
                )
            fi
            [ -n "$FROWS" ] || continue
            while IFS= read -r _ow_rec; do
                [ -n "$_ow_rec" ] || continue
                _ow_split_tsv "$_ow_rec"
                fid="$OW_F1"; fpid="$OW_F2"; nfiles="$OW_F3"; fwa="$OW_F4"
                [ "$fwa" = "-" ] && fwa=""
                [ -n "$fid" ] || continue
                # Liveness: a claim's liveness is its entry's PID liveness
                # (CSI-07); identity-aware per DEFER-095.
                if [ -n "$fpid" ]; then ow_pid_alive_as "$fpid" "$fwa" || continue; fi
                rname=$(basename "$repo")
                add_signal "shared-foreign-claim" "warn" "[\"$(esc "$fid")\"]" \
                    "$fid holds a live claim on $nfiles file(s) in shared repo $rname"
            done <<EOF
$FROWS
EOF
        done <<EOF
$SHARED_PATHS
EOF
    else
        mark_absent "shared-foreign-claim"
    fi
else
    mark_absent "shared-foreign-claim"
fi

# =============================================================================
# (c) touches pre-screen: a touches.json path claimed by >=2 distinct sessions
# =============================================================================
# Cheap multi-claimant pre-screen across PODS touches.json files. Tolerant of
# BOTH touches.json shapes: files{path:{agents:[...]}} (implementation) and
# files{path:{touches:[{sessionId}]}} (design doc). The FULL line-overlap
# classification is the Overseer-run C8 pass (Q-CSO-04) -- this only decides wake.
#
# Liveness predicate (DEC-215, DEFER-039 phantom fix): a claim is only as live
# as its CLAIMANT. A claimant is live iff (1) the claiming collab dir's
# workers/<id>/shell.pid names a live PID, or (2) an active registry entry
# whose JSON mentions this collab id (siblingGroup/LD marking, DEFER-037) has
# id==claimant and a live PID. Claims from archived/absent sessions are stale
# by construction (the 2026-07-22 Feb-dir incident: agents dead 5 months
# passed the old count-only screen). Probes are cached per (dir,claimant) --
# the MSYS PID probe costs ~0.5s each.
TOUCHES_GLOB=$(ls .claude/collaboration/*/file-tracking/touches.json 2>/dev/null)
if [ -n "$TOUCHES_GLOB" ] && [ -n "$PARSER" ]; then
    mark_seen "touches-conflict"

    _TC_LIVE_CACHE=""
    # tc_claimant_alive <collab_dir> <claimant-id> -> 0 live / 1 stale
    tc_claimant_alive() {
        local cdir="$1" cid="$2" key p
        key="$(printf '%s|%s' "$cdir" "$cid" | tr ' \t' '__')"
        case " $_TC_LIVE_CACHE " in
            *" $key=1 "*) return 0 ;;
            *" $key=0 "*) return 1 ;;
        esac
        # (1) worker shell.pid inside the claiming collab dir
        if [ -f "$cdir/workers/$cid/shell.pid" ]; then
            p="$(tr -cd '0-9' < "$cdir/workers/$cid/shell.pid" 2>/dev/null)"
            if [ -n "$p" ] && pid_alive "$p"; then
                _TC_LIVE_CACHE="$_TC_LIVE_CACHE $key=1"; return 0
            fi
        fi
        # (2) active registry entry mentioning this collab id with id==claimant
        # Rows: pid<TAB>writtenAt ("-" sentinel when absent -- DEFER-078; the
        # anchor rides only the claudeShellPid form, legacy .pid has none).
        local base rrows rpid rwa _tc_rec
        base="$(basename "$cdir")"
        if [ -n "$REGISTRY" ]; then
            if [ "$PARSER" = "jq" ]; then
                rrows=$(jq -r --arg b "$base" --arg cid "$cid" '
                    (.sessions // [])
                    | map(select((type=="object") and ((.status // "")=="active")
                          and ((.id // "")==$cid) and (tostring | contains($b))))
                    | .[]
                    | if (.claudeShellPid // null) != null
                        then "\(.claudeShellPid)\t\(.claudeShellPidWrittenAt // "-")"
                      elif (.pid // null) != null
                        then "\(.pid)\t-"
                      else empty end' "$REGISTRY" 2>/dev/null | tr -d '\r')
            else
                rrows=$(B="$base" CID="$cid" "$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
b = os.environ["B"]; cid = os.environ["CID"]
for s in (d.get("sessions") or []):
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    if (s.get("id") or "") != cid: continue
    if b not in json.dumps(s): continue
    if s.get("claudeShellPid") is not None:
        print("%s\t%s" % (s["claudeShellPid"], s.get("claudeShellPidWrittenAt") or "-"))
    elif s.get("pid") is not None:
        print("%s\t-" % s["pid"])
PY
                )
            fi
            while IFS= read -r _tc_rec; do
                [ -n "$_tc_rec" ] || continue
                _ow_split_tsv "$_tc_rec"
                rpid="$OW_F1"; rwa="$OW_F2"
                [ "$rwa" = "-" ] && rwa=""
                case "$rpid" in (''|*[!0-9]*) continue ;; esac
                if ow_pid_alive_as "$rpid" "$rwa"; then
                    _TC_LIVE_CACHE="$_TC_LIVE_CACHE $key=1"; return 0
                fi
            done <<EOF
$rrows
EOF
        fi
        _TC_LIVE_CACHE="$_TC_LIVE_CACHE $key=0"; return 1
    }

    while IFS= read -r tjson; do
        [ -n "$tjson" ] || continue
        _tc_cdir="${tjson%/file-tracking/touches.json}"
        # rows: path <TAB> claimant1,claimant2,...  for paths with >=2 distinct claimants
        if [ "$PARSER" = "jq" ]; then
            TROWS=$(jq -r '
                (.files // {})
                | to_entries[]
                | . as $e
                | (if (($e.value.agents // []) | length) > 0
                     then $e.value.agents
                     else ($e.value.touches // [] | map(.sessionId // empty))
                   end | unique) as $cl
                | select(($cl | length) >= 2)
                | "\($e.key)\t\($cl | join(","))"
            ' "$tjson" 2>/dev/null | tr -d '\r')
        else
            TROWS=$("$PARSER" - "$tjson" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
files=d.get("files") or {}
if not isinstance(files, dict): sys.exit(0)
for path, rec in files.items():
    if not isinstance(rec, dict): continue
    cl=rec.get("agents")
    if not cl:
        cl=[t.get("sessionId") for t in (rec.get("touches") or []) if isinstance(t,dict) and t.get("sessionId")]
    cl=sorted(set([c for c in (cl or []) if c]))
    if len(cl) >= 2:
        print(path + "\t" + ",".join(cl))
PY
            )
        fi
        [ -n "$TROWS" ] || continue
        while IFS=$'\t' read -r tpath claimants; do
            [ -n "$tpath" ] || continue
            # DEC-215: keep only LIVE claimants; a path needs >=2 live
            # claimants to signal (dead sessions' claims are stale).
            live_csv=""
            live_n=0
            _tc_saved_ifs="$IFS"; IFS=','
            for _tc_c in $claimants; do
                IFS="$_tc_saved_ifs"
                [ -n "$_tc_c" ] || { IFS=','; continue; }
                if tc_claimant_alive "$_tc_cdir" "$_tc_c"; then
                    if [ -z "$live_csv" ]; then live_csv="$_tc_c"; else live_csv="$live_csv,$_tc_c"; fi
                    live_n=$((live_n + 1))
                fi
                IFS=','
            done
            IFS="$_tc_saved_ifs"
            [ "$live_n" -ge 2 ] || continue
            # claimants CSV -> JSON array
            cj=$(printf '%s' "$live_csv" | awk -F',' '{out="["; for(i=1;i<=NF;i++){gsub(/\\/,"\\\\",$i); gsub(/"/,"\\\"",$i); out=out (i>1?",":"") "\"" $i "\""} print out "]"}')
            add_signal "touches-conflict" "warn" "$cj" \
                "$tpath claimed by >=2 live sessions ($live_csv) -- needs Overseer conflict pass"
        done <<EOF
$TROWS
EOF
    done <<EOF
$TOUCHES_GLOB
EOF
else
    mark_absent "touches-conflict"
fi

# =============================================================================
# (e) resource-contention: overlapping live resource claims, or a live claim
#     coexisting with a foreign RUNNING tracked job (CSO-21, § SACT; DEC-269)
# =============================================================================
# RELAY ONLY. Both inputs already exist; this leg invents no detection:
#   * live resource claims -- the SACT-02 `resourceClaim` sub-object, read with
#     csi_live_foreign_resource_claims' own predicate (entry active AND
#     boundUntil absent-or-unexpired AND pid alive). Inlined rather than sourced
#     for the same reason leg (b) inlines its sharedClaim read: this runs on the
#     session-start fast path and registry-write.sh is a ~2k-line lib. One
#     deliberate difference from that reader: self is NOT excluded -- "my claim
#     vs a peer's" is the most actionable contention there is, and the Overseer
#     is a third party to both claimants.
#   * running tracked jobs -- the background-job-status oracle's OWN pending[]
#     definition (status=="running"), minus the jobs that oracle itself flags
#     `stalled` (heartbeat older than GC_JOB_STALL_SECONDS, default 90; the same
#     env override is honoured here). A dead wrapper's leftover sentinel is not
#     contention, and an advisory naming it would be confidently wrong -- which
#     is precisely what the advisory-fidelity gate exists to stop.
#
# Deliberate bounds -- declared, not residue:
#   * A job whose session_id is null is EXCLUDED. "Owned by a different session"
#     is not establishable from null, and guessing is what NO-DATA forbids.
#   * No cmd/label -> resource classifier. Whether a running `docker build`
#     collides with a claim on "dev-stack" is judgment; this leg relays both
#     rows and the WOKEN Overseer decides. Inventing that classifier here would
#     breach the CSO relay boundary (CSO-04).
#   * Two unclaimed heavy jobs are NOT contention to this leg -- a claim is the
#     anchor. The unclaimed case belongs to machine-quiescence (anonymous by
#     design, QUIESCE-02), so absence of a signal here is not evidence of a
#     quiet machine.
#   * Contention is a coordination fact, never an arbitration: severity stays
#     `warn` and no actuation kind above `nudge` is mapped, because pausing the
#     second claimant would award the resource to the first -- the winner
#     arbitration DEC-96 D2 / DEC-240 explicitly rejected for advisory claims.

# _ow_iso_minus <seconds> -> ISO-8601 UTC cutoff, or "" when unobtainable.
# Lexical-cutoff strategy: the cutoff is computed ONCE in the shell so neither
# parser branch needs date arithmetic (jq's fromdateiso8601 and python's
# datetime would otherwise have to agree). GNU form first (Git Bash + Linux),
# BSD second (macOS). Empty return is handled as NO-DATA by the caller.
_ow_iso_minus() {
    local _secs="$1" _nowe
    _nowe=$(date -u +%s 2>/dev/null) || return 0
    case "$_secs" in (''|*[!0-9]*) return 0 ;; esac
    date -u -d "@$((_nowe - _secs))" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null && return 0
    date -u -r "$((_nowe - _secs))" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null && return 0
    return 0
}

RC_JOBS_DIR="$_OW_PWD/.claude/session-state/jobs"
if [ -n "$REGISTRY" ] && [ -n "$PARSER" ]; then
    mark_seen "resource-contention"
    RC_NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)

    # --- live resource claims: sid <TAB> resource <TAB> kind <TAB> pid <TAB> anchor
    if [ "$PARSER" = "jq" ]; then
        RC_ROWS=$(jq -r --arg now "$RC_NOW" '
            (.sessions // [])
            | map(select((type=="object") and ((.status // "")=="active")
                  and (.resourceClaim != null) and ((.resourceClaim.resource // "")!="")
                  and (((.resourceClaim.boundUntil // "")=="") or (.resourceClaim.boundUntil >= $now))))
            | .[] | "\(.id // "")\t\(.resourceClaim.resource)\t\(.resourceClaim.kind // "")\t\(.claudeShellPid // "")\t\(.claudeShellPidWrittenAt // "-")"
        ' "$REGISTRY" 2>/dev/null | tr -d '\r')
    else
        RC_ROWS=$(NOW="$RC_NOW" "$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
now = os.environ.get("NOW", "")
for s in (d.get("sessions") or []):
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    rc = s.get("resourceClaim")
    if not isinstance(rc, dict): continue
    res = rc.get("resource") or ""
    if not res: continue
    bu = rc.get("boundUntil") or ""
    if bu and bu < now: continue
    pid = s.get("claudeShellPid")
    wa = s.get("claudeShellPidWrittenAt") or "-"
    print("\t".join([s.get("id") or "", res, rc.get("kind") or "",
                     "" if pid is None else str(pid), str(wa)]))
PY
        )
    fi

    # Liveness filter (CSI-07 stance, identity-aware per DEFER-095): keep
    # sid <TAB> resource <TAB> kind for claims whose owning entry is alive.
    RC_LIVE=""
    if [ -n "$RC_ROWS" ]; then
        while IFS= read -r _rc_rec; do
            [ -n "$_rc_rec" ] || continue
            # This row has FIVE fields; _ow_split_tsv yields four with the
            # remainder in F4, so split the remainder again rather than
            # hand-rolling a tab regex (DEFER-078: the reader-side guard is
            # where this defence belongs).
            _ow_split_tsv "$_rc_rec"
            rcsid="$OW_F1"; rcres="$OW_F2"; rckind="$OW_F3"; _rc_rest="$OW_F4"
            _ow_split_tsv "$_rc_rest"
            rcpid="$OW_F1"; rcwa="$OW_F2"
            [ "$rcwa" = "-" ] && rcwa=""
            [ -n "$rcsid" ] && [ -n "$rcres" ] || continue
            if [ -n "$rcpid" ]; then ow_pid_alive_as "$rcpid" "$rcwa" || continue; fi
            RC_LIVE="$RC_LIVE$(printf '%s\t%s\t%s\n' "$rcsid" "$rcres" "$rckind")
"
        done <<EOF
$RC_ROWS
EOF
    fi

    # --- running, non-stalled, session-attributed tracked jobs: job_id <TAB> owner
    RC_JOB_ROWS=""
    if [ -d "$RC_JOBS_DIR" ]; then
        RC_CUTOFF=$(_ow_iso_minus "${GC_JOB_STALL_SECONDS:-90}")
        if [ -n "$RC_CUTOFF" ]; then
            for _rc_jf in "$RC_JOBS_DIR"/*.json; do
                [ -f "$_rc_jf" ] || continue
                if [ "$PARSER" = "jq" ]; then
                    _rc_jrow=$(jq -r --arg cut "$RC_CUTOFF" '
                        select((.status // "")=="running")
                        | select((.session_id // "") != "")
                        | select(((.last_heartbeat_at // "")=="") or (.last_heartbeat_at >= $cut))
                        | "\(.job_id // "")\t\(.session_id)"' "$_rc_jf" 2>/dev/null | tr -d '\r')
                else
                    _rc_jrow=$(CUT="$RC_CUTOFF" "$PARSER" - "$_rc_jf" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        j = json.load(f)
except Exception:
    sys.exit(0)
if not isinstance(j, dict) or j.get("status") != "running": sys.exit(0)
owner = j.get("session_id") or ""
if not owner: sys.exit(0)
hb = j.get("last_heartbeat_at") or ""
if hb and hb < os.environ.get("CUT", ""): sys.exit(0)
print("%s\t%s" % (j.get("job_id") or "", owner))
PY
                    )
                fi
                [ -n "$_rc_jrow" ] || continue
                RC_JOB_ROWS="$RC_JOB_ROWS$_rc_jrow
"
            done
        else
            # Cannot establish job liveness on this platform -> the job half is
            # honest NO-DATA. The claim-overlap half above is unaffected.
            mark_absent "resource-contention:jobs"
        fi
    fi

    # --- compose: one signal per contended resource (CSO-05 grouping)
    RC_RESOURCES=$(printf '%s' "$RC_LIVE" | cut -f2 | grep -v '^$' | sort -u)
    while IFS= read -r rc_res; do
        [ -n "$rc_res" ] || continue
        rc_claimants=$(printf '%s' "$RC_LIVE" | awk -F'\t' -v r="$rc_res" '$2==r && $1!="" {print $1}' | sort -u)
        rc_n=$(printf '%s\n' "$rc_claimants" | grep -c '[^[:space:]]')
        rc_csv=$(printf '%s' "$rc_claimants" | tr '\n' ',' | sed 's/,*$//')
        rc_kind=$(printf '%s' "$RC_LIVE" | awk -F'\t' -v r="$rc_res" '$2==r && $3!="" {print $3; exit}')
        if [ "$rc_n" -ge 2 ]; then
            rc_sj=$(printf '%s' "$rc_csv" | awk -F',' '{out="["; for(i=1;i<=NF;i++){gsub(/\\/,"\\\\",$i); gsub(/"/,"\\\"",$i); out=out (i>1?",":"") "\"" $i "\""} print out "]"}')
            add_signal "resource-contention" "warn" "$rc_sj" \
                "$rc_res claimed by $rc_n live sessions ($rc_csv)${rc_kind:+ [$rc_kind]} -- overlapping resource claims"
        elif [ "$rc_n" -eq 1 ] && [ -n "$RC_JOB_ROWS" ]; then
            # A lone claimant contends with a RUNNING job owned by anyone else.
            rc_owner="$rc_claimants"
            rc_fowners=$(printf '%s' "$RC_JOB_ROWS" | awk -F'\t' -v o="$rc_owner" '$2!="" && $2!=o {print $2}' | sort -u)
            if [ -n "$rc_fowners" ]; then
                rc_fjobs=$(printf '%s' "$RC_JOB_ROWS" | awk -F'\t' -v o="$rc_owner" '$2!="" && $2!=o {print $1}' | sort -u | tr '\n' ',' | sed 's/,*$//')
                rc_all=$(printf '%s\n%s\n' "$rc_owner" "$rc_fowners" | grep -v '^$' | sort -u | tr '\n' ',' | sed 's/,*$//')
                rc_sj=$(printf '%s' "$rc_all" | awk -F',' '{out="["; for(i=1;i<=NF;i++){gsub(/\\/,"\\\\",$i); gsub(/"/,"\\\"",$i); out=out (i>1?",":"") "\"" $i "\""} print out "]"}')
                add_signal "resource-contention" "warn" "$rc_sj" \
                    "$rc_owner holds a live claim on $rc_res${rc_kind:+ [$rc_kind]} while other session(s) run tracked job(s) $rc_fjobs -- verify they do not contend"
            fi
        fi
    done <<EOF
$RC_RESOURCES
EOF
else
    mark_absent "resource-contention"
fi

# =============================================================================
# Compose verdict
# =============================================================================
# wake fires only on positive evidence (>=1 signal). Absence of all sources is
# honest NO-DATA -- wake stays false (we never fabricate an all-clear CLAIM; the
# summary records the data gaps).
SEEN_TRIM=$(printf '%s' "$SOURCES_SEEN" | sed 's/^ *//')
ABSENT_TRIM=$(printf '%s' "$SOURCES_ABSENT" | sed 's/^ *//')

if [ "$SIG_COUNT" -gt 0 ]; then
    WAKE="true"
    SUMMARY="$SIG_COUNT airspace signal(s) -- wake Overseer"
    BRIEFING="overseer-wake: $SIG_COUNT airspace signal(s) [$(printf '%s' "$SIGNALS" | grep -o '"detectorId":"[^"]*"' | sed 's/"detectorId":"//; s/"//' | sort | uniq -c | awk '{print $2"("$1")"}' | tr '\n' ' ' | sed 's/ *$//')] -- run the Overseer (perceive -> advise) or /0-uldf-oracle overseer-wake"
else
    WAKE="false"
    if [ -n "$SEEN_TRIM" ]; then
        SUMMARY="no airspace signals (sources observed: $SEEN_TRIM)"
    else
        SUMMARY="no detector sources available (NO-DATA -- not an all-clear)"
    fi
    BRIEFING=""
fi

printf '{"wake":%s,"signals":[%s],"summary":"%s","briefing":"%s"}\n' \
    "$WAKE" "$SIGNALS" "$(esc "$SUMMARY")" "$(esc "$BRIEFING")"
exit 0
