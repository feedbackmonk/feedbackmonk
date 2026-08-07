#!/bin/bash
# autonomy-status oracle (Unix)
# Resolves the autonomy cascade and emits
# {level, source, arc_id, expires_at, source_detail, briefing,
#  domains, domain_overrides, domain_clamped, domain_invalid}.
#
# Cascade order (first non-skip-non-empty wins):
#   1. Session override (out-of-band; not readable from disk — caller may pass --session-override=<level>)
#   2. ltads/arc-state.json topmost-arc autonomyOverride field (skip if CONCLUDED/PAUSED)
#   3. .claude/session-state/task-arc-autonomy.json (skip if expired or grantor PID dead)
#   4. ltads/config.json autonomy.default (may not hold autopilot/supervised)
#   5. ~/.claude/machine-autonomy.json (the AUTODEF-02 machine default)
#   6. Default: collaborative
#
# AUTODEF (AUTODEF-01..04, DEC-189): steps 4 and 5 are resolved TOGETHER, not
# first-wins — a project's persisted config CAPS the machine default DOWNWARD
# (the resolved level is the MORE consultative of the two). Durable loosening
# lives on exactly one surface, the machine default; project config still may
# not hold autopilot/supervised. Autopilot submodes (incl. `director`) are
# parsed and emitted as `submode`; at `director` the `spec` domain has a
# submode-definitional clamp FLOOR of `ask-major` (no store can loosen it).
#
# AUTODOM (AUTODOM-01..05, DEC-172): the resolved level seeds a six-domain
# consultation vector (spec/plan/delegate/decide/quality/commit) from a frozen
# level→domain default matrix. Explicit overrides (session store, then persisted
# config) may only TIGHTEN a domain (make it more consultative); a loosening
# override is CLAMPED to the level default and reported, never silently honored
# — otherwise `commit=auto` at `manual` would be an autonomy-escalation backdoor
# around the level ladder and its persistence restrictions.
# This oracle IS the mechanical consumer of domain settings; before AUTODOM
# nothing on disk read them and setting one did literally nothing.

set -e

SESSION_OVERRIDE=""
for arg in "$@"; do
    case "$arg" in
        --session-override=*) SESSION_OVERRIDE="${arg#--session-override=}" ;;
    esac
done

# Set by whichever cascade step resolves; consumed by emit_json/resolve_domains.
SUBMODE=""       # autopilot submode token (continuous|phase|session|task|director), "" otherwise
CAPPED_FROM=""   # the machine-default designation a project config capped away

# -----------------------------------------------------------------------------
# Helpers
# -----------------------------------------------------------------------------

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# TWIN-03 (DEFER-042): grantor PIDs are NATIVE Windows PIDs; Git-Bash `kill -0`
# cannot see those, so a raw probe here reported every live grantor dead and
# silently voided task-arc supervised/autopilot grants on Windows. Platform-
# branched probe (inline — oracles are self-contained; same pattern as
# dispatchable-sessions / stranded-dirty-files).
_PID_PROBE="kill"
case "$(uname -s 2>/dev/null)" in MINGW*|MSYS*|CYGWIN*) _PID_PROBE="powershell" ;; esac
is_pid_alive() {
    local pid="$1"
    [ -z "$pid" ] || [ "$pid" = "0" ] && return 1
    case "$pid" in (*[!0-9]*) return 1 ;; esac
    if [ "$_PID_PROBE" = "powershell" ]; then
        powershell.exe -NoProfile -Command "if (Get-Process -Id $pid -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1 </dev/null
    else
        kill -0 "$pid" 2>/dev/null
    fi
}

# Split a designation (`autopilot:director`, `supervised`, …) into its base level
# and — for autopilot only — its submode. Sets PARSED_LEVEL / PARSED_SUBMODE;
# PARSED_LEVEL is empty when the token is not a recognized level. A submode the
# resolver cannot parse is a silent downgrade (DISC-AUTO-01), so every place a
# designation is read goes through this one function.
parse_designation() {
    local raw base sub
    raw="$(printf '%s' "$1" | tr -d ' \t' | tr '[:upper:]' '[:lower:]')"
    base="${raw%%:*}"
    sub=""
    case "$raw" in *:*) sub="${raw#*:}" ;; esac
    PARSED_LEVEL=""; PARSED_SUBMODE=""
    case "$base" in
        autopilot|supervised|collaborative|controlled|manual) PARSED_LEVEL="$base" ;;
        *) return 0 ;;
    esac
    if [ "$base" = "autopilot" ]; then
        case "$sub" in
            continuous|phase|session|task|director) PARSED_SUBMODE="$sub" ;;
        esac
    fi
}

