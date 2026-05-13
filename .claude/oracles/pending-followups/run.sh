#!/bin/bash
# pending-followups oracle (Unix)
# Parses CLAUDE.md 'Pending Follow-Ups' section and identifies overdue items.

set -e

CLAUDE_MD=""
if [ -f "CLAUDE.md" ]; then
    CLAUDE_MD="CLAUDE.md"
elif [ -f ".claude/CLAUDE.md" ]; then
    CLAUDE_MD=".claude/CLAUDE.md"
fi

if [ -z "$CLAUDE_MD" ]; then
    echo '{"has_followups_section":false,"total":0,"overdue":0,"items":[]}'
    exit 0
fi

# Extract the "Pending Follow-Ups" section (until next top-level heading)
section=$(awk '/^## Pending Follow-Ups|^## Pending Follow.?ups/{flag=1; next} /^## /{flag=0} flag' "$CLAUDE_MD" 2>/dev/null)

if [ -z "$section" ]; then
    echo '{"has_followups_section":false,"total":0,"overdue":0,"items":[]}'
    exit 0
fi

today=$(date +%Y-%m-%d 2>/dev/null || echo "")

# Parse bullet items. Pattern recognized:
#   - **After YYYY-MM-DD**: Title...
#   - **YYYY-MM-DD**: Title...
#   - **Trigger-based**: Title...
items_json="["
first=1
total=0
overdue=0

# Read the section line by line, looking for bullet starts
while IFS= read -r line; do
    # Extract "Details: `docs/pending/<slug>.md`" pointer if present (added by P2 externalization).
    # Encoded as JSON null when absent, JSON string when present.
    detail_json="null"
    if [[ "$line" =~ Details:[[:space:]]+\`(docs/pending/[^\`[:space:]]+\.md)\` ]]; then
        detail_path_esc=$(printf '%s' "${BASH_REMATCH[1]}" | sed 's/\\/\\\\/g; s/"/\\"/g')
        detail_json="\"$detail_path_esc\""
    fi

    # Match "- **After YYYY-MM-DD**" or "- **YYYY-MM-DD**" or "- **<some label>**"
    if [[ "$line" =~ ^-[[:space:]]+\*\*(After[[:space:]])?([0-9]{4}-[0-9]{2}-[0-9]{2})\*\* ]]; then
        due="${BASH_REMATCH[2]}"
        # Extract title: everything after the **...**:
        title=$(echo "$line" | sed -E 's/^-[[:space:]]+\*\*[^*]+\*\*:?[[:space:]]*//')
        title_trim=$(echo "$title" | cut -c1-120)
        title_esc=$(printf '%s' "$title_trim" | sed 's/\\/\\\\/g; s/"/\\"/g')

        # Compute overdue
        is_overdue=false
        days_overdue=0
        if [ -n "$today" ] && command -v date >/dev/null 2>&1; then
            today_epoch=$(date -d "$today" +%s 2>/dev/null || echo "")
            due_epoch=$(date -d "$due" +%s 2>/dev/null || echo "")
            if [ -n "$today_epoch" ] && [ -n "$due_epoch" ] && [ "$today_epoch" -gt "$due_epoch" ]; then
                is_overdue=true
                days_overdue=$(( (today_epoch - due_epoch) / 86400 ))
                overdue=$((overdue + 1))
            fi
        fi

        if [ "$first" -eq 1 ]; then first=0; else items_json+=","; fi
        items_json+="{\"title\":\"$title_esc\",\"due\":\"$due\",\"overdue\":$is_overdue,\"days_overdue\":$days_overdue,\"detail_path\":$detail_json}"
        total=$((total + 1))
    elif [[ "$line" =~ ^-[[:space:]]+\*\*([^*]+)\*\* ]]; then
        # Non-date label (e.g., "Trigger-based")
        label="${BASH_REMATCH[1]}"
        title=$(echo "$line" | sed -E 's/^-[[:space:]]+\*\*[^*]+\*\*:?[[:space:]]*//')
        title_trim=$(echo "$title" | cut -c1-120)
        title_esc=$(printf '%s' "$title_trim" | sed 's/\\/\\\\/g; s/"/\\"/g')
        label_esc=$(printf '%s' "$label" | sed 's/\\/\\\\/g; s/"/\\"/g')

        if [ "$first" -eq 1 ]; then first=0; else items_json+=","; fi
        items_json+="{\"title\":\"$title_esc\",\"due\":\"$label_esc\",\"overdue\":false,\"days_overdue\":0,\"detail_path\":$detail_json}"
        total=$((total + 1))
    fi
done <<< "$section"

items_json+="]"

cat <<EOF
{"has_followups_section":true,"total":$total,"overdue":$overdue,"items":$items_json}
EOF
