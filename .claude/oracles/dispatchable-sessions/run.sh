#!/bin/bash
# dispatchable-sessions oracle (Unix)
# Answers: what live sibling sessions can THIS session dispatch work to right now?
#
# Reads .claude/collaboration/active-sessions.json (with ltads/sessions/active-sessions.json
# as legacy fallback) and emits a JSON object with:
#   - count   : integer, number of live dispatchable peers
#   - peers   : array of {sessionId, sessionRole, role, workDir, claudeShellPid, dispatchable, spawnedAt, siblingGroup?}
#   - briefing: human-readable one-line summary for the session-start ORACLE BRIEFING
#
# Filter: status=="active" AND dispatchable==true AND claudeShellPid!=null AND PID alive.
# Legacy entries (no registryVersion or registryVersion=1) silently drop -- they predate dispatch.
# Strategy: always-fresh. Read-only on the registry (no mutation; stale-cleanup is a separate path).
#
# Modes (CSI-05 added --gc, --gc-cheap):
#   (default)    : read-only briefing path described above
#   --gc-cheap   : session-start hygiene sweep, ~100ms budget, defers if exceeded
#   --gc         : on-demand hygiene sweep, no time budget, prints {swept,before,after,threshold,thresholdSource}
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
case "${1:-}" in
    --gc)        MODE="gc" ;;
    --gc-cheap)  MODE="gc-cheap" ;;
    "")          MODE="briefing" ;;
    *)
        echo "dispatchable-sessions: unknown mode: $1" >&2
        echo "  usage: run.sh [--gc|--gc-cheap]" >&2
        exit 1
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
elif [ -f "ltads/sessions/active-sessions.json" ]; then
    REGISTRY="$_DS_PWD/ltads/sessions/active-sessions.json"
fi

if [ -z "$REGISTRY" ]; then
    if [ "$MODE" = "briefing" ]; then
        emit_empty
    elif [ "$MODE" = "gc" ]; then
        echo '{"swept":0,"before":0,"after":0,"threshold":"P1D","thresholdSource":"default","note":"no registry"}'
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
if [ -z "$PARSER" ]; then
    if [ "$MODE" = "briefing" ]; then
        emit_empty
    elif [ "$MODE" = "gc" ]; then
        echo '{"swept":0,"before":0,"after":0,"threshold":"P1D","thresholdSource":"default","note":"no JSON parser"}'
        exit 0
    else
        exit 0
    fi
fi

