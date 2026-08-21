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
    local attribution="$6"         # evidence grade (DEFER-039/190; DEC-372)
    local last_finalize_json="$7"  # quoted ISO string OR "null"
    local briefing="$8"            # raw string (will be esc'd here)
    local provably_unowned="${9:-0}"  # STRAND-02: count proved by mtime ordering
    cat <<EOF
{"has_stranded":$has_stranded,"count":$count,"oldest_mtime":$oldest_mtime_json,"sample":$sample_json,"live_peer_count":$live_peer_count,"provably_unowned":$provably_unowned,"attribution":"$(esc "$attribution")","last_finalize_at":$last_finalize_json,"briefing":"$(esc "$briefing")"}
EOF
    exit 0
}

emit_empty() {
    local last_finalize_json="${1:-null}"
    local live_peer_count="${2:-0}"
    local attribution="${3:-no-peers}"
    emit "false" "0" "null" "[]" "$live_peer_count" "$attribution" "$last_finalize_json" "" "0"
}

# ---- Graceful absence: not in a git repo -----------------------------------
if ! git rev-parse --git-dir >/dev/null 2>&1; then
    emit_empty "null" "0" "no-peers"
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
    emit "false" "-1" "null" "[]" "0" "not-measured" "$LAST_FINALIZE_JSON" "$BRIEFING" "0"
fi

# ---- Empty dirty set -> graceful empty -------------------------------------
if [ "$DIRTY_COUNT" -eq 0 ]; then
    emit_empty "$LAST_FINALIZE_JSON" "0" "no-peers"
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

# DEFER-190 (measured on the ULDF repo, 2026-08-16): under Git Bash `pwd`
# returns the MSYS form (`/s/ULDF`) while the registry records the Windows
# form (`S:\ULDF`), so the workDir equality NEVER matched and this twin
# reported live_peer_count: 0 against 8 live peers -- which would make the
# attribution ladder below grade 'no-peers' and KEEP recommending the sweep.
# `pwd -W` gives the Windows form; accept a registry workDir equal to EITHER,
# case-insensitively (Windows paths are case-insensitive; the ps1 twin's
# journal prefix match is OrdinalIgnoreCase for the same reason).
PROJ_ROOT_NORM2="$PROJ_ROOT_NORM"
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
        _wd_win="$(pwd -W 2>/dev/null | tr '\\' '/' | sed 's:/*$::')"
        [ -n "$_wd_win" ] && PROJ_ROOT_NORM2="$_wd_win"
        ;;
esac

LIVE_PEER_COUNT=0
ATTRIBUTING_PEER_COUNT=0   # live peers that published a dirtyFiles array (STRONG)
JOURNAL_PEER_COUNT=0       # live peers whose write journal was read (POSITIVE-ONLY)
OWNED_FILES=""  # newline-separated list

# STRAND-02 (DEC-372): the mtime-ordering proof. A live peer that had written
# file F would have set F's mtime at or after its OWN start, so
#   mtime(F) < min over live peers of spawnedAt
# proves F is not the in-flight work of any visible live peer -- the exact
# proposition `measured` was defined to deliver, from data every registry
# already carries and with no writer anywhere.
#
# Why spawnedAt is a sound lower bound (all three checked, not assumed):
#   * it is PRESERVED across re-registration -- hooks/session-start.sh
#     `spawnedAt: ($existing.spawnedAt // $created)` -- so it is a stable
#     session-start anchor, never refreshed to "now";
#   * for a PODS worker the roster row is written BEFORE the worker boots, so it
#     errs EARLY, which is the conservative direction (a stricter bar);
#   * the framework already trusts this exact anchor for this exact class of
#     reasoning -- commit-scoped.sh `_sw_session_ts` uses registry spawnedAt as
#     the session window.
# Fails CLOSED: one live peer with an absent or unparseable spawnedAt makes the
# proof unavailable for the WHOLE run (PEER_START_UNKNOWN=1), because the
# earliest start is then not known and no file can be shown to predate it.
EARLIEST_PEER_START=""     # epoch seconds; min over live peers
PEER_START_UNKNOWN=0

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