# Ladder position, least → most consultative. AUTODEF-03's "more consultative
# wins" comparison is this rank, nothing subtler.
level_rank() {
    case "$1" in
        autopilot) printf '%s' 1 ;;
        supervised) printf '%s' 2 ;;
        collaborative) printf '%s' 3 ;;
        controlled) printf '%s' 4 ;;
        manual) printf '%s' 5 ;;
        *) printf '%s' 0 ;;
    esac
}

# -----------------------------------------------------------------------------
# AUTODOM: domain vector (AUTODOM-01..04)
# -----------------------------------------------------------------------------

# Canonical domain order. Every emitted `domains` object carries all six keys.
DOMAIN_ORDER="spec plan delegate decide quality commit"

# AUTODOM-04 — option orderings, least-consultative → most-consultative.
# An override may move a domain RIGHT along its row (tighten); moving LEFT
# (loosen) is clamped back to the level default.
domain_options() {
    case "$1" in
        spec)     printf '%s' "auto critical-only ask-major ask-all" ;;
        plan)     printf '%s' "auto summarize approve step-by-step" ;;
        delegate) printf '%s' "auto summarize approve-roles approve-prompts" ;;
        decide)   printf '%s' "auto-all ask-critical ask-major ask-all" ;;
        quality)  printf '%s' "auto report approve" ;;
        commit)   printf '%s' "auto approve-message approve-diff" ;;
    esac
}

# AUTODOM-03 — the FROZEN level→domain default matrix, in DOMAIN_ORDER.
# `commit` is `auto` at collaborative and above ON PURPOSE: ~/.claude/CLAUDE.md
# § Propagation Operations (DEC-116 form 5) makes finalize the commit consent
# surface — after verified, directed work, committing is in-authority at
# collaborative+. Defaulting to `approve-message` there would manufacture
# exactly the consent gate DEC-116 / COMMS-GATE-02 bans.
level_defaults() {
    case "$1" in
        autopilot)     printf '%s' "auto auto auto auto-all auto auto" ;;
        supervised)    printf '%s' "critical-only summarize summarize ask-critical report auto" ;;
        collaborative) printf '%s' "ask-major approve approve-roles ask-major report auto" ;;
        controlled)    printf '%s' "ask-all approve approve-prompts ask-all approve approve-message" ;;
        manual)        printf '%s' "ask-all step-by-step approve-prompts ask-all approve approve-diff" ;;
        *)             printf '%s' "ask-major approve approve-roles ask-major report auto" ;;
    esac
}

# Rank of an option within its domain's ordering; -1 when unrecognized.
domain_rank() {
    local d="$1" want="$2" i=0 o
    for o in $(domain_options "$d"); do
        if [ "$o" = "$want" ]; then printf '%s' "$i"; return 0; fi
        i=$((i + 1))
    done
    printf '%s' "-1"
}

# Inner text of the first/only `"domains": { ... }` object in a JSON file.
# The domains object is flat (no nested braces), so [^}]* is exact.
_domains_block() {
    [ -f "$1" ] || return 0
    tr -d '\n\r' < "$1" 2>/dev/null | sed -n 's/.*"domains"[[:space:]]*:[[:space:]]*{\([^}]*\)}.*/\1/p'
}

_block_get() {
    printf '%s' "$1" | grep -oE "\"$2\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" | head -1 \
        | sed -E 's/.*:[[:space:]]*"([^"]*)"$/\1/'
}

# AUTODOM-01 — the two stores. Session/arc-scoped store overlays the persisted
# one, per domain. Both optional; absence is the common case.
DOMAIN_STORE_SESSION=".claude/session-state/autonomy-domains.json"
DOMAIN_STORE_CONFIG="ltads/config.json"

DOMAIN_CFG_BLOCK="$(_domains_block "$DOMAIN_STORE_CONFIG")"
DOMAIN_SESS_BLOCK="$(_domains_block "$DOMAIN_STORE_SESSION")"

