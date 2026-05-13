#!/bin/bash
# module-tree-map oracle (Unix)
# Walks the project tree and emits a hierarchical JSON map of modules
# with their Synopsis sections (HCT § 3) and File Index entries.
#
# Output schema: FOUNDATIONS/HIERARCHICAL_CONTEXT_TRIAGE.md § 4.2
# Spec: HCT-03 (docs/specs/SPECIFICATION.md)

set -e

EXCLUDES='node_modules|target|\.git|\.vscode|\.idea|dist|build|out|coverage|__pycache__|\.venv|venv|\.claude/oracles/cache|\.claude/checkpoints'

# JSON-escape a string: backslash, quote, newline, tab, CR, control chars.
json_escape() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\n'/\\n}"
    s="${s//$'\r'/\\r}"
    s="${s//$'\t'/\\t}"
    printf '%s' "$s"
}

# Extract content between `## $heading` and the next `## ` heading.
# Strips leading/trailing blank lines and HTML comment blocks.
extract_section() {
    local file="$1"
    local heading="$2"
    awk -v h="$heading" '
        BEGIN { in_section = 0; in_comment = 0 }
        {
            if (!in_section) {
                if ($0 ~ "^## " h "[[:space:]]*$") { in_section = 1; next }
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
            print
        }
    ' "$file" 2>/dev/null | awk '
        # Trim leading blank lines
        BEGIN { started = 0 }
        { if (!started && NF == 0) next; started = 1; lines[NR] = $0 }
        END {
            # Trim trailing blank lines
            n = NR
            while (n > 0 && lines[n] ~ /^[[:space:]]*$/) { delete lines[n]; n-- }
            for (i = 1; i <= n; i++) if (i in lines) print lines[i]
        }
    '
}

# Parse File Index entries (llms.txt-compatible) → emit one JSON object per entry on stdout.
parse_file_index() {
    local file="$1"
    local section
    section=$(extract_section "$file" "File Index")
    [ -z "$section" ] && return
    while IFS= read -r line; do
        # llms.txt: `- [name](./path): purpose`
        if [[ "$line" =~ ^[[:space:]]*-[[:space:]]*\[([^\]]+)\]\([^\)]+\)[[:space:]]*[:\-][[:space:]]*(.+)$ ]]; then
            local name="${BASH_REMATCH[1]}"
            local purpose="${BASH_REMATCH[2]}"
            printf '{"name":"%s","purpose":"%s"}\n' "$(json_escape "$name")" "$(json_escape "$purpose")"
            continue
        fi
        # Legacy: `- **name** - purpose`  (with optional backticks/quotes around name)
        if [[ "$line" =~ ^[[:space:]]*-[[:space:]]*\*\*[\`\"]?([^\`\"\*]+)[\`\"]?\*\*[[:space:]]*[-:][[:space:]]*(.+)$ ]]; then
            local name="${BASH_REMATCH[1]}"
            local purpose="${BASH_REMATCH[2]}"
            # Strip surrounding whitespace from name
            name="$(printf '%s' "$name" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
            printf '{"name":"%s","purpose":"%s"}\n' "$(json_escape "$name")" "$(json_escape "$purpose")"
        fi
    done <<< "$section"
}

# ---- Walk: collect all module READMEs ----
TMPDIR_ORACLE=$(mktemp -d 2>/dev/null || echo "/tmp/oracle-mtm-$$")
mkdir -p "$TMPDIR_ORACLE"
trap 'rm -rf "$TMPDIR_ORACLE"' EXIT

PATHS_FILE="$TMPDIR_ORACLE/paths.txt"
> "$PATHS_FILE"

total=0
synopsized=0
missing_synopsis=()

# Root README
root_synopsis_json="null"
root_file_index_json=""
if [ -f "README.md" ]; then
    total=$((total + 1))
    syn=$(extract_section "README.md" "Synopsis")
    if [ -n "$syn" ]; then
        root_synopsis_json="\"$(json_escape "$syn")\""
        synopsized=$((synopsized + 1))
    else
        missing_synopsis+=(".")
    fi
    fi_lines=$(parse_file_index "README.md" || true)
    if [ -n "$fi_lines" ]; then
        # Join file-index entries into a JSON array
        first=1
        root_file_index_json="["
        while IFS= read -r entry; do
            [ -z "$entry" ] && continue
            if [ $first -eq 1 ]; then first=0; else root_file_index_json+=","; fi
            root_file_index_json+="$entry"
        done <<< "$fi_lines"
        root_file_index_json+="]"
    fi
fi

# Subdirectory READMEs (no maxdepth — full tree)
while IFS= read -r dir; do
    dir_clean="${dir#./}"
    [ -z "$dir_clean" ] || [ "$dir_clean" = "." ] && continue
    if echo "$dir_clean" | grep -qE "^($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    if echo "$dir_clean" | grep -qE "/($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    [ -f "$dir_clean/README.md" ] || continue

    total=$((total + 1))
    syn=$(extract_section "$dir_clean/README.md" "Synopsis")
    syn_json="null"
    if [ -n "$syn" ]; then
        syn_json="\"$(json_escape "$syn")\""
        synopsized=$((synopsized + 1))
    else
        missing_synopsis+=("$dir_clean")
    fi

    fi_lines=$(parse_file_index "$dir_clean/README.md" || true)
    fi_json=""
    if [ -n "$fi_lines" ]; then
        first=1
        fi_json="["
        while IFS= read -r entry; do
            [ -z "$entry" ] && continue
            if [ $first -eq 1 ]; then first=0; else fi_json+=","; fi
            fi_json+="$entry"
        done <<< "$fi_lines"
        fi_json+="]"
    fi

    # Persist: path|synopsis_json|file_index_json (use TAB as field separator — paths and JSON may contain |)
    printf '%s\t%s\t%s\n' "$dir_clean" "$syn_json" "$fi_json" >> "$PATHS_FILE"
done < <(find . -type d 2>/dev/null | sort)

# Sort paths file lexically
sort -t $'\t' -k1,1 "$PATHS_FILE" -o "$PATHS_FILE"

# ---- Build hierarchical tree ----
# For each module, find its parent (longest existing module-path prefix).
# Modules without a module ancestor attach to the root.

# Build a list of all known module paths (for prefix-parent lookup).
ALL_PATHS_FILE="$TMPDIR_ORACLE/all_paths.txt"
cut -f1 "$PATHS_FILE" > "$ALL_PATHS_FILE"

# Compute parent for each module → emit lines: path|parent (parent="." for root attach).
PARENTS_FILE="$TMPDIR_ORACLE/parents.txt"
> "$PARENTS_FILE"
while IFS= read -r path; do
    parent="."
    # Walk up segment by segment looking for the longest module-path prefix.
    candidate="$path"
    while [ "$candidate" != "$(dirname "$candidate")" ] && [ "$candidate" != "." ] && [ "$candidate" != "/" ]; do
        candidate="$(dirname "$candidate")"
        [ "$candidate" = "." ] && break
        if grep -Fxq "$candidate" "$ALL_PATHS_FILE"; then
            parent="$candidate"
            break
        fi
    done
    printf '%s\t%s\n' "$path" "$parent" >> "$PARENTS_FILE"
done < "$ALL_PATHS_FILE"

# Recursive emitter: emits the JSON for a node given its path.
emit_node() {
    local node_path="$1"
    local syn_json
    local fi_json
    if [ "$node_path" = "." ]; then
        syn_json="$root_synopsis_json"
        fi_json="$root_file_index_json"
    else
        # Look up syn_json + fi_json for this path
        local row
        row=$(awk -F'\t' -v p="$node_path" '$1 == p { print; exit }' "$PATHS_FILE")
        syn_json=$(printf '%s' "$row" | awk -F'\t' '{ print $2 }')
        fi_json=$(printf '%s' "$row" | awk -F'\t' '{ print $3 }')
    fi

    # Find children of this path
    local children
    children=$(awk -F'\t' -v p="$node_path" '$2 == p { print $1 }' "$PARENTS_FILE" | sort)

    # Emit JSON object
    printf '{"path":"%s","synopsis":%s' "$(json_escape "$node_path")" "$syn_json"
    if [ -n "$fi_json" ]; then
        printf ',"file_index":%s' "$fi_json"
    fi
    printf ',"children":['
    local first=1
    while IFS= read -r child; do
        [ -z "$child" ] && continue
        if [ $first -eq 1 ]; then first=0; else printf ','; fi
        emit_node "$child"
    done <<< "$children"
    printf ']}'
}

# Build missing_synopsis JSON array (sorted)
ms_sorted=$(printf '%s\n' "${missing_synopsis[@]}" | sort 2>/dev/null || true)
ms_json="["
first=1
while IFS= read -r m; do
    [ -z "$m" ] && continue
    if [ $first -eq 1 ]; then first=0; else ms_json+=","; fi
    ms_json+="\"$(json_escape "$m")\""
done <<< "$ms_sorted"
ms_json+="]"

# Briefing line per HCT-05 acceptance: empty when total_modules == 0 (graceful absence).
briefing=""
if [ "$total" -gt 0 ]; then
    if [ "$total" -eq 1 ]; then mod_word="module"; else mod_word="modules"; fi
    briefing="${total} ${mod_word}, ${synopsized}/${total} with Synopsis. Invoke: /0-uldf-oracle module-tree-map"
fi
briefing_esc="$(json_escape "$briefing")"

# Emit final JSON
printf '{"root":'
emit_node "."
printf ',"stats":{"total_modules":%d,"synopsized":%d,"missing_synopsis":%s},"briefing":"%s"}\n' "$total" "$synopsized" "$ms_json" "$briefing_esc"
