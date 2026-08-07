#!/bin/bash
# pending-followups oracle (Unix)
# Parses 'Pending Follow-Ups' sections and identifies overdue items.
# Sources (merged, additive "scope" field on each item):
#   1. Project CLAUDE.md (scope "project") -- as always.
#   2. ~/.claude/MACHINE_CONFIG.md (scope "global") -- machine-global reminders
#      written by /0-uldf-schedule --scope=global. MACHINE_CONFIG.md is the
#      sync-survivor: ~/.claude/CLAUDE.md is overwritten by every framework
#      sync, so reminders there would silently vanish (scrutiny 07 F3).

set -e

today=$(date +%Y-%m-%d 2>/dev/null || echo "")

items_json="["
first=1
total=0
overdue=0
has_section=false

# parse_file <path> <scope-label>
# Appends parsed bullet items from the file's "Pending Follow-Ups" section
# to the accumulator vars. Missing file / missing section = contributes nothing.
parse_file() {
    local file="$1"
    local scope="$2"
    [ -f "$file" ] || return 0

    local section
    section=$(awk '/^## Pending Follow-Ups|^## Pending Follow.?ups/{flag=1; next} /^## /{flag=0} flag' "$file" 2>/dev/null)
    [ -n "$section" ] || return 0
    has_section=true

    local line detail_json due title title_trim title_esc label label_esc
    local is_overdue days_overdue today_epoch due_epoch
    while IFS= read -r line; do
        # Extract "Details: `docs/pending/<slug>.md`" pointer if present (P2 externalization).
        detail_json="null"
        if [[ "$line" =~ Details:[[:space:]]+\`(docs/pending/[^\`[:space:]]+\.md)\` ]]; then
            local detail_path_esc
            detail_path_esc=$(printf '%s' "${BASH_REMATCH[1]}" | sed 's/\\/\\\\/g; s/"/\\"/g')
            detail_json="\"$detail_path_esc\""
        fi

        # Match "- **After YYYY-MM-DD**" or "- **YYYY-MM-DD**"
        if [[ "$line" =~ ^-[[:space:]]+\*\*(After[[:space:]])?([0-9]{4}-[0-9]{2}-[0-9]{2})\*\* ]]; then
            due="${BASH_REMATCH[2]}"
            title=$(echo "$line" | sed -E 's/^-[[:space:]]+\*\*[^*]+\*\*:?[[:space:]]*//')
            title_trim=$(echo "$title" | cut -c1-120)
            title_esc=$(printf '%s' "$title_trim" | sed 's/\\/\\\\/g; s/"/\\"/g')

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
            items_json+="{\"title\":\"$title_esc\",\"due\":\"$due\",\"overdue\":$is_overdue,\"days_overdue\":$days_overdue,\"detail_path\":$detail_json,\"scope\":\"$scope\"}"
            total=$((total + 1))
        elif [[ "$line" =~ ^-[[:space:]]+\*\*([^*]+)\*\* ]]; then
            # Non-date label (e.g., "Trigger-based")
            label="${BASH_REMATCH[1]}"
            title=$(echo "$line" | sed -E 's/^-[[:space:]]+\*\*[^*]+\*\*:?[[:space:]]*//')
            title_trim=$(echo "$title" | cut -c1-120)
            title_esc=$(printf '%s' "$title_trim" | sed 's/\\/\\\\/g; s/"/\\"/g')
            label_esc=$(printf '%s' "$label" | sed 's/\\/\\\\/g; s/"/\\"/g')

            if [ "$first" -eq 1 ]; then first=0; else items_json+=","; fi
            items_json+="{\"title\":\"$title_esc\",\"due\":\"$label_esc\",\"overdue\":false,\"days_overdue\":0,\"detail_path\":$detail_json,\"scope\":\"$scope\"}"
            total=$((total + 1))
        fi
    done <<< "$section"
}

# Source 1: project CLAUDE.md
if [ -f "CLAUDE.md" ]; then
    parse_file "CLAUDE.md" "project"
elif [ -f ".claude/CLAUDE.md" ]; then
    parse_file ".claude/CLAUDE.md" "project"
fi

# Source 2: machine-global reminders (graceful absence)
parse_file "$HOME/.claude/MACHINE_CONFIG.md" "global"

items_json+="]"

if [ "$has_section" = "false" ]; then
    echo '{"has_followups_section":false,"total":0,"overdue":0,"items":[]}'
    exit 0
fi

cat <<EOF
{"has_followups_section":true,"total":$total,"overdue":$overdue,"items":$items_json}
EOF
