#!/bin/bash
# markdown-link-validity oracle (Unix)
# Verification Oracle: checks that all internal markdown links in tracked
# documentation files resolve to existing targets. Read-only and idempotent.
# Output: single JSON object matching oracle.json schema.

set -e

# Scan scope. Keep this aligned with oracle.json's config.scan_* fields.
SCAN_DIRS=(claude-template docs FOUNDATIONS)
SCAN_ROOT_FILES=(CLAUDE.md README.md)

# OVALID-05 (DEC-220): this oracle asserts "the citation resolves" but measures
# "a path exists in the WORKING TREE" -- and that tree is shared with live
# sibling sessions. A target listed by `git ls-files --deleted` is TRACKED and
# present in HEAD; only an UNCOMMITTED deletion removed it from the tree, so the
# citation is correct and the doc is not broken. Failing on it let any peer's
# in-flight WIP redden a commit gate, and the remediation advice it produced was
# actively wrong (repoint a CORRECT citation at some other path).
#
# The gate is DEFERRED, not weakened: such targets go to an informational
# `uncommitted_deletions` bucket and do not set status. Commit the deletion and
# the path leaves this set, at which point it fails as a genuine broken link.
# Not a blanket amnesty -- every other failure class still fails, in the same
# run, alongside a suppressed one (asserted invertibly by validate.sh case 6).
# Graceful absence: no git, or not a work tree -> empty set, oracle behaves
# exactly as it did before this clause (strict by default).
deleted_set=""
if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    deleted_set=$(git ls-files --deleted 2>/dev/null || true)
fi

# Textual path canonicalization. `git ls-files` emits repo-root-relative paths
# with no `.`/`..` segments, while this oracle resolves links against the citing
# file's directory and so produces e.g. `docs/../FOUNDATIONS/X.md`. Without
# normalization the comparison below would silently never match for any
# parent-relative citation -- a proxy bug inside the fix for a proxy bug.
norm_path() {
    np_in="$1"
    while [ "${np_in#./}" != "$np_in" ]; do np_in="${np_in#./}"; done
    np_out=""
    np_saved_ifs="$IFS"
    IFS='/'
    set -- $np_in
    IFS="$np_saved_ifs"
    for np_seg in "$@"; do
        case "$np_seg" in
            ""|".") continue ;;
            "..")
                if [ -z "$np_out" ] || [ "$np_out" = ".." ] || [ "${np_out%/..}" != "$np_out" ]; then
                    np_out="${np_out:+$np_out/}.."
                elif [ "${np_out#*/}" = "$np_out" ]; then
                    np_out=""
                else
                    np_out="${np_out%/*}"
                fi
                ;;
            *) np_out="${np_out:+$np_out/}$np_seg" ;;
        esac
    done
    printf '%s' "$np_out"
}

# Exact whole-path membership test -- newline-sentinel wrapping so `docs/a.md`
# cannot match `docs/a.md.bak`. Pure builtins, no subshell, no fork.
is_uncommitted_deletion() {
    [ -n "$deleted_set" ] || return 1
    case "
$deleted_set
" in
        *"
$1
"*) return 0 ;;
    esac
    return 1
}

# date +%s%3N is a GNU extension; fall back to seconds when unavailable.
start_ms=$(date +%s%3N 2>/dev/null)
case "$start_ms" in
    *N|"") start_ms=$(( $(date +%s) * 1000 )) ;;
esac

# Collect markdown files to scan. Use NUL-delimited pipeline so weird filenames
# can't break the loop.
files=()
for d in "${SCAN_DIRS[@]}"; do
    if [ -d "$d" ]; then
        while IFS= read -r -d '' f; do
            files+=("$f")
        done < <(find "$d" -type f -name '*.md' -print0 2>/dev/null)
    fi
done
for rf in "${SCAN_ROOT_FILES[@]}"; do
    if [ -f "$rf" ]; then
        files+=("$rf")
    fi
done

