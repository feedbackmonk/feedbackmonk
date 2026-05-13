#!/bin/bash
# planning-doc-staleness oracle (Unix)
# Verification Oracle (kind: "verification" per Oraculurgy Part 11): partitions
# planning docs in docs/planning/{intakes,plans}/ into {stale, fresh, unknown}
# using two heuristics:
#   1. commit-hash-found  - slug or basename appears in last 60d of git log
#   2. all-spec-entries-done - every spec ref in doc body is [DONE]/[DELIVERED]
# Read-only and idempotent. Action leg lives in /0-uldf-finalize Phase 8.7.
# Output: single JSON object matching oracle.json schema.
#
# Performance: <2s contract (Oraculurgy Part 11 §11.3.3). Bottleneck is
# fork count on Windows MSYS (~50-200ms per fork). Implementation does a
# single awk pass over SPECIFICATION.md and one awk pass per planning doc;
# total fork count is bounded by O(N planning docs) + 2 fixed.

set -e

INTAKES_DIR="docs/planning/intakes"
PLANS_DIR="docs/planning/plans"
SPEC_FILE="docs/specs/SPECIFICATION.md"
COMMIT_WINDOW_DAYS=60
FRESH_MTIME_DAYS=14
ARCHIVE_PREFIX="docs/planning/archive"

esc_json() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//	/\\t}"
    printf '%s' "$s"
}

