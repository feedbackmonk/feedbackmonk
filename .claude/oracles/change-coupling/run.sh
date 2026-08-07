#!/bin/bash
# change-coupling oracle (Unix) — scrutiny ADD proposal A2 (KEEP), Arc 1.
#
# Git co-change mining across module boundaries: two files/modules that
# repeatedly change in the same commits are coupled *in fact*, whatever the
# import graph says. Language-agnostic (needs only `git log`) so it covers
# config/doc/cross-language coupling no parser sees — including this repo's own
# bash/PowerShell/markdown substrate. One of the M3 "second trigger family"
# members: high boundary-crossing co-change is the architecture-decomposition
# trigger the capacity-only (>85% context) rule misses.
#
# ADVISORY, correlation-not-dependency: co-change is *evidence* of coupling,
# never proof of a dependency. NEVER exits nonzero on findings. Read-only
# except its own session-state cache.
#
# On-demand oracle (kind: project-state) — NOT in the every-session briefing
# fan-out. Invoke via `.claude/oracles/change-coupling/run.sh` or the audit.
#
# Output: single-line JSON. NO-DATA honesty: not-a-repo / no history /
# insufficient history / no co-change-analyzable commits => status "no-data"
# with a reason — never a silent empty-but-"ok" that reads as "no coupling".

set +e

# ----------------------------------------------------------------------------
# Defaults (overridable via .claude/config.json -> changeCoupling.*)
# ----------------------------------------------------------------------------
DEF_SINCE_DAYS=180
DEF_MAX_COMMITS=1000
DEF_BULK_MAX=20      # commits touching MORE than this many files are excluded
DEF_MIN_CO=5         # a pair must co-change at least this many times to report
CAP=50                             # max entries per output array (no-silent-caps rule)

NO_CACHE=0
for arg in "$@"; do
    case "$arg" in
        --no-cache) NO_CACHE=1 ;;
        *) ;;
    esac
done

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

emit_no_data() {
    # $1 = reason (already JSON-safe plain text)
    local reason; reason="$(esc "$1")"
    local sd="${SINCE_DAYS:-$DEF_SINCE_DAYS}" mc="${MAX_COMMITS:-$DEF_MAX_COMMITS}"
    local bm="${BULK_MAX:-$DEF_BULK_MAX}" mco="${MIN_CO:-$DEF_MIN_CO}"
    local ca="${COMMITS_ANALYZED:-0}"
    cat <<EOF
{"status":"no-data","reason":"$reason","shallow":${SHALLOW:-false},"window":{"sinceDays":$sd,"maxCommits":$mc,"commitsAnalyzed":$ca,"qualifyingCommits":0},"filters":{"bulkCommitMax":$bm,"minCoChanges":$mco,"excludedCommits":${EXCLUDED_BULK:-0}},"filePairs":[],"modulePairs":[],"crossBoundaryTop":[],"truncated":false,"cached":false,"briefing":"change-coupling: NO-DATA ($reason)"}
EOF
    exit 0
}

command -v git >/dev/null 2>&1 || { SINCE_DAYS=$DEF_SINCE_DAYS; MAX_COMMITS=$DEF_MAX_COMMITS; BULK_MAX=$DEF_BULK_MAX; MIN_CO=$DEF_MIN_CO; emit_no_data "git not available"; }

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)"
if [ -z "$REPO_ROOT" ]; then
    SINCE_DAYS=$DEF_SINCE_DAYS; MAX_COMMITS=$DEF_MAX_COMMITS; BULK_MAX=$DEF_BULK_MAX; MIN_CO=$DEF_MIN_CO
    emit_no_data "not a git repository"
fi

# ----------------------------------------------------------------------------
# Load config (jq optional — silent fall back to defaults)
# ----------------------------------------------------------------------------
SINCE_DAYS=$DEF_SINCE_DAYS; MAX_COMMITS=$DEF_MAX_COMMITS
BULK_MAX=$DEF_BULK_MAX; MIN_CO=$DEF_MIN_CO
CONFIG="$REPO_ROOT/.claude/config.json"
if [ -f "$CONFIG" ] && command -v jq >/dev/null 2>&1; then
    _sd="$(jq -r '.changeCoupling.sinceDays // empty' "$CONFIG" 2>/dev/null)"
    _mc="$(jq -r '.changeCoupling.maxCommits // empty' "$CONFIG" 2>/dev/null)"
    _bm="$(jq -r '.changeCoupling.bulkCommitMax // empty' "$CONFIG" 2>/dev/null)"
    _co="$(jq -r '.changeCoupling.minCoChanges // empty' "$CONFIG" 2>/dev/null)"
    case "$_sd" in ''|*[!0-9]*) ;; *) SINCE_DAYS="$_sd" ;; esac
    case "$_mc" in ''|*[!0-9]*) ;; *) MAX_COMMITS="$_mc" ;; esac
    case "$_bm" in ''|*[!0-9]*) ;; *) BULK_MAX="$_bm" ;; esac
    case "$_co" in ''|*[!0-9]*) ;; *) MIN_CO="$_co" ;; esac
