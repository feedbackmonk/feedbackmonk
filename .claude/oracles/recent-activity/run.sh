#!/bin/bash
# recent-activity oracle (Unix)
# Reports recent commits, touched areas, and commit cadence.

set -e

if ! git rev-parse --git-dir >/dev/null 2>&1; then
    echo '{"last_commits":[],"touched_directories_last_5":[],"commits_last_7_days":0,"commits_last_30_days":0}'
    exit 0
fi

# Last 5 commits as JSON array
commits_json="["
first=1
while IFS='|' read -r hash subject author date; do
    if [ "$first" -eq 1 ]; then first=0; else commits_json+=","; fi
    # JSON-escape
    subject_esc=$(printf '%s' "$subject" | sed 's/\\/\\\\/g; s/"/\\"/g')
    author_esc=$(printf '%s' "$author" | sed 's/\\/\\\\/g; s/"/\\"/g')
    commits_json+="{\"hash\":\"$hash\",\"subject\":\"$subject_esc\",\"author\":\"$author_esc\",\"date\":\"$date\"}"
done < <(git log -5 --format='%h|%s|%an|%ad' --date=short 2>/dev/null)
commits_json+="]"

# Touched directories from last 5 commits (top-level)
touched_dirs=$(git log -5 --name-only --format='' 2>/dev/null | grep -v '^$' | awk -F/ '{print $1}' | sort -u)
dirs_json="["
first=1
while IFS= read -r dir; do
    if [ -z "$dir" ]; then continue; fi
    if [ "$first" -eq 1 ]; then first=0; else dirs_json+=","; fi
    dir_esc=$(printf '%s' "$dir" | sed 's/\\/\\\\/g; s/"/\\"/g')
    dirs_json+="\"$dir_esc\""
done <<< "$touched_dirs"
dirs_json+="]"

# Commit counts by window
commits_7d=$(git log --since='7 days ago' --oneline 2>/dev/null | wc -l | tr -d ' ')
commits_30d=$(git log --since='30 days ago' --oneline 2>/dev/null | wc -l | tr -d ' ')

cat <<EOF
{"last_commits":$commits_json,"touched_directories_last_5":$dirs_json,"commits_last_7_days":$commits_7d,"commits_last_30_days":$commits_30d}
EOF
