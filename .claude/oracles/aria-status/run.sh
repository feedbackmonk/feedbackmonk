#!/bin/bash
# aria-status oracle (Unix)
# Answers: what is the ARIA instrumentation status of this project?
#
# Output:
#   {
#     surface_present: bool,
#     exposure_mechanism: "tauri-ipc"|"http"|"websocket"|"file"|"none",
#     endpoint_reachable: bool,
#     endpoint_url?: string,
#     foundation_layer: { errors: bool, async: bool, navigation: bool },
#     recent_success_at?: ISO8601,
#     query_count_24h?: int,
#     briefing: string  (≤200 chars; empty string suppresses the [aria-status] line)
#   }
#
# Briefing-line forms (ARIA-01 acceptance #5):
#   present-and-healthy:           "ARIA: errors+async+navigation healthy (qph=N)"
#   present-but-degraded:          "ARIA: <healthy categories> healthy; <degraded> UNREACHABLE — see /0-uldf-oracle aria-status"
#   instrumented-but-unreachable:  "ARIA: configured but endpoint unreachable at <url>"
#   surface-but-no-instrumentation:"ARIA: UI/runtime surface detected; no ARIA instrumentation. /0-uldf-ldis-plan can scaffold."
#   no-surface:                    "" (line suppressed by hook)

set -e

DEFAULT_ENDPOINT="http://127.0.0.1:14550/aria/health"
ARIA_CONFIG=".claude/aria.json"
ARIA_MANIFEST=".claude/aria-manifest.json"   # AOR capabilities disk companion (ARIA-17)
TELEMETRY_LOG=".claude/aria-telemetry.jsonl"
PROBE_TIMEOUT="0.3"  # seconds (300ms)

# ---- Surface detection (reuse ui-surface-detector logic via cache, or recompute) ----
SURFACE_KIND="none"
SURFACE_ORACLE_DIR="$(dirname "$0")/../ui-surface-detector"
if [ -f "$SURFACE_ORACLE_DIR/run.sh" ]; then
    SURFACE_OUT=$(bash "$SURFACE_ORACLE_DIR/run.sh" 2>/dev/null || echo '{"surface_kind":"none"}')
    SURFACE_KIND=$(printf '%s' "$SURFACE_OUT" | grep -oE '"surface_kind"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"' || echo "none")
fi
[ -z "$SURFACE_KIND" ] && SURFACE_KIND="none"

# Surface-present rule: anything other than "none" or "cli-tool" qualifies as a runtime surface
# (cli-tool may have ARIA in some cases, but foundation-layer is targeted at UI/service runtime perception)
SURFACE_PRESENT="false"
case "$SURFACE_KIND" in
    none|cli-tool) SURFACE_PRESENT="false" ;;
    *) SURFACE_PRESENT="true" ;;
esac

# ---- No-surface: emit empty briefing (hook will suppress) ----
if [ "$SURFACE_PRESENT" = "false" ]; then
    printf '{"surface_present":false,"exposure_mechanism":"none","endpoint_reachable":false,"foundation_layer":{"errors":false,"async":false,"navigation":false},"briefing":""}'
    exit 0
fi

# ---- Detect aria.json config (instrumentation marker) ----
HAS_CONFIG="false"
ENDPOINT_URL="$DEFAULT_ENDPOINT"
EXPOSURE_MECHANISM="http"
if [ -f "$ARIA_CONFIG" ]; then
    HAS_CONFIG="true"
    # Best-effort URL extraction
    cfg_url=$(grep -oE '"endpoint_url"[[:space:]]*:[[:space:]]*"[^"]+"' "$ARIA_CONFIG" 2>/dev/null | grep -oE '"[^"]+"$' | tr -d '"')
    [ -n "$cfg_url" ] && ENDPOINT_URL="$cfg_url"
    cfg_mech=$(grep -oE '"exposure_mechanism"[[:space:]]*:[[:space:]]*"[^"]+"' "$ARIA_CONFIG" 2>/dev/null | grep -oE '"[^"]+"$' | tr -d '"')
    [ -n "$cfg_mech" ] && EXPOSURE_MECHANISM="$cfg_mech"
fi

