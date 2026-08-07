#!/bin/bash
# dispatchable-sessions oracle (Unix)
# Answers: what live sibling sessions can THIS session dispatch work to right now?
#
# Reads .claude/collaboration/active-sessions.json. (Legacy ltads/sessions/active-sessions.json retired, Arc 1 DEC-198 -- dead per DEC-124.)
# Emits a JSON object with:
#   - count   : integer, number of live dispatchable peers
#   - peers   : array of {sessionId, sessionRole, role, workDir, claudeShellPid, dispatchable, spawnedAt, siblingGroup?}
#   - briefing: human-readable one-line summary for the session-start ORACLE BRIEFING
#
# Filter: status=="active" AND dispatchable==true AND claudeShellPid!=null AND PID alive.
# Legacy entries (no registryVersion or registryVersion=1) silently drop -- they predate dispatch.
# Strategy: always-fresh. Read-only on the registry (no mutation; stale-cleanup is a separate path).
#
# SELF-EXCLUSION (DISC-CSI-22): a session's OWN registry entry is not a sibling.
# The briefing path drops any entry whose id matches ULDF_SELF_SESSION_ID
# (exported by the session-start hook, which registered that id moments earlier)
# or, failing that, CLAUDE_SESSION_ID (present in spawned workers' env). When
# neither is set (e.g. an interactive session's agent re-running the oracle
# mid-session from a Bash-tool shell), no exclusion is possible and the caller's
# own entry MAY appear as a peer -- callers that know their id should invoke as
# `ULDF_SELF_SESSION_ID=<id> run.sh`. Trigger incident: 2026-07-07 session
# interactive-20260707T162816Z-457288 was briefed its own entry as a live sibling.
#
# PROJECT-SCOPED BY DESIGN: reads ONLY the current project's registry (there is no
# machine-global session registry; each project owns its own). It deliberately does
# NOT surface sessions from OTHER projects -- auto-surfacing would leak unrelated
# work into every briefing and invite mis-targeting. A cross-project session you
# spawned (spawn-claude-session -WorkDir <other>) is reached EXPLICITLY via
# `/0-uldf-dispatch --project=<path>`; completion notifies back via CSI-17. Model:
# FOUNDATIONS/CSI_DESIGN.md  4.5b; docs/planning/cross-project-session-dispatch-20260625.md.
#
# Modes (CSI-05 added --gc, --gc-cheap; RESUME-03 added --duplicate-of):
#   (default)           : read-only briefing path described above
#   --gc-cheap          : session-start hygiene sweep, ~1000ms probe-loop budget
#                         (DEC-79-class calibration; clock starts after candidate
#                         parse, first candidate always probed -- DEFER-064),
#                         defers the tail honestly if exceeded
#   --gc                : on-demand hygiene sweep, no time budget, prints {swept,before,after,threshold,thresholdSource}
#   --driver-of=<arc>   : CSI-07 live driver-claim query
#   --duplicate-of=<id> : RESUME-03 project-scoped duplicate-self-check. Answers:
#                         is ANOTHER live session currently holding identity <id>
#                         in THIS project? Reads ONLY the project registry (never
#                         a machine-wide process scan -- DEFER-002 trigger incident:
#                         a worker misidentified an unrelated project's session as
#                         its twin via a bare process scan). duplicate==true requires:
#                         active entry with id==<id> AND claudeShellPid alive AND
#                         the pid is NOT in the caller's own ancestor chain AND
#                         the entry's workDir matches the current workDir.
#
# Sweep criteria (--gc / --gc-cheap):
#   status=="active" AND claudeShellPid!=null AND PID dead AND spawnedAt older than threshold (default 24h).
#   Action: flip status to "expired" + sweptAt timestamp; move entry from sessions[] to closed[].
#   Atomic write via claude-template/scripts/lib/registry-write.sh helper (CSI-01).
#   Threshold: .claude/config.json `csi.registryHygieneThreshold` (numeric hours OR PnH/PnD), default 24.
# CSI-05 closes DISC-PRO-05's REGISTRY-GC-01 follow-up.

set -e

EMPTY_OUTPUT='{"count":0,"peers":[],"briefing":"No live siblings. /0-uldf-dispatch unavailable."}'

emit_empty() {
    echo "$EMPTY_OUTPUT"
    exit 0
}

# ---- Parse mode ----
MODE="briefing"
DRIVER_OF_ARC=""
DUPLICATE_OF_ID=""
case "${1:-}" in
    --gc)        MODE="gc" ;;
    --gc-cheap)  MODE="gc-cheap" ;;
    --dry-run)   MODE="dry-run" ;;
    --driver-of=*)
        # CSI-07 query interface: who (if anyone) live-drives arc <arc>?
        MODE="driver-of"
        DRIVER_OF_ARC="${1#--driver-of=}"
        if [ -z "$DRIVER_OF_ARC" ]; then
            echo "dispatchable-sessions: --driver-of requires an arc id" >&2
            exit 1
        fi
        ;;
    --duplicate-of=*)
        # RESUME-03 query interface: does another live session hold identity <id> here?
        MODE="duplicate-of"
        DUPLICATE_OF_ID="${1#--duplicate-of=}"
        if [ -z "$DUPLICATE_OF_ID" ]; then
            echo "dispatchable-sessions: --duplicate-of requires a session id" >&2
            exit 1
        fi
        ;;
    "")          MODE="briefing" ;;
    *)
        echo "dispatchable-sessions: unknown mode: $1" >&2
        echo "  usage: run.sh [--gc|--gc-cheap|--dry-run|--driver-of=<arc>|--duplicate-of=<id>]" >&2
        exit 1
        ;;
esac

# ---- OLOG fire record (ORA-FIRE-01, DEC-166 / DISC-ORA-02) ------------------
# Actuator leg: fires as a session-start --gc-cheap sweep, silent on success. Without
# this record the sweep is invisible to oracle-consultations.jsonl and DEC-82's
# retirement criterion scores it as unused no matter how hard it works. A fire is not
# a consultation -- it carries via:"sweep". (This oracle ALSO answers questions via the
# briefing/direct paths; those keep logging as consultations, so the two modes stay
# distinguishable.) Append-failure is swallowed.
case "$MODE" in
  gc|gc-cheap)
    _OFL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    for _ofl_cand in \
        "$_OFL_DIR/../../scripts/lib/oracle-fire-log.sh" \
        "$_OFL_DIR/../../../claude-template/scripts/lib/oracle-fire-log.sh" \
        "$HOME/.claude/scripts/lib/oracle-fire-log.sh"; do
        if [ -f "$_ofl_cand" ]; then
            # shellcheck source=/dev/null
            . "$_ofl_cand" && oracle_fire_log "dispatchable-sessions" "sweep"
            break
        fi
    done
    ;;
esac

# ---- Locate the registry file (first-match wins) ----
# HYGIENE-04: registry-write helpers consumed below validate path-is-absolute.
# Resolve via $(pwd) so the path is absolute regardless of caller cwd. The
# session-start hook already cd's to project root before invoking this oracle,
# so $(pwd) IS the project root in normal flow.
_DS_PWD="$(pwd)"
REGISTRY=""
if [ -f ".claude/collaboration/active-sessions.json" ]; then
    REGISTRY="$_DS_PWD/.claude/collaboration/active-sessions.json"
fi
# (legacy ltads/sessions/active-sessions.json fallback retired -- dead per DEC-124/Arc 1 DEC-198;
# spawn + the CSI hooks have written .claude/collaboration/active-sessions.json since DISPATCH-01)

if [ -z "$REGISTRY" ]; then
    if [ "$MODE" = "briefing" ]; then
        emit_empty
    elif [ "$MODE" = "gc" ] || [ "$MODE" = "dry-run" ]; then
        echo '{"swept":0,"before":0,"after":0,"threshold":"P1D","thresholdSource":"default","note":"no registry"}'
        exit 0
    elif [ "$MODE" = "driver-of" ]; then
        printf '{"arc":"%s","driver":null,"note":"no registry"}\n' "$DRIVER_OF_ARC"
        exit 0
    elif [ "$MODE" = "duplicate-of" ]; then
        # Gracefully absent: no registry means no project-scoped evidence of a
        # duplicate. duplicate:false -- a resume proceeds (never stand down on
        # absent evidence; DEFER-002).
        printf '{"sessionId":"%s","duplicate":false,"holder":null,"note":"no registry"}\n' "$DUPLICATE_OF_ID"
        exit 0
    else
        # --gc-cheap is silent on success or graceful absence
        exit 0
    fi
fi

# ---- Liveness probe (Linux/macOS use kill -0; Git Bash on Windows falls back to powershell Get-Process) ----
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
        # Git Bash on Windows: kill -0 is unreliable for foreign processes; defer to PowerShell's real OS API.
        powershell.exe -NoProfile -Command "if (Get-Process -Id $pid -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1
    else
        kill -0 "$pid" 2>/dev/null
    fi
}

# ---- Identity-aware liveness (SWEEP-10 / DEC-252) ----------------------------
# A PID is not an identity. Measured 2026-08-04: two entries sat in this
# registry's `active` bucket held by RuntimeBroker and svchost -- both reading
# ALIVE under the existence-only probe above, both belonging to sessions long
# dead. `active` is ground truth for dispatch targeting, claim staleness,
# conclude-eligibility and the heavy-work pre-flight, so a recycled id makes a
# dead session permanently visible to all of them.
#
# This file has carried its OWN inline probe since CSI-05 (the shared lib's
# header records the migration as deliberately deferred). Rather than duplicate
# the identity logic -- where a silent divergence between the two copies would
# be exactly the proxy-referent hazard OVALID warns about -- source the lib and
# use its function. The fallback below keeps the oracle working on a partial
# deploy, at the pre-DEC-252 behaviour.
#
# THE FALLBACK IS THE HAZARD, so it is made VISIBLE: a graceful-absence stub
# cannot show you that its own load path is broken -- a typo installs the no-op
# and every harness stays green with the fix never loaded (the QUIESCE-08 W4
# lesson, one subsystem over). ULDF_DS_PID_IDENTITY reports which path is live
# so a smoke can assert the real one.
for _ds_pl in \
    "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." 2>/dev/null && pwd)/scripts/lib/pid-liveness.sh" \
    "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." 2>/dev/null && pwd)/claude-template/scripts/lib/pid-liveness.sh" \
    "$HOME/.claude/scripts/lib/pid-liveness.sh"
