#!/bin/bash
# disk-artifact-bloat oracle (Unix / Git Bash)
#
# Cheap every-session tripwire for regenerable build-artifact disk bloat.
# Reads the free space of the dev drive (filesystem of CWD by default, or the
# configured root) via `df` and emits a [disk-artifact-bloat] briefing line
# ONLY when free space crosses the warn/alert threshold, or when free space
# has dropped by >= driftGb since the last sweep baseline. All-clear -> empty
# briefing (session-start suppresses the line). Graceful absence:
# undeterminable free space -> free_gb null + empty briefing.
#
# Output: single-line JSON matching the FROZEN schema in oracle.json.
# READ-ONLY. Never mutates state.

set +e

WARN_GB=80
ALERT_GB=40
DRIFT_GB=50
ROOT=""

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# Read a numeric/string key from a config file's diskArtifactBloat object.
# Uses jq if available, else python, else nothing (defaults stand).
cfg_parser=""
if command -v jq >/dev/null 2>&1; then
    cfg_parser="jq"
elif command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
    cfg_parser="python3"
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
    cfg_parser="python"
fi

cfg_get() {  # $1 file  $2 key -> value or empty
    local file="$1" key="$2"
    [ -f "$file" ] || return 0
    case "$cfg_parser" in
        jq)
            # No MSYS_NO_PATHCONV here: on Git Bash the native jq needs the
            # positional FILE arg path-converted (/c/... -> C:/...) to open it;
            # the --arg key value is a bare word and is never path-mangled.
            jq -r --arg k "$key" '(.diskArtifactBloat[$k] // empty) | tostring' "$file" 2>/dev/null
            ;;
        python3|python)
            "$cfg_parser" - "$file" "$key" <<'PY' 2>/dev/null
import json,sys
try:
    with open(sys.argv[1],encoding="utf-8-sig") as f: d=json.load(f)
except Exception: sys.exit(0)
v=(d.get("diskArtifactBloat") or {}).get(sys.argv[2])
if v is not None: print(v)
PY
            ;;
    esac
}

apply_config() {  # $1 file (global then project; later overrides)
    local file="$1" v
    v="$(cfg_get "$file" root)";    [ -n "$v" ] && ROOT="$v"
    v="$(cfg_get "$file" warnGb)";  [ -n "$v" ] && WARN_GB="$v"
    v="$(cfg_get "$file" alertGb)"; [ -n "$v" ] && ALERT_GB="$v"
    v="$(cfg_get "$file" driftGb)"; [ -n "$v" ] && DRIFT_GB="$v"
}

HOME_DIR="${HOME:-$USERPROFILE}"
apply_config "$HOME_DIR/.claude/config.json"
apply_config ".claude/config.json"

[ -n "$ULDF_BLOAT_ROOT" ]     && ROOT="$ULDF_BLOAT_ROOT"
[ -n "$ULDF_BLOAT_WARN_GB" ]  && WARN_GB="$ULDF_BLOAT_WARN_GB"
[ -n "$ULDF_BLOAT_ALERT_GB" ] && ALERT_GB="$ULDF_BLOAT_ALERT_GB"
[ -n "$ULDF_BLOAT_DRIFT_GB" ] && DRIFT_GB="$ULDF_BLOAT_DRIFT_GB"

PROBE="${ROOT:-$(pwd)}"

emit() {  # tripped level drive_json free_json total_json drift_json baseline_json briefing
    cat <<EOF
{"tripped":$1,"level":"$2","drive":$3,"free_gb":$4,"total_gb":$5,"warn_gb":$WARN_GB,"alert_gb":$ALERT_GB,"drift_gb":$6,"baseline_age_days":$7,"briefing":"$(esc "$8")"}
EOF
    exit 0
}

emit_empty() {  # drive_json
    emit "false" "ok" "${1:-null}" "null" "null" "null" "null" ""
}

# ---- Graceful absence: probe path missing ----------------------------------
if [ ! -e "$PROBE" ]; then emit_empty "null"; fi