# SWEEP-10 / DEFER-095: identity-aware liveness via lib/pid-liveness.sh. A
# recycled peer pid (live process, started after the entry's own
# claudeShellPidWrittenAt anchor) inflated LIVE_PEER_COUNT and its dirtyFiles
# kept masking genuinely stranded files. Anchor-only, no name glob (DEC-257);
# absent/unparseable anchor or lib unavailable -> byte-identical existence-only
# verdict. Path is REPORTABLE (QUIESCE-08 W4).
_SDF_THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
SDF_PID_IDENTITY="fallback"
for _sdf_pl in \
    "$_SDF_THIS_DIR/../../scripts/lib/pid-liveness.sh" \
    "$_SDF_THIS_DIR/../../../claude-template/scripts/lib/pid-liveness.sh" \
    "$HOME/.claude/scripts/lib/pid-liveness.sh"; do
    if [ -f "$_sdf_pl" ]; then
        # shellcheck source=/dev/null
        . "$_sdf_pl" 2>/dev/null || true
        break
    fi
done
command -v pid_is_alive_as >/dev/null 2>&1 && SDF_PID_IDENTITY="lib"
if [ "${ULDF_SDF_REPORT_PID_IDENTITY:-}" = "1" ]; then
    printf '%s\n' "$SDF_PID_IDENTITY"
    exit 0
fi