for _d in $DOMAIN_ORDER; do
    _v="$(_block_get "$DOMAIN_CFG_BLOCK" "$_d")"
    _v2="$(_block_get "$DOMAIN_SESS_BLOCK" "$_d")"
    [ -n "$_v2" ] && _v="$_v2"
    eval "OVR_$_d=\"\$_v\""
done

# Resolve the domain vector for a level. Sets:
#   DOM_JSON      — "spec":"…","plan":"…",…  (all six, resolved)
#   OVR_NAMES     — "commit","quality"       (explicit + honored)
#   OVR_PAIRS     — "commit":"approve-diff"  (same, as pairs, for the briefing)
#   CLAMP_JSON    — {"domain":…,"requested":…,"clamped_to":…},…
#   CLAMP_BRIEF   — "commit: auto -> approve-message",…
#   INVALID_JSON  — {"domain":…,"requested":…},…
#   INVALID_BRIEF — "commit=bogus",…
#   DOMAIN_ACTIVE — non-empty when any override/clamp/invalid exists
resolve_domains() {
    local level="$1" defaults i d def req reqrank defrank final
    defaults="$(level_defaults "$level")"
    # AUTODEF-01 — `autopilot:director` is `autopilot` with ONE pin: the `spec`
    # domain's floor is `ask-major`. The pin is submode-DEFINITIONAL, so it is
    # applied as the level default here, which makes the existing tighten-only
    # machinery do the rest: a `spec=auto` override is now a LOOSENING request
    # and is clamped + reported, while `spec=ask-all` still tightens further.
    if [ "$level" = "autopilot" ] && [ "$SUBMODE" = "director" ]; then
        defaults="ask-major $(printf '%s' "$defaults" | cut -d' ' -f2-)"
    fi
    DOM_JSON=""; OVR_NAMES=""; OVR_PAIRS=""; CLAMP_JSON=""; CLAMP_BRIEF=""
    INVALID_JSON=""; INVALID_BRIEF=""; DOMAIN_ACTIVE=""
    i=1
    for d in $DOMAIN_ORDER; do
        def="$(printf '%s' "$defaults" | cut -d' ' -f$i)"
        i=$((i + 1))
        eval "req=\"\$OVR_$d\""
        final="$def"
        if [ -n "$req" ]; then
            reqrank="$(domain_rank "$d" "$req")"
            defrank="$(domain_rank "$d" "$def")"
            if [ "$reqrank" -lt 0 ]; then
                # Unrecognized option: ignored (falls back to the level default)
                # and REPORTED. Silently dropping a typo'd tightening request is
                # the one failure this resolver must not commit.
                INVALID_JSON="$INVALID_JSON${INVALID_JSON:+,}{\"domain\":\"$d\",\"requested\":\"$(esc "$req")\"}"
                INVALID_BRIEF="$INVALID_BRIEF${INVALID_BRIEF:+,}\"$d=$(esc "$req")\""
                DOMAIN_ACTIVE="1"
            elif [ "$reqrank" -lt "$defrank" ]; then
                # AUTODOM-04 tighten-only: loosening override clamped, reported.
                CLAMP_JSON="$CLAMP_JSON${CLAMP_JSON:+,}{\"domain\":\"$d\",\"requested\":\"$req\",\"clamped_to\":\"$def\"}"
                # NB: no <, >, &, ' in briefing text — PowerShell's
                # JavaScriptStringEncode \u-escapes those, which would break
                # sh/ps1 byte parity on the briefing string.
                CLAMP_BRIEF="$CLAMP_BRIEF${CLAMP_BRIEF:+,}\"$d: $req clamped to $def\""
                DOMAIN_ACTIVE="1"
            else
                final="$req"
                OVR_NAMES="$OVR_NAMES${OVR_NAMES:+,}\"$d\""
                OVR_PAIRS="$OVR_PAIRS${OVR_PAIRS:+,}\"$d\":\"$req\""
                DOMAIN_ACTIVE="1"
            fi
        fi
        DOM_JSON="$DOM_JSON${DOM_JSON:+,}\"$d\":\"$final\""
    done
}