fi

# ----------------------------------------------------------------------------
# HEAD + shallow state
# ----------------------------------------------------------------------------
HEAD_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null)"
[ -z "$HEAD_SHA" ] && emit_no_data "no commit history"
SHALLOW=false
if [ "$(git -C "$REPO_ROOT" rev-parse --is-shallow-repository 2>/dev/null)" = "true" ]; then
    SHALLOW=true
fi

# ----------------------------------------------------------------------------
# Cache (keyed by HEAD sha + config) — .claude/session-state, never committed
# ----------------------------------------------------------------------------
CACHE_DIR="$REPO_ROOT/.claude/session-state"
CACHE_FILE="$CACHE_DIR/change-coupling-cache.json"
CACHE_KEY="${HEAD_SHA}|sd=${SINCE_DAYS};mc=${MAX_COMMITS};bm=${BULK_MAX};co=${MIN_CO}"
if [ "$NO_CACHE" -eq 0 ] && [ -f "$CACHE_FILE" ] && command -v jq >/dev/null 2>&1; then
    _ck="$(jq -r '.key // empty' "$CACHE_FILE" 2>/dev/null)"
    if [ -n "$_ck" ] && [ "$_ck" = "$CACHE_KEY" ]; then
        # Re-emit the cached result string with cached:true (fast path, <1s).
        # result is stored as a raw JSON STRING (unified with run.ps1) so the
        # two shells can share one cache file without format drift.
        _rj="$(jq -r '.result // empty' "$CACHE_FILE" 2>/dev/null)"
        if [ -n "$_rj" ]; then
            printf '%s\n' "$_rj" | sed 's/"cached":false/"cached":true/'
            exit 0
        fi
    fi
fi

# ----------------------------------------------------------------------------
# Mine the history
# ----------------------------------------------------------------------------
TMP="$(mktemp -d 2>/dev/null)" || TMP="${TMPDIR:-/tmp}/cc.$$"
mkdir -p "$TMP" 2>/dev/null
trap 'rm -rf "$TMP" 2>/dev/null' EXIT

RAW="$TMP/raw.txt"
git -C "$REPO_ROOT" -c core.quotepath=off log --name-only --no-merges \
    --since="${SINCE_DAYS} days ago" --max-count="${MAX_COMMITS}" \
    --pretty=format:'@@C@@%H' > "$RAW" 2>/dev/null

if [ ! -s "$RAW" ]; then
    COMMITS_ANALYZED=0
    emit_no_data "no commits in the ${SINCE_DAYS}-day window"
fi

# awk pass 1: per-commit file lists + stats.
# Emits qualifying commits' file lists (2..BULK_MAX files) to $QUAL, one
# tab-separated line per commit. Prints "analyzed excludedBulk qualifying"
# to stdout.
QUAL="$TMP/qual.tsv"
STATS="$(awk -v BULK="$BULK_MAX" -v OUT="$QUAL" '
    function flush(   n,i,line) {
        if (marker_seen == 0) return
        analyzed++
        n = fcount
        if (n > BULK) { excluded++; }
        else if (n >= 2) {
            qualifying++
            line = files[1]
            for (i = 2; i <= n; i++) line = line "\t" files[i]
            print line >> OUT
        }
        # reset
        fcount = 0
        delete files
    }
    {
        if (substr($0,1,5) == "@@C@@") { flush(); marker_seen = 1; next }
        if ($0 == "") { next }              # blank line between commits
        files[++fcount] = $0
    }
    END {
        flush()
        printf "%d %d %d", analyzed, excluded, qualifying
    }
' "$RAW")"

COMMITS_ANALYZED="$(printf '%s' "$STATS" | awk '{print $1}')"
EXCLUDED_BULK="$(printf '%s' "$STATS" | awk '{print $2}')"
QUALIFYING="$(printf '%s' "$STATS" | awk '{print $3}')"
case "$COMMITS_ANALYZED" in ''|*[!0-9]*) COMMITS_ANALYZED=0 ;; esac
case "$EXCLUDED_BULK" in ''|*[!0-9]*) EXCLUDED_BULK=0 ;; esac
case "$QUALIFYING" in ''|*[!0-9]*) QUALIFYING=0 ;; esac

