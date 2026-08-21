#!/bin/bash
# pending-followups oracle (Unix)
# Parses 'Pending Follow-Ups' sections and identifies overdue items.
# Sources (merged, additive "scope" field on each item):
#   1. Project CLAUDE.md (scope "project") -- as always.
#   2. An EXTRACTED follow-ups file the CLAUDE.md section points at, or one
#      named by .claude/config.json `pendingFollowups.path` (scope "project").
#      Projects legitimately move this content out of CLAUDE.md to keep
#      auto-loaded context lean, leaving a pointer paragraph behind (DEC-346).
#   3. ~/.claude/MACHINE_CONFIG.md (scope "global") -- machine-global reminders
#      written by /0-uldf-schedule --scope=global. MACHINE_CONFIG.md is the
#      sync-survivor: ~/.claude/CLAUDE.md is overwritten by every framework
#      sync, so reminders there would silently vanish (scrutiny 07 F3).
#
# NO-DATA IS NEVER ZERO (DEC-346). Before this oracle followed pointers it
# reported {"has_followups_section":true,"total":0} against a project with ~80
# live entries -- it asserted it had FOUND the section and then reported nothing
# pending, which is worse than a plain miss. `status` now separates the three
# cases a bare `total: 0` used to collapse:
#   ok      -- the sources were read and the count is trustworthy
#   no-data -- a section/pointer exists that we could NOT turn into entries
# A consumer must not read total==0 as an all-clear unless status=="ok".

set -e

today=$(date +%Y-%m-%d 2>/dev/null || echo "")

items_json="["
first=1
total=0
overdue=0
# FOLLOWUP-02: entries that DECLARE a date trigger whose date this oracle could
# not turn into a comparison. They are rendered `overdue:null` -- never `false`
# (CSI-36/DEC-342: unknown is not a verdict), and they make `overdue` a floor
# rather than a magnitude (QUIESCE-09's shape).
unevaluable=0
# The single most-overdue date, for the briefing line. Dates and integers only:
# nothing copied out of an entry title ever reaches the briefing string.
oldest_due=""
oldest_days=0
has_section=false

sources_json="["
sources_first=1
unresolved_json="["
unresolved_first=1
# Non-blank, non-heading lines seen inside a parsed section. A section that
# carries content but yields no entries is the false-all-clear shape.
content_lines=0

json_esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

add_source() {
    if [ "$sources_first" -eq 1 ]; then sources_first=0; else sources_json+=","; fi
    sources_json+="\"$(json_esc "$1")\""
}

add_unresolved() {
    if [ "$unresolved_first" -eq 1 ]; then unresolved_first=0; else unresolved_json+=","; fi
    unresolved_json+="\"$(json_esc "$1")\""
}

