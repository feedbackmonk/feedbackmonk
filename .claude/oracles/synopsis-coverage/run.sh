#!/bin/bash
# synopsis-coverage Verification Oracle (Unix)
# Reports the fraction of module READMEs conforming to the HCT Synopsis discipline:
# presence of a `## Synopsis` H2 section AND content between 1 and 5 non-empty lines.
#
# Output schema: see oracle.json. Spec: HCT-04 (docs/specs/SPECIFICATION.md).
# Verification Oracle category: FOUNDATIONS/ORACULURGY_DESIGN.md Part 11.

set -e

EXCLUDES='node_modules|target|\.git|\.vscode|\.idea|dist|build|out|coverage|__pycache__|\.venv|venv|\.claude/oracles/cache|\.claude/checkpoints'

json_escape() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\n'/\\n}"
    s="${s//$'\r'/\\r}"
    s="${s//$'\t'/\\t}"
    printf '%s' "$s"
}

# Returns the count of non-empty content lines inside `## Synopsis`,
# or "MISSING" if the section is not present. HTML comments are stripped.
extract_synopsis_count() {
    local file="$1"
    awk '
        BEGIN { in_section = 0; in_comment = 0; found = 0; count = 0 }
        {
            if (!in_section) {
                if ($0 ~ /^##[[:space:]]+Synopsis[[:space:]]*$/) {
                    in_section = 1
                    found = 1
                    next
                }
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
            if ($0 ~ /[^[:space:]]/) { count++ }
        }
        END {
            if (!found) { print "MISSING" }
            else { print count }
        }
    ' "$file" 2>/dev/null
}

total=0
conformant=0
missing=()
over_length=()

# Root README
if [ -f "README.md" ]; then
    total=$((total + 1))
    res=$(extract_synopsis_count "README.md")
    if [ "$res" = "MISSING" ]; then
        missing+=(".")
    elif [ "$res" -gt 5 ]; then
        over_length+=(".")
    elif [ "$res" -lt 1 ]; then
        missing+=(".")
    else
        conformant=$((conformant + 1))
    fi
fi

# Subdirectory READMEs (full tree)
while IFS= read -r dir; do
    dir_clean="${dir#./}"
    [ -z "$dir_clean" ] || [ "$dir_clean" = "." ] && continue
    if echo "$dir_clean" | grep -qE "^($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    if echo "$dir_clean" | grep -qE "/($EXCLUDES)(/|$)" 2>/dev/null; then continue; fi
    [ -f "$dir_clean/README.md" ] || continue

    total=$((total + 1))
    res=$(extract_synopsis_count "$dir_clean/README.md")
    if [ "$res" = "MISSING" ]; then
        missing+=("$dir_clean")
    elif [ "$res" -gt 5 ]; then
        over_length+=("$dir_clean")
    elif [ "$res" -lt 1 ]; then
        missing+=("$dir_clean")
    else
        conformant=$((conformant + 1))
    fi
done < <(find . -type d 2>/dev/null | sort)

# Coverage_pct (integer floor 0-100); 100 when total == 0 (graceful absence)
if [ "$total" -eq 0 ]; then
    coverage_pct=100
else
    coverage_pct=$((conformant * 100 / total))
fi

# Briefing per HCT-05 format; empty when coverage_pct == 100 (gracefully absent)
briefing=""
if [ "$total" -gt 0 ] && [ "$coverage_pct" -lt 100 ]; then
    missing_count=${#missing[@]}
    over_count=${#over_length[@]}
    briefing="${coverage_pct}% (${missing_count} missing, ${over_count} over-length). Run /0-uldf-uladp-compliance for details."
fi

# Sort + emit JSON arrays
ms_sorted=$(printf '%s\n' "${missing[@]}" | sort 2>/dev/null || true)
ms_json="["; first=1
while IFS= read -r m; do
    [ -z "$m" ] && continue
    if [ $first -eq 1 ]; then first=0; else ms_json+=","; fi
    ms_json+="\"$(json_escape "$m")\""
done <<< "$ms_sorted"
ms_json+="]"

ol_sorted=$(printf '%s\n' "${over_length[@]}" | sort 2>/dev/null || true)
ol_json="["; first=1
while IFS= read -r m; do
    [ -z "$m" ] && continue
    if [ $first -eq 1 ]; then first=0; else ol_json+=","; fi
    ol_json+="\"$(json_escape "$m")\""
done <<< "$ol_sorted"
ol_json+="]"

briefing_esc="$(json_escape "$briefing")"

cat <<EOF
{"coverage_pct":$coverage_pct,"conformant_count":$conformant,"total_modules":$total,"missing":$ms_json,"over_length":$ol_json,"briefing_summary":"$briefing_esc","briefing":"$briefing_esc"}
EOF
