#!/bin/bash
# git-state oracle (Unix)
# Reports current git state: branch, uncommitted counts, last commit.
# Output: single JSON object matching oracle.json schema.

set -e

# Not a git repo? Report gracefully.
if ! git rev-parse --git-dir >/dev/null 2>&1; then
    cat <<'EOF'
{"is_git_repo":false,"branch":null,"modified":0,"staged":0,"untracked":0,"deleted":0,"clean":true,"last_commit":{"hash":null,"subject":null,"date":null}}
EOF
    exit 0
fi

branch=$(git branch --show-current 2>/dev/null || echo "")

# Parse git status --porcelain
status_output=$(git status --porcelain 2>/dev/null || echo "")
modified=0
staged=0
untracked=0
deleted=0

while IFS= read -r line; do
    if [ -z "$line" ]; then continue; fi
    xy="${line:0:2}"
    x="${xy:0:1}"
    y="${xy:1:1}"
    case "$xy" in
        '??') untracked=$((untracked + 1)) ;;
    esac
    case "$x" in
        'M'|'A'|'R'|'C') staged=$((staged + 1)) ;;
        'D') deleted=$((deleted + 1)); staged=$((staged + 1)) ;;
    esac
    case "$y" in
        'M') modified=$((modified + 1)) ;;
        'D') deleted=$((deleted + 1)) ;;
    esac
done <<< "$status_output"

clean=false
if [ "$modified" -eq 0 ] && [ "$staged" -eq 0 ] && [ "$untracked" -eq 0 ] && [ "$deleted" -eq 0 ]; then
    clean=true
fi

# Last commit
last_hash=""
last_subject=""
last_date=""
if last_commit_line=$(git log -1 --format='%h|%s|%ad' --date=short 2>/dev/null); then
    last_hash=$(echo "$last_commit_line" | cut -d'|' -f1)
    last_subject=$(echo "$last_commit_line" | cut -d'|' -f2)
    last_date=$(echo "$last_commit_line" | cut -d'|' -f3)
fi

# JSON escape helper
esc() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

hash_json="null"
if [ -n "$last_hash" ]; then hash_json="\"$(esc "$last_hash")\""; fi
subject_json="null"
if [ -n "$last_subject" ]; then subject_json="\"$(esc "$last_subject")\""; fi
date_json="null"
if [ -n "$last_date" ]; then date_json="\"$(esc "$last_date")\""; fi
branch_json="null"
if [ -n "$branch" ]; then branch_json="\"$(esc "$branch")\""; fi

cat <<EOF
{"is_git_repo":true,"branch":$branch_json,"modified":$modified,"staged":$staged,"untracked":$untracked,"deleted":$deleted,"clean":$clean,"last_commit":{"hash":$hash_json,"subject":$subject_json,"date":$date_json}}
EOF