emit_json() {
    local level="$1"
    local source="$2"
    local arc_id="$3"
    local expires_at="$4"
    local detail="$5"

    local arc_json="null"
    [ -n "$arc_id" ] && arc_json="\"$(esc "$arc_id")\""
    local expires_json="null"
    [ -n "$expires_at" ] && expires_json="\"$(esc "$expires_at")\""

    resolve_domains "$level"

    # AUTODOM-05 — the briefing (printed verbatim by session-start as
    # `[autonomy] …`) now also fires when a domain override is ACTIVE at
    # collaborative, so an active override is unavoidably in context at every
    # session start rather than invisible.
    # AUTODEF-04 — an ambient standing grant must be UNMISSABLE: when the
    # machine default governs (or a project config capped it away), the briefing
    # is emitted unconditionally, never gracefully absent.
    local force_brief=""
    case "$source" in machine-default|config-cap) force_brief="1" ;; esac

    local briefing=""
    if [ "$level" != "collaborative" ] || [ -n "$DOMAIN_ACTIVE" ] || [ -n "$force_brief" ]; then
        local base="\"level\":\"$level\",\"source\":\"$source\""
        [ -n "$SUBMODE" ] && base="$base,\"submode\":\"$SUBMODE\""
        [ -n "$CAPPED_FROM" ] && base="$base,\"capped_from\":\"$(esc "$CAPPED_FROM")\""
        if [ -n "$arc_id" ] && [ -n "$expires_at" ]; then
            base="$base,\"arc_id\":\"$(esc "$arc_id")\",\"expires_at\":\"$(esc "$expires_at")\""
        fi
        local dom_brief=""
        [ -n "$OVR_PAIRS" ] && dom_brief="$dom_brief,\"domains\":{$OVR_PAIRS}"
        [ -n "$CLAMP_BRIEF" ] && dom_brief="$dom_brief,\"clamped\":[$CLAMP_BRIEF]"
        [ -n "$INVALID_BRIEF" ] && dom_brief="$dom_brief,\"invalid\":[$INVALID_BRIEF]"
        briefing="{$base$dom_brief}"
    fi

    local submode_json="null"
    [ -n "$SUBMODE" ] && submode_json="\"$SUBMODE\""

    cat <<EOF
{"level":"$level","submode":$submode_json,"source":"$source","arc_id":$arc_json,"expires_at":$expires_json,"source_detail":"$(esc "$detail")","briefing":"$(esc "$briefing")","domains":{$DOM_JSON},"domain_overrides":[$OVR_NAMES],"domain_clamped":[$CLAMP_JSON],"domain_invalid":[$INVALID_JSON]}
EOF
}

# -----------------------------------------------------------------------------
# Step 1: Session override (caller-supplied)
# -----------------------------------------------------------------------------

if [ -n "$SESSION_OVERRIDE" ]; then
    parse_designation "$SESSION_OVERRIDE"
    if [ -n "$PARSED_LEVEL" ]; then
        SUBMODE="$PARSED_SUBMODE"
        emit_json "$PARSED_LEVEL" "session-override" "" "" "Session override passed via --session-override flag"
        exit 0
    fi
fi

# -----------------------------------------------------------------------------
# Step 2: arc-state.json autonomyOverride (topmost arc; skip if CONCLUDED/PAUSED)
# -----------------------------------------------------------------------------
# ARC-03 / DEC-199: the session-scoped override lives on the topmost arc as the
# `autonomyOverride` field (scrubbed mechanically at the terminus flip). Read
# via jq/python; absent tool or malformed doc -> leg skipped (graceful; the
# cascade continues). Legacy prose-only projects carry no arc-state.json, so
# this leg is naturally absent during the dual-format window.