do
    if [ -f "$_ds_pl" ]; then
        # shellcheck disable=SC1090
        . "$_ds_pl" 2>/dev/null || true
        break
    fi
done
if command -v pid_is_alive_as >/dev/null 2>&1; then
    ULDF_DS_PID_IDENTITY="lib"
    pid_alive_as() { pid_is_alive_as "$@"; }
else
    ULDF_DS_PID_IDENTITY="fallback"
    pid_alive_as() { pid_alive "$1"; }   # pre-DEC-252 behaviour, identity ignored
fi
[ "${ULDF_DS_REPORT_PID_IDENTITY:-}" = "1" ] && { printf '%s\n' "$ULDF_DS_PID_IDENTITY"; exit 0; }

# ---- Pick a JSON parser. Prefer jq; fall back to python; else degrade gracefully. ----
# Probe python actually runs (the Windows-Store `python3` shim returns 0 from
# `command -v` but errors out with an "install from Store" message on real use).
PARSER=""
if command -v jq >/dev/null 2>&1; then
    PARSER="jq"
else
    for _cand in python3 python; do
        if command -v "$_cand" >/dev/null 2>&1; then
            if "$_cand" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
                PARSER="$_cand"
                break
            fi
        fi
    done
fi

# A REAL python interpreter, resolved independently of $PARSER. `_iso_to_epoch`'s
# date fallback is python source and must never be handed to jq -- when jq is the
# preferred parser, `"$PARSER" -c '<python>'` exits 3 and, under `set -e`, took the
# whole sweep down on any unparseable spawnedAt (DEFER-078 fixture). Empty when no
# usable python exists, which the fallback treats as "no age available".
PYBIN_ISO=""
for _cand in python3 python; do
    if command -v "$_cand" >/dev/null 2>&1; then
        if "$_cand" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
            PYBIN_ISO="$_cand"
            break
        fi
    fi
done

if [ -z "$PARSER" ]; then
    if [ "$MODE" = "briefing" ]; then
        emit_empty
    elif [ "$MODE" = "gc" ] || [ "$MODE" = "dry-run" ]; then
        echo '{"swept":0,"before":0,"after":0,"threshold":"P1D","thresholdSource":"default","note":"no JSON parser"}'
        exit 0
    elif [ "$MODE" = "driver-of" ]; then
        printf '{"arc":"%s","driver":null,"note":"no JSON parser"}\n' "$DRIVER_OF_ARC"
        exit 0
    elif [ "$MODE" = "duplicate-of" ]; then
        printf '{"sessionId":"%s","duplicate":false,"holder":null,"note":"no JSON parser"}\n' "$DUPLICATE_OF_ID"
        exit 0
    else
        exit 0
    fi
fi

