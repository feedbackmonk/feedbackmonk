#!/bin/bash
# stranded-dirty-files oracle (Unix)
#
# CSI-15 (Phase 1.7): emit a [stranded-dirty-files] briefing line when this
# project's working tree contains dirty files older than the most recent finalize
# commit AND no live peer (per dispatchable-sessions registry) owns them.
#
# Visibility-only — never mutates state. Cleanup is user-driven via
# /0-uldf-finalize --include-stranded (FINALIZE-04, same Arc 2).
#
# Output: single-line JSON matching the FROZEN output schema (oracle.json).
# Briefing field is empty string when count == 0 so the session-start hook
# gracefully suppresses the line (parallel to stale-ltads-state's silence pattern).
#
# Performance ceiling: <=250ms on <=500 dirty files; <=500ms on <=2000;
# >2000 -> detection skipped, count == -1.

set +e

REGISTRY=".claude/collaboration/active-sessions.json"
SCOPE_GUARD_MAX=2000
SAMPLE_CAP=10
LARGE_THRESHOLD=50

# JSON-string escape (backslash + double-quote)
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# Emit a JSON object given primitive shell values. ALL string values must be
# pre-escaped via esc(); ALL nullable fields are null when empty.
emit() {
    local has_stranded="$1"
    local count="$2"
    local oldest_mtime_json="$3"   # quoted ISO string OR "null"
    local sample_json="$4"         # JSON array literal
    local live_peer_count="$5"
    local last_finalize_json="$6"  # quoted ISO string OR "null"
    local briefing="$7"            # raw string (will be esc'd here)
    cat <<EOF
{"has_stranded":$has_stranded,"count":$count,"oldest_mtime":$oldest_mtime_json,"sample":$sample_json,"live_peer_count":$live_peer_count,"last_finalize_at":$last_finalize_json,"briefing":"$(esc "$briefing")"}
EOF
    exit 0
}

emit_empty() {
    local last_finalize_json="${1:-null}"
    local live_peer_count="${2:-0}"
    emit "false" "0" "null" "[]" "$live_peer_count" "$last_finalize_json" ""
}

# ---- Graceful absence: not in a git repo -----------------------------------
if ! git rev-parse --git-dir >/dev/null 2>&1; then
    emit_empty "null" "0"
fi

# ---- last_finalize_at = HEAD's commit timestamp ----------------------------
LAST_FINALIZE_ISO="$(git log -1 --format=%aI HEAD 2>/dev/null)"
LAST_FINALIZE_JSON="null"
LAST_FINALIZE_EPOCH=""
if [ -n "$LAST_FINALIZE_ISO" ]; then
    LAST_FINALIZE_JSON="\"$(esc "$LAST_FINALIZE_ISO")\""
fi

# Convert ISO-8601 to epoch seconds. Tries GNU date -> BSD date -> python.
iso_to_epoch() {
    local iso="$1"
    [ -n "$iso" ] || return 0
    local epoch
    epoch=$(date -u -d "$iso" +%s 2>/dev/null) && [ -n "$epoch" ] && { echo "$epoch"; return 0; }
    # Strip fractional seconds + timezone for BSD parsing
    local s
    s="$(echo "$iso" | sed -E 's/\+[0-9:]+$//; s/\.[0-9]+Z?$//; s/Z$//')"
    epoch=$(date -u -j -f "%Y-%m-%dT%H:%M:%S" "$s" +%s 2>/dev/null) && [ -n "$epoch" ] && { echo "$epoch"; return 0; }
    if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
        python3 - "$iso" <<'PY' 2>/dev/null
import sys
from datetime import datetime, timezone
s = sys.argv[1]
# Normalize: strip Z, strip fractional seconds, strip +HH:MM
if s.endswith("Z"):
    s = s[:-1]
if "." in s:
    s = s.split(".")[0]
# Strip TZ if present (e.g., +05:00)
if "+" in s[10:]:
    s = s[: 10 + s[10:].index("+")]
elif "-" in s[10:]:
    s = s[: 10 + s[10:].index("-")]
try:
    dt = datetime.strptime(s, "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc)
    print(int(dt.timestamp()))
except Exception:
    pass
PY
    elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
        python - "$iso" <<'PY' 2>/dev/null
import sys
from datetime import datetime
s = sys.argv[1]
if s.endswith("Z"):
    s = s[:-1]
if "." in s:
    s = s.split(".")[0]
if "+" in s[10:]:
    s = s[: 10 + s[10:].index("+")]
elif "-" in s[10:]:
    s = s[: 10 + s[10:].index("-")]
try:
    dt = datetime.strptime(s, "%Y-%m-%dT%H:%M:%S")
    import calendar
    print(int(calendar.timegm(dt.timetuple())))
except Exception:
    pass
PY
    fi
}

if [ -n "$LAST_FINALIZE_ISO" ]; then
    LAST_FINALIZE_EPOCH="$(iso_to_epoch "$LAST_FINALIZE_ISO")"
fi

# ---- Dirty file set --------------------------------------------------------
# git status --porcelain emits two-character XY codes followed by " path".
# Renames "R  old -> new" are split; we keep the new name. Deleted files (D)
# have no mtime to stat -> excluded. Untracked (??) included.
DIRTY_FILES=()
while IFS= read -r line; do
    [ -n "$line" ] || continue
    xy="${line:0:2}"
    rest="${line:3}"
    case "$xy" in
        ' D'|'D '|'DD'|'AD'|'MD'|'RD'|'CD')
            # Working-tree deletion -> no mtime; skip.
            continue
            ;;
        'R '|'RM'|'RD'|' R')
            # Rename: "old -> new"; keep new name
            new_path="${rest##* -> }"
            DIRTY_FILES+=("$new_path")
            ;;
        *)
            DIRTY_FILES+=("$rest")
            ;;
    esac