ARC_STATE_FILE="ltads/arc-state.json"
if [ -f "$ARC_STATE_FILE" ]; then
    AS_STATUS=""
    AS_OVERRIDE=""
    if command -v jq >/dev/null 2>&1; then
        AS_STATUS=$(jq -r 'try (.arcs[0].status) // empty' "$ARC_STATE_FILE" 2>/dev/null || true)
        AS_OVERRIDE=$(jq -r 'try (.arcs[0].autonomyOverride) // empty' "$ARC_STATE_FILE" 2>/dev/null || true)
    elif command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
        AS_OUT=$(python3 -c "
import json,sys
try:
    d=json.load(open('$ARC_STATE_FILE',encoding='utf-8-sig'))
    a=(d.get('arcs') or [{}])[0]
    print((a.get('status') or '')+'|'+(a.get('autonomyOverride') or ''))
except Exception:
    print('|')
" 2>/dev/null || printf '|')
        AS_STATUS="${AS_OUT%%|*}"
        AS_OVERRIDE="${AS_OUT#*|}"
    fi
    if [ "$AS_STATUS" != "CONCLUDED" ] && [ "$AS_STATUS" != "PAUSED" ] && [ -n "$AS_OVERRIDE" ]; then
        # The field may carry a submode (e.g. `autopilot:continuous`, the
        # DEFAULT). The base level is what the cascade emits; the submode rides
        # alongside in `submode` (DISC-AUTO-01 discipline preserved).
        parse_designation "$AS_OVERRIDE"
        if [ -n "$PARSED_LEVEL" ]; then
            SUBMODE="$PARSED_SUBMODE"
            emit_json "$PARSED_LEVEL" "ltads-session" "" "" "ltads/arc-state.json topmost-arc autonomyOverride field"
            exit 0
        fi
    fi
fi

# -----------------------------------------------------------------------------
# Step 3: .claude/session-state/task-arc-autonomy.json (skip if expired or grantor dead)
# -----------------------------------------------------------------------------

ARC_FILE=".claude/session-state/task-arc-autonomy.json"
if [ -f "$ARC_FILE" ]; then
    # Read fields with simple grep (avoid jq dependency)
    ARC_LEVEL=$(grep -oE '"level"[[:space:]]*:[[:space:]]*"[^"]+"' "$ARC_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    ARC_ID=$(grep -oE '"arc_id"[[:space:]]*:[[:space:]]*"[^"]+"' "$ARC_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    ARC_EXPIRES=$(grep -oE '"expires_at"[[:space:]]*:[[:space:]]*"[^"]+"' "$ARC_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    # `|| true` is load-bearing: grantor_pid is OPTIONAL, and under `set -e` a
    # no-match grep in this substitution killed the whole oracle (rc=1, no
    # output), silently downgrading a valid autopilot grant to collaborative.
    # Surfaced by csi-08-smoke (CSI Phase 2 arc, 2026-06-12).
    ARC_PID=$(grep -oE '"grantor_pid"[[:space:]]*:[[:space:]]*[0-9]+' "$ARC_FILE" 2>/dev/null | head -1 | grep -oE '[0-9]+$' || true)

    parse_designation "$ARC_LEVEL"
    case "$PARSED_LEVEL" in
        autopilot|supervised|collaborative|controlled|manual)
            # TTL check
            ARC_EXPIRED=false
            if [ -n "$ARC_EXPIRES" ]; then
                # Compare ISO-8601 strings lexically (works for UTC Zulu timestamps)
                NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
                if [ "$NOW" \> "$ARC_EXPIRES" ]; then ARC_EXPIRED=true; fi
            fi
            # Liveness check
            ARC_DEAD=false
            if [ -n "$ARC_PID" ]; then
                if ! is_pid_alive "$ARC_PID"; then ARC_DEAD=true; fi
            fi
            if [ "$ARC_EXPIRED" != "true" ] && [ "$ARC_DEAD" != "true" ]; then
                SUBMODE="$PARSED_SUBMODE"
                emit_json "$PARSED_LEVEL" "task-arc-autonomy" "$ARC_ID" "$ARC_EXPIRES" ".claude/session-state/task-arc-autonomy.json (TTL valid, grantor alive)"
                exit 0
            fi
            ;;
    esac
fi

# -----------------------------------------------------------------------------
# Steps 4+5: ltads/config.json autonomy.default AND ~/.claude/machine-autonomy.json
#
# Resolved TOGETHER (AUTODEF-03): a project's persisted config CAPS the machine
# default downward — the resolved level is the MORE consultative of the two.
# Project config still may not hold autopilot/supervised: durable LOOSENING
# lives on exactly one auditable, revocable, user-word-only surface (the machine
# default), so there is a single place to audit and revoke it.
# -----------------------------------------------------------------------------

CONFIG_FILE="ltads/config.json"
CFG_LEVEL=""; CFG_LEGACY=""
if [ -f "$CONFIG_FILE" ]; then
    CONFIG_RAW=$(grep -oE '"default"[[:space:]]*:[[:space:]]*"[^"]+"' "$CONFIG_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    # Parse through the shared designation parser, NOT a bare-token case list: a
    # legacy config value carrying a submode (`autopilot:phase`) must still be
    # recognized and neutralized. Matching only bare tokens made such a value
    # fall through as if the file said nothing -- which, once a machine default
    # exists, silently ESCALATES a project that had explicitly asked for a
    # BOUNDED autopilot into the ambient continuous-class one. Found by running
    # the live cascade across the portfolio right after the first grant.
    parse_designation "$CONFIG_RAW"
    case "$PARSED_LEVEL" in
        collaborative|controlled|manual) CFG_LEVEL="$PARSED_LEVEL" ;;
        autopilot|supervised)
            # Legacy value: still neutralized to collaborative, advisory retargeted.
            # The full designation (submode included) is echoed back so the advisory
            # names what the user actually wrote.
            CFG_LEVEL="collaborative"; CFG_LEGACY="$CONFIG_RAW"
            ;;
    esac