# =============================================================================
# Mode dispatch
# =============================================================================
if [ "$MODE" = "gc" ] || [ "$MODE" = "gc-cheap" ]; then
    # -------------------------------------------------------------------------
    # CSI-05 hygiene sweep
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
    with open(sys.argv[1], "r", encoding="utf-8") as f:
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
    # We bound *the per-entry liveness probe loop* to 100ms wall-time. If we
    # blow the budget mid-scan, we abort cleanly and let the next session-start
    # try again. This protects the briefing path.
    BUDGET_MS=100
    if [ "$MODE" = "gc" ]; then BUDGET_MS=0; fi  # 0 = unbounded

    # date +%s%N is GNU; on macOS BSD date doesn't have %N. Use perl/python fallback.
    _ms_now() {
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

    START_MS=$(_ms_now)

    # ---- Build candidate list (sweep candidates) ----------------------------
    # Output shape (TSV per line): index<TAB>id<TAB>pid<TAB>spawnedAt<TAB>status
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
                | "\(.key)\t\(.value.id // "")\t\(.value.claudeShellPid)\t\(.value.spawnedAt // "")\t\(.value.status // "")"
            ' "$REGISTRY" 2>/dev/null
        else
            "$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
sessions = d.get("sessions") or []
for i, s in enumerate(sessions):
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    pid = s.get("claudeShellPid")
    if pid is None: continue
    print("\t".join([str(i), s.get("id") or "", str(pid), s.get("spawnedAt") or "", s.get("status") or ""]))
PY
        fi
    }

    # Helper: parse ISO-8601 spawnedAt -> epoch seconds. Empty input -> echo nothing.
    _iso_to_epoch() {
        local iso="$1"
        [ -n "$iso" ] || return 0
        # Try GNU date first.
        local epoch
        epoch=$(date -u -d "$iso" +%s 2>/dev/null) && [ -n "$epoch" ] && { echo "$epoch"; return 0; }
        # macOS BSD date.
        epoch=$(date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$iso" +%s 2>/dev/null) && [ -n "$epoch" ] && { echo "$epoch"; return 0; }
        # python fallback
        "$PARSER" -c "
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
" "$iso" 2>/dev/null
    }

    # ---- Iterate candidates -> dead-PID + age-threshold filter --------------
    SWEEP_INDICES=""   # space-separated indices into sessions[]
    SWEEP_IDS=""       # for summary message
    BUDGET_EXCEEDED=""

    while IFS=$'\t' read -r idx sid pid spawned status; do
        [ -n "$idx" ] || continue

        # Cheap-mode budget check before each probe (probes dominate cost).
        if [ "$BUDGET_MS" -gt 0 ]; then
            NOW_MS=$(_ms_now)
            if [ -n "$START_MS" ] && [ -n "$NOW_MS" ] && [ "$((NOW_MS - START_MS))" -gt "$BUDGET_MS" ]; then
                BUDGET_EXCEEDED=1
                break
            fi
        fi

        # PID alive? -> never sweep
        if pid_alive "$pid"; then
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
    done < <(candidates)

    # ---- Get before-count for summary ---------------------------------------
    if [ "$PARSER" = "jq" ]; then
        BEFORE=$(jq '.sessions | length' "$REGISTRY" 2>/dev/null)
    else
        BEFORE=$("$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    print(0); sys.exit(0)
print(len(d.get("sessions") or []))
PY
        )
    fi
    [ -n "$BEFORE" ] || BEFORE=0

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
    with open(sys.argv[1], "r", encoding="utf-8") as f:
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
        # session's project, flip the matching ltads/sessions/current-session.md
        # to CONCLUDED (Concluded-By: csi-05-gc-sweep). Cross-workDir
        # reconciliation forbidden per Phase 1.6 plan -- shared-repo state is
        # reconciled by SHARED-CSI-04 paths, not here.
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
                CS_MD_PATH="$THIS_PROJECT_ROOT/ltads/sessions/current-session.md"

                if [ -f "$CS_MD_PATH" ]; then
                    # Extract the session id recorded in current-session.md.
                    CS_SESSION_ID="$(grep -m 1 -E '^Session:[[:space:]]*' "$CS_MD_PATH" 2>/dev/null | sed -E 's/^Session:[[:space:]]*([^[:space:]]+).*/\1/')"

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
    with open(sys.argv[1], "r", encoding="utf-8") as f:
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
                                csi_flip_current_session_concluded "$CS_MD_PATH" "$CS_SESSION_ID" "csi-05-gc-sweep" 2>/dev/null || true
                            fi
                        fi
                    fi
                fi
            fi
        fi
    fi

    AFTER=$((BEFORE - SWEEP_COUNT))

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
        # CR-stripping (`tr -d '\r'`) defends against CRLF tools on Git Bash;
        # downstream consumers use the path as a literal directory name.
        STATE_FILE=".claude/session-state/this-session.json"
        if [ -f "$STATE_FILE" ]; then
            if [ "$PARSER" = "jq" ]; then
                SHARED_REPOS_PATHS_FOR_SWEEP=$(jq -r '.sharedRepos.repos[]?.path // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' | tr '\n' ' ')
            else
                SHARED_REPOS_PATHS_FOR_SWEEP=$("$PARSER" - "$STATE_FILE" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
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

            while IFS=$'\t' read -r idx sid pid spawned status; do
                [ -n "$idx" ] || continue

                # Per-repo budget gate
                CUR_MS=$(_ms_now)
                if [ -n "$SHARED_REPO_START_MS" ] && [ -n "$CUR_MS" ] \
                   && [ "$((CUR_MS - SHARED_REPO_START_MS))" -gt "$SHARED_PER_REPO_BUDGET_MS" ]; then
                    break
                fi

                if pid_alive "$pid"; then
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
                        | "\(.key)\t\(.value.id // "")\t\(.value.claudeShellPid)\t\(.value.spawnedAt // "")\t\(.value.status // "")"
                    ' "$shared_reg" 2>/dev/null
                else
                    "$PARSER" - "$shared_reg" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
sessions = d.get("sessions") or []
for i, s in enumerate(sessions):
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    pid = s.get("claudeShellPid")
    if pid is None: continue
    print("\t".join([str(i), s.get("id") or "", str(pid), s.get("spawnedAt") or "", s.get("status") or ""]))
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
    with open(sys.argv[1], "r", encoding="utf-8") as f:
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
        printf '}\n'
    fi
    exit 0
fi

# =============================================================================
# Default mode: briefing path
# =============================================================================

# ---- Step 1: extract candidate PIDs (one per line) ----
candidate_pids() {
    if [ "$PARSER" = "jq" ]; then
        jq -r '
            .sessions // []
            | map(select(
                (.status // "") == "active"
                and (.dispatchable // false) == true
                and (.claudeShellPid // null) != null
              ))
            | .[].claudeShellPid
        ' "$REGISTRY" 2>/dev/null
    else
        "$PARSER" - "$REGISTRY" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        data = json.load(f)
except Exception:
    sys.exit(0)
sessions = data.get("sessions", []) if isinstance(data, dict) else []
for s in sessions:
    if not isinstance(s, dict):
        continue
    if s.get("status") != "active":
        continue
    if s.get("dispatchable") is not True:
        continue
    pid = s.get("claudeShellPid")
    if pid is not None:
        print(pid)
PY
    fi
}

# ---- Step 2: build the list of live PIDs ----
LIVE_PIDS=""
while IFS= read -r pid; do
    pid="${pid%$'\r'}"  # strip trailing CR (jq output on Windows tools sometimes carries CRLF)
    [ -n "$pid" ] || continue
    if pid_alive "$pid"; then
        if [ -z "$LIVE_PIDS" ]; then LIVE_PIDS="$pid"; else LIVE_PIDS="$LIVE_PIDS $pid"; fi
    fi
done < <(candidate_pids)

[ -n "$LIVE_PIDS" ] || emit_empty

# ---- Step 3: emit final JSON, with the parser doing all the JSON-aware work ----
if [ "$PARSER" = "jq" ]; then
    # WT-03: peer object additively gains siblingGroup when present in the
    # registry entry. Omitted (key not added) when absent — frozen-schema
    # additive contract preserved for v1 consumers.
    jq -c --arg pids "$LIVE_PIDS" '
        ($pids | split(" ")) as $live
        | .sessions // []
        | map(select(
            (.status // "") == "active"
            and (.dispatchable // false) == true
            and (.claudeShellPid // null) != null
            and ((.claudeShellPid | tostring) | IN($live[]))
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
          )
        | . as $peers
        | ($peers | map(.sessionId + " (" + .sessionRole + ")") | join(", ")) as $labels
        | {
            count: ($peers | length),
            peers: $peers,
            briefing: (($peers | length | tostring) + " live sibling(s): " + $labels)
          }
    ' "$REGISTRY" 2>/dev/null || emit_empty
else
    "$PARSER" - "$REGISTRY" "$LIVE_PIDS" <<'PY' 2>/dev/null || emit_empty
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        data = json.load(f)
except Exception:
    print('{"count":0,"peers":[],"briefing":"No live siblings. /0-uldf-dispatch unavailable."}')
    sys.exit(0)
live = set(sys.argv[2].split())
peers = []
for s in (data.get("sessions") or []):
    if not isinstance(s, dict):
        continue
    if s.get("status") != "active":
        continue
    if s.get("dispatchable") is not True:
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
    peers.append(peer_obj)
labels = ", ".join(f'{p["sessionId"]} ({p["sessionRole"]})' for p in peers)
result = {
    "count":    len(peers),
    "peers":    peers,
    "briefing": f"{len(peers)} live sibling(s): {labels}",
}
print(json.dumps(result, separators=(",", ":")))
PY
fi