# sdf_peer_alive <pid> <anchor> -- identity-aware when the lib resolved.
sdf_peer_alive() {
    if [ "$SDF_PID_IDENTITY" = "lib" ]; then
        pid_is_alive_as "$1" "$2"
    else
        pid_alive "$1"
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
        peers_data="$(MSYS_NO_PATHCONV=1 WD="$PROJ_ROOT_NORM" WD2="$PROJ_ROOT_NORM2" jq -r '
            (env.WD  | ascii_downcase) as $wd
            | (env.WD2 | ascii_downcase) as $wd2
            | (.sessions // [])
            | map(select(
                ((.status // "") == "active")
                and ((.claudeShellPid // null) != null)
                and ((((.workDir // "") | gsub("\\\\"; "/") | sub("/+$"; "") | ascii_downcase)) as $swd
                     | ($swd == $wd or $swd == $wd2))
              ))
            | .[]
            | (([.claudeShellPid | tostring]
                + [((.claudeShellPidWrittenAt // "-") | tostring)]
                + [(if ((.sessionId // "") | tostring | length) > 0 then (.sessionId | tostring) else "-" end)]
                + [(if (.dirtyFiles != null) then "1" else "0" end)]
                + [(if ((.spawnedAt // "") | tostring | length) > 0 then (.spawnedAt | tostring) else "-" end)]
                + ((.dirtyFiles // []) | map(tostring))) | join("	"))
        ' "$REGISTRY" 2>/dev/null)"
    else
        peers_data="$(WD="$PROJ_ROOT_NORM" WD2="$PROJ_ROOT_NORM2" "$parser" - "$REGISTRY" <<'PY' 2>/dev/null
import json, os, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
wd = os.environ.get("WD", "").lower()
wd2 = os.environ.get("WD2", "").lower()
sessions = d.get("sessions") or [] if isinstance(d, dict) else []
for s in sessions:
    if not isinstance(s, dict): continue
    if s.get("status") != "active": continue
    pid = s.get("claudeShellPid")
    if pid is None: continue
    swd = (s.get("workDir") or "").replace("\\", "/").rstrip("/").lower()
    if swd != wd and swd != wd2: continue
    df = s.get("dirtyFiles")
    pub = "0" if df is None else "1"
    if not isinstance(df, list): df = []
    wa = s.get("claudeShellPidWrittenAt") or "-"
    sid = str(s.get("sessionId") or "-") or "-"
    sa = str(s.get("spawnedAt") or "-") or "-"
    parts = [str(pid), str(wa), sid, pub, sa] + [str(x) for x in df]
    print("\t".join(parts))
PY
)"
    fi

    if [ -n "$peers_data" ]; then
        # Each line:
        #   "<pid>\t<writtenAt|->\t<sessionId|->\t<pub 0|1>\t<spawnedAt|->\t<file1>..."
        # (tab-separated; field 2 is the DEFER-095 identity anchor, field 3 the
        # sessionId for the write-journal lookup, field 4 whether the peer
        # PUBLISHED a dirtyFiles array, field 5 the STRAND-02 session-start
        # anchor -- "-" sentinels, never an empty field into a tab-IFS protocol,
        # DEFER-078).
        while IFS= read -r line; do
            [ -n "$line" ] || continue
            old_IFS="$IFS"
            IFS=$'\t'
            # shellcheck disable=SC2086
            set -- $line
            IFS="$old_IFS"
            pid="$1"
            anchor="${2:-}"
            sid="${3:-}"
            pub="${4:-0}"
            started="${5:-}"
            n=0; while [ $# -gt 0 ] && [ "$n" -lt 5 ]; do shift; n=$((n + 1)); done
            [ "$anchor" = "-" ] && anchor=""
            [ "$sid" = "-" ] && sid=""
            [ "$started" = "-" ] && started=""
            [ -n "$pid" ] || continue
            if sdf_peer_alive "$pid" "$anchor"; then
                LIVE_PEER_COUNT=$((LIVE_PEER_COUNT + 1))

                # STRAND-02: fold this peer into the earliest-start floor. An
                # absent or unparseable spawnedAt disables the proof for the
                # whole run rather than silently dropping this peer from the
                # min() -- dropping it would RAISE the floor and prove more,
                # which is the unsafe direction.
                if [ -n "$started" ]; then
                    _st_epoch="$(iso_to_epoch "$started")"
                    if [ -n "$_st_epoch" ]; then
                        if [ -z "$EARLIEST_PEER_START" ] || [ "$_st_epoch" -lt "$EARLIEST_PEER_START" ]; then
                            EARLIEST_PEER_START="$_st_epoch"
                        fi
                    else
                        PEER_START_UNKNOWN=1
                    fi
                else
                    PEER_START_UNKNOWN=1
                fi

                # Source 1 (original): the peer's published dirtyFiles array --
                # complete by construction, so authoritative when present.
                # DEFER-039/DEFER-190: measured (GitCellar 2026-08-12, ULDF
                # 2026-08-16), NO session writes this field yet.
                if [ "$pub" = "1" ]; then
                    ATTRIBUTING_PEER_COUNT=$((ATTRIBUTING_PEER_COUNT + 1))
                fi
                for f in "$@"; do
                    [ -n "$f" ] || continue
                    OWNED_FILES="${OWNED_FILES}${f}
"
                done

                # Source 2 (DEFER-039 option 2): the peer's WRITE JOURNAL
                # (.claude/session-state/write-journal/<sessionId>.jsonl,
                # {ts, path, op} per Edit/Write) -- the same input
                # lib/file-owner.sh resolves ownership from. POSITIVE-ONLY
                # evidence: it records TOOL CALLS, so a script write or an
                # external editor leaves no record. A journal HIT rescues a
                # file from the strand list; a journal MISS proves nothing --
                # which is why journals alone yield 'journal-partial' below,
                # never 'measured'.
                if [ -n "$sid" ] && [ -f ".claude/session-state/write-journal/${sid}.jsonl" ]; then
                    JOURNAL_PEER_COUNT=$((JOURNAL_PEER_COUNT + 1))
                    jpaths=""
                    if [ "$parser" = "jq" ]; then
                        jpaths="$(MSYS_NO_PATHCONV=1 R1="$PROJ_ROOT_NORM" R2="$PROJ_ROOT_NORM2" jq -R -r '
                            (env.R1) as $r1 | (env.R2) as $r2
                            | (($r1 | ascii_downcase) + "/") as $r1l
                            | (($r2 | ascii_downcase) + "/") as $r2l
                            | fromjson? | ((.path // empty) | tostring | gsub("\\\\"; "/")) as $p
                            | ($p | ascii_downcase) as $pl
                            | if ($pl | startswith($r1l)) then $p[($r1l | length):]
                              elif ($pl | startswith($r2l)) then $p[($r2l | length):]
                              else empty end
                        ' ".claude/session-state/write-journal/${sid}.jsonl" 2>/dev/null)"
                    else
                        jpaths="$(R1="$PROJ_ROOT_NORM" R2="$PROJ_ROOT_NORM2" "$parser" - ".claude/session-state/write-journal/${sid}.jsonl" <<'PYJ' 2>/dev/null
import json, os, sys
r1 = os.environ.get("R1", "").lower() + "/"
r2 = os.environ.get("R2", "").lower() + "/"
try:
    with open(sys.argv[1], "r", encoding="utf-8-sig") as f:
        for line in f:
            line = line.strip()
            if not line: continue
            try: rec = json.loads(line)
            except Exception: continue
            p = rec.get("path") if isinstance(rec, dict) else None
            if not p: continue
            p = str(p).replace("\\", "/")
            pl = p.lower()
            if len(r1) > 1 and pl.startswith(r1): print(p[len(r1):])
            elif len(r2) > 1 and pl.startswith(r2): print(p[len(r2):])
except Exception:
    pass
PYJ
)"
                    fi
                    if [ -n "$jpaths" ]; then
                        OWNED_FILES="${OWNED_FILES}${jpaths}
"
                    fi
                fi
            fi
        done <<< "$peers_data"
    fi
fi

# ---- Attribution verdict, part 1: the peer-supplied grades ------------------
# (DEFER-039 / DEFER-190; extended by STRAND-02 / DEC-372 below)
# 'measured'        -- at least one live peer published a dirtyFiles array
#                      (complete by construction): an absent path is evidence
#                      of non-ownership. RESERVED: no producer exists anywhere
#                      in the framework (DISC-ORA-12); kept because it is
#                      authoritative if a producer ever appears and because
#                      already-deployed gates map it.
# 'predates-peers'  -- STRAND-02: every reported strand file predates the
#                      earliest live peer's session start, so no visible live
#                      peer can have written any of them. Decided AFTER the walk
#                      (it is a property of the reported set), below.
# 'journal-partial' -- attribution came only from write journals (positive-only:
#                      hits filter, a miss proves nothing) -- sweep withheld.
# 'unavailable'     -- live peers exist but none supplied attribution by either
#                      route. Silence is not evidence -- sweep withheld.
# 'no-peers'        -- no live peer in this project; no owner to miss.
if [ "$LIVE_PEER_COUNT" -eq 0 ]; then
    ATTRIBUTION="no-peers"
elif [ "$ATTRIBUTING_PEER_COUNT" -gt 0 ]; then
    ATTRIBUTION="measured"
elif [ "$JOURNAL_PEER_COUNT" -gt 0 ]; then
    ATTRIBUTION="journal-partial"
else
    ATTRIBUTION="unavailable"
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
PROVED_COUNT=0   # STRAND-02: reported strands proved un-owned by mtime ordering
UNPROVED_COUNT=0 # STRAND-02: reported strands the proof does NOT cover

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
    # STRAND-02: is this file proved un-owned by every live peer? With zero live
    # peers the proposition is vacuously true (there is no owner to miss), which
    # is the same fact 'no-peers' already reports.
    if [ "$LIVE_PEER_COUNT" -eq 0 ]; then
        PROVED_COUNT=$((PROVED_COUNT + 1))
    elif [ "$PEER_START_UNKNOWN" -eq 0 ] && [ -n "$EARLIEST_PEER_START" ] \
         && [ "$fmt_e" -lt "$EARLIEST_PEER_START" ]; then
        PROVED_COUNT=$((PROVED_COUNT + 1))
    else
        UNPROVED_COUNT=$((UNPROVED_COUNT + 1))
    fi
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
    emit_empty "$LAST_FINALIZE_JSON" "$LIVE_PEER_COUNT" "$ATTRIBUTION"
fi

OLDEST_ISO="$(epoch_to_iso "$OLDEST_EPOCH")"
OLDEST_JSON="\"$(esc "$OLDEST_ISO")\""
OLDEST_AGE_DAYS=$(( (NOW_EPOCH - OLDEST_EPOCH) / 86400 ))

# ---- Attribution verdict, part 2: the STRAND-02 mtime proof ------------------
# Promote a withholding grade to 'predates-peers' when EVERY reported strand
# file predates the earliest live peer's start. All-or-nothing over the reported
# set, because the sweep it gates is all-or-nothing: the consumer stages Set 2
# whole, so a single unproved file must hold the whole stage.
# It sits BELOW 'measured' (a published array is a direct statement of the
# peer's own working set) and ABOVE the two withholding grades, which it
# replaces only when the proof covers everything reported.
if [ "$LIVE_PEER_COUNT" -gt 0 ] && [ "$ATTRIBUTING_PEER_COUNT" -eq 0 ] \
   && [ "$PEER_START_UNKNOWN" -eq 0 ] && [ -n "$EARLIEST_PEER_START" ] \
   && [ "$UNPROVED_COUNT" -eq 0 ]; then
    ATTRIBUTION="predates-peers"
fi

# Withholding forms take precedence over the small/large split: an unproven
# list must not be rendered as a cleanup instruction regardless of its size.
# STRAND-02: a withheld run states how much of the list IS proved, so the
# refusal is actionable rather than opaque (DEC-360's escape-chain requirement
# applied to the report, not just to the reason string).
PROVED_CLAUSE=""
if [ "$LIVE_PEER_COUNT" -gt 0 ] && [ "$PEER_START_UNKNOWN" -eq 0 ] && [ -n "$EARLIEST_PEER_START" ]; then
    PROVED_CLAUSE=" ${PROVED_COUNT} of ${STRAND_COUNT} predate every live peer's session start; ${UNPROVED_COUNT} do not."
fi

if [ "$ATTRIBUTION" = "journal-partial" ]; then
    # Journal hits removed what they could from the list above; what remains is
    # unproven, not un-owned. Same withholding as 'unavailable', different reason.
    BRIEFING="stranded-dirty-files: ${STRAND_COUNT} pre-finalize dirty file(s) (oldest ${OLDEST_AGE_DAYS} days) - OWNERSHIP UNPROVEN: attribution came only from ${JOURNAL_PEER_COUNT} write journal(s), which record tool calls and cannot see script or external writes, so a miss is not evidence of non-ownership.${PROVED_CLAUSE} Do NOT sweep; inspect with /0-uldf-oracle stranded-dirty-files"
elif [ "$ATTRIBUTION" = "unavailable" ]; then
    # DEFER-039: OVALID -- an oracle that cannot see its subject says so instead
    # of asserting over it. Sweeping here would mean finalizing a live peer's
    # in-flight work, which is exactly what DEC-239 exists to prevent.
    BRIEFING="stranded-dirty-files: ${STRAND_COUNT} pre-finalize dirty file(s) (oldest ${OLDEST_AGE_DAYS} days) - OWNERSHIP UNKNOWN: ${LIVE_PEER_COUNT} live peer(s) and none publishes attribution, so none of these is shown to be un-owned.${PROVED_CLAUSE} Do NOT sweep; inspect with /0-uldf-oracle stranded-dirty-files"
elif [ "$ATTRIBUTION" = "predates-peers" ] && [ "$STRAND_COUNT" -lt "$LARGE_THRESHOLD" ]; then
    BRIEFING="stranded-dirty-files: ${STRAND_COUNT} files (oldest ${OLDEST_AGE_DAYS} days; no live owner — every one predates all ${LIVE_PEER_COUNT} live peer(s)' session start) — run /0-uldf-finalize --include-stranded for cleanup"
elif [ "$STRAND_COUNT" -lt "$LARGE_THRESHOLD" ]; then
    if [ "$ATTRIBUTION" = "measured" ]; then
        BRIEFING="stranded-dirty-files: ${STRAND_COUNT} files (oldest ${OLDEST_AGE_DAYS} days; no live owner per ${ATTRIBUTING_PEER_COUNT} attributing peer(s)) — run /0-uldf-finalize --include-stranded for cleanup"
    else
        # no-peers: trivially true "no live owner" -- there is no owner to miss.
        BRIEFING="stranded-dirty-files: ${STRAND_COUNT} files (oldest ${OLDEST_AGE_DAYS} days; no live owner — no live peers in this project) — run /0-uldf-finalize --include-stranded for cleanup"
    fi
else
    BRIEFING="stranded-dirty-files: ${STRAND_COUNT} files (oldest ${OLDEST_AGE_DAYS} days) — significant accumulation; see /0-uldf-oracle stranded-dirty-files for full sample"
fi

emit "true" "$STRAND_COUNT" "$OLDEST_JSON" "[${SAMPLE_BUF}]" "$LIVE_PEER_COUNT" "$ATTRIBUTION" "$LAST_FINALIZE_JSON" "$BRIEFING" "$PROVED_COUNT"
