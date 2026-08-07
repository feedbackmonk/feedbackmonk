#!/bin/bash
# module-tree-map oracle (Unix)
# Walks the project tree and emits a hierarchical JSON map of modules
# with their Synopsis sections (HCT § 3) and File Index entries.
#
# Output schema: FOUNDATIONS/HIERARCHICAL_CONTEXT_TRIAGE.md § 4.2
# Spec: HCT-03 (docs/specs/SPECIFICATION.md)

set -e

EXCLUDES='node_modules|target|\.git|\.vscode|\.idea|dist|build|out|coverage|__pycache__|\.venv|venv|\.claude/oracles/cache|\.claude/checkpoints'

# Walk-time prune (WinDirFul 2026-07-10, DEFER-windirful § 2; twin of the
# synopsis-coverage block): name-based excludes prune at descent time instead
# of grep-after-full-walk; the two path-based excludes stay grep-filtered.
# Output set is identical to the grep-only behavior.
_MTM_PRUNE=( \( -name node_modules -o -name target -o -name .git -o -name .vscode -o -name .idea -o -name dist -o -name build -o -name out -o -name coverage -o -name __pycache__ -o -name .venv -o -name venv \) -prune -o )

# =============================================================================
# Trigger-invalidate cache (implements the freshness contract the manifest
# declares — previously declared-but-unimplemented; the ~90s full recompute on
# every invocation is why DEC-86 deferred this oracle from the session-start
# briefing budget).
#
# Digest = hash of the sorted "path<TAB>mtime<TAB>size" lines of every
# trigger-set file (**/README.md, EXCLUDES-filtered). Catches edits (mtime or
# size), adds, deletes, and renames — set-level, not just newest-mtime, so a
# deleted README invalidates too (Oraculurgy anti-pattern #1: a
# stale-undetected cache is worse than no cache). Warm path cost = one find
# pass + hash. The digest is re-computed AFTER a cold compute and the cache is
# stored only when pre/post digests agree — a README mutated mid-compute can
# never be frozen into a fresh-looking cache.
#
# Briefing context (ULDF_BRIEFING=1, set by the briefing fan-out lib): a COLD
# cache is NOT recomputed inline — the declared expected_runtime_ms is the
# warm cost, so an inline cold compute would be killed at 3x-declared and the
# cache could never warm through the briefing path. Instead the script spawns
# ONE detached background refresh (stampede-guarded) and emits a minimal
# valid-schema object with an empty briefing (graceful absence this session;
# the line returns next session from the warmed cache). Stale cache is never
# served — absent beats wrong. Direct invocations recompute synchronously.
#
# Flags: --refresh / --no-cache force a synchronous recompute.
# =============================================================================
_MTM_ORACLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
_MTM_CACHE_DIR="$_MTM_ORACLE_DIR/cache"
_MTM_CACHE_FILE="$_MTM_CACHE_DIR/latest.json"
_MTM_DIGEST_FILE="$_MTM_CACHE_DIR/trigger-digest.txt"
_MTM_REFRESH_MARK="$_MTM_CACHE_DIR/refresh-in-progress"

_MTM_FORCE=""
for _a in "$@"; do
    case "$_a" in --refresh|--no-cache) _MTM_FORCE=1 ;; esac
done

_mtm_trigger_digest() {
    {
        # GNU find (Linux/Git Bash): single-pass stat — cheap under win32
        # spawn tax. BSD/macOS find lacks -printf; falls through to the
        # per-file stat loop (small absolute cost on those platforms).
        find . "${_MTM_PRUNE[@]}" -type f -name 'README.md' -printf '%p\t%T@\t%s\n' 2>/dev/null \
        || find . "${_MTM_PRUNE[@]}" -type f -name 'README.md' -print 2>/dev/null \
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

_MTM_DIGEST_PRE="$(_mtm_trigger_digest)"
if [ -z "$_MTM_FORCE" ] && [ -n "$_MTM_DIGEST_PRE" ] \
    && [ -f "$_MTM_CACHE_FILE" ] && [ -f "$_MTM_DIGEST_FILE" ] \
    && [ "$(cat "$_MTM_DIGEST_FILE" 2>/dev/null)" = "$_MTM_DIGEST_PRE" ]; then
    cat "$_MTM_CACHE_FILE"
    exit 0
fi

# Cold cache in briefing context: detached refresh + graceful absence.
if [ -z "$_MTM_FORCE" ] && [ "${ULDF_BRIEFING:-}" = "1" ]; then
    _MTM_SPAWN_REFRESH=1
    if [ -f "$_MTM_REFRESH_MARK" ]; then
        # Stampede guard: skip if a refresh started <10 min ago.
        _MTM_MARK_AGE=$(( $(date +%s) - $(stat -c '%Y' "$_MTM_REFRESH_MARK" 2>/dev/null || stat -f '%m' "$_MTM_REFRESH_MARK" 2>/dev/null || echo 0) ))
        [ "$_MTM_MARK_AGE" -lt 600 ] && _MTM_SPAWN_REFRESH=""
    fi
    if [ -n "$_MTM_SPAWN_REFRESH" ]; then
        mkdir -p "$_MTM_CACHE_DIR" 2>/dev/null || true
        : > "$_MTM_REFRESH_MARK" 2>/dev/null || true
        ( ULDF_BRIEFING="" nohup bash -c \
            "cd \"$(pwd)\" && bash \"$_MTM_ORACLE_DIR/run.sh\" --refresh >/dev/null 2>&1; rm -f \"$_MTM_REFRESH_MARK\"" \
            >/dev/null 2>&1 & ) 2>/dev/null
    fi
    printf '{"root":{"path":".","synopsis":null,"children":[]},"stats":{"total_modules":0,"synopsized":0,"missing_synopsis":[]},"briefing":"","cache":"cold-refreshing"}\n'
    exit 0
fi

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
done < <(find . "${_MTM_PRUNE[@]}" -type d -print 2>/dev/null | sort)

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

# Emit final JSON (via tmp so the cache stores exactly what is emitted)
_MTM_OUT="$TMPDIR_ORACLE/out.json"
{
    printf '{"root":'
    emit_node "."
    printf ',"stats":{"total_modules":%d,"synopsized":%d,"missing_synopsis":%s},"briefing":"%s"}\n' "$total" "$synopsized" "$ms_json" "$briefing_esc"
} > "$_MTM_OUT"

# Store cache only when the trigger set is byte-identical to what we scanned
# before computing (no mid-compute mutation) — see cache block header.
# Under --refresh the pre-digest was taken at refresh start, which is the
# correct freshness anchor for the stored cache.
_MTM_DIGEST_POST="$(_mtm_trigger_digest)"
if [ -n "$_MTM_DIGEST_POST" ] && [ "$_MTM_DIGEST_POST" = "$_MTM_DIGEST_PRE" ]; then
    mkdir -p "$_MTM_CACHE_DIR" 2>/dev/null || true
    cp "$_MTM_OUT" "$_MTM_CACHE_FILE" 2>/dev/null \
        && printf '%s' "$_MTM_DIGEST_POST" > "$_MTM_DIGEST_FILE" 2>/dev/null \
        || true
fi

cat "$_MTM_OUT"
