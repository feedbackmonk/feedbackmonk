#!/bin/bash
# spec-status oracle (Unix)
# Counts DONE / PENDING / IN_PROGRESS / REMOVED items in the project specification.
# Uses a flexible parser that looks for common status markers.

set -e

SPEC_FILE=""
if [ -f "docs/specs/SPECIFICATION.md" ]; then
    SPEC_FILE="docs/specs/SPECIFICATION.md"
elif [ -f "docs/specs/PROJECT_SPEC.md" ]; then
    SPEC_FILE="docs/specs/PROJECT_SPEC.md"
elif [ -f "docs/specs/spec.md" ]; then
    SPEC_FILE="docs/specs/spec.md"
fi

if [ -z "$SPEC_FILE" ]; then
    echo '{"spec_exists":false,"spec_file":null,"total_items":0,"done":0,"pending":0,"in_progress":0,"removed":0,"progress_percent":0}'
    exit 0
fi

# Count status markers. Matches forms:
#   "**Status**: DONE"
#   "- [x] ..."  (GitHub-style checkbox)
#   "Status: PENDING"
#   "REQ-NNN: DONE"
content=$(cat "$SPEC_FILE" 2>/dev/null || echo "")

done_count=0
pending_count=0
in_progress_count=0
removed_count=0

# Helper: grep -c with guaranteed single-number output (0 on no matches, no duplication)
count_matches() {
    local pattern="$1"
    local text="$2"
    local n
    n=$(printf '%s' "$text" | grep -ciE "$pattern" 2>/dev/null) || n=0
    # Take only first line; strip whitespace
    n=$(echo "$n" | head -1 | tr -d '[:space:]')
    [ -z "$n" ] && n=0
    # Validate it's a number
    case "$n" in
        ''|*[!0-9]*) n=0 ;;
    esac
    echo "$n"
}

# Status field format: **Status**: VALUE, - **Status**: VALUE, Status: VALUE
done_count=$(count_matches '(\*\*|^)status(\*\*)?[[:space:]]*:[[:space:]]*(done|complete|completed)' "$content")
pending_count=$(count_matches '(\*\*|^)status(\*\*)?[[:space:]]*:[[:space:]]*(pending|todo|not[[:space:]]?started)' "$content")
in_progress_count=$(count_matches '(\*\*|^)status(\*\*)?[[:space:]]*:[[:space:]]*(in[_[:space:]]?progress|wip|active)' "$content")
removed_count=$(count_matches '(\*\*|^)status(\*\*)?[[:space:]]*:[[:space:]]*(removed|cancelled|deferred)' "$content")

# Also count GitHub-style checkboxes as a fallback/supplement
checkbox_done=$(count_matches '^[[:space:]]*-[[:space:]]*\[x\]' "$content")
checkbox_pending=$(count_matches '^[[:space:]]*-[[:space:]]*\[ \]' "$content")

# Use whichever style is more prevalent
if [ "$checkbox_done" -gt "$done_count" ] || [ "$checkbox_pending" -gt "$pending_count" ]; then
    done_count=$checkbox_done
    pending_count=$checkbox_pending
fi

total=$((done_count + pending_count + in_progress_count))
progress_pct=0
if [ "$total" -gt 0 ]; then
    progress_pct=$((done_count * 100 / total))
fi

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

cat <<EOF
{"spec_exists":true,"spec_file":"$(esc "$SPEC_FILE")","total_items":$total,"done":$done_count,"pending":$pending_count,"in_progress":$in_progress_count,"removed":$removed_count,"progress_percent":$progress_pct}
EOF