# ---- Probe endpoint (300ms timeout) ----
ENDPOINT_REACHABLE="false"
ARIA_HEALTH_BODY=""
if command -v curl >/dev/null 2>&1; then
    ARIA_HEALTH_BODY=$(curl -sS --max-time "$PROBE_TIMEOUT" -H "Accept: application/json" "$ENDPOINT_URL" 2>/dev/null || echo "")
    if [ -n "$ARIA_HEALTH_BODY" ]; then
        ENDPOINT_REACHABLE="true"
    fi
fi

# ---- Parse aria_health response (validate _meta) ----
FL_ERRORS="false"
FL_ASYNC="false"
FL_NAV="false"
RECENT_SUCCESS_AT=""

if [ "$ENDPOINT_REACHABLE" = "true" ]; then
    # Verify it looks like an aria_health response
    if printf '%s' "$ARIA_HEALTH_BODY" | grep -q '"_meta"'; then
        # Extract probeMaturity and oracleStatus
        ORACLE_STATUS=$(printf '%s' "$ARIA_HEALTH_BODY" | grep -oE '"oracleStatus"[[:space:]]*:[[:space:]]*"[^"]+"' | grep -oE '"[^"]+"$' | tr -d '"')
        # Extract degradedCategories (optional array)
        DEGRADED=$(printf '%s' "$ARIA_HEALTH_BODY" | grep -oE '"degradedCategories"[[:space:]]*:[[:space:]]*\[[^]]*\]' | grep -oE '\[[^]]*\]' || echo "")

        case "$ORACLE_STATUS" in
            healthy)
                FL_ERRORS="true"; FL_ASYNC="true"; FL_NAV="true"
                ;;
            degraded)
                # Default to true; flip to false for any category named in degradedCategories
                FL_ERRORS="true"; FL_ASYNC="true"; FL_NAV="true"
                if printf '%s' "$DEGRADED" | grep -q '"errors"'; then FL_ERRORS="false"; fi
                if printf '%s' "$DEGRADED" | grep -q '"async"'; then FL_ASYNC="false"; fi
                if printf '%s' "$DEGRADED" | grep -q '"navigation"'; then FL_NAV="false"; fi
                ;;
            *)
                # Unknown oracleStatus — treat as schema-mismatch / degraded
                ENDPOINT_REACHABLE="false"
                ;;
        esac
        if [ "$ENDPOINT_REACHABLE" = "true" ]; then
            RECENT_SUCCESS_AT=$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || echo "")
        fi
    else
        # No _meta envelope — reachable but contract-violating; treat as unreachable for foundation purposes
        ENDPOINT_REACHABLE="false"
    fi
fi