if [ "$COMMITS_ANALYZED" -lt 2 ]; then
    emit_no_data "insufficient history (${COMMITS_ANALYZED} commit(s) in window)"
fi
if [ "$QUALIFYING" -eq 0 ]; then
    emit_no_data "no co-change-analyzable commits (all single-file or bulk-filtered)"
fi

# ----------------------------------------------------------------------------
# Module map: distinct files -> nearest-ancestor-README module.
# Compute once per distinct directory (memoised).
# ----------------------------------------------------------------------------
DISTINCT="$TMP/files.txt"
tr '\t' '\n' < "$QUAL" | sort -u > "$DISTINCT"

# Fork-free mapping, bash-3.2-safe (no declare -A): resolve each DISTINCT
# DIRECTORY once (the dir set is the memo — dirs are pre-deduped), then join
# files to modules with the FNR==NR two-file awk idiom (awk's native assoc
# array does the lookup portably). On Git Bash a subshell (dirname / $(...))
# costs ~10-40ms; ~1.6k files would be seconds of pure fork overhead. The
# only syscall per distinct dir is the [ -f ] stat.
DIRS="$TMP/dirs.txt"
awk '{ if ($0 == "") next; if (index($0, "/") == 0) { print "." } else { sub(/\/[^\/]*$/, "", $0); print } }' "$DISTINCT" | sort -u > "$DIRS"

DIRMAP="$TMP/dirmap.tsv"
: > "$DIRMAP"
while IFS= read -r dir; do
    [ -z "$dir" ] && continue
    d="$dir"; mod=""
    while [ -n "$d" ] && [ "$d" != "." ] && [ "$d" != "/" ]; do
        if [ -f "$REPO_ROOT/$d/README.md" ]; then mod="$d"; break; fi
        if [ "${d#*/}" = "$d" ]; then d="."; else d="${d%/*}"; fi
    done
    if [ -z "$mod" ]; then
        if [ "$dir" = "." ]; then mod="(root)"; else mod="${dir%%/*}"; fi
        [ -z "$mod" ] && mod="(root)"
    fi
    printf '%s\t%s\n' "$dir" "$mod" >> "$DIRMAP"
done < "$DIRS"

MAP="$TMP/map.tsv"
awk -F'\t' '
    FNR==NR { m[$1] = $2; next }
    {
        f = $0; if (f == "") next
        dir = f
        if (index(f, "/") == 0) { dir = "." } else { sub(/\/[^\/]*$/, "", dir) }
        print f "\t" m[dir]
    }' "$DIRMAP" "$DISTINCT" > "$MAP"

# ----------------------------------------------------------------------------
# awk pass 2: pair counting (file-level + module-level) using the map.
# Reads MAP first (file->module), then QUAL commit file-lists.
# Emits FP / MP lines only for pairs reaching MIN_CO.
# ----------------------------------------------------------------------------
FP="$TMP/fp.tsv"; MP="$TMP/mp.tsv"
# -F'\t' is load-bearing: QUAL/MAP are tab-separated and file paths (quotepath
# off) may contain spaces — awk's default FS (space AND tab) would mis-split
# them and corrupt the counts.
awk -F '\t' -v MINCO="$MIN_CO" -v FPOUT="$FP" -v MPOUT="$MP" '
    function pair(x, y,   t) { if (x <= y) return x SUBSEP y; return y SUBSEP x }
    FNR == NR { mod[$1] = $2; next }               # MAP file: file<TAB>module
    {
        n = NF
        # de-dup files within a commit (safety) and collect
        delete cf
        m = 0
        for (i = 1; i <= n; i++) { cf[++m] = $i }
        for (i = 1; i < m; i++) {
            for (j = i + 1; j <= m; j++) {
                a = cf[i]; b = cf[j]
                if (a == b) continue
                fpc[pair(a,b)]++
                ma = mod[a]; mb = mod[b]
                if (ma == "") ma = "(unknown)"
                if (mb == "") mb = "(unknown)"
                mpc[pair(ma,mb)]++
            }
        }
    }
    END {
        for (k in fpc) {
            if (fpc[k] < MINCO) continue
            split(k, p, SUBSEP)
            printf "%s\t%s\t%d\n", p[1], p[2], fpc[k] >> FPOUT
        }
        for (k in mpc) {
            if (mpc[k] < MINCO) continue
            split(k, p, SUBSEP)
            cross = (p[1] == p[2]) ? 0 : 1
            printf "%s\t%s\t%d\t%d\n", p[1], p[2], mpc[k], cross >> MPOUT
        }
    }
' "$MAP" "$QUAL"

[ -f "$FP" ] || : > "$FP"
[ -f "$MP" ] || : > "$MP"

