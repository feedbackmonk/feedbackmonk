#!/bin/bash
# workspace-shared-repos oracle (Unix)
# Answers: which sibling git repos does this project consume via workspace declarations?
#
# Operates from the project root (CWD). Discovers shared repos from four sources, in
# priority order:
#   1. .claude/config.json  ->  sharedRepos: [{path, role?}] OR [string, ...]   (always wins)
#   2. pnpm-workspace.yaml  ->  packages: list (literal paths or glob patterns)
#   3. Cargo.toml           ->  [workspace] members = [...]
#   4. package.json         ->  workspaces: array OR {packages: [...]}
#
# Each candidate path is:
#   - resolved to absolute (relative paths resolve against the project root)
#   - filtered to those with their own <path>/.git/ directory
#   - skipped if it IS the local working tree (degenerate self-reference)
#   - skipped if it is INSIDE the local working tree (nested workspace package, not a sibling)
#   - deduplicated across sources by absolute path; explicit > pnpm > cargo > npm
#
# Output: JSON object per the FROZEN schema documented in README.md and oracle.json:
#   {count, repos:[{path, declarationSource, hasGit, hasClaudeDir}], discoveryMethod, _meta}
#
# Compute budget: 80ms (typical case 1-3 shared repos; bounded by file reads + git checks).
# Strategy: trigger-invalidate. Read-only on the filesystem (no mutation).
#
# Modes: default only (no --gc / --gc-cheap; this oracle does not sweep state).
#
# Spec: SHARED-CSI-01 in docs/specs/SPECIFICATION.md
# Decision: DEC-35 (discovery format scope) in docs/specs/DECISIONS.md
# Lineage: CSI-05 (dispatchable-sessions) and RETENTION-01..06 (archive-retention) per DISC-CSI-09.

set -e

ORACLE_VERSION="1.0"
SCHEMA_VERSION=1

# Reject unknown modes (this oracle only has the default briefing path).
if [ -n "${1:-}" ]; then
    case "$1" in
        --*)
            echo "workspace-shared-repos: unknown mode: $1" >&2
            echo "  usage: run.sh    (no flags; this oracle has no --gc / --gc-cheap modes)" >&2
            exit 1
            ;;
    esac
fi

# ---- Pick a JSON parser. Prefer jq; fall back to python; else degrade gracefully. ----
# Probe python actually runs (the Windows-Store python3 stub returns 0 from
# `command -v` but errors out with "install from Store" message on real use).
PARSER=""
if command -v jq >/dev/null 2>&1; then
    PARSER="jq"
else
    for _cand in python3 python; do
        if command -v "$_cand" >/dev/null 2>&1; then
            if "$_cand" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
                PARSER="$_cand"
                break
            fi
        fi
    done
fi

# ---- Millisecond timer (best-effort) -------------------------------------
_ms_now() {
    local ms
    ms=$(date -u +%s%3N 2>/dev/null)
    if [ -n "$ms" ]; then
        case "$ms" in (*[!0-9]*) ;; *) echo "$ms"; return 0 ;; esac
    fi
    if command -v perl >/dev/null 2>&1; then
        perl -MTime::HiRes=time -e 'printf("%d\n", time*1000)' 2>/dev/null && return 0
    fi
    if [ -n "$PARSER" ] && [ "$PARSER" != "jq" ]; then
        "$PARSER" -c 'import time; print(int(time.time()*1000))' 2>/dev/null
    fi
}

START_MS=$(_ms_now)

# ---- Resolve project root (absolute path, no trailing slash) -------------
# We use $(pwd -P) to follow any symlinks the user might have on the project root.
PROJECT_ROOT=$(pwd -P 2>/dev/null || pwd)
# Strip trailing slash if present (defensive; pwd should never emit one)
PROJECT_ROOT="${PROJECT_ROOT%/}"

# ---- emit_empty: graceful absence (no discovery sources, no surviving repos) ----
emit_empty() {
    local end_ms compute_ms
    end_ms=$(_ms_now)
    compute_ms=0
    if [ -n "$START_MS" ] && [ -n "$end_ms" ]; then
        compute_ms=$((end_ms - START_MS))
        [ "$compute_ms" -lt 0 ] && compute_ms=0
    fi
    printf '{"count":0,"repos":[],"discoveryMethod":"none","_meta":{"oracleVersion":"%s","computeMs":%s,"schemaVersion":%s}}\n' \
        "$ORACLE_VERSION" "$compute_ms" "$SCHEMA_VERSION"
    exit 0
}