# ---- Compute query_count_24h from telemetry log (v1+v2 capture contract, ARIA-13) ----
# NO-DATA semantics (DISC-ARIA-09 / DEC-81): a missing log file is NO-DATA, never zero;
# a 24h window with zero heartbeats AND zero query events is NO-DATA for the window.
# Counting rules: v2 lines ("schemaVersion" present) count when event=="query";
# heartbeats (event=="telemetry-armed") tracked separately; probe:true lines excluded;
# v1 legacy lines (no "schemaVersion", {tool,timestamp,...}) count as query events.
QUERY_COUNT_24H=""
HEARTBEAT_COUNT_24H=""
TELEMETRY_STATE="no-data"
if [ -f "$TELEMETRY_LOG" ]; then
    if command -v date >/dev/null 2>&1; then
        CUTOFF=$(date -u -d '24 hours ago' +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null \
                 || date -u -v-24H +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null \
                 || echo "")
        if [ -n "$CUTOFF" ]; then
            # awk (not grep -P — MSYS locale portability) single pass: per-line ts extraction
            # (v2 "ts" preferred, v1 "timestamp" fallback), lexicographic ISO-8601 cutoff filter,
            # then classify heartbeat / probe / query.
            COUNTS=$(awk -v cutoff="$CUTOFF" '
                BEGIN { q = 0; hb = 0 }
                {
                    tsfield = ""
                    if (match($0, /"ts"[ \t]*:[ \t]*"[^"]+"/)) tsfield = substr($0, RSTART, RLENGTH)
                    else if (match($0, /"timestamp"[ \t]*:[ \t]*"[^"]+"/)) tsfield = substr($0, RSTART, RLENGTH)
                    if (tsfield == "") next
                    if (!match(tsfield, /"[^"]*"$/)) next
                    ts = substr(tsfield, RSTART + 1, RLENGTH - 2)
                    if (ts < cutoff) next
                    if ($0 ~ /"probe"[ \t]*:[ \t]*true/) next
                    if ($0 ~ /"event"[ \t]*:[ \t]*"telemetry-armed"/) { hb++; next }
                    if ($0 ~ /"event"[ \t]*:[ \t]*"query"/) { q++; next }
                    if ($0 !~ /"schemaVersion"/ && $0 ~ /"timestamp"/) { q++ }
                }
                END { print q, hb }
            ' "$TELEMETRY_LOG" 2>/dev/null)
            QUERY_COUNT_24H=$(printf '%s' "$COUNTS" | awk '{print $1}')
            HEARTBEAT_COUNT_24H=$(printf '%s' "$COUNTS" | awk '{print $2}')
            if [ -n "$QUERY_COUNT_24H" ]; then
                if [ "$QUERY_COUNT_24H" -gt 0 ] || [ "${HEARTBEAT_COUNT_24H:-0}" -gt 0 ]; then
                    TELEMETRY_STATE="measured"
                else
                    # zero heartbeats AND zero queries in window -> NO-DATA, not zero
                    QUERY_COUNT_24H=""
                fi
            fi
        fi
    fi
fi

# ---- AOR capabilities disk companion (ARIA-17) ----
# Static, value-free projection of the runtime manifest (AOR_CONTRACTS.md §4),
# committed by an AOR-conformant app so an agent learns the surface shape BEFORE
# the app is running. Source of truth is the runtime manifest()/GET /aor/capabilities;
# this is its committed projection. NO-DATA semantics: absent/malformed -> field
# omitted, NEVER an empty-capabilities claim. Emitted verbatim (compacted+validated
# via a working python; the WindowsApps `python3` shim is skipped).
CAPABILITIES_JSON=""
if [ -f "$ARIA_MANIFEST" ] && [ -s "$ARIA_MANIFEST" ]; then
    _aor_py=""
    for _c in python python3 py; do
        if command -v "$_c" >/dev/null 2>&1 && "$_c" -c 'import json,sys' >/dev/null 2>&1; then _aor_py="$_c"; break; fi
    done
    if [ -n "$_aor_py" ]; then
        CAPABILITIES_JSON=$("$_aor_py" -c 'import json,sys
try:
    d=json.load(open(sys.argv[1], encoding="utf-8-sig"))
    assert isinstance(d,dict) and "verbs" in d
    sys.stdout.write(json.dumps(d,separators=(",",":")))
except Exception:
    pass' "$ARIA_MANIFEST" 2>/dev/null)
    fi
fi

# ---- Compose briefing ----
BRIEFING=""
if [ "$ENDPOINT_REACHABLE" = "true" ]; then
    if [ "$FL_ERRORS" = "true" ] && [ "$FL_ASYNC" = "true" ] && [ "$FL_NAV" = "true" ]; then
        # Healthy. qph (queries-per-hour) = query_count_24h / 24.
        # NO-DATA is never presented as qph=0 (DISC-ARIA-09).
        if [ "$TELEMETRY_STATE" = "measured" ]; then
            QPH=$(( ${QUERY_COUNT_24H:-0} / 24 ))
            BRIEFING="ARIA: errors+async+navigation healthy (qph=$QPH)"
        else
            BRIEFING="ARIA: errors+async+navigation healthy (qph=no-data; telemetry meter not armed, see ARIA-13)"
        fi
    else
        # Degraded — list healthy and degraded categories
        HEALTHY_CATS=""
        DEGRADED_CATS=""
        for pair in "navigation:$FL_NAV" "errors:$FL_ERRORS" "async:$FL_ASYNC"; do
            cat="${pair%%:*}"; val="${pair##*:}"
            if [ "$val" = "true" ]; then
                if [ -z "$HEALTHY_CATS" ]; then HEALTHY_CATS="$cat"; else HEALTHY_CATS="$HEALTHY_CATS+$cat"; fi
            else
                if [ -z "$DEGRADED_CATS" ]; then DEGRADED_CATS="$cat"; else DEGRADED_CATS="$DEGRADED_CATS|$cat"; fi
            fi
        done
        if [ -n "$HEALTHY_CATS" ] && [ -n "$DEGRADED_CATS" ]; then
            BRIEFING="ARIA: $HEALTHY_CATS healthy; $DEGRADED_CATS UNREACHABLE — see /0-uldf-oracle aria-status"
        elif [ -n "$DEGRADED_CATS" ]; then
            BRIEFING="ARIA: $DEGRADED_CATS UNREACHABLE — see /0-uldf-oracle aria-status"
        else
            BRIEFING="ARIA: status unknown — see /0-uldf-oracle aria-status"
        fi
    fi
elif [ "$HAS_CONFIG" = "true" ]; then
    BRIEFING="ARIA: configured but endpoint unreachable at $ENDPOINT_URL"
else
    BRIEFING="ARIA: UI/runtime surface detected; no ARIA instrumentation. /0-uldf-ldis-plan can scaffold."
fi

# Cap briefing at 200 chars (defensive — should already be under)
if [ ${#BRIEFING} -gt 200 ]; then
    BRIEFING="${BRIEFING:0:197}..."
fi

# ---- Emit JSON ----
# JSON-escape briefing
ESC_BRIEFING=$(printf '%s' "$BRIEFING" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')
ESC_URL=$(printf '%s' "$ENDPOINT_URL" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')

OUT="{"
OUT="${OUT}\"surface_present\":true,"
OUT="${OUT}\"exposure_mechanism\":\"$EXPOSURE_MECHANISM\","
OUT="${OUT}\"endpoint_reachable\":$ENDPOINT_REACHABLE,"
OUT="${OUT}\"endpoint_url\":\"$ESC_URL\","
OUT="${OUT}\"foundation_layer\":{\"errors\":$FL_ERRORS,\"async\":$FL_ASYNC,\"navigation\":$FL_NAV}"
if [ -n "$RECENT_SUCCESS_AT" ]; then
    OUT="${OUT},\"recent_success_at\":\"$RECENT_SUCCESS_AT\""
fi
if [ "$TELEMETRY_STATE" = "measured" ] && [ -n "$QUERY_COUNT_24H" ]; then
    OUT="${OUT},\"query_count_24h\":$QUERY_COUNT_24H"
fi
OUT="${OUT},\"telemetry_state\":\"$TELEMETRY_STATE\""
# AOR capabilities (ARIA-17) — emitted verbatim only when the disk companion is
# present + valid; omitted (NO-DATA) otherwise, never an empty-capabilities claim.
if [ -n "$CAPABILITIES_JSON" ]; then
    OUT="${OUT},\"capabilities\":$CAPABILITIES_JSON"
    # Operability Ladder surface (OPER-05, DEC-133) — additive top-level
    # convenience projection of the capabilities' optional operabilityTier /
    # switchboard fields (AOR_CONTRACTS.md Appendix A). Consumers: the
    # Testability Gate Q6 (OPER-04) and /0-uldf-verify-fast's AOR consultation.
    # NO-DATA: fields omitted when the companion does not declare them —
    # never inferred, never defaulted to "T0".
    if [ -n "$_aor_py" ]; then
        OPER_FRAGMENT=$("$_aor_py" -c 'import json,sys
try:
    d=json.load(open(sys.argv[1], encoding="utf-8-sig"))
    out=[]
    t=d.get("operabilityTier")
    if isinstance(t,str) and t in ("T0","T1","T2","T3"):
        out.append("\"operability_tier\":%s"%json.dumps(t))
    s=d.get("switchboard")
    if isinstance(s,list) and all(isinstance(x,str) and x.startswith("aor.") for x in s):
        out.append("\"switchboard\":%s"%json.dumps(s,separators=(",",":")))
    sys.stdout.write(",".join(out))
except Exception:
    pass' "$ARIA_MANIFEST" 2>/dev/null)
        if [ -n "$OPER_FRAGMENT" ]; then
            OUT="${OUT},$OPER_FRAGMENT"
        fi
    fi
fi
OUT="${OUT},\"briefing\":\"$ESC_BRIEFING\"}"

echo "$OUT"
exit 0