# ---- Free space via df (POSIX -P -k => 1024-byte blocks) -------------------
# Columns: Filesystem 1024-blocks Used Available Capacity Mounted-on
DF_LINE="$(df -P -k "$PROBE" 2>/dev/null | awk 'NR==2{print $2" "$4" "$NF}')"
if [ -z "$DF_LINE" ]; then emit_empty "null"; fi

TOTAL_K="$(printf '%s' "$DF_LINE" | awk '{print $1}')"
AVAIL_K="$(printf '%s' "$DF_LINE" | awk '{print $2}')"
MOUNT="$(printf '%s' "$DF_LINE" | awk '{print $3}')"
case "$AVAIL_K" in (''|*[!0-9]*) emit_empty "null" ;; esac

# Drive label: on Git Bash df may report a Windows path; otherwise the mount.
DRIVE_LABEL="$MOUNT"
DRIVE_JSON="\"$(esc "$DRIVE_LABEL")\""

FREE_BYTES=$(( AVAIL_K * 1024 ))
# GiB with one decimal, integer-math (x10 then format).
FREE_GB_X10=$(( AVAIL_K * 10 / 1048576 ))
FREE_GB="$(printf '%d.%d' $((FREE_GB_X10/10)) $((FREE_GB_X10%10)))"
TOTAL_JSON="null"
if [ -n "$TOTAL_K" ] && [ "$TOTAL_K" -gt 0 ] 2>/dev/null; then
    TOTAL_GB_X10=$(( TOTAL_K * 10 / 1048576 ))
    TOTAL_GB="$(printf '%d.%d' $((TOTAL_GB_X10/10)) $((TOTAL_GB_X10%10)))"
    TOTAL_JSON="$TOTAL_GB"
fi

# ---- Drift vs the last sweep baseline --------------------------------------
DRIFT_JSON="null"
BASELINE_AGE_JSON="null"
DRIFT_CLAUSE=""
DRIFT_TRIP=0
BASELINE="$HOME_DIR/.claude/session-state/disk-artifact-baseline.json"
if [ -f "$BASELINE" ] && [ -n "$cfg_parser" ]; then
    BL_FREE=""
    BL_DRIVE=""
    BL_TS=""
    if [ "$cfg_parser" = "jq" ]; then
        BL_FREE="$(jq -r '(.free_bytes // empty)|tostring' "$BASELINE" 2>/dev/null)"
        BL_DRIVE="$(jq -r '(.drive // empty)|tostring' "$BASELINE" 2>/dev/null)"
        BL_TS="$(jq -r '(.scanned_at // empty)|tostring' "$BASELINE" 2>/dev/null)"
    else
        eval "$("$cfg_parser" - "$BASELINE" <<'PY' 2>/dev/null
import json,sys
try:
    d=json.load(open(sys.argv[1],encoding="utf-8-sig"))
except Exception: sys.exit(0)
def g(k):
    v=d.get(k); return "" if v is None else str(v)
