#!/bin/bash
# ltads-state oracle (Unix)
# Formalized from the state detection originally embedded in session-start.sh.
# Reports the LTADS state: none / permanent / temporary / legacy / incomplete_temp / broken
#
# ARC-01/ARC-06 (DEC-199): the oracle is also the arc-state.json VALIDATOR —
# additive output fields `arc_state` (valid|invalid|legacy|absent|unvalidated),
# `arc_mode` (FRESH|CONTINUATION per the ARC-06 two-fact rule), `topmost_arc`
# ({id, status, boundConsentExpired}) and `size_warning` (ARC-01 cap-approach
# WARN). When arc-state.json is valid, session_id/session_status come from the
# topmost arc; otherwise the legacy prose read below still serves them, and a
# prose-only project reports `arc_state: "legacy"` (migration pending, ARC-11).
#
# Arc-state fields are read via the single-writer lib's getters
# (scripts/lib/arc-state.sh, ARC-02) — never hand-rolled. The prose parser lib
# retired per ARC-04: on a `legacy` project session_status is unavailable
# (empty) — the `legacy` verdict itself (file presence) is the load-bearing
# signal, surfacing the ARC-11 converter offer.

set -e

LTADS_PATH="ltads"

# ---- Source the arc-state lib -----------------------------------------------
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

# No ltads/ directory = none
if [ ! -d "$LTADS_PATH" ]; then
    echo '{"state":"none","has_ltads_dir":false,"is_tracked":false,"config_exists":false,"is_temporary":false,"cleanup_candidate":false,"session_id":null,"session_status":null,"summary":"No LTADS on this project","arc_state":"absent","arc_mode":"FRESH","topmost_arc":null,"size_warning":false}'
    exit 0
fi

# Read config.json
config_exists=false
is_temporary=false
if [ -f "$LTADS_PATH/config.json" ]; then
    config_exists=true
    if grep -qE '"temporary"[[:space:]]*:[[:space:]]*true' "$LTADS_PATH/config.json" 2>/dev/null; then
        is_temporary=true
    fi
fi

# ---- arc-state.json validation + topmost-arc report (ARC-01/ARC-06) --------
arc_state="absent"
arc_mode="FRESH"
topmost_json="null"
size_warning=false
arc_state_path="$LTADS_PATH/arc-state.json"
current_session_path="$LTADS_PATH/sessions/current-session.md"
if [ -f "$arc_state_path" ]; then
    if ! command -v jq >/dev/null 2>&1 || ! command -v ast_validate >/dev/null 2>&1; then
        # NO-DATA, never false-green (and never a false "invalid").
        echo "ltads-state: arc-state.sh lib or jq unavailable; arc-state.json cannot be validated" >&2
        arc_state="unvalidated"
        arc_mode=""
    else
        arc_state="$(ast_validate "$arc_state_path")"
        if [ "$arc_state" = "valid" ]; then
            arc_mode="$(ast_get_arc_mode "$arc_state_path")"
            topmost_json="$(jq -c 'if (.arcs | length) > 0 then {id: .arcs[0].id, status: .arcs[0].status, boundConsentExpired: (.arcs[0].boundConsent.expired // null)} else null end' "$arc_state_path" 2>/dev/null)"
            [ -n "$topmost_json" ] || topmost_json="null"
        else
            arc_mode=""   # malformed: never guess a session mode (ARC-06)
        fi
        # ARC-01 size-cap approach WARN (>80% of cap; breach is lib-rotated).
        _cap="${ULDF_ARC_STATE_SIZE_CAP:-4096}"
        _size=$(wc -c < "$arc_state_path" 2>/dev/null | tr -d '[:space:]')
        if [ -n "$_size" ] && [ "$_size" -gt $((_cap * 8 / 10)) ] 2>/dev/null; then
            size_warning=true
        fi
    fi
elif [ -f "$current_session_path" ]; then
    # Prose-only arc record: the ARC-11 migration-pending surfacing trigger.
    arc_state="legacy"
    arc_mode=""
fi

# Read current-session.md (legacy prose read; superseded by arc-state when valid)
session_status=""
session_id=""
if [ "$arc_state" = "valid" ] && [ "$topmost_json" != "null" ]; then
    session_id="$(ast_get_arc_id "$arc_state_path")"
    session_status="$(ast_get_status "$arc_state_path")"
elif [ -f "$current_session_path" ]; then
    # Legacy prose project (ARC-04: parser lib retired): session_status is
    # deliberately unavailable — the `legacy` arc_state verdict below is the
    # load-bearing signal; the ARC-11 converter is the migration path.
    session_id=$(grep -oE '^-?[[:space:]]*\*\*ID\*\*[[:space:]]*:[[:space:]]*[^[:space:]].*' "$current_session_path" 2>/dev/null | head -1 | sed -E 's/^-?[[:space:]]*\*\*ID\*\*[[:space:]]*:[[:space:]]*//')