scanned_files=${#files[@]}

if [ "$scanned_files" -eq 0 ]; then
    echo '{"status":"pass","details":{"checked":0,"broken_count":0,"scanned_files":0,"scan_duration_ms":0,"broken":[],"uncommitted_deletion_count":0,"uncommitted_deletions":[]}}'
    exit 0
fi

# Single awk pass over all files. Awk:
#   - Tracks fenced code blocks (lines bracketed by ``` or ~~~) and skips them
#   - Strips inline `...` code spans before pattern matching
#   - Extracts every [text](dest) on each remaining line
# Output is TAB-separated: <file><TAB><lineno><TAB><dest>
awk_extract='
    BEGIN { in_fence = 0 }
    FNR == 1 { in_fence = 0 }
    /^[[:space:]]*```/ { in_fence = !in_fence; next }
    /^[[:space:]]*~~~/ { in_fence = !in_fence; next }
    in_fence { next }
    {
        line = $0
        gsub(/`[^`]*`/, "", line)
        # Find every [text](dest) match on this line
        rest = line
        while (match(rest, /\[[^]]*\]\([^)]+\)/)) {
            m = substr(rest, RSTART, RLENGTH)
            # Extract dest from inside parens. The match is "[text](dest)".
            paren = index(m, "](")
            dest = substr(m, paren + 2, length(m) - paren - 2)
            print FILENAME "\t" FNR "\t" dest
            rest = substr(rest, RSTART + RLENGTH)
        }
    }
'

# JSON escape using pure bash parameter expansion: backslash, double-quote.
# Tabs / CR / control chars in markdown link destinations are vanishingly rare;
# we substitute them with their JSON escape forms via a tr+printf round-trip
# only when needed (cheap fallback).
esc_json() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//	/\\t}"
    printf '%s' "$s"
}

checked=0
broken_count=0
broken_json=""
uncommitted_deletions=0
uncommitted_json=""

# Process awk output. Per-link work uses only shell builtins (no fork-per-link).
while IFS=$'\t' read -r src lineno dest; do
    [ -z "$src" ] && continue
    [ -z "$dest" ] && continue

    # Strip an optional `"title"` suffix from dest.
    case "$dest" in
        *' "'*'"')
            # Remove trailing whitespace + "title"
            dest="${dest% \"*}"
            ;;
    esac

    # Trim leading/trailing whitespace using parameter expansion.
    dest="${dest#"${dest%%[![:space:]]*}"}"
    dest="${dest%"${dest##*[![:space:]]}"}"

    # Skip protocol/external links and same-page anchors.
    case "$dest" in
        ""|http://*|https://*|ftp://*|mailto:*|tel:*) continue ;;
        "#"*) continue ;;
    esac

    # Strip query string and fragment for filesystem resolution.
    target="${dest%%#*}"
    target="${target%%\?*}"
    [ -z "$target" ] && continue

    checked=$((checked + 1))

    # Compute source directory using parameter expansion (no dirname subprocess).
    case "$src" in
        */*) dir="${src%/*}" ;;
        *) dir="." ;;
    esac

    # Resolve target relative to source dir; absolute paths kept as-is.
    case "$target" in
        /*) resolved="$target" ;;
        *) resolved="$dir/$target" ;;
    esac

    if [ ! -e "$resolved" ]; then
        entry="{\"source\":\"$(esc_json "$src")\",\"line\":$lineno,\"link\":\"$(esc_json "$dest")\",\"resolved_path\":\"$(esc_json "$resolved")\"}"
        # OVALID-05: tracked-and-present-in-HEAD, removed only by an uncommitted
        # deletion -> informational, does not set status. See the header note.
        if is_uncommitted_deletion "$(norm_path "$resolved")"; then
            uncommitted_deletions=$((uncommitted_deletions + 1))
            if [ -z "$uncommitted_json" ]; then
                uncommitted_json="$entry"
            else
                uncommitted_json="$uncommitted_json,$entry"
            fi
        else
            broken_count=$((broken_count + 1))
            if [ -z "$broken_json" ]; then
                broken_json="$entry"
            else
                broken_json="$broken_json,$entry"
            fi
        fi
    fi
done < <(awk "$awk_extract" "${files[@]}" 2>/dev/null)

end_ms=$(date +%s%3N 2>/dev/null)
case "$end_ms" in
    *N|"") end_ms=$(( $(date +%s) * 1000 )) ;;
esac
duration_ms=$((end_ms - start_ms))
[ "$duration_ms" -lt 0 ] && duration_ms=0

status="pass"
if [ "$broken_count" -gt 0 ]; then
    status="fail"
fi

cat <<EOF
{"status":"$status","details":{"checked":$checked,"broken_count":$broken_count,"scanned_files":$scanned_files,"scan_duration_ms":$duration_ms,"broken":[$broken_json],"uncommitted_deletion_count":$uncommitted_deletions,"uncommitted_deletions":[$uncommitted_json]}}
EOF