fi

# AUTODEF-02 — the machine default. ULDF_MACHINE_AUTONOMY_FILE is a TEST SEAM
# (smokes point it at a sandbox); production always resolves ~/.claude/.
MACHINE_FILE="${ULDF_MACHINE_AUTONOMY_FILE:-$HOME/.claude/machine-autonomy.json}"
MACH_LEVEL=""; MACH_SUB=""; MACH_RAW=""
if [ -f "$MACHINE_FILE" ]; then
    # Malformed or absent => skipped SILENTLY (graceful absence): a machine-global
    # consent-bearing file must never be able to break every session's briefing.
    MACH_RAW=$(grep -oE '"level"[[:space:]]*:[[:space:]]*"[^"]+"' "$MACHINE_FILE" 2>/dev/null | head -1 | sed -E 's/.*"([^"]+)"$/\1/')
    parse_designation "$MACH_RAW"
    MACH_LEVEL="$PARSED_LEVEL"; MACH_SUB="$PARSED_SUBMODE"
fi

if [ -n "$CFG_LEVEL" ] && [ -n "$MACH_LEVEL" ]; then
    if [ "$(level_rank "$CFG_LEVEL")" -gt "$(level_rank "$MACH_LEVEL")" ]; then
        CAPPED_FROM="$MACH_RAW"
        DETAIL="ltads/config.json autonomy.default $CFG_LEVEL CAPS the machine default $MACH_RAW (more consultative wins)"
        [ -n "$CFG_LEGACY" ] && DETAIL="$DETAIL; config value is legacy $CFG_LEGACY, neutralized to collaborative - run /0-uldf-autonomy-set collaborative --persist to clean up, or grant durable autopilot with /0-uldf-autonomy-set autopilot:director --machine-default"
        emit_json "$CFG_LEVEL" "config-cap" "" "" "$DETAIL"
        exit 0
    fi
    SUBMODE="$MACH_SUB"
    emit_json "$MACH_LEVEL" "machine-default" "" "" "machine default $MACH_RAW from ~/.claude/machine-autonomy.json (project config $CFG_LEVEL is not more consultative)"
    exit 0
fi

if [ -n "$CFG_LEVEL" ]; then
    if [ -n "$CFG_LEGACY" ]; then
        emit_json "$CFG_LEVEL" "config" "" "" "ltads/config.json autonomy.default is $CFG_LEGACY (CAPPED to collaborative - project config cannot hold autopilot/supervised; durable autopilot lives on the machine default, /0-uldf-autonomy-set autopilot:director --machine-default)"
        exit 0
    fi
    emit_json "$CFG_LEVEL" "config" "" "" "ltads/config.json autonomy.default"
    exit 0
fi

if [ -n "$MACH_LEVEL" ]; then
    SUBMODE="$MACH_SUB"
    emit_json "$MACH_LEVEL" "machine-default" "" "" "machine default $MACH_RAW from ~/.claude/machine-autonomy.json (machine-global standing grant, AUTODEF-02)"
    exit 0
fi

# -----------------------------------------------------------------------------
# Step 6: Default
# -----------------------------------------------------------------------------

# NB: ASCII hyphen, not an em dash. run.ps1's twin emits ASCII, and the
# autonomy-domains-smoke sh/ps1 parity leg compares the two outputs BYTE-wise —
# a non-ASCII char here also risks PW-005 mojibake on the PowerShell side.
emit_json "collaborative" "default" "" "" "No override / LTADS / arc-autonomy / config - falling through to documented default"
