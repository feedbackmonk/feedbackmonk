#!/bin/bash
# stale-ltads-state oracle (Unix)
#
# CSI-14 (Phase 1.6): emit a [stale-ltads-state] briefing line when the
# topmost arc in ltads/arc-state.json has status ACTIVE/PAUSED but the
# matching active-sessions.json entry is closed/expired/missing/PID-dead.
#
# Output: single-line JSON (always-fresh; ~60ms budget).
# Gracefully absent: when state is consistent, briefing field is empty so the
# session-start hook emits no line (parallel to dispatchable-sessions's
# empty-result silence).
#
# Field reading (ARC-03 migration, DEC-199; previously the prose parser lib):
#   Status + correlation id come from the ARC-02 arc-state lib
#   (scripts/lib/arc-state.sh — ast_get_status / ast_get_arc_owner_id).
#   - Status: topmost arc's `status` field (schema-validated JSON — the
#     DISC-HOOK-01/02 fictional-field class dies structurally).
#   - Correlation id: the most-recent checkpoints[].by of the topmost arc
#     (DEC-44). This — NOT the arc `id` (e.g. A042, never a registry key) —
#     is what active-sessions.json `.id` keys on. A fresh active arc with
#     zero mid-arc finalizes carries no checkpoint -> no id -> degrade to
#     consistent (stale:false). DISC-CSI-11's "committed but never closed"
#     target always has >=1 mid-arc finalize, so the checkpoint exists.
#
# Legacy window (ARC-11): a prose-only project (no arc-state.json) degrades to
# consistent — staleness detection resumes at conversion; the ltads-state
# oracle's `legacy` verdict is the surfacing signal for the migration itself.

set +e

ARC_STATE="ltads/arc-state.json"
REGISTRY=".claude/collaboration/active-sessions.json"

# ---- Source the ARC-02 arc-state lib ----------------------------------------
# Probe order mirrors pid-orphan-detector's lib resolution: deployed project
# (.claude/oracles/<o>/ -> .claude/scripts/lib/), template repo, global.
_THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for _ast_cand in \
    "$_THIS_DIR/../../scripts/lib/arc-state.sh" \
    "$_THIS_DIR/../../../claude-template/scripts/lib/arc-state.sh" \
    "$HOME/.claude/scripts/lib/arc-state.sh"; do
    if [ -f "$_ast_cand" ]; then
        # shellcheck source=/dev/null
        . "$_ast_cand"
        break
    fi
done

# ---- SWEEP-10 / DEFER-095: identity-aware liveness via lib/pid-liveness.sh --
# A recycled owner pid (live process, started after the entry's own
# claudeShellPidWrittenAt anchor) defended a stale ACTIVE arc forever -- the
# exact staleness this oracle exists to surface. Identity-refused -> dead ->
# registry-pid-dead-state-active. Anchor-only, no name glob (DEC-257); absent/
# unparseable anchor or lib unavailable -> byte-identical existence-only
# verdict. Path is REPORTABLE (QUIESCE-08 W4).
SLS_PID_IDENTITY="fallback"
for _sls_pl in \
    "$_THIS_DIR/../../scripts/lib/pid-liveness.sh" \
    "$_THIS_DIR/../../../claude-template/scripts/lib/pid-liveness.sh" \
    "$HOME/.claude/scripts/lib/pid-liveness.sh"; do
    if [ -f "$_sls_pl" ]; then
        # shellcheck source=/dev/null
        . "$_sls_pl" 2>/dev/null || true
        break
    fi
done
command -v pid_is_alive_as >/dev/null 2>&1 && SLS_PID_IDENTITY="lib"
if [ "${ULDF_SLS_REPORT_PID_IDENTITY:-}" = "1" ]; then
    printf '%s\n' "$SLS_PID_IDENTITY"
    exit 0
fi

# ---- JSON string escaping (used by every emitter below) ---------------------
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# ---- Default empty/consistent output ---------------------------------------
emit_consistent() {
    local status_json="$1"   # JSON string for current_session_status (e.g. '"ACTIVE"' or 'null')
    local sid_json="$2"      # JSON string for current_session_id
    cat <<EOF
{"evaluable":true,"stale":false,"details":{"current_session_status":$status_json,"current_session_id":$sid_json,"registry_status":"active","registry_pid_alive":null,"inconsistency_kind":"none","unevaluable_reason":null},"briefing":""}
EOF
    exit 0
}

