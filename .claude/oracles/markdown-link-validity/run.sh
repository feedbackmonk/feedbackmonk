#!/bin/bash
# markdown-link-validity oracle (Unix)
# Verification Oracle: checks that all internal markdown links in tracked
# documentation files resolve to existing targets. Read-only and idempotent.
# Output: single JSON object matching oracle.json schema.

set -e

# Scan scope. Keep this aligned with oracle.json's config.scan_* fields.
SCAN_DIRS=(claude-template docs FOUNDATIONS)
SCAN_ROOT_FILES=(CLAUDE.md README.md)

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
    echo '{"status":"pass","details":{"checked":0,"broken_count":0,"scanned_files":0,"scan_duration_ms":0,"broken":[]}}'
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
        broken_count=$((broken_count + 1))
        entry="{\"source\":\"$(esc_json "$src")\",\"line\":$lineno,\"link\":\"$(esc_json "$dest")\",\"resolved_path\":\"$(esc_json "$resolved")\"}"
        if [ -z "$broken_json" ]; then
            broken_json="$entry"
        else
            broken_json="$broken_json,$entry"
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
{"status":"$status","details":{"checked":$checked,"broken_count":$broken_count,"scanned_files":$scanned_files,"scan_duration_ms":$duration_ms,"broken":[$broken_json]}}
EOF