fi

# Check git tracking
is_tracked=false
if command -v git >/dev/null 2>&1; then
    tracked_files=$(git ls-files "$LTADS_PATH/" 2>/dev/null || echo "")
    if [ -n "$tracked_files" ]; then
        is_tracked=true
    fi
fi

# Spec exhaustion (LTADS-GC-01): parse the "True Progress: X/Y" header of
# spec-progress.md. Exhausted iff X == Y and Y > 0. Missing/unparseable -> not
# exhausted (conservative).
spec_exhausted=false
spec_progress_path="$LTADS_PATH/execution/spec-progress.md"
if [ -f "$spec_progress_path" ]; then
    # `|| true`: under `set -e` a no-match grep (exit 1) in this assignment
    # would kill the script before the [ -n "$tp" ] guard runs — the guard IS
    # the intended missing-header handling (DISC-001, WinTailor 2026-07-27).
    tp=$(grep -oE 'True Progress\**[[:space:]]*:?[[:space:]]*[0-9]+/[0-9]+' "$spec_progress_path" 2>/dev/null | head -1 | grep -oE '[0-9]+/[0-9]+' || true)
    if [ -n "$tp" ]; then
        tp_done="${tp%%/*}"
        tp_total="${tp##*/}"
        if [ "$tp_total" -gt 0 ] 2>/dev/null && [ "$tp_done" -eq "$tp_total" ] 2>/dev/null; then
            spec_exhausted=true
        fi
    fi
fi

# Classify
state="broken"
summary=""
cleanup_candidate=false
if [ "$config_exists" = "false" ]; then
    if [ "$is_tracked" = "true" ]; then
        state="legacy"
        summary="Legacy permanent LTADS (no config.json, tracked in git). Run /0-uldf-ltads-admin init to upgrade."
    else
        state="incomplete_temp"
        # Orphan from failed auto-init: always a cleanup candidate; the shared
        # cleanup snippet ALWAYS prompts for this state (never auto-deletes).
        cleanup_candidate=true
        summary="Incomplete temporary state (no config.json, untracked). Cleanup candidate - offer ~/.claude/segments/-ltads/_temp-ltads-cleanup.md (always prompts for orphans)."
    fi
elif [ "$is_temporary" = "true" ]; then
    state="temporary"
    summary="Temporary LTADS"
    if [ -n "$session_id" ]; then summary="$summary, session $session_id"; fi
    if [ -n "$session_status" ]; then summary="$summary ($session_status)"; fi
    if [ "$spec_exhausted" = "true" ]; then
        # LTADS-GC-01: temporary + spec exhausted -> the ephemeral ltads/ is
        # eligible for cleanup regardless of whether any end-command ever ran
        # (fixes the leak-by-construction, skill-corpus scrutiny 04 ADD-2).
        cleanup_candidate=true
        summary="$summary - spec exhausted; temporary ltads/ eligible for cleanup (run ~/.claude/segments/-ltads/_temp-ltads-cleanup.md)"
    fi
else
    state="permanent"
    summary="Permanent LTADS"
    if [ -n "$session_id" ]; then summary="$summary, session $session_id"; fi
    if [ -n "$session_status" ]; then summary="$summary ($session_status)"; fi
fi

# Arc-record verdict annotations (ARC-01/ARC-06/ARC-11)
case "$arc_state" in
    legacy)      summary="$summary; prose arc record (legacy) - migration pending (ARC-11: convert at next arc start)" ;;
    invalid)     summary="$summary; arc-state.json MALFORMED - writers refuse until fixed/restored" ;;
    unvalidated) summary="$summary; arc-state.json present but unvalidatable (lib/jq missing) - NO-DATA" ;;
    valid)       summary="$summary; arc-state $arc_mode" ;;
esac
if [ "$size_warning" = "true" ]; then
    summary="$summary; arc-state near size cap (concluded-arc rotation on next lib write)"
fi

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

session_id_json="null"
if [ -n "$session_id" ]; then session_id_json="\"$(esc "$session_id")\""; fi
session_status_json="null"
if [ -n "$session_status" ]; then session_status_json="\"$(esc "$session_status")\""; fi
arc_mode_json="null"
if [ -n "$arc_mode" ]; then arc_mode_json="\"$arc_mode\""; fi

cat <<EOF
{"state":"$state","has_ltads_dir":true,"is_tracked":$is_tracked,"config_exists":$config_exists,"is_temporary":$is_temporary,"cleanup_candidate":$cleanup_candidate,"session_id":$session_id_json,"session_status":$session_status_json,"summary":"$(esc "$summary")","arc_state":"$arc_state","arc_mode":$arc_mode_json,"topmost_arc":$topmost_json,"size_warning":$size_warning}
EOF