# Collect candidate planning docs (depth 1; archive subdir excluded).
files=()
for dir in "$INTAKES_DIR" "$PLANS_DIR"; do
    [ -d "$dir" ] || continue
    while IFS= read -r -d '' f; do
        case "$f" in
            "$ARCHIVE_PREFIX"/*) continue ;;
        esac
        files+=("$f")
    done < <(find "$dir" -maxdepth 1 -type f -name '*.md' -print0 2>/dev/null)
done

# If no files, emit empty result and exit early (avoids any further forks).
if [ "${#files[@]}" -eq 0 ]; then
    cat <<'EOF'
{"stale":[],"fresh":[],"unknown":[],"briefing":""}
EOF
    exit 0
fi

# Build commit-message corpus (last 60 days). One git invocation, lowercased.
commit_log_lower=""
if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    commit_log_lower=$(git log --since="$COMMIT_WINDOW_DAYS days ago" --pretty=format:%s 2>/dev/null | tr '[:upper:]' '[:lower:]' || true)
fi

# Build spec ID -> status table in ONE awk pass over SPECIFICATION.md.
# Status values: DONE | DELIVERED | OTHER. Used by Heuristic 2.
spec_lookup=""
if [ -f "$SPEC_FILE" ]; then
    spec_lookup=$(awk '
        /^#+[[:space:]]+[A-Z][A-Z0-9]*-[0-9]+:/ {
            for (i=2; i<=NF; i++) {
                if ($i ~ /^[A-Z][A-Z0-9]*-[0-9]+:$/) {
                    id = $i; sub(":","",id)
                    status = "OTHER"
                    if (index($0, "[DONE]")) status = "DONE"
                    else if (index($0, "[DELIVERED]")) status = "DELIVERED"
                    print id "|" status
                    next
                }
            }
        }
    ' "$SPEC_FILE" 2>/dev/null || true)
fi

now=$(date +%s 2>/dev/null || echo 0)
fresh_threshold=$(( FRESH_MTIME_DAYS * 86400 ))

# Pre-compute all mtimes in ONE stat call (per-file stat is fork-heavy on MSYS).
mtime_table=""
if [ "${#files[@]}" -gt 0 ]; then
    mtime_table=$(stat -c '%n|%Y' "${files[@]}" 2>/dev/null \
        || stat -f '%N|%m' "${files[@]}" 2>/dev/null \
        || true)
fi

# Heuristic 2 batch: ONE awk pass across all planning docs.
# Prints "<filepath>|<refs_found>|<refs_done>" per file.
# The spec_lookup table is built in BEGIN; per-file state resets on FNR==1.
sig2_table=""
if [ -n "$spec_lookup" ] && [ "${#files[@]}" -gt 0 ]; then
    sig2_table=$(awk -v lookup="$spec_lookup" '
        BEGIN {
            n = split(lookup, lines, "\n")
            for (i=1; i<=n; i++) {
                if (split(lines[i], pair, "|") == 2 && pair[1] != "") {
                    statuses[pair[1]] = pair[2]
                }
            }
        }
        FNR == 1 {
            if (current != "") print current "|" found "|" done
            current = FILENAME
            found = 0; done = 0
            delete seen
        }
        {
            rest = $0
            while (match(rest, /[A-Z][A-Z0-9]*-[0-9]+/)) {
                id = substr(rest, RSTART, RLENGTH)
                if ((id in statuses) && !(id in seen)) {
                    seen[id] = 1
                    found++
                    if (statuses[id] == "DONE" || statuses[id] == "DELIVERED") done++
                }
                rest = substr(rest, RSTART + RLENGTH)
            }
        }
        END {
            if (current != "") print current "|" found "|" done
        }
    ' "${files[@]}" 2>/dev/null || true)
fi

stale_json=""
fresh_json=""
unknown_json=""
stale_count=0

for f in "${files[@]}"; do
    base="${f##*/}"
    name="${base%.md}"
    slug="$name"
    case "$name" in
        [0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]T[0-9][0-9][0-9][0-9][0-9][0-9]-*)
            slug="${name#????????T??????-}"
            ;;
    esac

    # Heuristic 1: commit-hash-found. Use bash 4 ${,,} for lowercasing
    # to avoid per-file `tr` forks.
    sig1=false
    if [ -n "$commit_log_lower" ]; then
        slug_lower="${slug,,}"
        base_lower="${base,,}"
        if [ "${#slug_lower}" -ge 4 ]; then
            case "$commit_log_lower" in
                *"$slug_lower"*) sig1=true ;;
            esac
        fi
        if ! $sig1 && [ "${#base_lower}" -ge 4 ]; then
            case "$commit_log_lower" in
                *"$base_lower"*) sig1=true ;;
            esac
        fi
    fi

    # Heuristic 2: lookup in pre-computed sig2_table via pure-bash scan
    # (no per-file fork). The table is small (~30 lines max).
    sig2=false
    refs_found=0
    refs_done=0
    if [ -n "$sig2_table" ]; then
        IFS=$'\n'
        for entry in $sig2_table; do
            case "$entry" in
                "$f|"*)
                    rest="${entry#*|}"
                    refs_found="${rest%%|*}"
                    refs_done="${rest##*|}"
                    if [ "${refs_found:-0}" -gt 0 ] && [ "$refs_found" = "$refs_done" ]; then
                        sig2=true
                    fi
                    break
                    ;;
            esac
        done
        unset IFS
    fi

    signal=""
    if $sig1 && $sig2; then
        signal="both"
    elif $sig1; then
        signal="commit-hash-found"
    elif $sig2; then
        signal="all-spec-entries-done"
    fi

    # mtime -> ISO-8601 UTC + age in days. Look up in pre-computed mtime_table
    # to avoid per-file `stat` forks; format via bash printf %(...)T builtin.
    mtime_epoch="$now"
    if [ -n "$mtime_table" ]; then
        IFS=$'\n'
        for mline in $mtime_table; do
            case "$mline" in
                "$f|"*)
                    mtime_epoch="${mline#*|}"
                    break
                    ;;
            esac
        done
        unset IFS
    fi
    age_seconds=$(( now - mtime_epoch ))
    [ "$age_seconds" -lt 0 ] && age_seconds=0
    age_days=$(( age_seconds / 86400 ))
    # bash 4.2+ printf %(...)T avoids the date(1) fork.
    iso_mtime=""
    if iso_mtime=$(TZ=UTC printf '%(%Y-%m-%dT%H:%M:%SZ)T' "$mtime_epoch" 2>/dev/null); then
        :
    else
        iso_mtime=$(date -u -d "@$mtime_epoch" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null \
            || date -u -r "$mtime_epoch" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null \
            || echo "")
    fi

    fpath_esc=$(esc_json "$f")

    if [ -n "$signal" ]; then
        entry="{\"path\":\"$fpath_esc\",\"staleness_signal\":\"$signal\",\"last_modified\":\"$iso_mtime\"}"
        if [ -z "$stale_json" ]; then stale_json="$entry"; else stale_json="$stale_json,$entry"; fi
        stale_count=$((stale_count + 1))
    elif [ "$age_seconds" -lt "$fresh_threshold" ]; then
        entry="{\"path\":\"$fpath_esc\"}"
        if [ -z "$fresh_json" ]; then fresh_json="$entry"; else fresh_json="$fresh_json,$entry"; fi
    else
        reason="no commit reference; no spec mapping detectable; mtime ${age_days} days"
        reason_esc=$(esc_json "$reason")
        entry="{\"path\":\"$fpath_esc\",\"reason\":\"$reason_esc\"}"
        if [ -z "$unknown_json" ]; then unknown_json="$entry"; else unknown_json="$unknown_json,$entry"; fi
    fi
done

# Briefing: fires when stale[] non-empty.
briefing=""
if [ "$stale_count" -gt 0 ]; then
    briefing="[planning-doc-staleness] ${stale_count} stale planning docs, run /0-uldf-finalize to archive"
fi
briefing_esc=$(esc_json "$briefing")

cat <<EOF
{"stale":[$stale_json],"fresh":[$fresh_json],"unknown":[$unknown_json],"briefing":"$briefing_esc"}
EOF