# Sort desc by count; cap; detect truncation.
FP_SORTED="$TMP/fp.sorted"; MP_SORTED="$TMP/mp.sorted"; CB_SORTED="$TMP/cb.sorted"
sort -t "$(printf '\t')" -k3,3nr "$FP" > "$FP_SORTED" 2>/dev/null
sort -t "$(printf '\t')" -k3,3nr "$MP" > "$MP_SORTED" 2>/dev/null
awk -F '\t' '$4 == 1' "$MP_SORTED" > "$CB_SORTED" 2>/dev/null

FP_TOTAL=$(wc -l < "$FP_SORTED" 2>/dev/null | tr -d ' '); FP_TOTAL=${FP_TOTAL:-0}
MP_TOTAL=$(wc -l < "$MP_SORTED" 2>/dev/null | tr -d ' '); MP_TOTAL=${MP_TOTAL:-0}
CB_TOTAL=$(wc -l < "$CB_SORTED" 2>/dev/null | tr -d ' '); CB_TOTAL=${CB_TOTAL:-0}

TRUNCATED=false
{ [ "$FP_TOTAL" -gt "$CAP" ] || [ "$MP_TOTAL" -gt "$CAP" ] || [ "$CB_TOTAL" -gt "$CAP" ]; } && TRUNCATED=true

# ----------------------------------------------------------------------------
# Build JSON arrays (jq: TSV -> objects, order preserved from the sort)
# ----------------------------------------------------------------------------
FP_JSON='[]'; MP_JSON='[]'; CB_JSON='[]'
if command -v jq >/dev/null 2>&1; then
    FP_JSON="$(head -n "$CAP" "$FP_SORTED" | jq -R -s -c '
        split("\n") | map(select(length>0) | split("\t")
        | {a:.[0], b:.[1], coChanges:(.[2]|tonumber)})')"
    MP_JSON="$(head -n "$CAP" "$MP_SORTED" | jq -R -s -c '
        split("\n") | map(select(length>0) | split("\t")
        | {moduleA:.[0], moduleB:.[1], coChanges:(.[2]|tonumber), crossBoundary:(.[3]=="1")})')"
    CB_JSON="$(head -n "$CAP" "$CB_SORTED" | jq -R -s -c '
        split("\n") | map(select(length>0) | split("\t")
        | {moduleA:.[0], moduleB:.[1], coChanges:(.[2]|tonumber), crossBoundary:true})')"
fi
[ -z "$FP_JSON" ] && FP_JSON='[]'
[ -z "$MP_JSON" ] && MP_JSON='[]'
[ -z "$CB_JSON" ] && CB_JSON='[]'

# Briefing: summarise the top cross-boundary coupling (advisory).
if [ "$CB_TOTAL" -gt 0 ]; then
    TOP_LINE="$(head -n1 "$CB_SORTED")"
    T_A="$(printf '%s' "$TOP_LINE" | cut -f1)"
    T_B="$(printf '%s' "$TOP_LINE" | cut -f2)"
    T_N="$(printf '%s' "$TOP_LINE" | cut -f3)"
    BRIEFING="$(esc "change-coupling: ${CB_TOTAL} cross-boundary pair(s); top ${T_A} <-> ${T_B} (${T_N}x). Advisory co-change evidence, not proven dependency.")"
else
    BRIEFING="$(esc "change-coupling: no cross-boundary pair reached ${MIN_CO} co-changes over ${COMMITS_ANALYZED} analyzed commits (checked, clean).")"
fi

RESULT="$(cat <<EOF
{"status":"ok","shallow":$SHALLOW,"window":{"sinceDays":$SINCE_DAYS,"maxCommits":$MAX_COMMITS,"commitsAnalyzed":$COMMITS_ANALYZED,"qualifyingCommits":$QUALIFYING},"filters":{"bulkCommitMax":$BULK_MAX,"minCoChanges":$MIN_CO,"excludedCommits":$EXCLUDED_BULK},"filePairs":$FP_JSON,"modulePairs":$MP_JSON,"crossBoundaryTop":$CB_JSON,"truncated":$TRUNCATED,"cached":false,"briefing":"$BRIEFING"}
EOF
)"

# Write cache (best-effort; never fail the run). result is stored as a raw JSON
# STRING so run.sh and run.ps1 share one interchangeable cache format.
if command -v jq >/dev/null 2>&1; then
    mkdir -p "$CACHE_DIR" 2>/dev/null
    jq -n --arg key "$CACHE_KEY" --arg result "$RESULT" '{key:$key, result:$result}' \
        > "$CACHE_FILE.tmp" 2>/dev/null && mv "$CACHE_FILE.tmp" "$CACHE_FILE" 2>/dev/null
fi

printf '%s\n' "$RESULT"
exit 0