done < <(git status --porcelain 2>/dev/null)

DIRTY_COUNT="${#DIRTY_FILES[@]}"

# ---- Scope guard: too many dirty files -> detection skipped ----------------
if [ "$DIRTY_COUNT" -gt "$SCOPE_GUARD_MAX" ]; then
    BRIEFING="stranded-dirty-files: detection skipped — too many dirty files (>${SCOPE_GUARD_MAX}); run /0-uldf-finalize --include-stranded for full audit"
    emit "false" "-1" "null" "[]" "0" "$LAST_FINALIZE_JSON" "$BRIEFING"
fi

# ---- Empty dirty set -> graceful empty -------------------------------------
if [ "$DIRTY_COUNT" -eq 0 ]; then
    emit_empty "$LAST_FINALIZE_JSON" "0"
fi

# ---- Build live-peer ownership map -----------------------------------------
# A file is "owned by a live peer" iff a registry entry exists with
# status=="active" AND workDir matches this project root AND PID alive AND
# the entry's dirtyFiles[] list contains the file path.
#
# Forward-compatible: dirtyFiles[] defaults to empty (peers haven't published
# ownership yet); until they do, default = "no live peer" which is the
# correct default-to-strand behavior given the trigger pattern.
PROJ_ROOT="$(pwd)"
# Normalize: backslashes -> forward slashes, strip trailing slashes.
# Use tr (avoids cross-distro sed-escape variance for `\\`).
PROJ_ROOT_NORM="$(printf '%s' "$PROJ_ROOT" | tr '\\' '/' | sed 's:/*$::')"

LIVE_PEER_COUNT=0
OWNED_FILES=""  # newline-separated list

# Pick parser: jq preferred, python fallback (probe-verify).
parser=""
if command -v jq >/dev/null 2>&1; then
    parser="jq"
elif command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
    parser="python3"
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
    parser="python"
fi

# Liveness probe (matches dispatchable-sessions oracle's contract)
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
        powershell.exe -NoProfile -Command "if (Get-Process -Id $pid -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }" >/dev/null 2>&1
    else
        kill -0 "$pid" 2>/dev/null
    fi
}

