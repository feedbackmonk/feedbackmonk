#!/bin/bash
# synopsis-coverage Verification Oracle (Unix)
# Reports the fraction of module READMEs conforming to the HCT Synopsis discipline:
# presence of a `## Synopsis` H2 section AND content between 1 and 5 non-empty lines.
#
# Output schema: see oracle.json. Spec: HCT-04 (docs/specs/SPECIFICATION.md).
# Verification Oracle category: FOUNDATIONS/ORACULURGY_DESIGN.md Part 11.

set -e

EXCLUDES='node_modules|target|\.git|\.vscode|\.idea|dist|build|out|coverage|__pycache__|\.venv|venv|\.claude/oracles/cache|\.claude/checkpoints'

# Walk-time prune (WinDirFul 2026-07-10, DEFER-windirful § 2): EXCLUDES used to
# be applied via grep AFTER find had walked the full tree, so both walks
# descended node_modules / Rust target/ etc. — 7+ min on a built project vs
# ~130s cold / <1s cached with pruning. Name-based excludes prune here; the two
# path-based excludes (.claude/oracles/cache, .claude/checkpoints) remain
# grep-filtered below (their parent trees must still be descended). Output set
# is identical to the grep-only behavior.
_SC_PRUNE=( \( -name node_modules -o -name target -o -name .git -o -name .vscode -o -name .idea -o -name dist -o -name build -o -name out -o -name coverage -o -name __pycache__ -o -name .venv -o -name venv \) -prune -o )

# =============================================================================
# Trigger-invalidate cache — implements the freshness contract the manifest
# declares (previously declared-but-unimplemented; ~65s recompute per call was
# why DEC-86 deferred this oracle from the briefing budget). Contract and
# rationale: see the twin block in module-tree-map/run.sh — set-level digest
# (path+mtime+size of every **/README.md, EXCLUDES-filtered) so edits, adds,
# deletes, AND renames all invalidate; store-only-when-pre/post-digests-agree
# guards mid-compute mutation. Briefing context (ULDF_BRIEFING=1): a COLD
# cache spawns ONE detached background refresh (stampede-guarded) and emits a
# minimal valid-schema object with empty briefing — graceful absence this
# session, warm line next session; stale cache is never served (absent beats
# wrong). --refresh / --no-cache force a synchronous recompute.
# =============================================================================
_SC_ORACLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
_SC_CACHE_DIR="$_SC_ORACLE_DIR/cache"
_SC_CACHE_FILE="$_SC_CACHE_DIR/latest.json"
_SC_DIGEST_FILE="$_SC_CACHE_DIR/trigger-digest.txt"
_SC_REFRESH_MARK="$_SC_CACHE_DIR/refresh-in-progress"

_SC_FORCE=""
for _a in "$@"; do
    case "$_a" in --refresh|--no-cache) _SC_FORCE=1 ;; esac
done

_sc_trigger_digest() {
    {
        find . "${_SC_PRUNE[@]}" -type f -name 'README.md' -printf '%p\t%T@\t%s\n' 2>/dev/null \
        || find . "${_SC_PRUNE[@]}" -type f -name 'README.md' -print 2>/dev/null \
            | while IFS= read -r _f; do
                _m=$(stat -f '%m %z' "$_f" 2>/dev/null || echo 0)
                printf '%s\t%s\n' "$_f" "$_m"
              done
    } \
        | grep -vE "(^\./|/)($EXCLUDES)(/|$)" \
        | LC_ALL=C sort \
        | { md5sum 2>/dev/null || shasum 2>/dev/null || cksum; } \
        | awk '{print $1}'
}

_SC_DIGEST_PRE="$(_sc_trigger_digest)"
if [ -z "$_SC_FORCE" ] && [ -n "$_SC_DIGEST_PRE" ] \
    && [ -f "$_SC_CACHE_FILE" ] && [ -f "$_SC_DIGEST_FILE" ] \
    && [ "$(cat "$_SC_DIGEST_FILE" 2>/dev/null)" = "$_SC_DIGEST_PRE" ]; then
    cat "$_SC_CACHE_FILE"
    exit 0
fi

# Cold cache in briefing context: detached refresh + graceful absence.
if [ -z "$_SC_FORCE" ] && [ "${ULDF_BRIEFING:-}" = "1" ]; then
    _SC_SPAWN_REFRESH=1
    if [ -f "$_SC_REFRESH_MARK" ]; then
        _SC_MARK_AGE=$(( $(date +%s) - $(stat -c '%Y' "$_SC_REFRESH_MARK" 2>/dev/null || stat -f '%m' "$_SC_REFRESH_MARK" 2>/dev/null || echo 0) ))
        [ "$_SC_MARK_AGE" -lt 600 ] && _SC_SPAWN_REFRESH=""
    fi
    if [ -n "$_SC_SPAWN_REFRESH" ]; then
        mkdir -p "$_SC_CACHE_DIR" 2>/dev/null || true
        : > "$_SC_REFRESH_MARK" 2>/dev/null || true
        ( ULDF_BRIEFING="" nohup bash -c \
            "cd \"$(pwd)\" && bash \"$_SC_ORACLE_DIR/run.sh\" --refresh >/dev/null 2>&1; rm -f \"$_SC_REFRESH_MARK\"" \
            >/dev/null 2>&1 & ) 2>/dev/null
    fi
    printf '{"coverage_pct":100,"conformant_count":0,"total_modules":0,"missing":[],"over_length":[],"briefing_summary":"","briefing":"","cache":"cold-refreshing"}\n'
    exit 0