# ---- Path utilities -------------------------------------------------------
# Resolve $1 (a path; may be relative) against $2 (the base directory; default CWD).
# Echoes the absolute, normalized path. Empty input echoes nothing.
_resolve_abs() {
    local p="$1"
    local base="${2:-$PROJECT_ROOT}"
    [ -n "$p" ] || return 0
    case "$p" in
        /*|[A-Za-z]:[\\/]*) ;;     # already absolute (POSIX or Windows-style)
        *) p="$base/$p" ;;
    esac
    # Normalize: strip "./" segments, resolve "../" segments, drop trailing "/"
    # Pure-shell normalization without realpath (which is not portable on macOS BSD).
    local out
    out=$(cd "$p" 2>/dev/null && pwd -P 2>/dev/null) && [ -n "$out" ] && { echo "${out%/}"; return 0; }
    # Path doesn't exist (yet); do best-effort lexical normalization.
    "$PARSER" -c "
import os, sys
p = sys.argv[1]
print(os.path.normpath(p).replace('\\\\','/').rstrip('/'))
" "$p" 2>/dev/null
}

# Test whether $1 is the project root or strictly inside it.
# Returns 0 (true) if $1 == PROJECT_ROOT or $1 starts with PROJECT_ROOT + "/"
# Returns 1 (false) otherwise.
_is_self_or_nested() {
    local abs="$1"
    [ -n "$abs" ] || return 1
    # On Windows + Git Bash, paths come through as /c/Users/... but PROJECT_ROOT may be
    # /c/users/... (case differences).  Normalize both to lowercase for comparison
    # only on case-insensitive filesystems (Windows/macOS default).
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*|Darwin)
            local a="$(echo "$abs" | tr '[:upper:]' '[:lower:]')"
            local r="$(echo "$PROJECT_ROOT" | tr '[:upper:]' '[:lower:]')"
            [ "$a" = "$r" ] && return 0
            case "$a" in "$r/"*) return 0 ;; esac
            ;;
        *)
            [ "$abs" = "$PROJECT_ROOT" ] && return 0
            case "$abs" in "$PROJECT_ROOT/"*) return 0 ;; esac
            ;;
    esac
    return 1
}

# Append "$1" to the deduplication list under priority "$2", but only if no
# higher-priority entry already claims that path.  Maintains:
#   PATHS_LIST    -- newline-separated absolute paths (in insertion order)
#   PATHS_SOURCE  -- newline-separated source labels (parallel to PATHS_LIST)
# Priority order: explicit > pnpm > cargo > npm.
_priority_rank() {
    case "$1" in
        explicit) echo 4 ;;
        pnpm)     echo 3 ;;
        cargo)    echo 2 ;;
        npm)      echo 1 ;;
        *)        echo 0 ;;
    esac
}

PATHS_LIST=""
PATHS_SOURCE=""
SOURCES_USED=""

_register() {
    local abs="$1"
    local source="$2"
    [ -n "$abs" ] || return 0

    # Filter: must have .git/
    [ -d "$abs/.git" ] || return 0

    # Filter: skip self / nested
    if _is_self_or_nested "$abs"; then
        return 0
    fi

    # Dedup: see if this path is already registered. If yes, compare priority.
    if [ -n "$PATHS_LIST" ]; then
        local existing_source
        # Walk parallel arrays; can't use bash arrays cleanly across whitespace paths,
        # but our paths are absolute and don't contain newlines.
        local idx=0
        while IFS= read -r registered_path; do
            idx=$((idx + 1))
            if [ "$registered_path" = "$abs" ]; then
                existing_source=$(echo "$PATHS_SOURCE" | sed -n "${idx}p")
                local new_rank existing_rank
                new_rank=$(_priority_rank "$source")
                existing_rank=$(_priority_rank "$existing_source")
                if [ "$new_rank" -gt "$existing_rank" ]; then
                    # Replace: rewrite PATHS_SOURCE with new source at this index
                    PATHS_SOURCE=$(echo "$PATHS_SOURCE" | awk -v i="$idx" -v s="$source" 'NR==i{print s; next} {print}')
                    # Track that this source contributed (even if existing wasn't kept)
                    case ",$SOURCES_USED," in
                        *,"$source",*) ;;
                        *) SOURCES_USED="${SOURCES_USED:+$SOURCES_USED,}$source" ;;
                    esac
                fi
                return 0
            fi
        done <<EOF
$PATHS_LIST
EOF
    fi

    # New entry
    if [ -z "$PATHS_LIST" ]; then
        PATHS_LIST="$abs"
        PATHS_SOURCE="$source"
    else
        PATHS_LIST="$PATHS_LIST
$abs"
        PATHS_SOURCE="$PATHS_SOURCE
$source"
    fi
    case ",$SOURCES_USED," in
        *,"$source",*) ;;
        *) SOURCES_USED="${SOURCES_USED:+$SOURCES_USED,}$source" ;;
    esac
}

# Expand a path that may contain glob wildcards.  For each match, _register against $2.
# Glob expansion is done relative to $3 (defaults to PROJECT_ROOT).
_expand_and_register() {
    local pattern="$1"
    local source="$2"
    local base="${3:-$PROJECT_ROOT}"

    [ -n "$pattern" ] || return 0

    # Strip a trailing slash (declarations sometimes write "../foo/")
    pattern="${pattern%/}"

    # If the pattern contains any glob metacharacters, expand. Otherwise treat as literal.
    case "$pattern" in
        *\**|*\?*|*\[*)
            # Glob path. Resolve to absolute first so the glob expands against the
            # filesystem at the right anchor.
            local abs_pattern
            case "$pattern" in
                /*|[A-Za-z]:[\\/]*) abs_pattern="$pattern" ;;
                *) abs_pattern="$base/$pattern" ;;
            esac
            shopt -s nullglob 2>/dev/null || true
            local match
            for match in $abs_pattern; do
                [ -d "$match" ] || continue
                local resolved
                resolved=$(_resolve_abs "$match")
                _register "$resolved" "$source"
            done
            shopt -u nullglob 2>/dev/null || true
            ;;
        *)
            local abs
            abs=$(_resolve_abs "$pattern" "$base")
            _register "$abs" "$source"
            ;;
    esac
}

# ---- Source 1: explicit list (.claude/config.json sharedRepos) ----------
_discover_explicit() {
    [ -f ".claude/config.json" ] || return 0
    [ -n "$PARSER" ] || return 0

    local lines
    if [ "$PARSER" = "jq" ]; then
        # sharedRepos may be array of strings OR array of {path, role?}.  Emit one
        # path per line.  Both shapes coalesce to a string.
        lines=$(jq -r '
            .sharedRepos // []
            | .[]
            | if type == "string" then .
              elif type == "object" then (.path // empty)
              else empty end
        ' .claude/config.json 2>/dev/null)
    else
        lines=$("$PARSER" - .claude/config.json <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
sr = d.get("sharedRepos") or []
if not isinstance(sr, list):
    sys.exit(0)
for entry in sr:
    if isinstance(entry, str):
        print(entry)
    elif isinstance(entry, dict):
        p = entry.get("path")
        if isinstance(p, str):
            print(p)
PY
        )
    fi

    while IFS= read -r p; do
        p="${p%$'\r'}"
        [ -n "$p" ] || continue
        # NOTE: paths in .claude/config.json are documented as resolving against the
        # project root (NOT the .claude/ directory); see README.md "Path resolution".
        _expand_and_register "$p" "explicit" "$PROJECT_ROOT"
    done <<EOF
$lines
EOF
}

# ---- Source 2: pnpm-workspace.yaml ----------------------------------------
_discover_pnpm() {
    [ -f "pnpm-workspace.yaml" ] || return 0

    # Simple line-based parser for the packages: list.  We don't pull a YAML library;
    # pnpm-workspace.yaml's packages: block has a stable, simple shape:
    #
    #   packages:
    #     - "../foo"
    #     - '../bar'
    #     - ../baz
    #
    # We read lines from after "packages:" until indentation drops or a new top-level
    # key appears.  Each list item is "- <quoted-or-bare-string>".
    local in_packages=0
    local line trimmed
    while IFS= read -r line || [ -n "$line" ]; do
        # Strip CR
        line="${line%$'\r'}"
        # Skip pure-comment / blank
        case "$line" in
            ''|'#'*) continue ;;
        esac

        if [ "$in_packages" = "0" ]; then
            # Look for the start
            case "$line" in
                packages:*) in_packages=1 ;;
            esac
            continue
        fi

        # in_packages == 1
        # If line is at column 0 and not a list item, we've left the block.
        case "$line" in
            ' '*|$'\t'*) ;;   # still inside (indented)
            -*) ;;             # list item at col 0 -- unusual but accept
            *)
                # Non-indented, non-list line at col 0: end of packages block
                in_packages=0
                continue
                ;;
        esac

        # Trim leading whitespace
        trimmed=$(echo "$line" | sed 's/^[[:space:]]*//')
        case "$trimmed" in
            -*) ;;
            *) continue ;;     # not a list item; skip
        esac
        # Strip the leading "-" and any whitespace, then strip surrounding quotes
        trimmed="${trimmed#-}"
        trimmed=$(echo "$trimmed" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
        # Strip a trailing inline comment
        trimmed=$(echo "$trimmed" | sed 's/[[:space:]]\+#.*$//')
        # Strip surrounding quotes (single or double)
        case "$trimmed" in
            \"*\") trimmed="${trimmed#\"}"; trimmed="${trimmed%\"}" ;;
            \'*\') trimmed="${trimmed#\'}"; trimmed="${trimmed%\'}" ;;
        esac

        [ -n "$trimmed" ] || continue
        _expand_and_register "$trimmed" "pnpm" "$PROJECT_ROOT"
    done < "pnpm-workspace.yaml"
}

# ---- Source 3: Cargo.toml [workspace] members ----------------------------
_discover_cargo() {
    [ -f "Cargo.toml" ] || return 0

    # Extract the [workspace] section's `members = [...]` array.  Members may be on a
    # single line or spread across multiple lines.  We slurp everything from the
    # opening `[` through the matching `]`, then split by comma.
    local in_workspace=0
    local in_members=0
    local buf=""
    local line trimmed

    while IFS= read -r line || [ -n "$line" ]; do
        line="${line%$'\r'}"
        # Strip line-level comments (Cargo.toml uses `#` for comments)
        # but not inside strings.  Simple cut: find first `#` not inside quotes.
        # For our purposes (members are quoted paths) a naive strip is fine.
        case "$line" in
            *'#'*) line=$(echo "$line" | sed 's/[[:space:]]*#.*$//') ;;
        esac
        trimmed=$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')

        if [ "$in_workspace" = "0" ]; then
            case "$trimmed" in
                '[workspace]'|'[workspace.dependencies]'|'[workspace.metadata]'*) ;;
            esac
            case "$trimmed" in
                '[workspace]') in_workspace=1 ;;
            esac
            continue
        fi

        # Inside [workspace] block. A new bracket section ends our scan.
        case "$trimmed" in
            '['*']'*)
                # New section header.  If it's a sub-section of [workspace.X], stay open.
                case "$trimmed" in
                    '[workspace.'*) ;;
                    *) in_workspace=0; in_members=0; continue ;;
                esac
                ;;
        esac

        if [ "$in_members" = "0" ]; then
            case "$trimmed" in
                'members'*=*'['*)
                    in_members=1
                    # Capture anything after the opening `[` on the same line.
                    buf="${trimmed#*[}"
                    # If the closing `]` is on the same line, we're done collecting.
                    case "$buf" in
                        *']'*)
                            # Truncate at the closing bracket
                            buf="${buf%%]*}"
                            in_members=0
                            ;;
                    esac
                    continue
                    ;;
            esac
        else
            # in_members == 1: keep accumulating until we see a `]`
            case "$line" in
                *']'*)
                    buf="$buf $(echo "$line" | sed 's/].*$//')"
                    in_members=0
                    ;;
                *)
                    buf="$buf $line"
                    ;;
            esac
        fi
    done < "Cargo.toml"

    [ -n "$buf" ] || return 0

    # Split buf on commas; for each piece, strip whitespace and quotes; register.
    local IFS_BACKUP="$IFS"
    IFS=','
    set -f                  # disable globbing while we read pieces (we'll re-enable per-piece)
    for piece in $buf; do
        set +f
        piece=$(echo "$piece" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
        [ -n "$piece" ] || continue
        case "$piece" in
            \"*\") piece="${piece#\"}"; piece="${piece%\"}" ;;
            \'*\') piece="${piece#\'}"; piece="${piece%\'}" ;;
        esac
        [ -n "$piece" ] || continue
        _expand_and_register "$piece" "cargo" "$PROJECT_ROOT"
        set -f
    done
    set +f
    IFS="$IFS_BACKUP"
}

# ---- Source 4: package.json workspaces ------------------------------------
_discover_npm() {
    [ -f "package.json" ] || return 0
    [ -n "$PARSER" ] || return 0

    local lines
    if [ "$PARSER" = "jq" ]; then
        lines=$(jq -r '
            .workspaces // []
            | if type == "array" then .[]
              elif type == "object" then (.packages // [])[]
              else empty end
            | select(type == "string")
        ' package.json 2>/dev/null)
    else
        lines=$("$PARSER" - package.json <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        d = json.load(f)
except Exception:
    sys.exit(0)
ws = d.get("workspaces")
items = []
if isinstance(ws, list):
    items = ws
elif isinstance(ws, dict):
    items = ws.get("packages") or []
for x in items:
    if isinstance(x, str):
        print(x)
PY
        )
    fi

    while IFS= read -r p; do
        p="${p%$'\r'}"
        [ -n "$p" ] || continue
        _expand_and_register "$p" "npm" "$PROJECT_ROOT"
    done <<EOF
$lines
EOF
}

# ---- Run all four sources in priority order ------------------------------
_discover_explicit
_discover_pnpm
_discover_cargo
_discover_npm

# ---- Empty result?  Emit graceful-absence form ---------------------------
if [ -z "$PATHS_LIST" ]; then
    emit_empty
fi

# ---- Build repo[] entries ------------------------------------------------
REPOS_JSON=""
COUNT=0
idx=0
while IFS= read -r abs; do
    idx=$((idx + 1))
    [ -n "$abs" ] || continue
    src=$(echo "$PATHS_SOURCE" | sed -n "${idx}p")

    # hasClaudeDir
    has_claude="false"
    [ -d "$abs/.claude" ] && has_claude="true"

    # Path needs to be JSON-escaped: backslashes -> \\, double quotes -> \", strip CR/LF.
    esc_path=$(printf '%s' "$abs" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\r//g; s/\n/ /g')

    entry=$(printf '{"path":"%s","declarationSource":"%s","hasGit":true,"hasClaudeDir":%s}' \
        "$esc_path" "$src" "$has_claude")

    if [ -z "$REPOS_JSON" ]; then
        REPOS_JSON="$entry"
    else
        REPOS_JSON="$REPOS_JSON,$entry"
    fi
    COUNT=$((COUNT + 1))
done <<EOF
$PATHS_LIST
EOF

# ---- Compute method label (only sources that contributed a surviving entry) ----
# Reorder SOURCES_USED into priority order.
ORDERED_METHOD=""
for src in explicit pnpm cargo npm; do
    case ",$SOURCES_USED," in
        *,"$src",*)
            # Did this source actually contribute a SURVIVING entry?
            case $'\n'"$PATHS_SOURCE"$'\n' in
                *$'\n'"$src"$'\n'*)
                    if [ -z "$ORDERED_METHOD" ]; then
                        ORDERED_METHOD="$src"
                    else
                        ORDERED_METHOD="$ORDERED_METHOD,$src"
                    fi
                    ;;
            esac
            ;;
    esac
done
[ -n "$ORDERED_METHOD" ] || ORDERED_METHOD="none"

# If there are zero surviving repos, emit the empty form.
if [ "$COUNT" -eq 0 ]; then
    emit_empty
fi

# ---- Emit final JSON -----------------------------------------------------
END_MS=$(_ms_now)
COMPUTE_MS=0
if [ -n "$START_MS" ] && [ -n "$END_MS" ]; then
    COMPUTE_MS=$((END_MS - START_MS))
    [ "$COMPUTE_MS" -lt 0 ] && COMPUTE_MS=0
fi

printf '{"count":%s,"repos":[%s],"discoveryMethod":"%s","_meta":{"oracleVersion":"%s","computeMs":%s,"schemaVersion":%s}}\n' \
    "$COUNT" "$REPOS_JSON" "$ORDERED_METHOD" "$ORACLE_VERSION" "$COMPUTE_MS" "$SCHEMA_VERSION"