if [ -n "$parser" ] && [ -f "$REGISTRY" ]; then
    # Emit one line per live peer matching this workDir:
    #   "<pid>	<file1>	<file2>..."   (tab-separated; first field PID).
    if [ "$parser" = "jq" ]; then
        # CRITICAL: pass PROJ_ROOT_NORM via env.WD (NOT --arg) to defeat MSYS path
        # conversion on Git Bash, which silently rewrites POSIX-looking --arg
        # values like "/tmp/foo" to "C:/Users/<user>/AppData/Local/Temp/foo"
        # before jq sees them. MSYS_NO_PATHCONV=1 disables that conversion for
        # this single jq invocation -- both --arg and env.X paths get path-
        # converted by default; this is the only flag that suppresses both.
        # The registry's workDir field (loaded via the JSON file) is NOT
        # path-converted; conversion only affects values entering jq via CLI
        # args / env vars. Disabling per-call keeps the scope tight.
        peers_data="$(MSYS_NO_PATHCONV=1 WD="$PROJ_ROOT_NORM" jq -r '
            (env.WD) as $wd
            | (.sessions // [])
            | map(select(
                ((.status // "") == "active")
                and ((.claudeShellPid // null) != null)
                and (((.workDir // "") | gsub("\\\\"; "/") | sub("/+$"; "")) == $wd)
              ))
            | .[]
            | (([.claudeShellPid | tostring] + ((.dirtyFiles // []) | map(tostring))) | join("	"))
        ' "$REGISTRY" 2>/dev/null)"
    else
        peers_data="$(WD="$PROJ_ROOT_NORM" "$parser" - "$REGISTRY" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
wd = os.environ.get("WD", "")
sessions = d.get("sessions") or [] if isinstance(d, dict) else []
for s in sessions:
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    pid = s.get("claudeShellPid")
    if pid is None: continue
    swd = (s.get("workDir") or "").replace("\\", "/").rstrip("/")
    if swd != wd: continue
    df = s.get("dirtyFiles") or []
    if not isinstance(df, list): df = []
    parts = [str(pid)] + [str(x) for x in df]
    print("\t".join(parts))
PY
)"
    fi

    if [ -n "$peers_data" ]; then
        # Each line: "<pid>\t<file1>\t<file2>..."  (tab-separated; first field PID)
        while IFS= read -r line; do
            [ -n "$line" ] || continue
            old_IFS="$IFS"
            IFS=$'\t'
            # shellcheck disable=SC2086
            set -- $line
            IFS="$old_IFS"
            pid="$1"
            shift
            [ -n "$pid" ] || continue
            if pid_alive "$pid"; then
                LIVE_PEER_COUNT=$((LIVE_PEER_COUNT + 1))
                for f in "$@"; do
                    [ -n "$f" ] || continue
                    OWNED_FILES="${OWNED_FILES}${f}
"
                done
            fi
        done <<< "$peers_data"
    fi
fi

# Helper: returns 0 if path is in OWNED_FILES (line-exact), 1 otherwise.
is_owned_by_peer() {
    local p="$1"
    [ -n "$OWNED_FILES" ] || return 1
    printf '%s' "$OWNED_FILES" | grep -Fxq "$p"
}

# ---- Walk dirty files, classify, build sample ------------------------------
NOW_EPOCH="$(date -u +%s)"

STRAND_COUNT=0
OLDEST_EPOCH=""
SAMPLE_BUF=""    # JSON entries joined with ","

# Per-platform stat for mtime in epoch seconds.
stat_mtime_epoch() {
    local f="$1"
    [ -e "$f" ] || return 1
    # GNU stat
    local m
    m=$(stat -c %Y "$f" 2>/dev/null) && [ -n "$m" ] && { echo "$m"; return 0; }
    # BSD stat (macOS)
    m=$(stat -f %m "$f" 2>/dev/null) && [ -n "$m" ] && { echo "$m"; return 0; }
    return 1
}

epoch_to_iso() {
    local e="$1"
    [ -n "$e" ] || return 0
    local out
    out=$(date -u -d "@$e" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null) && [ -n "$out" ] && { echo "$out"; return 0; }
    out=$(date -u -r "$e" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null) && [ -n "$out" ] && { echo "$out"; return 0; }
    if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
        python3 -c "import sys; from datetime import datetime, timezone; print(datetime.fromtimestamp(int(sys.argv[1]), tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))" "$e" 2>/dev/null
    fi
}

for f in "${DIRTY_FILES[@]}"; do
    [ -n "$f" ] || continue
    [ -e "$f" ] || continue
    # Live-peer ownership filter
    if is_owned_by_peer "$f"; then
        continue
    fi
    fmt_e="$(stat_mtime_epoch "$f")"
    [ -n "$fmt_e" ] || continue
    # Mtime-vs-finalize gate. If we have no last_finalize_epoch, we cannot classify
    # (no commits yet); skip — strands need a finalize boundary to predate.
    if [ -z "$LAST_FINALIZE_EPOCH" ]; then
        continue
    fi
    if [ "$fmt_e" -ge "$LAST_FINALIZE_EPOCH" ]; then
        continue
    fi
    STRAND_COUNT=$((STRAND_COUNT + 1))
    if [ -z "$OLDEST_EPOCH" ] || [ "$fmt_e" -lt "$OLDEST_EPOCH" ]; then
        OLDEST_EPOCH="$fmt_e"
    fi
    if [ "$STRAND_COUNT" -le "$SAMPLE_CAP" ]; then
        f_iso="$(epoch_to_iso "$fmt_e")"
        age_days=$(( (NOW_EPOCH - fmt_e) / 86400 ))
        # Normalize forward slashes for cross-platform contract
        f_norm="$(printf '%s' "$f" | tr '\\' '/')"
        entry="{\"path\":\"$(esc "$f_norm")\",\"mtime\":\"$(esc "$f_iso")\",\"age_days\":$age_days}"
        if [ -z "$SAMPLE_BUF" ]; then
            SAMPLE_BUF="$entry"
        else
            SAMPLE_BUF="${SAMPLE_BUF},${entry}"
        fi
    fi
done

# ---- Emit results ----------------------------------------------------------
if [ "$STRAND_COUNT" -eq 0 ]; then
    emit_empty "$LAST_FINALIZE_JSON" "$LIVE_PEER_COUNT"
fi

OLDEST_ISO="$(epoch_to_iso "$OLDEST_EPOCH")"
OLDEST_JSON="\"$(esc "$OLDEST_ISO")\""
OLDEST_AGE_DAYS=$(( (NOW_EPOCH - OLDEST_EPOCH) / 86400 ))

if [ "$STRAND_COUNT" -lt "$LARGE_THRESHOLD" ]; then
    BRIEFING="stranded-dirty-files: ${STRAND_COUNT} files (oldest ${OLDEST_AGE_DAYS} days; no live owner) — run /0-uldf-finalize --include-stranded for cleanup"
else
    BRIEFING="stranded-dirty-files: ${STRAND_COUNT} files (oldest ${OLDEST_AGE_DAYS} days) — significant accumulation; see /0-uldf-oracle stranded-dirty-files for full sample"
fi

emit "true" "$STRAND_COUNT" "$OLDEST_JSON" "[${SAMPLE_BUF}]" "$LIVE_PEER_COUNT" "$LAST_FINALIZE_JSON" "$BRIEFING"