# pointer_candidate <line>
# Echoes a relative *.md path referenced by the line, or nothing.
# Per-item "Details: `docs/pending/<slug>.md`" tokens are stripped first: those
# name one entry's detail body, not an extracted follow-ups file, and following
# them would recurse into every entry.
pointer_candidate() {
    local line="$1" clean=""
    clean=$(printf '%s' "$line" | sed -E 's/Details:[[:space:]]*`[^`]*`//g')
    case "$clean" in *http://*|*https://*) return 0 ;; esac
    if [[ "$clean" =~ \]\(([^\)[:space:]]+\.md)\) ]]; then
        printf '%s' "${BASH_REMATCH[1]}"
    elif [[ "$clean" =~ \`([^\`]+\.md)\` ]]; then
        printf '%s' "${BASH_REMATCH[1]}"
    elif [[ "$clean" =~ ([A-Za-z0-9_][A-Za-z0-9_./-]*\.md) ]]; then
        printf '%s' "${BASH_REMATCH[1]}"
    fi
    # Always succeed: a no-match must not abort the oracle under `set -e`.
    return 0
}

# parse_section_text <section-text> <scope-label> <collect-pointers 0|1>
# Appends parsed bullet items to the accumulators. When collecting pointers,
# lines that are NOT entries are searched for an extracted-file reference and
# the winner is left in $found_pointer.
parse_section_text() {
    local section="$1" scope="$2" collect="$3"
    local line detail_json due title title_trim title_esc label label_esc cand
    local is_overdue days_overdue today_epoch due_epoch matched
    local evaluable overdue_field days_field

    while IFS= read -r line; do
        matched=0
        case "$line" in
            ''|'#'*) ;;
            *) content_lines=$((content_lines + 1)) ;;
        esac

        # Extract "Details: `docs/pending/<slug>.md`" pointer if present (P2 externalization).
        detail_json="null"
        if [[ "$line" =~ Details:[[:space:]]+\`(docs/pending/[^\`[:space:]]+\.md)\` ]]; then
            detail_json="\"$(json_esc "${BASH_REMATCH[1]}")\""
        fi

        # FOLLOWUP-01: a date TRIGGER is DECLARED by the date opening the bold
        # prefix, optionally behind a single leading word (`After`, `On`, `By`,
        # ...). This is a declaration test, not an enumeration of trigger words
        # (DEC-343: a list that could never be finished). It deliberately allows
        # anything between the date and the closing `**`, which is what
        # /0-uldf-schedule mandates -- `**After YYYY-MM-DD ({title})**`.
        #
        # Position is load-bearing. Measured over 15 projects: 61 bold prefixes
        # carry a YYYY-MM-DD somewhere and only 2 declare a trigger this way.
        # The other 59 are status labels that merely mention a date ("... Phase 3
        # SHIPPED 2026-08-14 ...", "[ACTIVE, added 2026-07-30]"). Matching on
        # "contains a date" would produce 59 wrong verdicts; treating them as
        # unknown would produce 59 false unknowns. They are not dated entries at
        # all, so they keep falling to the label branch (DEC-379: an unknown may
        # only be emitted where a real question existed).
        #
        # The day/month are matched loosely ([0-9]{1,2}) ON PURPOSE: a prefix
        # reading `**After 2026-8-12 (...)**` HAS declared a date trigger, it is
        # simply not in the mandated form. Loose here, strict below -- that is
        # what turns a hand-typo into a visible unknown instead of a silent
        # "not due" that never fires.
        if [[ "$line" =~ ^-[[:space:]]+\*\*([A-Za-z]+[[:space:]]+)?([0-9]{4}-[0-9]{1,2}-[0-9]{1,2})([^*]*)\*\* ]]; then
            matched=1
            due="${BASH_REMATCH[2]}"
            title=$(echo "$line" | sed -E 's/^-[[:space:]]+\*\*[^*]+\*\*:?[[:space:]]*//')
            title_trim=$(echo "$title" | cut -c1-120)
            title_esc=$(json_esc "$title_trim")

            is_overdue=false
            days_overdue=0
            evaluable=1
            # Strict ISO is the only form we will compute against.
            case "$due" in
                [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]) ;;
                *) evaluable=0 ;;
            esac
            if [ "$evaluable" -eq 1 ]; then
                if [ -n "$today" ] && command -v date >/dev/null 2>&1; then
                    today_epoch=$(date -d "$today" +%s 2>/dev/null || echo "")
                    due_epoch=$(date -d "$due" +%s 2>/dev/null || echo "")
                    if [ -z "$today_epoch" ] || [ -z "$due_epoch" ]; then
                        # An impossible date (2026-02-30) or no usable `date`.
                        evaluable=0
                    elif [ "$today_epoch" -gt "$due_epoch" ]; then
                        is_overdue=true
                        days_overdue=$(( (today_epoch - due_epoch) / 86400 ))
                        overdue=$((overdue + 1))
                        if [ -z "$oldest_due" ] || [ "$days_overdue" -gt "$oldest_days" ]; then
                            oldest_due="$due"
                            oldest_days="$days_overdue"
                        fi
                    fi
                else
                    evaluable=0
                fi
            fi
            if [ "$evaluable" -eq 0 ]; then
                unevaluable=$((unevaluable + 1))
                overdue_field="null"
                days_field="null"
            else
                overdue_field="$is_overdue"
                days_field="$days_overdue"
            fi

            if [ "$first" -eq 1 ]; then first=0; else items_json+=","; fi
            items_json+="{\"title\":\"$title_esc\",\"due\":\"$due\",\"overdue\":$overdue_field,\"days_overdue\":$days_field,\"detail_path\":$detail_json,\"scope\":\"$scope\"}"
            total=$((total + 1))
        elif [[ "$line" =~ ^-[[:space:]]+\*\*([^*]+)\*\* ]]; then
            # Non-date label (e.g., "Trigger-based")
            matched=1
            label="${BASH_REMATCH[1]}"
            title=$(echo "$line" | sed -E 's/^-[[:space:]]+\*\*[^*]+\*\*:?[[:space:]]*//')
            title_trim=$(echo "$title" | cut -c1-120)
            title_esc=$(json_esc "$title_trim")
            label_esc=$(json_esc "$label")

            if [ "$first" -eq 1 ]; then first=0; else items_json+=","; fi
            items_json+="{\"title\":\"$title_esc\",\"due\":\"$label_esc\",\"overdue\":false,\"days_overdue\":0,\"detail_path\":$detail_json,\"scope\":\"$scope\"}"
            total=$((total + 1))
        fi

        if [ "$collect" = "1" ] && [ "$matched" -eq 0 ] && [ -z "$found_pointer" ]; then
            cand=$(pointer_candidate "$line") || cand=""
            if [ -n "$cand" ]; then found_pointer="$cand"; fi
        fi
    done <<< "$section"
}

# extract_section <file>
# Echoes the file's "Pending Follow-Ups" section body, or the whole file when it
# has no such heading (an extracted file is allowed to be nothing but the list).
extract_section() {
    awk '/^## Pending Follow-Ups|^## Pending Follow.?ups/{flag=1; next} /^## /{flag=0} flag' "$1" 2>/dev/null
}

# parse_file <path> <scope-label> <collect-pointers 0|1>
parse_file() {
    local file="$1" scope="$2" collect="${3:-0}"
    [ -f "$file" ] || return 0

    local section
    section=$(extract_section "$file")
    if [ -z "$section" ]; then
        # No heading: an extracted follow-ups file may be a bare list. Never do
        # this for CLAUDE.md / MACHINE_CONFIG.md (collect==1 or scope global),
        # whose bodies are full of unrelated bullets.
        if [ "$collect" = "2" ]; then
            section=$(cat "$file" 2>/dev/null)
        fi
        [ -n "$section" ] || return 0
    else
        has_section=true
    fi
    add_source "$file"
    parse_section_text "$section" "$scope" "$collect"
}

# --- Source 1: project CLAUDE.md (and the file it points at) ------------------
found_pointer=""
project_md=""
if [ -f "CLAUDE.md" ]; then project_md="CLAUDE.md"
elif [ -f ".claude/CLAUDE.md" ]; then project_md=".claude/CLAUDE.md"; fi

if [ -n "$project_md" ]; then
    parse_file "$project_md" "project" 1
fi

# --- Source 2: the extracted follow-ups file ---------------------------------
# Explicit config wins over a pointer discovered in the section.
configured=""
if [ -f ".claude/config.json" ]; then
    configured=$( { grep -oE '"pendingFollowups"[[:space:]]*:[[:space:]]*\{[^}]*"path"[[:space:]]*:[[:space:]]*"[^"]+"' .claude/config.json 2>/dev/null \
        | grep -oE '"[^"]+"$' | tr -d '"' | head -1; } || true )
fi
if [ -n "$configured" ]; then found_pointer="$configured"; fi

if [ -n "$found_pointer" ] && [ "$found_pointer" != "$project_md" ]; then
    if [ -f "$found_pointer" ]; then
        parse_file "$found_pointer" "project" 2
    else
        add_unresolved "$found_pointer"
    fi
fi

# --- Source 3: machine-global reminders (graceful absence) -------------------
parse_file "$HOME/.claude/MACHINE_CONFIG.md" "global" 0

items_json+="]"
sources_json+="]"
unresolved_json+="]"

if [ "$has_section" = "false" ] && [ "$total" -eq 0 ]; then
    # Graceful absence: an empty `briefing` suppresses the session-start line
    # entirely (the documented convention), so a project with no follow-ups
    # costs nothing.
    echo "{\"briefing\":\"\",\"has_followups_section\":false,\"status\":\"ok\",\"total\":0,\"overdue\":0,\"overdue_evaluable\":true,\"unevaluable\":0,\"items\":[],\"sources\":$sources_json,\"unresolved\":$unresolved_json}"
    exit 0
fi

# NO-DATA is never zero: either a pointer we could not read, or a section that
# carried content and yielded no entries (content lives somewhere we cannot see).
status="ok"
if [ "$unresolved_first" -eq 0 ]; then
    status="no-data"
elif [ "$total" -eq 0 ] && [ "$content_lines" -gt 0 ]; then
    status="no-data"
fi

# FOLLOWUP-02: the briefing line.
#
# Until now this oracle emitted NO `briefing` field, so session-start fell back
# to dumping the whole JSON as the line -- and dropped it silently whenever that
# exceeded MAX_CHARS (3000). Measured 2026-08-19: this repo 3824 chars, a
# sibling project 75081. So on exactly the projects that HAVE follow-ups the
# line never appeared at all, with no `deferred`/`killed` note to say so. A
# check that goes dark is worse than one that is plainly absent (OVALID-09).
#
# The string is deliberately free of `"` and of any content copied out of the
# corpus: session-start extracts it with `"briefing"[[:space:]]*:[[:space:]]*"[^"]*"`,
# so one quote inside an entry title would truncate the line. Dates and counts
# only. `briefing` is emitted FIRST so the extractor's `head -1` can never pick
# up a same-named token from an item body.
overdue_evaluable=true
if [ "$unevaluable" -gt 0 ]; then overdue_evaluable=false; fi

briefing=""
if [ "$status" = "no-data" ]; then
    briefing="NO-DATA -- a follow-up source exists that could not be turned into entries; total=$total is NOT an all-clear"
elif [ "$overdue" -gt 0 ]; then
    briefing="$total follow-ups, $overdue OVERDUE (oldest $oldest_due, ${oldest_days}d past)"
else
    briefing="$total follow-ups, none date-overdue"
fi
if [ "$unevaluable" -gt 0 ]; then
    briefing="$briefing; $unevaluable declare a date this oracle cannot read -- the overdue count is a FLOOR, not a verdict"
fi

cat <<EOF
{"briefing":"$briefing","has_followups_section":true,"status":"$status","total":$total,"overdue":$overdue,"overdue_evaluable":$overdue_evaluable,"unevaluable":$unevaluable,"items":$items_json,"sources":$sources_json,"unresolved":$unresolved_json}
EOF