# ---- CANNOT-EVALUATE output (DEFER-204) ------------------------------------
# An unresolvable DEPENDENCY is not a graceful absence: graceful absence means
# "asked, and there is nothing to compare"; this means "could not ask at all".
# Rendering it as stale:false published a clean bill of health earned by no
# measurement -- the reassuring direction, which is worse than a plainly absent
# check because the briefing line is byte-identical to a real all-clear. Mirrors
# concurrent-mutation's evaluable:false contract (CSI-36/DEC-342) and OVALID-05:
# an oracle DEFERS an assertion it cannot make, it never weakens it.
emit_unevaluable() {
    local reason="$1"      # short machine-readable reason
    local briefing="$2"    # human line for the ORACLE BRIEFING (never empty)
    cat <<EOF
{"evaluable":false,"stale":null,"details":{"current_session_status":null,"current_session_id":null,"registry_status":null,"registry_pid_alive":null,"inconsistency_kind":null,"unevaluable_reason":"$(esc "$reason")"},"briefing":"$(esc "$briefing")"}
EOF
    exit 0
}

# ---- Graceful absence: no arc record -> nothing to compare ------------------
# (Legacy prose-only projects land here too — see header.)
if [ ! -f "$ARC_STATE" ]; then
    emit_consistent "null" "null"
fi

# ---- Read topmost arc status via the ARC-02 lib -----------------------------
# There IS an arc record here (checked above), so the question is live and the
# lib is the only way to read it. Unresolvable lib -> UNEVALUABLE, never
# "consistent" (see emit_unevaluable's header). The remedy is named in the
# briefing because the witnessed instance (SessionHelm, 2026-08-18) was a
# project whose installed .claude/oracles/ still dot-sourced the parser lib
# retired by ARC-04 -- a stale install, fixed by refreshing the oracles.
if ! command -v ast_get_status >/dev/null 2>&1; then
    echo "stale-ltads-state: arc-state.sh could not be resolved" >&2
    emit_unevaluable "arc-state-lib-unresolvable"         "stale-ltads-state could not evaluate: the arc-state lib (scripts/lib/arc-state.sh) could not be resolved, so LTADS/registry consistency is UNKNOWN -- not clear. Refresh this project's oracles (/0-uldf-migrate-oracles) or re-sync ~/.claude."
fi
status_value=$(ast_get_status "$ARC_STATE")

status_json="null"
if [ -n "$status_value" ]; then status_json="\"$(esc "$status_value")\""; fi

# Only ACTIVE/PAUSED warrant the inconsistency check (IN_PROGRESS was prose-era
# vocabulary; the schema normalizes it to ACTIVE at migration).
case "$status_value" in
    ACTIVE|PAUSED) ;;
    *) emit_consistent "$status_json" "null" ;;
esac

# ---- Derive the correlation sessionId from the topmost arc's checkpoints ----
# Most-recent checkpoints[].by (DEC-44); `unknown`/empty -> "".
session_id=$(ast_get_arc_owner_id "$ARC_STATE")

# No recoverable owner id -> can't correlate; degrade to consistent (no false
# positive). The file self-heals on its next mid-arc finalize, which appends a
# checkpoint carrying `by`.
if [ -z "$session_id" ]; then
    emit_consistent "$status_json" "null"
fi

sid_json="\"$(esc "$session_id")\""

# ---- No registry -> session never registered; emit missing ------------------
if [ ! -f "$REGISTRY" ]; then
    cat <<EOF
{"evaluable":true,"stale":true,"details":{"current_session_status":$status_json,"current_session_id":$sid_json,"registry_status":"missing","registry_pid_alive":null,"inconsistency_kind":"registry-missing-state-active","unevaluable_reason":null},"briefing":"arc-state.json topmost arc: $status_value (arc owner $session_id) but active-sessions.json missing"}
EOF
    exit 0
fi

# ---- Find matching registry entry (active, closed, or absent) --------------
# Pick parser: jq preferred, python fallback. Probe-verify python (Windows
# Microsoft Store stub silently exits non-zero).
parser=""
if command -v jq >/dev/null 2>&1; then
    parser="jq"
elif command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
    parser="python3"
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
    parser="python"
fi

if [ -z "$parser" ]; then
    # No parser -> graceful absence (don't emit a stale signal we can't verify)
    emit_consistent "$status_json" "$sid_json"
fi

