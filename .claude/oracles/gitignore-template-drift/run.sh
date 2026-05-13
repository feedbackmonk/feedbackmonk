#!/bin/bash
# gitignore-template-drift oracle (Unix)
# HYGIENE-03 — detect when project's .gitignore lacks framework-managed
# patterns from the current claude-template baseline.
#
# Output schema (FROZEN — see channels/messages.md [ARC1-W3] / oracle.json):
#   { drifted: bool, missing_patterns: [string], baseline_patterns: int,
#     project_patterns: int, briefing: string }

set -e
set -u

# ---- Source resolution ------------------------------------------------------
# Test override env vars take precedence (used by validate.sh fixtures).
BASELINE_FILE="${CLAUDE_GITIGNORE_BASELINE:-}"
PROJECT_FILE="${CLAUDE_GITIGNORE_PROJECT:-.gitignore}"

# Discover baseline if not overridden.
if [ -z "$BASELINE_FILE" ]; then
    if [ -n "${HOME:-}" ] && [ -f "$HOME/.claude/.gitignore" ]; then
        BASELINE_FILE="$HOME/.claude/.gitignore"
    else
        # Framework-dev fallback: walk up from current directory looking for
        # claude-template/.gitignore (we are running inside the ULDF repo).
        _probe_dir="$(pwd)"
        for _i in 1 2 3 4 5 6; do
            if [ -f "$_probe_dir/claude-template/.gitignore" ]; then
                BASELINE_FILE="$_probe_dir/claude-template/.gitignore"
                break
            fi
            _parent="$(dirname "$_probe_dir")"
            [ "$_parent" = "$_probe_dir" ] && break
            _probe_dir="$_parent"
        done
    fi
fi

# ---- Header marker ----------------------------------------------------------
# Matches the framework-managed section in claude-template/.gitignore.
# Em-dash is U+2014 (UTF-8: 0xE2 0x80 0x94). Source file is UTF-8; byte match.
FRAMEWORK_HEADER="# Claude Code (session artifacts — never commit)"

# ---- Graceful absent: no baseline found -------------------------------------
if [ -z "$BASELINE_FILE" ] || [ ! -f "$BASELINE_FILE" ]; then
    printf '{"drifted":false,"missing_patterns":[],"baseline_patterns":0,"project_patterns":0,"briefing":""}\n'
    exit 0
fi

# ---- Extract framework-managed patterns from baseline -----------------------
# Read all non-comment, non-blank lines AFTER the header line through EOF.
# Pattern lines are preserved verbatim (line-trimmed of CR + leading/trailing
# whitespace). Comments (lines beginning with `#` after trimming) and blank
# lines are skipped. Any subsequent `# ...` sub-header lines (e.g. "# CSI
# registry (mutates every session-start)") are also comments and skipped.
baseline_patterns="$(awk -v header="$FRAMEWORK_HEADER" '
    BEGIN { in_section = 0 }
    {
        sub(/\r$/, "")
        line = $0
        if (in_section == 0) {
            if (line == header) { in_section = 1 }
            next
        }
        # Trim leading/trailing whitespace
        sub(/^[ \t]+/, "", line)
        sub(/[ \t]+$/, "", line)
        if (line == "") next
        if (substr(line, 1, 1) == "#") next
        print line
    }
' "$BASELINE_FILE")"

baseline_count=0
if [ -n "$baseline_patterns" ]; then
    baseline_count=$(printf '%s\n' "$baseline_patterns" | grep -c '.')
fi

# ---- Extract project patterns -----------------------------------------------
project_patterns=""
project_count=0
if [ -f "$PROJECT_FILE" ]; then
    project_patterns="$(awk '
        {
            sub(/\r$/, "")
            line = $0
            sub(/^[ \t]+/, "", line)
            sub(/[ \t]+$/, "", line)
            if (line == "") next
            if (substr(line, 1, 1) == "#") next
            print line
        }
    ' "$PROJECT_FILE")"
    if [ -n "$project_patterns" ]; then
        project_count=$(printf '%s\n' "$project_patterns" | grep -c '.')
    fi
fi

# ---- Compute missing: baseline patterns NOT present in project --------------
# Exact-match (line-trimmed) using grep -F -x (fixed string, whole line).
missing_count=0
missing_lines=""
if [ -n "$baseline_patterns" ]; then
    while IFS= read -r p; do
        [ -z "$p" ] && continue
        if [ -n "$project_patterns" ]; then
            if printf '%s\n' "$project_patterns" | grep -F -x -q -- "$p"; then
                continue
            fi
        fi
        if [ -z "$missing_lines" ]; then
            missing_lines="$p"
        else
            missing_lines="$missing_lines
$p"
        fi
        missing_count=$((missing_count + 1))
    done <<EOF
$baseline_patterns
EOF
fi

# ---- JSON-escape a single string (handles backslash, double-quote, control) -
# Patterns themselves rarely contain these, but defense-in-depth.
json_escape_line() {
    # Use sed to escape: backslash first, then double-quote.
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

# ---- Build missing_patterns JSON array --------------------------------------
missing_json="["
if [ "$missing_count" -gt 0 ]; then
    _first=1
    while IFS= read -r p; do
        [ -z "$p" ] && continue
        esc="$(json_escape_line "$p")"
        if [ "$_first" -eq 1 ]; then
            missing_json="${missing_json}\"${esc}\""
            _first=0
        else
            missing_json="${missing_json},\"${esc}\""
        fi
    done <<EOF
$missing_lines
EOF
fi
missing_json="${missing_json}]"

# ---- Build briefing ---------------------------------------------------------
if [ "$missing_count" -gt 0 ]; then
    drifted="true"
    briefing="gitignore-template-drift: ${missing_count} framework patterns missing — run /0-uldf-migrate-hygiene to update"
else
    drifted="false"
    briefing=""
fi

briefing_esc="$(json_escape_line "$briefing")"

# ---- Emit final JSON --------------------------------------------------------
printf '{"drifted":%s,"missing_patterns":%s,"baseline_patterns":%d,"project_patterns":%d,"briefing":"%s"}\n' \
    "$drifted" "$missing_json" "$baseline_count" "$project_count" "$briefing_esc"