print("BL_FREE=%r"%g("free_bytes"))
print("BL_DRIVE=%r"%g("drive"))
print("BL_TS=%r"%g("scanned_at"))
PY
)"
    fi
    same_drive=1
    if [ -n "$BL_DRIVE" ] && [ -n "$DRIVE_LABEL" ]; then
        [ "$BL_DRIVE" = "$DRIVE_LABEL" ] || same_drive=0
    fi
    if [ "$same_drive" -eq 1 ] && [ -n "$BL_FREE" ]; then
        case "$BL_FREE" in (*[!0-9]*) BL_FREE="" ;; esac
        if [ -n "$BL_FREE" ]; then
            DROP_BYTES=$(( BL_FREE - FREE_BYTES ))
            # Signed one-decimal GiB.
            if [ "$DROP_BYTES" -lt 0 ]; then
                ABS=$(( -DROP_BYTES )); SIGN="-"
            else
                ABS=$DROP_BYTES; SIGN=""
            fi
            DROP_X10=$(( ABS * 10 / 1073741824 ))
            DRIFT_GB_STR="$SIGN$(printf '%d.%d' $((DROP_X10/10)) $((DROP_X10%10)))"
            DRIFT_JSON="$DRIFT_GB_STR"
            # Trip if drop (in whole GiB) >= DRIFT_GB
            DROP_GB_WHOLE=$(( ABS / 1073741824 ))
            DRIFT_THRESH_WHOLE="$(printf '%.0f' "$DRIFT_GB" 2>/dev/null)"
            [ -z "$DRIFT_THRESH_WHOLE" ] && DRIFT_THRESH_WHOLE="$DRIFT_GB"
            if [ "$DROP_BYTES" -gt 0 ] && [ "$DROP_GB_WHOLE" -ge "$DRIFT_THRESH_WHOLE" ] 2>/dev/null; then
                DRIFT_TRIP=1
            fi
            AGE_TXT="since last sweep"
            if [ -n "$BL_TS" ]; then
                BL_EPOCH="$(date -u -d "$BL_TS" +%s 2>/dev/null)"
                if [ -n "$BL_EPOCH" ]; then
                    NOW_EPOCH="$(date -u +%s)"
                    AGE_DAYS=$(( (NOW_EPOCH - BL_EPOCH) / 86400 ))
                    BASELINE_AGE_JSON="$AGE_DAYS"
                    AGE_TXT="${AGE_DAYS}d ago"
                fi
            fi
            if [ "$DRIFT_TRIP" -eq 1 ]; then
                DRIFT_CLAUSE="; free dropped $DRIFT_GB_STR GB $AGE_TXT"
            fi
        fi
    fi
fi

# ---- Classify + compose briefing -------------------------------------------
# Integer compare on whole-GiB free vs thresholds (one-decimal precision not
# needed for the trip decision).
FREE_GB_WHOLE=$(( AVAIL_K / 1048576 ))
ALERT_WHOLE="$(printf '%.0f' "$ALERT_GB" 2>/dev/null)"; [ -z "$ALERT_WHOLE" ] && ALERT_WHOLE="$ALERT_GB"
WARN_WHOLE="$(printf '%.0f' "$WARN_GB" 2>/dev/null)";  [ -z "$WARN_WHOLE" ]  && WARN_WHOLE="$WARN_GB"

LEVEL="ok"
BRIEFING=""
if [ "$FREE_GB_WHOLE" -lt "$ALERT_WHOLE" ] 2>/dev/null; then
    LEVEL="alert"
    BRIEFING="ALERT $DRIVE_LABEL $FREE_GB GB free (alert < $ALERT_GB GB)$DRIFT_CLAUSE -- run ~/.claude/scripts/disk-artifact-sweep.ps1 to reclaim regenerable artifacts"
elif [ "$FREE_GB_WHOLE" -lt "$WARN_WHOLE" ] 2>/dev/null; then
    LEVEL="warn"
    BRIEFING="$DRIVE_LABEL $FREE_GB GB free (warn < $WARN_GB GB)$DRIFT_CLAUSE -- run ~/.claude/scripts/disk-artifact-sweep.ps1 (dry-run) to inspect"
elif [ "$DRIFT_TRIP" -eq 1 ]; then
    LEVEL="warn"
    AGE_TXT="since last sweep"
    [ "$BASELINE_AGE_JSON" != "null" ] && AGE_TXT="${BASELINE_AGE_JSON}d ago"
    BRIEFING="$DRIVE_LABEL free dropped $DRIFT_JSON GB $AGE_TXT ($FREE_GB GB free) -- run ~/.claude/scripts/disk-artifact-sweep.ps1 -Scan"
fi

TRIPPED="false"; [ -n "$BRIEFING" ] && TRIPPED="true"
emit "$TRIPPED" "$LEVEL" "$DRIVE_JSON" "$FREE_GB" "$TOTAL_JSON" "$DRIFT_JSON" "$BASELINE_AGE_JSON" "$BRIEFING"