reg_status=""
reg_pid=""
reg_anchor=""
if [ "$parser" = "jq" ]; then
    reg_status="$(jq -r --arg sid "$session_id" '
        ((.sessions // []) | map(select(.id == $sid)) | .[0]) as $a
        | ((.closed   // []) | map(select(.id == $sid)) | .[0]) as $c
        | if   $a then "active"
          elif $c then ($c.status // "closed")
          else "missing"
          end
    ' "$REGISTRY" 2>/dev/null)"
    if [ "$reg_status" = "active" ]; then
        reg_pid="$(jq -r --arg sid "$session_id" '
            (.sessions // []) | map(select(.id == $sid)) | .[0].claudeShellPid // ""
        ' "$REGISTRY" 2>/dev/null)"
        # DEFER-095: the identity anchor for the pid probe (empty when absent).
        reg_anchor="$(jq -r --arg sid "$session_id" '
            (.sessions // []) | map(select(.id == $sid)) | .[0].claudeShellPidWrittenAt // ""
        ' "$REGISTRY" 2>/dev/null)"
    fi
else
    out="$(SID="$session_id" "$parser" - "$REGISTRY" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    print("missing\t\t")
    sys.exit(0)
if not isinstance(d, dict):
    print("missing\t\t")
    sys.exit(0)
sid = os.environ["SID"]
for s in (d.get("sessions") or []):
    if isinstance(s, dict) and s.get("id") == sid:
        pid = s.get("claudeShellPid", "")
        wa = s.get("claudeShellPidWrittenAt") or ""
        print(f"active\t{pid}\t{wa}")
        sys.exit(0)
for s in (d.get("closed") or []):
    if isinstance(s, dict) and s.get("id") == sid:
        st = s.get("status", "closed")
        print(f"{st}\t\t")
        sys.exit(0)
print("missing\t\t")
PY
)"
    reg_status="${out%%	*}"
    _sls_rest="${out#*	}"
    # `${var#*<tab>}` returns the WHOLE string when no tab remains (DEC-232
    # class) -- split only while a tab is present.
    case "$_sls_rest" in
        *"	"*) reg_pid="${_sls_rest%%	*}"; reg_anchor="${_sls_rest#*	}" ;;
        *)      reg_pid="$_sls_rest"; reg_anchor="" ;;
    esac
fi

[ -z "$reg_status" ] && reg_status="missing"

# ---- Determine inconsistency kind ------------------------------------------
inconsistency_kind="none"
pid_alive_json="null"

# TWIN-03 (DEFER-042): registry PIDs are NATIVE Windows PIDs; Git-Bash
# `kill -0` cannot see those, so a raw probe false-reported live sessions dead
# (spurious registry-pid-dead-state-active). Platform-branched probe (inline —
# oracles are self-contained; same pattern as dispatchable-sessions).
_PID_PROBE="kill"
case "$(uname -s 2>/dev/null)" in MINGW*|MSYS*|CYGWIN*) _PID_PROBE="powershell" ;; esac
_pid_alive() {
    case "$1" in ''|*[!0-9]*) return 1 ;; esac
    if [ "$_PID_PROBE" = "powershell" ]; then
        powershell.exe -NoProfile -Command "if (Get-Process -Id $1 -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1 </dev/null
    else
        kill -0 "$1" 2>/dev/null
    fi
}

case "$reg_status" in
    active)
        # Check if registered PID is alive. If not, that's a stale-state
        # signal (session died but no GC ran yet AND the arc is still ACTIVE).
        # DEFER-095: "alive" is identity-aware -- a recycled pid (started after
        # the anchor) reads dead; absent anchor/lib -> existence-only.
        if [ -n "$reg_pid" ] && [ "$reg_pid" != "0" ]; then
            if { [ "$SLS_PID_IDENTITY" = "lib" ] && pid_is_alive_as "$reg_pid" "$reg_anchor"; } \
               || { [ "$SLS_PID_IDENTITY" != "lib" ] && _pid_alive "$reg_pid"; }; then
                pid_alive_json="true"
            else
                pid_alive_json="false"
                inconsistency_kind="registry-pid-dead-state-active"
            fi
        fi
        ;;
    closed)
        inconsistency_kind="registry-closed-state-active"
        ;;
    expired)
        inconsistency_kind="registry-expired-state-active"
        ;;
    missing)
        inconsistency_kind="registry-missing-state-active"
        ;;
esac

if [ "$inconsistency_kind" = "none" ]; then
    emit_consistent "$status_json" "$sid_json"
fi

# ---- Compose briefing -------------------------------------------------------
case "$inconsistency_kind" in
    registry-closed-state-active)
        briefing="arc-state.json topmost arc: $status_value (arc owner $session_id) but registry shows that session as CLOSED -- run /0-uldf-finalize --complete-arc to reconcile"
        ;;
    registry-expired-state-active)
        briefing="arc-state.json topmost arc: $status_value (arc owner $session_id) but registry shows that session as EXPIRED (CSI-05 GC swept it) -- state should have been auto-flipped by CSI-13"
        ;;
    registry-pid-dead-state-active)
        briefing="arc-state.json topmost arc: $status_value (arc owner $session_id) but that session's PID is dead -- next GC sweep will reconcile, or run /0-uldf-finalize manually"
        ;;
    registry-missing-state-active)
        briefing="arc-state.json topmost arc: $status_value (arc owner $session_id) but no matching registry entry -- that session never registered or the registry was reset"
        ;;
    *)
        briefing="stale-ltads-state inconsistency"
        ;;
esac

cat <<EOF
{"evaluable":true,"stale":true,"details":{"current_session_status":$status_json,"current_session_id":$sid_json,"registry_status":"$reg_status","registry_pid_alive":$pid_alive_json,"inconsistency_kind":"$inconsistency_kind","unevaluable_reason":null},"briefing":"$(esc "$briefing")"}
EOF
exit 0
