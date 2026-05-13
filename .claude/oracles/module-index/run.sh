#!/bin/bash
# module-index oracle (Unix)
# Walks the project tree to inventory modules (directories with code) and their README status.
# Excludes common non-module directories (node_modules, target, .git, etc.)

set -e

# Skip these directory patterns entirely
EXCLUDES='node_modules|target|\.git|\.vscode|\.idea|dist|build|out|coverage|__pycache__|\.venv|venv|\.claude/oracles/cache|\.claude/checkpoints'

# Collect module candidates: directories containing code files or README.md,
# excluding the root itself and excluded patterns.
modules_json="["
first=1
total=0
with_readme=0
without_readme=0

# Use find to enumerate directories; depth-limit to avoid excessive walking
while IFS= read -r dir; do
    # Normalize: strip leading ./
    dir_clean="${dir#./}"
    if [ -z "$dir_clean" ] || [ "$dir_clean" = "." ]; then continue; fi
    # Skip excluded patterns
    if echo "$dir_clean" | grep -qE "^($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    if echo "$dir_clean" | grep -qE "/($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi

    # A directory is a "module" candidate if it has a README.md OR it has code files
    has_readme=false
    readme_path=""
    if [ -f "$dir_clean/README.md" ]; then
        has_readme=true
        readme_path="$dir_clean/README.md"
    fi

    # Only consider directories that have README.md (as a conservative filter).
    # A less conservative version would also include dirs with source files.
    if [ "$has_readme" = "true" ]; then
        # Extract purpose: first non-heading, non-empty paragraph under first heading
        purpose=$(awk '/^#/{if(h){exit}; h=1; next} h && /^[^[:space:]]/{print; exit}' "$readme_path" 2>/dev/null | head -c 200 | tr '\n' ' ' | sed 's/"/\\"/g; s/\\/\\\\/g' || echo "")
        if [ -z "$purpose" ]; then purpose=""; fi

        if [ "$first" -eq 1 ]; then first=0; else modules_json+=","; fi
        path_esc=$(printf '%s' "$dir_clean" | sed 's/\\/\\\\/g; s/"/\\"/g')
        modules_json+="{\"path\":\"$path_esc\",\"has_readme\":true,\"purpose\":\"$purpose\"}"
        total=$((total + 1))
        with_readme=$((with_readme + 1))
    fi
done < <(find . -type d -maxdepth 4 2>/dev/null | sort)

modules_json+="]"

# Count directories without README (top-level code dirs that are missing README)
# Only count directories that look like modules but lack README
while IFS= read -r dir; do
    dir_clean="${dir#./}"
    if [ -z "$dir_clean" ] || [ "$dir_clean" = "." ]; then continue; fi
    if echo "$dir_clean" | grep -qE "^($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    if echo "$dir_clean" | grep -qE "/($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    if [ -f "$dir_clean/README.md" ]; then continue; fi
    # Has any code file?
    if find "$dir_clean" -maxdepth 1 -type f \( -name "*.rs" -o -name "*.ts" -o -name "*.tsx" -o -name "*.js" -o -name "*.jsx" -o -name "*.py" -o -name "*.go" -o -name "*.java" -o -name "*.cs" \) 2>/dev/null | grep -q .; then
        without_readme=$((without_readme + 1))
    fi
done < <(find . -type d -maxdepth 3 2>/dev/null | sort)

cat <<EOF
{"total_modules":$total,"with_readme":$with_readme,"without_readme":$without_readme,"modules":$modules_json}
EOF