# =============================================================================
# --driver-of=<arc> (CSI-07 query interface)
# =============================================================================
# Returns the LIVE driver claim for the given arc, or driver:null. A claim is
# live when its owning entry is status==active, its boundUntil (if any) has
# not passed, and the entry's claudeShellPid (if any) is alive. Read-only.
# Output (frozen): {"arc":"<arc>","driver":null}
#   or {"arc":"<arc>","driver":{"sessionId","arc","consentScope","boundUntil",
#       "entryId","claudeShellPid"}}
if [ "$MODE" = "driver-of" ]; then
    DOF_NOW="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    # Three-line protocol from the parser: line 1 = probe PID (may be empty),
    # line 2 = claudeShellPidWrittenAt ("-" when absent; SWEEP-10 identity
    # anchor), line 3 = the candidate result JSON. Liveness is probed in bash
    # because MSYS needs the powershell fallback (pid_alive above).
    if [ "$PARSER" = "jq" ]; then
        DOF_OUT="$(jq -r --arg arc "$DRIVER_OF_ARC" --arg now "$DOF_NOW" '
            ([ .sessions[]?
               | select(.status == "active")
               | select(.driver != null)
               | select((.driver.arc // "") == $arc)
               | select((.driver.boundUntil // "") == "" or .driver.boundUntil >= $now)
             ] | first) as $c
            | if $c == null then "", "-", ({arc: $arc, driver: null} | tojson)
              else ($c.claudeShellPid // "" | tostring),
                   ((($c.claudeShellPidWrittenAt // "") | tostring) | if . == "" then "-" else . end),
                   ({arc: $arc, driver: {
                       sessionId: ($c.driver.sessionId // $c.sessionId),
                       arc: $c.driver.arc,
                       consentScope: ($c.driver.consentScope // null),
                       boundUntil: ($c.driver.boundUntil // null),
                       entryId: $c.id,
                       claudeShellPid: ($c.claudeShellPid // null)
                   }} | tojson)
              end' "$REGISTRY" 2>/dev/null)"
    else
        DOF_OUT="$("$PARSER" -c '
import json,sys
arc=sys.argv[2]; now=sys.argv[3]
try:
    d=json.load(open(sys.argv[1], encoding="utf-8-sig"))
except Exception:
    d={}
cand=None
for s in d.get("sessions",[]) or []:
    if s.get("status")!="active": continue
    drv=s.get("driver")
    if not isinstance(drv,dict): continue
    if (drv.get("arc") or "") != arc: continue
    bu=drv.get("boundUntil") or ""
    if bu and bu < now: continue
    cand=s; break
if cand is None:
    print("")
    print("-")
    print(json.dumps({"arc":arc,"driver":None},separators=(",",":")))
else:
    drv=cand["driver"]
    pid=cand.get("claudeShellPid")
    print(pid if pid is not None else "")
    print(cand.get("claudeShellPidWrittenAt") or "-")
    print(json.dumps({"arc":arc,"driver":{
        "sessionId":drv.get("sessionId") or cand.get("sessionId"),
        "arc":drv.get("arc"),
        "consentScope":drv.get("consentScope"),
        "boundUntil":drv.get("boundUntil"),
        "entryId":cand.get("id"),
        "claudeShellPid":pid}},separators=(",",":")))' "$REGISTRY" "$DRIVER_OF_ARC" "$DOF_NOW" 2>/dev/null)"
    fi

    DOF_PID="$(printf '%s\n' "$DOF_OUT" | sed -n 1p)"
    DOF_WRITTEN="$(printf '%s\n' "$DOF_OUT" | sed -n 2p)"
    DOF_JSON="$(printf '%s\n' "$DOF_OUT" | sed -n 3p)"
    [ "$DOF_WRITTEN" = "-" ] && DOF_WRITTEN=""
    if [ -z "$DOF_JSON" ]; then
        # Parser failure -> graceful null (same convention as briefing path).
        printf '{"arc":"%s","driver":null,"note":"unparseable registry"}\n' "$DRIVER_OF_ARC"
        exit 0
    fi
    # SWEEP-10 / DEFER-088: identity-aware -- a recycled pid must not hold the
    # arc; absent anchor keeps the existence-only verdict.
    if [ -n "$DOF_PID" ] && ! pid_alive_as "$DOF_PID" "$DOF_WRITTEN"; then
        # Claim owner is dead -> stale claim, not a live driver (CSI-05 sweeps it).
        printf '{"arc":"%s","driver":null,"note":"driver pid dead (stale claim)"}\n' "$DRIVER_OF_ARC"
        exit 0
    fi
    printf '%s\n' "$DOF_JSON"
    exit 0
fi

# =============================================================================
# --duplicate-of=<sessionId> (RESUME-03 project-scoped duplicate-self-check)
# =============================================================================
# Answers: is ANOTHER live session currently holding identity <sessionId> in
# THIS project? Project-scoped by construction: only the project registry is
# consulted -- NEVER a machine-wide process scan (pgrep claude / Get-Process
# claude), which spans every Claude session on the machine and misidentifies
# unrelated projects' sessions as twins (DEFER-002 trigger incident).
#
# Output (schema-stable across shells):
#   {"sessionId":"<id>","duplicate":<bool>,"holder":null|{entryId,claudeShellPid,
#    workDir,workDirMatch,sessionRole,spawnedAt,isSelf},"note":"<reason>"}
#
# duplicate==true iff: an ACTIVE registry entry with id==<sessionId> exists,
# its claudeShellPid is ALIVE, the pid is NOT in the calling process's own
# ancestor chain (self-exclusion: the registry entry a resuming session wrote
# for itself is not its own twin), AND the entry's workDir matches the current
# workDir (normalized). A live holder in a different workDir reports the
# holder but duplicate:false.
if [ "$MODE" = "duplicate-of" ]; then
    # ---- Pass 1: extract the candidate entry (6-line protocol) --------------
    # line 1: "found" | "none"; line 2: pid; line 3: workDir; line 4: sessionRole;
    # line 5: spawnedAt; line 6: claudeShellPidWrittenAt ("-" when absent;
    # SWEEP-10 identity anchor). Liveness + ancestry + workDir verdict happen in
    # bash (MSYS needs the powershell pid probe); pass 2 re-assembles final JSON
    # via the parser so workDir paths are escaped correctly.
    if [ "$PARSER" = "jq" ]; then
        DUP_META="$(jq -r --arg id "$DUPLICATE_OF_ID" '
            ([ .sessions[]?
               | select((.id // "") == $id)
               | select(.status == "active")
             ] | first) as $c
            | if $c == null then "none", "", "", "", "", "-"
              else "found",
                   (($c.claudeShellPid // "") | tostring),
                   ($c.workDir // ""),
                   ($c.sessionRole // ""),
                   ($c.spawnedAt // ""),
                   ((($c.claudeShellPidWrittenAt // "") | tostring) | if . == "" then "-" else . end)
              end' "$REGISTRY" 2>/dev/null)"
    else
        DUP_META="$("$PARSER" -c '
import json,sys
sid=sys.argv[2]
try:
    d=json.load(open(sys.argv[1], encoding="utf-8-sig"))
except Exception:
    d={}
cand=None
for s in d.get("sessions",[]) or []:
    if (s.get("id") or "") != sid: continue
    if s.get("status")!="active": continue
    cand=s; break
if cand is None:
    print("none"); print(""); print(""); print(""); print(""); print("-")
else:
    pid=cand.get("claudeShellPid")
    print("found")
    print(pid if pid is not None else "")
    print(cand.get("workDir") or "")
    print(cand.get("sessionRole") or "")
    print(cand.get("spawnedAt") or "")
    print(cand.get("claudeShellPidWrittenAt") or "-")' "$REGISTRY" "$DUPLICATE_OF_ID" 2>/dev/null)"
    fi

    DUP_FOUND="$(printf '%s\n' "$DUP_META" | sed -n 1p)"
    DUP_PID="$(printf '%s\n' "$DUP_META" | sed -n 2p)"
    DUP_WORKDIR="$(printf '%s\n' "$DUP_META" | sed -n 3p)"
    DUP_ROLE="$(printf '%s\n' "$DUP_META" | sed -n 4p)"
    DUP_SPAWNED="$(printf '%s\n' "$DUP_META" | sed -n 5p)"
    DUP_WRITTEN="$(printf '%s\n' "$DUP_META" | sed -n 6p)"
    [ "$DUP_WRITTEN" = "-" ] && DUP_WRITTEN=""

    if [ -z "$DUP_FOUND" ]; then
        printf '{"sessionId":"%s","duplicate":false,"holder":null,"note":"unparseable registry"}\n' "$DUPLICATE_OF_ID"
        exit 0
    fi
    if [ "$DUP_FOUND" = "none" ]; then
        printf '{"sessionId":"%s","duplicate":false,"holder":null,"note":"no active registry entry for this identity"}\n' "$DUPLICATE_OF_ID"
        exit 0
    fi
    case "$DUP_PID" in
        ""|*[!0-9]*)
            printf '{"sessionId":"%s","duplicate":false,"holder":null,"note":"entry has no pid (cannot be a live duplicate)"}\n' "$DUPLICATE_OF_ID"
            exit 0
            ;;
    esac
    # SWEEP-10 / DEFER-088: identity-aware -- a recycled pid cannot fabricate a
    # live duplicate holder; absent anchor keeps the existence-only verdict.
    if ! pid_alive_as "$DUP_PID" "$DUP_WRITTEN"; then
        printf '{"sessionId":"%s","duplicate":false,"holder":null,"note":"holder pid dead (stale entry; csi-05 sweeps it)"}\n' "$DUPLICATE_OF_ID"
        exit 0
    fi

    # ---- Ancestor chain of THIS invocation (bounded, cycle-tolerant) --------
    # The caller's claude process AND its host shell are both ancestors of this
    # oracle process, so whichever of the two the registry recorded is covered.
    # MSYS: bash-side PIDs live in a separate namespace from Windows PIDs, so
    # the walk runs inside PowerShell (its Windows ancestor chain includes the
    # bash.exe processes, claude.exe, and the host shell).
    DUP_ANCESTORS=""
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*)
            # Two-stage hybrid (a pure Windows PPID walk from a deep MSYS
            # process is UNRELIABLE -- MSYS fork-stub intermediates exit and
            # leave dangling parent links that break the chain mid-walk):
            #   Stage 1: climb the MSYS-side PPID chain from this shell,
            #            collecting each ancestor's WINPID (ps column 4).
            #   Stage 2: continue the WINDOWS walk from the MSYS root's
            #            WINPID -- its parents are the long-lived native
            #            chain (claude.exe, host shell, terminal).
            _dup_cur=$$
            _dup_root_win=""
            _dup_i=0
            while [ "$_dup_i" -lt 30 ] && [ -n "$_dup_cur" ] && [ "$_dup_cur" -gt 1 ] 2>/dev/null; do
                _dup_line="$(ps -p "$_dup_cur" 2>/dev/null | awk 'NR==2 {print $2, $4}')"
                [ -z "$_dup_line" ] && break
                _dup_ppid="${_dup_line%% *}"
                _dup_win="${_dup_line##* }"
                case "$_dup_win" in
                    ""|*[!0-9]*) : ;;
                    *)
                        DUP_ANCESTORS="$DUP_ANCESTORS$_dup_win
"
                        _dup_root_win="$_dup_win"
                        ;;
                esac
                [ "$_dup_ppid" = "$_dup_cur" ] && break
                case "$_dup_ppid" in (*[!0-9]*|"") break ;; esac
                _dup_cur="$_dup_ppid"
                _dup_i=$((_dup_i + 1))
            done
            if [ -n "$_dup_root_win" ]; then
                DUP_ANCESTORS="$DUP_ANCESTORS$(powershell.exe -NoProfile -Command '
                    $cur = '"$_dup_root_win"'; $seen = @{}
                    for ($i = 0; $i -lt 30 -and $cur -gt 4; $i++) {
                        if ($seen.ContainsKey($cur)) { break }
                        $seen[$cur] = $true
                        $p = $null
                        try { $p = Get-CimInstance Win32_Process -Filter "ProcessId=$cur" -ErrorAction SilentlyContinue } catch { }
                        if (-not $p -or -not $p.ParentProcessId) { break }
                        $cur = [int]$p.ParentProcessId
                        Write-Output $cur
                    }' 2>/dev/null | tr -d '\r')"
            fi
            ;;
        *)
            _dup_cur=$$
            _dup_i=0
            while [ "$_dup_i" -lt 30 ] && [ -n "$_dup_cur" ] && [ "$_dup_cur" -gt 1 ] 2>/dev/null; do
                DUP_ANCESTORS="$DUP_ANCESTORS$_dup_cur
"
                _dup_cur="$(ps -o ppid= -p "$_dup_cur" 2>/dev/null | tr -d '[:space:]')"
                case "$_dup_cur" in (*[!0-9]*) break ;; esac
                _dup_i=$((_dup_i + 1))
            done
            ;;
    esac

    DUP_IS_SELF=0
    if printf '%s\n' "$DUP_ANCESTORS" | grep -qx "$DUP_PID"; then
        DUP_IS_SELF=1
    fi

    # ---- workDir match (normalized; case-folded on Windows) -----------------
    DUP_CWD="$_DS_PWD"
    DUP_CASEFOLD=0
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*)
            DUP_CASEFOLD=1
            if command -v cygpath >/dev/null 2>&1; then
                # -l resolves 8.3 short names (CARBON~1) to long form so the
                # comparison matches hook-written long-form workDirs.
                DUP_CWD="$(cygpath -w -l "$_DS_PWD" 2>/dev/null || cygpath -w "$_DS_PWD" 2>/dev/null || printf '%s' "$_DS_PWD")"
            fi
            ;;
    esac
    dup_norm_path() {
        # backslashes -> slashes, strip trailing slashes, optional case-fold
        # (tr, not ${var,,} -- macOS bash 3.2 portability)
        _p="$1"
        if [ "$DUP_CASEFOLD" = "1" ] && command -v cygpath >/dev/null 2>&1; then
            # Resolve 8.3 short names on either side of the comparison.
            _r="$(cygpath -w -l "$_p" 2>/dev/null)"
            [ -n "$_r" ] && _p="$_r"
        fi
        _p="$(printf '%s' "$_p" | tr '\\' '/' | sed 's:/*$::')"
        if [ "$DUP_CASEFOLD" = "1" ]; then
            _p="$(printf '%s' "$_p" | tr '[:upper:]' '[:lower:]')"
        fi
        printf '%s' "$_p"
    }
    DUP_WDM=0
    if [ -n "$DUP_WORKDIR" ] && [ "$(dup_norm_path "$DUP_WORKDIR")" = "$(dup_norm_path "$DUP_CWD")" ]; then
        DUP_WDM=1
    fi

    DUP_VERDICT=0
    if [ "$DUP_IS_SELF" = "0" ] && [ "$DUP_WDM" = "1" ]; then
        DUP_VERDICT=1
    fi
    if [ "$DUP_IS_SELF" = "1" ]; then
        DUP_NOTE="identity held by the calling session itself"
    elif [ "$DUP_WDM" = "0" ]; then
        DUP_NOTE="live holder in a different workDir (not a same-project duplicate)"
    else
        DUP_NOTE="another live session holds this identity in this workDir"
    fi

    # ---- Pass 2: assemble final JSON via the parser (correct escaping) ------
    if [ "$PARSER" = "jq" ]; then
        jq -nc --arg id "$DUPLICATE_OF_ID" --arg pid "$DUP_PID" --arg wd "$DUP_WORKDIR" \
               --arg wdm "$DUP_WDM" --arg self "$DUP_IS_SELF" --arg role "$DUP_ROLE" \
               --arg sp "$DUP_SPAWNED" --arg note "$DUP_NOTE" --arg dup "$DUP_VERDICT" '
            {sessionId: $id,
             duplicate: ($dup == "1"),
             holder: {entryId: $id,
                      claudeShellPid: ($pid | tonumber),
                      workDir: $wd,
                      workDirMatch: ($wdm == "1"),
                      sessionRole: $role,
                      spawnedAt: $sp,
                      isSelf: ($self == "1")},
             note: $note}'
    else
        "$PARSER" -c '
import json,sys
sid,pid,wd,wdm,self_,role,sp,note,dup=sys.argv[1:10]
print(json.dumps({"sessionId":sid,"duplicate":dup=="1","holder":{
    "entryId":sid,"claudeShellPid":int(pid),"workDir":wd,"workDirMatch":wdm=="1",
    "sessionRole":role,"spawnedAt":sp,"isSelf":self_=="1"},"note":note},separators=(",",":")))' \
            "$DUPLICATE_OF_ID" "$DUP_PID" "$DUP_WORKDIR" "$DUP_WDM" "$DUP_IS_SELF" \
            "$DUP_ROLE" "$DUP_SPAWNED" "$DUP_NOTE" "$DUP_VERDICT"
    fi
    exit 0
fi

# =============================================================================
# Mode dispatch
# =============================================================================
if [ "$MODE" = "gc" ] || [ "$MODE" = "gc-cheap" ] || [ "$MODE" = "dry-run" ]; then
    # -------------------------------------------------------------------------
    # CSI-05 hygiene sweep
    # --dry-run: identical candidate computation (registry read, liveness probe,
    # age filter), ZERO mutation (SWEEPER-05) -- exits before lock acquisition.
    # -------------------------------------------------------------------------

    # ---- Read threshold from .claude/config.json -----------------------------
    # Accepts numeric hours (e.g. 24, 48) OR ISO-8601 duration "PnH" / "PnD".
    THRESHOLD_HOURS=24
    THRESHOLD_SOURCE="default"
    THRESHOLD_DISPLAY="P1D"

    CONFIG=""
    if [ -f ".claude/config.json" ]; then
        CONFIG=".claude/config.json"
    fi

    if [ -n "$CONFIG" ]; then
        if [ "$PARSER" = "jq" ]; then
            CFG_RAW=$(jq -r '.csi.registryHygieneThreshold // empty' "$CONFIG" 2>/dev/null)
        else
            CFG_RAW=$("$PARSER" - "$CONFIG" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
v = (d.get("csi") or {}).get("registryHygieneThreshold")
if v is not None:
    print(v)
PY
            )
        fi
        if [ -n "$CFG_RAW" ]; then
            CFG_RAW="${CFG_RAW%$'\r'}"
            # Numeric (hours)
            case "$CFG_RAW" in
                ''|*[!0-9]*)
                    # ISO-8601 duration: PnH or PnD
                    case "$CFG_RAW" in
                        P*H)
                            v="${CFG_RAW#P}"; v="${v%H}"
                            case "$v" in (''|*[!0-9]*) ;; *) THRESHOLD_HOURS="$v"; THRESHOLD_SOURCE="config"; THRESHOLD_DISPLAY="$CFG_RAW" ;; esac
                            ;;
                        P*D)
                            v="${CFG_RAW#P}"; v="${v%D}"
                            case "$v" in (''|*[!0-9]*) ;; *) THRESHOLD_HOURS=$((v * 24)); THRESHOLD_SOURCE="config"; THRESHOLD_DISPLAY="$CFG_RAW" ;; esac
                            ;;
                    esac
                    ;;
                *)
                    THRESHOLD_HOURS="$CFG_RAW"
                    THRESHOLD_SOURCE="config"
                    THRESHOLD_DISPLAY="PT${CFG_RAW}H"
                    ;;
            esac
        fi
    fi

    NOW_EPOCH=$(date -u +%s)
    CUTOFF_EPOCH=$((NOW_EPOCH - THRESHOLD_HOURS * 3600))
    NOW_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)

    # ---- Cheap-mode budget ---------------------------------------------------
    # We bound *the per-entry liveness probe loop* to this wall-time. If we blow
    # the budget mid-scan, we abort cleanly (BUDGET_EXCEEDED -> honest deferral,
    # surfaced as budgetExceeded:true in gc mode) and let the next session-start
    # try again. Entries collected before the break are still swept, so hygiene
    # makes incremental progress across session starts. This protects the
    # briefing path.
    #
    # Three rules keep the budget from consuming itself (DEFER-064; the same
    # self-consuming-budget class as the 100ms->1000ms recalibration, DEC-79):
    #   1. The clock starts AFTER the candidates() parser fork completes --
    #      setup is O(1) and unavoidable; only the per-entry probe loop (the
    #      part that scales with registry size) is budgeted. With the clock
    #      started before the fork, MSYS fork cost alone (~1.2-2.7s measured
    #      2026-08-03) exceeded the whole budget and the sweep NEVER fired.
    #   2. The clock itself must be ~free: _ms_now prefers $EPOCHREALTIME
    #      (zero forks). The old date-fork path cost ~372ms PER CHECK on the
    #      same machine -- the measurement was eating the budget.
    #   3. Minimum-progress guarantee: the FIRST candidate is always probed
    #      (budget is checked only after at least one probe has run), so a
    #      pathological setup cost can starve at most the tail, never the
    #      whole sweep. validate.sh T8 locks this structurally.
    # ULDF_DS_GC_BUDGET_MS: test seam (validate.sh T8/T9); non-numeric or
    # empty falls back to the default.
    BUDGET_MS="${ULDF_DS_GC_BUDGET_MS:-1000}"
    case "$BUDGET_MS" in (''|*[!0-9]*) BUDGET_MS=1000 ;; esac
    if [ "$MODE" = "gc" ] || [ "$MODE" = "dry-run" ]; then BUDGET_MS=0; fi  # 0 = unbounded

    # date +%s%N is GNU; on macOS BSD date doesn't have %N. Use perl/python fallback.
    _ms_now() {
        # Zero-fork fast path: bash >= 4.4 exposes $EPOCHREALTIME ("<sec>.<usec>";
        # the separator is locale-dependent, so split on either '.' or ',').
        # Forked clocks are NOT free on MSYS (~300-500ms per fork on a busy
        # Windows host, measured 2026-08-03) -- with a forked clock the budget
        # measurement itself consumes the budget (DEFER-064).
        if [ -n "${EPOCHREALTIME:-}" ]; then
            local _er_s="${EPOCHREALTIME%%[.,]*}" _er_f="${EPOCHREALTIME#*[.,]}"
            case "$_er_s$_er_f" in (*[!0-9]*) : ;; *)
                _er_f="${_er_f}000"
                echo "$(( _er_s * 1000 + 10#${_er_f:0:3} ))"
                return 0
                ;;
            esac
        fi
        # Prefer GNU `date +%s%3N` (millisecond precision)
        local ms
        ms=$(date -u +%s%3N 2>/dev/null)
        if [ -n "$ms" ] && [ "${ms#*%}" = "$ms" ]; then
            # Strip trailing 'N' or unsupported markers; sanity check it's all digits
            case "$ms" in (*[!0-9]*) ;; *) echo "$ms"; return 0 ;; esac
        fi
        # Fallback via perl (always available on macOS)
        if command -v perl >/dev/null 2>&1; then
            perl -MTime::HiRes=time -e 'printf("%d\n", time*1000)' 2>/dev/null && return 0
        fi
        # Fallback via python (we already required one above)
        "$PARSER" -c 'import time; print(int(time.time()*1000))' 2>/dev/null
    }

    # ---- Build candidate list (sweep candidates) ----------------------------
    # Output shape (TSV per line):
    #   index<TAB>id<TAB>pid<TAB>spawnedAt<TAB>status<TAB>writtenAt
    # writtenAt is claudeShellPidWrittenAt (SWEEP-10 / DEFER-088): the identity
    # anchor for the liveness verdict below. "-" sentinel when absent, never an
    # empty field (DEFER-078: tab is IFS whitespace; an empty middle field
    # collapses under a whitespace-IFS read).
    candidates() {
        if [ "$PARSER" = "jq" ]; then
            jq -r '
                (.sessions // [])
                | to_entries
                | map(
                    select(
                        ((.value.status // "") == "active")
                        and ((.value.claudeShellPid // null) != null)
                    )
                  )
                | .[]
                | "\(.key)\t\(.value.id // "")\t\(.value.claudeShellPid)\t\(.value.spawnedAt // "")\t\(.value.status // "")\t\((((.value.claudeShellPidWrittenAt // "") | tostring) | if . == "" then "-" else . end))"
            ' "$REGISTRY" 2>/dev/null
        else
            "$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
sessions = d.get("sessions") or []
for i, s in enumerate(sessions):
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    pid = s.get("claudeShellPid")
    if pid is None: continue
    print("\t".join([str(i), s.get("id") or "", str(pid), s.get("spawnedAt") or "", s.get("status") or "", str(s.get("claudeShellPidWrittenAt") or "-")]))
PY
        fi
    }

    # ---- Field-safe TSV split (DEFER-078) -----------------------------------
    # `IFS=$'\t' read -r a b c` is BROKEN for records with an empty NON-FINAL
    # field. Tab is IFS *whitespace*, so POSIX field splitting collapses a run
    # of tabs into one delimiter, drops the empty field between them, shifts
    # every later field left, and leaves the last variable bound to nothing.
    #
    # Both candidate producers below default `id` and `spawnedAt` to "" -- i.e.
    # they ANNOUNCE that absence is expected -- and the collapsing read defeats
    # that default inside the registry PRUNE path: on an entry with no `id`,
    # `pid` binds the spawnedAt timestamp (so the liveness probe answers about
    # a non-PID), `spawned` binds the status string, and an entry the age band
    # should have protected is swept. DEFER-067 is the same class one seam over.
    #
    # Splitting explicitly on literal tabs preserves empty fields. Both
    # producers emit values that cannot themselves contain a tab (jq @tsv-style
    # interpolation of ids/pids/ISO stamps), so a literal tab is always a field
    # boundary. bash-3.2 safe by construction (macOS /bin/bash): no arrays, no
    # `readarray -d` (4.4+), no namerefs (4.3+) -- this file uses none of them.
    #
    # `${r#*<tab>}` returns the WHOLE string when no tab is present, so the
    # remainder is only stripped while a tab actually remains (the DEC-232 bug,
    # one seam over: an unguarded strip made a field-less record yield itself).
    # Sets DS_F1..DS_F6; absent trailing fields are empty, exactly as `read`
    # would leave them. A record with MORE than 6 fields leaves the remainder
    # in DS_F6, also matching `read`.
    _ds_split_tsv() {
        local _r="$1" _t
        _t=$(printf '\t')
        case "$_r" in (*"$_t"*) DS_F1="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) DS_F1="$_r"; _r="" ;; esac
        case "$_r" in (*"$_t"*) DS_F2="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) DS_F2="$_r"; _r="" ;; esac
        case "$_r" in (*"$_t"*) DS_F3="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) DS_F3="$_r"; _r="" ;; esac
        case "$_r" in (*"$_t"*) DS_F4="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) DS_F4="$_r"; _r="" ;; esac
        case "$_r" in (*"$_t"*) DS_F5="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) DS_F5="$_r"; _r="" ;; esac
        DS_F6="$_r"
    }

    # Helper: parse ISO-8601 spawnedAt -> epoch seconds. Empty input -> echo nothing.
    #
    # NEVER kills its caller. The sweep runs under `set -e`, and this helper is
    # invoked from a command substitution in an assignment -- so a failing last
    # command here terminates the WHOLE oracle. It used to: the python fallback
    # ran "$PARSER", and when PARSER is `jq` (the preferred parser) that is
    # `jq -c '<python source>'`, which exits 3. Any unparseable spawnedAt -- a
    # legacy or hand-written entry, precisely the population a sweeper meets --
    # therefore crashed `--gc-cheap` with no output on the session-start path,
    # where a silent nonzero exit is invisible. Found by the DEFER-078 fixture;
    # the fallback now uses a probed python binary (never jq) and the function
    # cannot fail. Unparseable input yields NO output, which every caller
    # already treats as "no usable age".
    _iso_to_epoch() {
        local iso="$1"
        [ -n "$iso" ] || return 0
        # Try GNU date first.
        local epoch
        epoch=$(date -u -d "$iso" +%s 2>/dev/null) && [ -n "$epoch" ] && { echo "$epoch"; return 0; }
        # macOS BSD date.
        epoch=$(date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$iso" +%s 2>/dev/null) && [ -n "$epoch" ] && { echo "$epoch"; return 0; }
        # python fallback -- only when a real python interpreter is available.
        [ -n "$PYBIN_ISO" ] || return 0
        "$PYBIN_ISO" -c "
import sys
from datetime import datetime, timezone
try:
    s = sys.argv[1].rstrip('Z')
    # Strip fractional seconds if present
    if '.' in s:
        s = s.split('.')[0]
    dt = datetime.strptime(s, '%Y-%m-%dT%H:%M:%S').replace(tzinfo=timezone.utc)
    print(int(dt.timestamp()))
except Exception:
    pass
" "$iso" 2>/dev/null || true
    }

    # ---- Iterate candidates -> dead-PID + age-threshold filter --------------
    # Capture the candidate list BEFORE starting the budget clock: the parser
    # fork is O(1) setup, not per-entry probe cost, and on MSYS it alone can
    # exceed the whole budget (DEFER-064 rule 1 above).
    CAND_TSV="$(candidates)"

    SWEEP_INDICES=""   # space-separated indices into sessions[]
    SWEEP_IDS=""       # for summary message
    BUDGET_EXCEEDED=""
    PROBED_ANY=""      # minimum-progress guarantee (DEFER-064 rule 3)

    START_MS=$(_ms_now)
    # DEFER-078: read the whole line, split explicitly. `IFS=$'\t' read` here
    # collapsed the empty-defaulted `id` / `spawnedAt` fields -- inside the path
    # that PRUNES registry entries.
    while IFS= read -r _ds_rec; do
        [ -n "$_ds_rec" ] || continue
        _ds_split_tsv "$_ds_rec"
        idx="$DS_F1"; sid="$DS_F2"; pid="$DS_F3"; spawned="$DS_F4"; status="$DS_F5"; written="$DS_F6"
        [ "$written" = "-" ] && written=""
        [ -n "$idx" ] || continue

        # Cheap-mode budget check before each probe (probes dominate cost) --
        # but never before the FIRST probe, so setup cost can defer only the
        # tail of the scan, never the whole sweep.
        if [ "$BUDGET_MS" -gt 0 ] && [ -n "$PROBED_ANY" ]; then
            NOW_MS=$(_ms_now)
            if [ -n "$START_MS" ] && [ -n "$NOW_MS" ] && [ "$((NOW_MS - START_MS))" -gt "$BUDGET_MS" ]; then
                BUDGET_EXCEEDED=1
                break
            fi
        fi

        # PID alive AND still the recorded process? -> never sweep.
        # SWEEP-10 / DEFER-088: identity-aware, so a pid the OS recycled onto an
        # unrelated process no longer pins a dead entry in `active` forever --
        # an identity refusal is positive evidence of death, which makes the
        # entry sweepable (this is the leg that self-heals every other reader).
        # Anchor is claudeShellPidWrittenAt, never spawnedAt (a PODS worker's
        # entry predates its own boot). Absent/unparseable anchor degrades to
        # the existence-only verdict -- byte-identical to the old behaviour.
        # Cost note (SWEEP-09): on MSYS a LIVE entry now costs two powershell
        # forks instead of one (liveness + identity); dead entries still cost
        # one. The budget gate above is unchanged and still bounds the tail.
        PROBED_ANY=1
        if pid_alive_as "$pid" "$written"; then
            continue
        fi

        # Age check: spawnedAt must be older than the cutoff. Missing spawnedAt
        # is treated as "old enough to sweep" (no protection band).
        if [ -n "$spawned" ]; then
            spawned_epoch=$(_iso_to_epoch "$spawned")
            if [ -n "$spawned_epoch" ]; then
                if [ "$spawned_epoch" -gt "$CUTOFF_EPOCH" ]; then
                    continue
                fi
            fi
        fi

        if [ -z "$SWEEP_INDICES" ]; then
            SWEEP_INDICES="$idx"
            SWEEP_IDS="$sid"
        else
            SWEEP_INDICES="$SWEEP_INDICES $idx"
            SWEEP_IDS="$SWEEP_IDS,$sid"
        fi
    done <<EOF_CANDIDATES
$CAND_TSV
EOF_CANDIDATES

    # ---- Get before-count for summary ---------------------------------------
    if [ "$PARSER" = "jq" ]; then
        BEFORE=$(jq '.sessions | length' "$REGISTRY" 2>/dev/null)
    else
        BEFORE=$("$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    print(0); sys.exit(0)
print(len(d.get("sessions") or []))
PY
        )
    fi
    [ -n "$BEFORE" ] || BEFORE=0

    # ---- --dry-run: report the would-sweep set and exit BEFORE the lock -----
    if [ "$MODE" = "dry-run" ]; then
        WOULD=0
        if [ -n "$SWEEP_INDICES" ]; then
            WOULD=$(echo "$SWEEP_INDICES" | wc -w | tr -d ' ')
        fi
        printf '{"dryRun":true,"wouldSweep":%s,"before":%s,"after":%s,"threshold":"%s","thresholdSource":"%s"'             "$WOULD" "$BEFORE" "$BEFORE" "$THRESHOLD_DISPLAY" "$THRESHOLD_SOURCE"
        if [ -n "$SWEEP_IDS" ]; then
            printf ',"wouldSweepIds":"%s"' "$SWEEP_IDS"
        fi
        printf '}
'
        exit 0
    fi

    SWEEP_COUNT=0
    if [ -n "$SWEEP_INDICES" ]; then
        # Convert space-separated index list to JSON array for jq.
        SWEEP_COUNT=$(echo "$SWEEP_INDICES" | wc -w | tr -d ' ')

        # ---- Acquire lock + atomic write ------------------------------------
        # We mirror registry-write.sh's lock semantics directly here because the
        # helper's csi_registry_upsert is single-entry; CSI-05 needs a multi-
        # entry move (sessions[i] -> closed[]). Same lock path/contract.
        LOCK_DIR="$REGISTRY.lock"
        LOCK_OK=""
        # 4 attempts spaced by 50ms / 200ms / 800ms (1050ms full retry budget,
        # mirrors registry-write.sh `_csi_acquire_lock` per DEC-22).
        for attempt in 1 2 3 4; do
            if mkdir "$LOCK_DIR" 2>/dev/null; then
                LOCK_OK=1
                break
            fi
            case "$attempt" in
                1) sleep 0.05 ;;
                2) sleep 0.2 ;;
                3) sleep 0.8 ;;
                4) ;;
            esac
        done
        if [ -z "$LOCK_OK" ]; then
            if [ "$MODE" = "gc" ]; then
                echo "{\"swept\":0,\"before\":$BEFORE,\"after\":$BEFORE,\"threshold\":\"$THRESHOLD_DISPLAY\",\"thresholdSource\":\"$THRESHOLD_SOURCE\",\"note\":\"lock contention\"}"
                exit 0
            else
                # cheap-mode: graceful absence
                exit 0
            fi
        fi

        TMP="$REGISTRY.tmp.$$"
        DROP_JSON="["$(echo "$SWEEP_INDICES" | tr ' ' ',')"]"

        if [ "$PARSER" = "jq" ]; then
            jq --argjson drop "$DROP_JSON" --arg now "$NOW_ISO" '
                (.sessions // []) as $orig
                | (.closed // []) as $closed
                | (
                    [ range(0; $orig | length) as $i
                      | if ($drop | index($i)) != null
                          then ($orig[$i] + {status: "expired", sweptAt: $now})
                          else empty
                        end
                    ]
                  ) as $expired
                | .sessions = (
                    [ range(0; $orig | length) as $i
                      | if ($drop | index($i)) == null then $orig[$i] else empty end
                    ]
                  )
                | .closed = ($closed + $expired)
                | .lastUpdated = $now
                | .lastPrunedAt = $now
            ' "$REGISTRY" > "$TMP" 2>/dev/null || rm -f "$TMP"
        else
            DROP_ENV="$DROP_JSON" NOW="$NOW_ISO" \
            "$PARSER" - "$REGISTRY" "$TMP" <<'PY' 2>/dev/null || rm -f "$TMP"
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(1)
if not isinstance(d, dict): sys.exit(1)
d.setdefault("sessions", []); d.setdefault("closed", [])
drop = set(json.loads(os.environ["DROP_ENV"]))
now = os.environ["NOW"]
sessions = d.get("sessions") or []
new_sessions, expired = [], []
for i, s in enumerate(sessions):
    if i in drop:
        if isinstance(s, dict):
            s2 = dict(s); s2["status"] = "expired"; s2["sweptAt"] = now
            expired.append(s2)
    else:
        new_sessions.append(s)
d["sessions"] = new_sessions
d["closed"] = (d.get("closed") or []) + expired
d["lastUpdated"] = now
d["lastPrunedAt"] = now
with open(sys.argv[2], "w", encoding="utf-8") as f:
    json.dump(d, f, indent=4)
PY
        fi

        if [ -s "$TMP" ]; then
            mv "$TMP" "$REGISTRY"
        else
            SWEEP_COUNT=0
            rm -f "$TMP" 2>/dev/null
        fi
        rmdir "$LOCK_DIR" 2>/dev/null || true

        # =====================================================================
        # CSI-13: After registry close, reconcile local LTADS state.
        # =====================================================================
        # For each newly-expired entry whose workDir matches THIS GC-running
        # session's project, flip the topmost arc in ltads/arc-state.json to
        # CONCLUDED (concludedBy.via: csi-05-gc-sweep) via the ARC-02 lib
        # (ARC-03 migration). Cross-workDir reconciliation forbidden per Phase
        # 1.6 plan -- shared-repo state is reconciled by SHARED-CSI-04 paths,
        # not here. Legacy prose-only projects: no flip (graceful; the
        # ltads-state oracle's `legacy` verdict + ARC-11 converter are the
        # migration path).
        if [ "$SWEEP_COUNT" -gt 0 ]; then
            # Resolve the registry-write lib (parallel to session-end.sh's
            # resolution: prefer in-repo dev path, fall back to deployed copy).
            CSI_LIB=""
            for cand in \
                "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../scripts/lib" 2>/dev/null && pwd)/registry-write.sh" \
                "$HOME/.claude/scripts/lib/registry-write.sh"
            do
                if [ -n "$cand" ] && [ -f "$cand" ]; then
                    CSI_LIB="$cand"
                    break
                fi
            done

            if [ -n "$CSI_LIB" ]; then
                # shellcheck disable=SC1090
                . "$CSI_LIB" 2>/dev/null || CSI_LIB=""
            fi

            if [ -n "$CSI_LIB" ]; then
                THIS_PROJECT_ROOT="$(pwd)"
                ARC_STATE_PATH="$THIS_PROJECT_ROOT/ltads/arc-state.json"

                if [ -f "$ARC_STATE_PATH" ]; then
                    # Extract the arc-owner CSI sessionId via the ARC-02 lib:
                    # the most-recent checkpoints[].by of the topmost arc --
                    # the only machine-recoverable registry key (DEC-44; JSON
                    # successor of the prose Mid-arc Checkpoint token read).
                    if ! command -v ast_get_arc_owner_id >/dev/null 2>&1; then
                        for _ast_cand in \
                            "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../scripts/lib" 2>/dev/null && pwd)/arc-state.sh" \
                            "$HOME/.claude/scripts/lib/arc-state.sh"; do
                            if [ -n "$_ast_cand" ] && [ -f "$_ast_cand" ]; then
                                # shellcheck source=/dev/null
                                . "$_ast_cand"
                                break
                            fi
                        done
                    fi
                    CS_SESSION_ID=""
                    if command -v ast_get_arc_owner_id >/dev/null 2>&1; then
                        CS_SESSION_ID="$(ast_get_arc_owner_id "$ARC_STATE_PATH")"
                    fi

                    if [ -n "$CS_SESSION_ID" ] && [ -n "$SWEEP_IDS" ]; then
                        # Check if cs.md's session id is among the just-swept ids.
                        # SWEEP_IDS is comma-separated; pad with commas for word match.
                        SID_MATCH=""
                        case ",$SWEEP_IDS," in
                            *",$CS_SESSION_ID,"*) SID_MATCH=1 ;;
                        esac

                        if [ -n "$SID_MATCH" ]; then
                            # The just-swept entry should have workDir set to
                            # this session's project root for us to flip cs.md.
                            # Look up workDir from the closed[] entry (just
                            # written this iteration). Use python directly --
                            # avoids MSYS path-arg conversion problems.
                            PY_BIN=""
                            if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
                                PY_BIN="python3"
                            elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
                                PY_BIN="python"
                            fi
                            ENTRY_WD=""
                            if [ -n "$PY_BIN" ]; then
                                ENTRY_WD="$(SID="$CS_SESSION_ID" "$PY_BIN" - "$REGISTRY" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
sid = os.environ.get("SID","")
for s in (d.get("closed") or []):
    if isinstance(s, dict) and s.get("id") == sid and s.get("status") == "expired":
        print(s.get("workDir","") or "")
        break
PY
)"
                            elif [ "$PARSER" = "jq" ]; then
                                # jq fallback: read file via stdin to avoid MSYS
                                # converting the file path argument.
                                ENTRY_WD="$(jq -r --arg sid "$CS_SESSION_ID" '
                                    (.closed // [])
                                    | map(select(.id == $sid and .status == "expired"))
                                    | .[0].workDir // ""
                                ' < "$REGISTRY" 2>/dev/null)"
                            fi

                            # Normalize: forward slashes, strip trailing slash.
                            ENTRY_WD_NORM="$(printf '%s' "$ENTRY_WD" | sed -E 's#\\#/#g; s#/+$##')"
                            PROJ_WD_NORM="$(printf '%s' "$THIS_PROJECT_ROOT" | sed -E 's#\\#/#g; s#/+$##')"

                            if [ -n "$ENTRY_WD_NORM" ] && [ "$ENTRY_WD_NORM" = "$PROJ_WD_NORM" ]; then
                                csi_flip_arc_concluded "$ARC_STATE_PATH" "$CS_SESSION_ID" "csi-05-gc-sweep" 2>/dev/null || true
                            fi
                        fi
                    fi
                fi
            fi
        fi
    fi

    AFTER=$((BEFORE - SWEEP_COUNT))

    # =========================================================================
    # DISC-PRO-13: closed[]-entry size audit (Arc 3 stream 3)
    # =========================================================================
    # One archived closed[] entry accumulated ~81 MB (DISC-PRO-13); the 803 MB
    # DISC-CSI-13 incident was the same unbounded-field class. This audit
    # bounds individual closed[] entries at ~4 KB serialized: oversized
    # entries are truncated to a bounded summary (small scalar fields kept,
    # bulk dropped, `truncated:true` + `originalBytes` recorded) so the
    # every-session registry parse cost cannot silently explode again.
    # SWEEP-08: one JSONL audit line per truncated entry is appended to
    # _registry-audit.jsonl and verified BEFORE the truncating rewrite; on
    # audit-write failure the rewrite is skipped (never destroy without a
    # recovery surface). Runs in gc AND gc-cheap (the one-time parse cost of
    # a bloated registry is exactly what the truncation eliminates forward).
    CLOSED_TRUNCATED=0
    REGISTRY_AUDIT_LOG="$(dirname "$REGISTRY")/_registry-audit.jsonl"
    CLOSED_BOUND_BYTES=4096
    if [ -f "$REGISTRY" ]; then
        # Pass 1 (read-only): oversized closed[] entries as TSV: index \t id \t bytes
        if [ "$PARSER" = "jq" ]; then
            OVERSIZED=$(jq -r --argjson bound "$CLOSED_BOUND_BYTES" '
                (.closed // [])
                | to_entries
                | map(select((.value | tojson | length) > $bound))
                | .[]
                | "\(.key)\t\(.value.id // "")\t\(.value | tojson | length)"
            ' "$REGISTRY" 2>/dev/null)
        else
            OVERSIZED=$(BOUND="$CLOSED_BOUND_BYTES" "$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
bound = int(os.environ.get("BOUND", "4096"))
for i, s in enumerate(d.get("closed") or []):
    if not isinstance(s, dict):
        continue
    n = len(json.dumps(s, ensure_ascii=False).encode("utf-8"))
    if n > bound:
        print("\t".join([str(i), str(s.get("id") or ""), str(n)]))
PY
            )
        fi

        if [ -n "$OVERSIZED" ]; then
            # SWEEP-08: audit line per oversized entry BEFORE the rewrite.
            AUDIT_OK=1
            # DEFER-078: `id // ""` is an empty-defaulted MIDDLE field here. Under
            # the collapsing read `tid` bound the byte count and `tbytes` bound
            # nothing, so the audit line below emitted `"originalBytes":` with no
            # value -- INVALID JSON written into the registry audit trail.
            while IFS= read -r _ds_rec; do
                [ -n "$_ds_rec" ] || continue
                _ds_split_tsv "$_ds_rec"
                tidx="$DS_F1"; tid="$DS_F2"; tbytes="$DS_F3"
                [ -n "$tidx" ] || continue
                tid_esc=$(printf '%s' "$tid" | sed 's/\\/\\\\/g; s/"/\\"/g')
                tline=$(printf '{"ts":"%s","kind":"closed-entry-truncated","id":"%s","index":%s,"originalBytes":%s,"boundBytes":%s}' \
                    "$NOW_ISO" "$tid_esc" "$tidx" "$tbytes" "$CLOSED_BOUND_BYTES")
                if ! printf '%s\n' "$tline" >> "$REGISTRY_AUDIT_LOG" 2>/dev/null; then
                    AUDIT_OK=""
                    break
                fi
                tlast=$(tail -n 1 "$REGISTRY_AUDIT_LOG" 2>/dev/null)
                if [ "$tlast" != "$tline" ]; then
                    AUDIT_OK=""
                    break
                fi
            done <<EOF_OVERSIZED
$OVERSIZED
EOF_OVERSIZED

            if [ -z "$AUDIT_OK" ]; then
                echo "dispatchable-sessions: _registry-audit.jsonl write failed; closed[] truncation skipped (SWEEP-08)" >&2
            else
                # Pass 2: truncating rewrite under the same mkdir-lock contract.
                CT_LOCK="$REGISTRY.lock"
                CT_LOCK_OK=""
                for ct_attempt in 1 2 3 4; do
                    if mkdir "$CT_LOCK" 2>/dev/null; then
                        CT_LOCK_OK=1
                        break
                    fi
                    case "$ct_attempt" in
                        1) sleep 0.05 ;;
                        2) sleep 0.2 ;;
                        3) sleep 0.8 ;;
                    esac
                done
                if [ -z "$CT_LOCK_OK" ]; then
                    echo "dispatchable-sessions: lock contention; closed[] truncation deferred" >&2
                else
                    CT_TMP="$REGISTRY.trunc.tmp.$$"
                    CT_RC=0
                    if [ "$PARSER" = "jq" ]; then
                        jq --argjson bound "$CLOSED_BOUND_BYTES" --arg now "$NOW_ISO" '
                            .closed = ((.closed // []) | map(
                                if (type == "object") and ((tojson | length) > $bound) then
                                    (tojson | length) as $orig
                                    | (with_entries(select(.key as $k
                                        | ["id","spawnedAt","status","sweptAt","closedAt","closedBy","role","workDir"]
                                        | index($k)))
                                       | map_values(if type == "string" then .[0:256] else . end))
                                    + {truncated: true, originalBytes: $orig, truncatedAt: $now}
                                else . end))
                            | .lastUpdated = $now
                        ' "$REGISTRY" > "$CT_TMP" 2>/dev/null || CT_RC=1
                    else
                        BOUND="$CLOSED_BOUND_BYTES" NOW="$NOW_ISO" \
                        "$PARSER" - "$REGISTRY" "$CT_TMP" <<'PY' 2>/dev/null || CT_RC=1
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(1)
if not isinstance(d, dict):
    sys.exit(1)
bound = int(os.environ.get("BOUND", "4096"))
now = os.environ["NOW"]
KEEP = ["id", "spawnedAt", "status", "sweptAt", "closedAt", "closedBy", "role", "workDir"]
closed = d.get("closed") or []
out = []
for s in closed:
    if isinstance(s, dict):
        n = len(json.dumps(s, ensure_ascii=False).encode("utf-8"))
        if n > bound:
            t = {}
            for k in KEEP:
                if k in s:
                    v = s[k]
                    if isinstance(v, str) and len(v) > 256:
                        v = v[:256]
                    t[k] = v
            t["truncated"] = True
            t["originalBytes"] = n
            t["truncatedAt"] = now
            out.append(t)
            continue
    out.append(s)
d["closed"] = out
d["lastUpdated"] = now
with open(sys.argv[2], "w", encoding="utf-8") as f:
    json.dump(d, f, indent=4)
PY
                    fi

                    if [ "$CT_RC" -eq 0 ] && [ -s "$CT_TMP" ]; then
                        if mv "$CT_TMP" "$REGISTRY" 2>/dev/null; then
                            CLOSED_TRUNCATED=$(printf '%s\n' "$OVERSIZED" | grep -c . 2>/dev/null)
                        fi
                    fi
                    rm -f "$CT_TMP" 2>/dev/null
                    rmdir "$CT_LOCK" 2>/dev/null || true
                fi
            fi
        fi
    fi

    # =========================================================================
    # SHARED-CSI-06: Cross-repo --gc-cheap extension
    # =========================================================================
    # In cheap mode, after the local sweep, iterate shared-repo registries and
    # apply the same staleness criteria (status="active" AND PID dead AND
    # spawnedAt older than threshold). Per-shared-repo budget <=50ms; if the
    # cumulative gc-cheap budget is exceeded, remaining shared sweeps skip
    # silently. Always exits 0; never blocks the briefing.
    SHARED_SWEPT=0
    if [ "$MODE" = "gc-cheap" ]; then
        SHARED_REPOS_PATHS_FOR_SWEEP=""
        # Prefer cached oracle output (set by SHARED-CSI-02 in session-start).
        # CSI-16: the cache now lives in per-session files under
        # .claude/session-state/sessions/; sharedRepos is workdir-derived (not
        # identity), so ANY session's cache is valid — take the newest. Legacy
        # shared this-session.json remains the fallback for unmigrated trees.
        # CR-stripping (`tr -d '\r'`) defends against CRLF tools on Git Bash;
        # downstream consumers use the path as a literal directory name.
        STATE_FILE=""
        for _sr_cand in $(ls -t .claude/session-state/sessions/*.json 2>/dev/null); do
            if grep -q '"sharedRepos"' "$_sr_cand" 2>/dev/null; then
                STATE_FILE="$_sr_cand"
                break
            fi
        done
        [ -n "$STATE_FILE" ] || STATE_FILE=".claude/session-state/this-session.json"
        if [ -f "$STATE_FILE" ]; then
            if [ "$PARSER" = "jq" ]; then
                SHARED_REPOS_PATHS_FOR_SWEEP=$(jq -r '.sharedRepos.repos[]?.path // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' | tr '\n' ' ')
            else
                SHARED_REPOS_PATHS_FOR_SWEEP=$("$PARSER" - "$STATE_FILE" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
sr = (d.get("sharedRepos") or {}).get("repos") or []
out = []
for r in sr:
    if isinstance(r, dict):
        p = r.get("path")
        if isinstance(p, str): out.append(p)
print(" ".join(out))
PY
                )
            fi
        fi
        # If the cache was missing or empty, fall back to invoking the discovery
        # oracle directly. Bounded: this is one extra ~20ms call at most.
        if [ -z "$SHARED_REPOS_PATHS_FOR_SWEEP" ]; then
            for cand in ".claude/oracles/workspace-shared-repos/run.sh" "claude-template/oracles/workspace-shared-repos/run.sh"; do
                if [ -f "$cand" ]; then
                    DISC_JSON=$(bash "$cand" 2>/dev/null)
                    if [ -n "$DISC_JSON" ]; then
                        if [ "$PARSER" = "jq" ]; then
                            SHARED_REPOS_PATHS_FOR_SWEEP=$(printf '%s' "$DISC_JSON" | jq -r '.repos[]?.path // empty' 2>/dev/null | tr -d '\r' | tr '\n' ' ')
                        else
                            SHARED_REPOS_PATHS_FOR_SWEEP=$(printf '%s' "$DISC_JSON" | grep -oE '"path"[[:space:]]*:[[:space:]]*"[^"]*"' | sed -E 's/^"path"[[:space:]]*:[[:space:]]*"(.*)"$/\1/' | tr -d '\r' | tr '\n' ' ')
                        fi
                    fi
                    break
                fi
            done
        fi

        # Per-repo budget is the inner gate per spec SHARED-CSI-06. The 200ms
        # value is conservative -- on Linux/macOS each iteration finishes in
        # micro-seconds via kill -0; on Git Bash + Windows each PID probe
        # spawns powershell.exe (Get-Process), which costs ~30-80ms cold-
        # start. 200ms gives a single PowerShell-based iteration headroom
        # while still bounding total work. The spec's "<=50ms" target stands
        # for production fast paths; the smoke harness on Windows needs the
        # larger envelope.
        SHARED_PER_REPO_BUDGET_MS=200
        # Shared-loop cumulative budget is independent of the local cheap-mode
        # budget (BUDGET_MS=100 above) so that local-sweep cost never starves
        # shared sweeps. 1000ms covers 5 repos at 200ms each.
        SHARED_LOOP_START_MS=$(_ms_now)
        SHARED_LOOP_BUDGET_MS=1000
        for shared_path in $SHARED_REPOS_PATHS_FOR_SWEEP; do
            [ -n "$shared_path" ] || continue

            # Cumulative shared-sweep budget gate
            CUR_MS=$(_ms_now)
            if [ -n "$SHARED_LOOP_START_MS" ] && [ -n "$CUR_MS" ] \
               && [ "$((CUR_MS - SHARED_LOOP_START_MS))" -gt "$SHARED_LOOP_BUDGET_MS" ]; then
                BUDGET_EXCEEDED=1
                break
            fi

            shared_reg="$shared_path/.claude/collaboration/active-sessions.json"
            [ -f "$shared_reg" ] || continue

            # Per-shared-repo sweep with 50ms budget. We deliberately use a
            # streamlined inline sweep (no jq/python branch divergence at this
            # call site) -- the heavyweight extraction lives in the local sweep
            # above; cross-repo sweep is a thin pass.
            SHARED_REPO_START_MS=$(_ms_now)
            shared_drop_indices=""
            shared_idx=0
            shared_sweep_now=0

            # DEFER-078: same producer, same empty-defaulted `id`/`spawnedAt` --
            # and this loop prunes ANOTHER project's registry.
            while IFS= read -r _ds_rec; do
                [ -n "$_ds_rec" ] || continue
                _ds_split_tsv "$_ds_rec"
                idx="$DS_F1"; sid="$DS_F2"; pid="$DS_F3"; spawned="$DS_F4"; status="$DS_F5"; written="$DS_F6"
                [ "$written" = "-" ] && written=""
                [ -n "$idx" ] || continue

                # Per-repo budget gate
                CUR_MS=$(_ms_now)
                if [ -n "$SHARED_REPO_START_MS" ] && [ -n "$CUR_MS" ] \
                   && [ "$((CUR_MS - SHARED_REPO_START_MS))" -gt "$SHARED_PER_REPO_BUDGET_MS" ]; then
                    break
                fi

                # Identity-aware (SWEEP-10 / DEFER-088): same verdict as the
                # local sweep -- a recycled pid is positive evidence of death,
                # so the foreign entry becomes sweepable; absent anchor keeps
                # the existence-only behaviour.
                if pid_alive_as "$pid" "$written"; then
                    continue
                fi
                if [ -n "$spawned" ]; then
                    spawned_epoch=$(_iso_to_epoch "$spawned")
                    if [ -n "$spawned_epoch" ] && [ "$spawned_epoch" -gt "$CUTOFF_EPOCH" ]; then
                        continue
                    fi
                fi
                if [ -z "$shared_drop_indices" ]; then
                    shared_drop_indices="$idx"
                else
                    shared_drop_indices="$shared_drop_indices $idx"
                fi
                shared_sweep_now=$((shared_sweep_now + 1))
            done < <(
                if [ "$PARSER" = "jq" ]; then
                    jq -r '
                        (.sessions // [])
                        | to_entries
                        | map(select(((.value.status // "") == "active") and ((.value.claudeShellPid // null) != null)))
                        | .[]
                        | "\(.key)\t\(.value.id // "")\t\(.value.claudeShellPid)\t\(.value.spawnedAt // "")\t\(.value.status // "")\t\((((.value.claudeShellPidWrittenAt // "") | tostring) | if . == "" then "-" else . end))"
                    ' "$shared_reg" 2>/dev/null
                else
                    "$PARSER" - "$shared_reg" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
sessions = d.get("sessions") or []
for i, s in enumerate(sessions):
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    pid = s.get("claudeShellPid")
    if pid is None: continue
    print("\t".join([str(i), s.get("id") or "", str(pid), s.get("spawnedAt") or "", s.get("status") or "", str(s.get("claudeShellPidWrittenAt") or "-")]))
PY
                fi
            )

            if [ -z "$shared_drop_indices" ]; then
                continue
            fi

            # Acquire lock + write (mirrors local sweep's lock primitive)
            shared_lock="$shared_reg.lock"
            shared_lock_ok=""
            for sa in 1 2 3 4; do
                if mkdir "$shared_lock" 2>/dev/null; then
                    shared_lock_ok=1
                    break
                fi
                case "$sa" in
                    1) sleep 0.05 ;;
                    2) sleep 0.2 ;;
                    3) sleep 0.8 ;;
                esac
            done
            if [ -z "$shared_lock_ok" ]; then
                continue
            fi

            shared_tmp="$shared_reg.tmp.$$"
            shared_drop_json="["$(printf '%s' "$shared_drop_indices" | tr ' ' ',')"]"

            shared_rc=0
            if [ "$PARSER" = "jq" ]; then
                jq --argjson drop "$shared_drop_json" --arg now "$NOW_ISO" '
                    (.sessions // []) as $orig
                    | (.closed // []) as $closed
                    | (
                        [ range(0; $orig | length) as $i
                          | if ($drop | index($i)) != null
                              then ($orig[$i] + {status: "expired", sweptAt: $now})
                              else empty
                            end
                        ]
                      ) as $expired
                    | .sessions = (
                        [ range(0; $orig | length) as $i
                          | if ($drop | index($i)) == null then $orig[$i] else empty end
                        ]
                      )
                    | .closed = ($closed + $expired)
                    | .lastUpdated = $now
                    | .lastPrunedAt = $now
                ' "$shared_reg" > "$shared_tmp" 2>/dev/null || shared_rc=1
            else
                DROP_ENV="$shared_drop_json" NOW="$NOW_ISO" \
                "$PARSER" - "$shared_reg" "$shared_tmp" <<'PY' 2>/dev/null || shared_rc=1
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(1)
if not isinstance(d, dict): sys.exit(1)
d.setdefault("sessions", []); d.setdefault("closed", [])
drop = set(json.loads(os.environ["DROP_ENV"]))
now = os.environ["NOW"]
sessions = d.get("sessions") or []
new_sessions, expired = [], []
for i, s in enumerate(sessions):
    if i in drop:
        if isinstance(s, dict):
            s2 = dict(s); s2["status"] = "expired"; s2["sweptAt"] = now
            expired.append(s2)
    else:
        new_sessions.append(s)
d["sessions"] = new_sessions
d["closed"] = (d.get("closed") or []) + expired
d["lastUpdated"] = now
d["lastPrunedAt"] = now
with open(sys.argv[2], "w", encoding="utf-8") as f:
    json.dump(d, f, indent=4)
PY
            fi

            if [ "$shared_rc" -eq 0 ] && [ -s "$shared_tmp" ]; then
                if mv "$shared_tmp" "$shared_reg" 2>/dev/null; then
                    SHARED_SWEPT=$((SHARED_SWEPT + shared_sweep_now))
                fi
            fi
            rm -f "$shared_tmp" 2>/dev/null
            rmdir "$shared_lock" 2>/dev/null || true
        done
    fi

    # ---- Emit summary -------------------------------------------------------
    if [ "$MODE" = "gc" ]; then
        printf '{"swept":%s,"before":%s,"after":%s,"threshold":"%s","thresholdSource":"%s"' \
            "$SWEEP_COUNT" "$BEFORE" "$AFTER" "$THRESHOLD_DISPLAY" "$THRESHOLD_SOURCE"
        if [ -n "$BUDGET_EXCEEDED" ]; then
            printf ',"budgetExceeded":true'
        fi
        if [ -n "$SWEEP_IDS" ]; then
            printf ',"sweptIds":"%s"' "$SWEEP_IDS"
        fi
        if [ "$CLOSED_TRUNCATED" -gt 0 ] 2>/dev/null; then
            printf ',"closedTruncated":%s' "$CLOSED_TRUNCATED"
        fi
        printf '}\n'
    fi
    exit 0
fi

# =============================================================================
# Default mode: briefing path
# =============================================================================

# ---- Step 0: resolve the calling session's own id (DISC-CSI-22) ----
# Precedence: ULDF_SELF_SESSION_ID (hook-set, authoritative) > CLAUDE_SESSION_ID
# (spawned-session env; may be inherited by child claudes -- DISC-CSI-21 -- but
# an inherited id excludes at worst one legitimately-registered ancestor entry,
# strictly better than reporting self as a peer). Empty => no exclusion.
SELF_SESSION_ID="${ULDF_SELF_SESSION_ID:-${CLAUDE_SESSION_ID:-}}"

# ---- Step 1: extract candidate PIDs (one per line) ----
candidate_pids() {
    if [ "$PARSER" = "jq" ]; then
        jq -r --arg self "$SELF_SESSION_ID" '
            .sessions // []
            | map(select(
                (.status // "") == "active"
                and (.dispatchable // false) == true
                and (.claudeShellPid // null) != null
                and ($self == "" or (.id // "") != $self)
              ))
            | .[]
            | ((.claudeShellPid | tostring) + "\t"
               + (((.claudeShellPidWrittenAt // "") | tostring)
                  | if . == "" then "-" else . end))
        ' "$REGISTRY" 2>/dev/null
    else
        "$PARSER" - "$REGISTRY" "$SELF_SESSION_ID" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        data = json.load(f)
except Exception:
    sys.exit(0)
self_id = sys.argv[2] if len(sys.argv) > 2 else ""
sessions = data.get("sessions", []) if isinstance(data, dict) else []
for s in sessions:
    if not isinstance(s, dict):
        continue
    if s.get("status") != "active":
        continue
    if s.get("dispatchable") is not True:
        continue
    if self_id and (s.get("id") or "") == self_id:
        continue
    pid = s.get("claudeShellPid")
    if pid is not None:
        # "-" sentinel, never an empty field: tab is IFS whitespace, so an empty
        # trailing field silently COLLAPSES on read (DEFER-078 class).
        print("%s\t%s" % (pid, s.get("claudeShellPidWrittenAt") or "-"))
PY
    fi
}

# ---- Step 2: build the list of live PIDs ----
LIVE_PIDS=""
while IFS= read -r _cand_row; do
    _cand_row="${_cand_row%$'\r'}"  # strip trailing CR (jq output on Windows tools sometimes carries CRLF)
    [ -n "$_cand_row" ] || continue
    pid="${_cand_row%%	*}"
    written="${_cand_row#*	}"
    [ "$written" = "$pid" ] && written="-"   # no tab in the row at all
    [ "$written" = "-" ] && written=""
    [ -n "$pid" ] || continue
    # SWEEP-10 (DEC-252): identity-aware, so an id the OS has RECYCLED onto an
    # unrelated process does not keep a dead session in `active` forever. The
    # anchor is claudeShellPidWrittenAt -- stamped WITH the pid -- never
    # spawnedAt, which for a PODS worker predates its own claude by design.
    if pid_alive_as "$pid" "$written"; then
        if [ -z "$LIVE_PIDS" ]; then LIVE_PIDS="$pid"; else LIVE_PIDS="$LIVE_PIDS $pid"; fi
    fi
done < <(candidate_pids)

[ -n "$LIVE_PIDS" ] || emit_empty

# ---- Step 3: emit final JSON, with the parser doing all the JSON-aware work ----
if [ "$PARSER" = "jq" ]; then
    # WT-03: peer object additively gains siblingGroup when present in the
    # registry entry. Omitted (key not added) when absent — frozen-schema
    # additive contract preserved for v1 consumers.
    # SACT-06 (DEC-240): peers additively gain intent + resourceClaim when
    # present (omitted when absent — same additive contract as siblingGroup);
    # the briefing answers WHO AND WHAT. Fixtures without the fields render
    # the v1 briefing byte-identically. Expired claims (boundUntil < now) are
    # not surfaced at all.
    NOW_ISO="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    jq -c --arg pids "$LIVE_PIDS" --arg self "$SELF_SESSION_ID" --arg now "$NOW_ISO" '
        ($pids | split(" ")) as $live
        | .sessions // []
        | map(select(
            (.status // "") == "active"
            and (.dispatchable // false) == true
            and (.claudeShellPid // null) != null
            and ((.claudeShellPid | tostring) | IN($live[]))
            and ($self == "" or (.id // "") != $self)
          ))
        | map(
            ({
                sessionId: (.id // ""),
                sessionRole: (.sessionRole // ""),
                role: (.role // ""),
                workDir: (.workDir // ""),
                claudeShellPid: .claudeShellPid,
                dispatchable: true,
                spawnedAt: (.spawnedAt // "")
            })
            + (if (.siblingGroup // "") | length > 0 then {siblingGroup: .siblingGroup} else {} end)
            + (if (.intent // "") | length > 0 then {intent: .intent} else {} end)
            + (if ((.resourceClaim // null) != null)
                  and (((.resourceClaim.boundUntil // "") == "") or (.resourceClaim.boundUntil >= $now))
                 then {resourceClaim: .resourceClaim} else {} end)
          )
        | . as $peers
        | ($peers | map(
            .sessionId + " (" + .sessionRole
            + (if (.intent // "") | length > 0
                 then ": " + (if (.intent | length) > 40 then (.intent[0:40] + "...") else .intent end)
                 else "" end)
            + ")") | join(", ")) as $labels
        | ($peers | map(select(.resourceClaim != null))
            | map(.sessionId + ":" + (.resourceClaim.resource // "") + "(" + (.resourceClaim.kind // "") + ")")
            | join(", ")) as $claims
        | {
            count: ($peers | length),
            peers: $peers,
            briefing: ((($peers | length | tostring) + " live sibling(s): " + $labels)
                       + (if $claims != "" then " | resource claims: " + $claims else "" end))
          }
    ' "$REGISTRY" 2>/dev/null || emit_empty
else
    "$PARSER" - "$REGISTRY" "$LIVE_PIDS" "$SELF_SESSION_ID" <<'PY' 2>/dev/null || emit_empty
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        data = json.load(f)
except Exception:
    print('{"count":0,"peers":[],"briefing":"No live siblings. /0-uldf-dispatch unavailable."}')
    sys.exit(0)
live = set(sys.argv[2].split())
self_id = sys.argv[3] if len(sys.argv) > 3 else ""
peers = []
for s in (data.get("sessions") or []):
    if not isinstance(s, dict):
        continue
    if s.get("status") != "active":
        continue
    if s.get("dispatchable") is not True:
        continue
    if self_id and (s.get("id") or "") == self_id:
        continue
    pid = s.get("claudeShellPid")
    if pid is None or str(pid) not in live:
        continue
    peer_obj = {
        "sessionId":      s.get("id") or "",
        "sessionRole":    s.get("sessionRole") or "",
        "role":           s.get("role") or "",
        "workDir":        s.get("workDir") or "",
        "claudeShellPid": pid,
        "dispatchable":   True,
        "spawnedAt":      s.get("spawnedAt") or "",
    }
    # WT-03: additively include siblingGroup when present (omit when absent;
    # frozen-schema additive contract for v1 consumers).
    sg = s.get("siblingGroup")
    if isinstance(sg, str) and sg:
        peer_obj["siblingGroup"] = sg
    # SACT-06 (DEC-240): additively include intent + unexpired resourceClaim.
    intent = s.get("intent")
    if isinstance(intent, str) and intent:
        peer_obj["intent"] = intent
    rc = s.get("resourceClaim")
    if isinstance(rc, dict):
        import datetime
        now = datetime.datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ")
        bu = rc.get("boundUntil") or ""
        if (not bu) or (bu >= now):
            peer_obj["resourceClaim"] = rc
    peers.append(peer_obj)
def _label(p):
    i = p.get("intent") or ""
    if i:
        i = i[:40] + "..." if len(i) > 40 else i
        return f'{p["sessionId"]} ({p["sessionRole"]}: {i})'
    return f'{p["sessionId"]} ({p["sessionRole"]})'
labels = ", ".join(_label(p) for p in peers)
claims = ", ".join(
    f'{p["sessionId"]}:{p["resourceClaim"].get("resource") or ""}({p["resourceClaim"].get("kind") or ""})'
    for p in peers if isinstance(p.get("resourceClaim"), dict))
briefing = f"{len(peers)} live sibling(s): {labels}"
if claims:
    briefing += f" | resource claims: {claims}"
result = {
    "count":    len(peers),
    "peers":    peers,
    "briefing": briefing,
}
print(json.dumps(result, separators=(",", ":")))
PY
fi