fi

json_escape() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\n'/\\n}"
    s="${s//$'\r'/\\r}"
    s="${s//$'\t'/\\t}"
    printf '%s' "$s"
}

# Returns the count of non-empty content lines inside `## Synopsis`,
# or "MISSING" if the section is not present. HTML comments are stripped.
extract_synopsis_count() {
    local file="$1"
    awk '
        BEGIN { in_section = 0; in_comment = 0; found = 0; count = 0 }
        {
            if (!in_section) {
                if ($0 ~ /^##[[:space:]]+Synopsis[[:space:]]*$/) {
                    in_section = 1
                    found = 1
                    next
                }
                next
            }
            if ($0 ~ /^## /) { exit }
            if ($0 ~ /^[[:space:]]*<!--/) {
                in_comment = 1
                if ($0 ~ /-->[[:space:]]*$/) { in_comment = 0 }
                next
            }
            if (in_comment) {
                if ($0 ~ /-->[[:space:]]*$/) { in_comment = 0 }
                next
            }
            if ($0 ~ /[^[:space:]]/) { count++ }
        }
        END {
            if (!found) { print "MISSING" }
            else { print count }
        }
    ' "$file" 2>/dev/null
}

total=0
conformant=0
missing=()
over_length=()

# Root README
if [ -f "README.md" ]; then
    total=$((total + 1))
    res=$(extract_synopsis_count "README.md")
    if [ "$res" = "MISSING" ]; then
        missing+=(".")
    elif [ "$res" -gt 5 ]; then
        over_length+=(".")
    elif [ "$res" -lt 1 ]; then
        missing+=(".")
    else
        conformant=$((conformant + 1))
    fi
fi

# Subdirectory READMEs (full tree)
while IFS= read -r dir; do
    dir_clean="${dir#./}"
    [ -z "$dir_clean" ] || [ "$dir_clean" = "." ] && continue
    if echo "$dir_clean" | grep -qE "^($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    if echo "$dir_clean" | grep -qE "/($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    [ -f "$dir_clean/README.md" ] || continue

    total=$((total + 1))
    res=$(extract_synopsis_count "$dir_clean/README.md")
    if [ "$res" = "MISSING" ]; then
        missing+=("$dir_clean")
    elif [ "$res" -gt 5 ]; then
        over_length+=("$dir_clean")
    elif [ "$res" -lt 1 ]; then
        missing+=("$dir_clean")
    else
        conformant=$((conformant + 1))
    fi
done < <(find . "${_SC_PRUNE[@]}" -type d -print 2>/dev/null | sort)

# Coverage_pct (integer floor 0-100); 100 when total == 0 (graceful absence)
if [ "$total" -eq 0 ]; then
    coverage_pct=100
else
    coverage_pct=$((conformant * 100 / total))
fi

# Briefing per HCT-05 format; empty when coverage_pct == 100 (gracefully absent)
briefing=""
if [ "$total" -gt 0 ] && [ "$coverage_pct" -lt 100 ]; then
    missing_count=${#missing[@]}
    over_count=${#over_length[@]}
    briefing="${coverage_pct}% (${missing_count} missing, ${over_count} over-length). Run /0-uldf-uladp-compliance for details."
fi

# Sort + emit JSON arrays
ms_sorted=$(printf '%s\n' "${missing[@]}" | sort 2>/dev/null || true)
ms_json="["; first=1
while IFS= read -r m; do
    [ -z "$m" ] && continue
    if [ $first -eq 1 ]; then first=0; else ms_json+=","; fi
    ms_json+="\"$(json_escape "$m")\""
done <<< "$ms_sorted"
ms_json+="]"

ol_sorted=$(printf '%s\n' "${over_length[@]}" | sort 2>/dev/null || true)
ol_json="["; first=1
while IFS= read -r m; do
    [ -z "$m" ] && continue
    if [ $first -eq 1 ]; then first=0; else ol_json+=","; fi
    ol_json+="\"$(json_escape "$m")\""
done <<< "$ol_sorted"
ol_json+="]"

briefing_esc="$(json_escape "$briefing")"

# Emit via tmp so the cache stores exactly what is emitted; store only when
# the trigger set didn't mutate mid-compute (see cache block header).
_SC_OUT="$(mktemp 2>/dev/null || echo "/tmp/oracle-sc-$$.json")"
cat > "$_SC_OUT" <<EOF
{"coverage_pct":$coverage_pct,"conformant_count":$conformant,"total_modules":$total,"missing":$ms_json,"over_length":$ol_json,"briefing_summary":"$briefing_esc","briefing":"$briefing_esc"}
EOF

_SC_DIGEST_POST="$(_sc_trigger_digest)"
if [ -n "$_SC_DIGEST_POST" ] && [ "$_SC_DIGEST_POST" = "$_SC_DIGEST_PRE" ]; then
    mkdir -p "$_SC_CACHE_DIR" 2>/dev/null || true
    cp "$_SC_OUT" "$_SC_CACHE_FILE" 2>/dev/null \
        && printf '%s' "$_SC_DIGEST_POST" > "$_SC_DIGEST_FILE" 2>/dev/null \
        || true
fi

cat "$_SC_OUT"
rm -f "$_SC_OUT" 2>/dev/null || true
