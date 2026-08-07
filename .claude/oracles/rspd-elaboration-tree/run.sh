#!/bin/bash
# rspd-elaboration-tree oracle (Unix) -- RSPD-06.
# Derives the decomposition tree's elaboration state from RSPD-05 spec node
# statuses across the root spec + nested delegated sub-spec stores, and
# classifies each delegated node. The load-bearing distinction is MISSING:
# a delegated row whose <domain>/SPECIFICATION.md store does NOT exist
# (deferred-by-design vs. lost). kind: project-state. Gracefully absent:
# emits applicable:false (never an error) when no delegated nodes exist.
#
# Output: a single JSON object (see README.md for the FROZEN schema).
# PowerShell mirror: run.ps1.

set -uo pipefail

SPECS_DIR="docs/specs"
ROOT_SPEC="$SPECS_DIR/SPECIFICATION.md"

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

emit_absent() { # $1 = spec_root JSON literal (quoted string or null)
    printf '{"status":"ok","applicable":false,"spec_root":%s,"tree":[],"summary":{"total":0,"charter":0,"elaborated":0,"in_progress":0,"done":0,"missing":0},"briefing":""}\n' "$1"
}

if [ ! -f "$ROOT_SPEC" ]; then
    emit_absent "null"
    exit 0
fi

# ---- Field-safe TSV split (DEFER-078) ---------------------------------------
# `IFS=$'\t' read -r id name status ptr` DROPS an empty NON-FINAL field: tab is
# IFS *whitespace*, so a run of tabs collapses to one delimiter and every later
# field shifts left. The extractor below sets `curname=""` whenever a `####`
# heading carries no colon, and `curstatus=""` whenever it carries no `[STATUS]`
# bracket -- BOTH ordinary headings -- so the collapsing read routinely lost the
# trailing `SPECIFICATION.md` pointer, `store` resolved to a bare directory, and
# the node was reported `missing` / `exists:false`. Splitting on literal tabs
# preserves empties. bash-3.2 safe (no arrays / readarray -d / namerefs).
# `${r#*<tab>}` returns the whole string when no tab remains, so strip only while
# one does. Sets RE_F1..RE_F4, empty for absent fields, as `read` would.
_re_split_tsv() {
    local _r="$1" _t
    _t=$(printf '\t')
    case "$_r" in (*"$_t"*) RE_F1="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) RE_F1="$_r"; _r="" ;; esac
    case "$_r" in (*"$_t"*) RE_F2="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) RE_F2="$_r"; _r="" ;; esac
    case "$_r" in (*"$_t"*) RE_F3="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) RE_F3="$_r"; _r="" ;; esac
    RE_F4="$_r"
}

# awk extractor: for each requirement block in a SPECIFICATION.md whose
# Spec-Owner line marks it 'child (delegated)' with a *.../SPECIFICATION.md
# pointer, emit:  id <TAB> name <TAB> status <TAB> pointer
extract() { # $1 = spec file
    awk '
        function trim(s){ sub(/^[ \t]+/,"",s); sub(/[ \t]+$/,"",s); return s }
        /^####[ \t]/ {
            line=$0
            sub(/^####[ \t]+/,"",line)
            sub(/\*\([^)]*\)\*[ \t]*$/,"",line)          # strip *(reconciled)* suffix
            curstatus=""
            if (match(line, /\[[A-Za-z_]+\][ \t]*$/)) {
                curstatus=substr(line, RSTART+1, RLENGTH-1)
                sub(/\].*$/,"",curstatus)
                line=substr(line, 1, RSTART-1)
            }
            line=trim(line)
            ci=index(line, ":")
            if (ci>0){ curid=trim(substr(line,1,ci-1)); curname=trim(substr(line,ci+1)) }
            else { curid=trim(line); curname="" }
            curhave=0
            next
        }
        /^(##|###)[ \t]/ { curid=""; next }              # left block scope
        /[Ss]pec-[Oo]wner/ {
            if (curid!="" && curhave==0 && $0 ~ /child[ \t]*\(delegated\)/) {
                if (match($0, /[A-Za-z0-9_.\/-]+SPECIFICATION\.md/)) {
                    ptr=substr($0, RSTART, RLENGTH)
                    printf "%s\t%s\t%s\t%s\n", curid, curname, curstatus, ptr
                    curhave=1
                }
            }
        }
    ' "$1"
}

map_status() { # $1 = declared status (store assumed present) -> elaboration
    case "$1" in
        CHARTER)     echo charter ;;
        ELABORATED)  echo elaborated ;;
        IN_PROGRESS) echo in_progress ;;
        DONE)        echo done ;;
        *)           printf '%s' "$1" | tr 'A-Z' 'a-z' ;;
    esac
}

# Worklist recursion over stores (root -> delegated children -> grandchildren).
declare -a QUEUE_FILE QUEUE_DOMAIN
QUEUE_FILE=("$ROOT_SPEC"); QUEUE_DOMAIN=("")
SEEN=""   # bash-3.2-safe newline-delimited visited-set (no associative arrays)
nodes_json=""
t_total=0; t_charter=0; t_elab=0; t_inprog=0; t_done=0; t_missing=0
qi=0

while [ "$qi" -lt "${#QUEUE_FILE[@]}" ]; do
    spec="${QUEUE_FILE[$qi]}"
    qi=$((qi+1))
    [ -f "$spec" ] || continue
    printf '%s\n' "$SEEN" | grep -Fxq -- "$spec" && continue
    SEEN="${SEEN}${spec}"$'\n'
    spec_dir="$(dirname "$spec")"
    while IFS= read -r _re_rec; do
        [ -z "$_re_rec" ] && continue
        _re_split_tsv "$_re_rec"
        id="$RE_F1"; name="$RE_F2"; status="$RE_F3"; ptr="$RE_F4"
        [ -z "$id" ] && continue
        store="$spec_dir/$ptr"
        store="$(printf '%s' "$store" | sed 's#//*#/#g; s#/\./#/#g')"
        domain="${store#$SPECS_DIR/}"; domain="${domain%/SPECIFICATION.md}"
        if [ -f "$store" ]; then
            elab="$(map_status "$status")"
            exists="true"
        else
            elab="missing"
            exists="false"
        fi
        t_total=$((t_total+1))
        case "$elab" in
            charter)     t_charter=$((t_charter+1)) ;;
            elaborated)  t_elab=$((t_elab+1)) ;;
            in_progress) t_inprog=$((t_inprog+1)) ;;
            done)        t_done=$((t_done+1)) ;;
            missing)     t_missing=$((t_missing+1)) ;;
        esac
        node="{\"id\":\"$(esc "$id")\",\"domain\":\"$(esc "$domain")\",\"status\":\"[$(esc "$status")]\",\"store\":\"$(esc "$store")\",\"store_exists\":$exists,\"elaboration\":\"$(esc "$elab")\"}"
        if [ -z "$nodes_json" ]; then nodes_json="$node"; else nodes_json="$nodes_json,$node"; fi
        if [ "$exists" = "true" ]; then
            QUEUE_FILE+=("$store"); QUEUE_DOMAIN+=("$domain")
        fi
    done < <(extract "$spec")
done

if [ "$t_total" -eq 0 ]; then
    emit_absent "\"$(esc "$ROOT_SPEC")\""
    exit 0
fi

briefing=""
if [ "$t_missing" -gt 0 ]; then
    briefing="$t_missing delegated node(s) MISSING their sub-spec store (broke); $t_charter charter, $t_elab elaborated, $t_inprog in_progress, $t_done done."
fi

printf '{"status":"ok","applicable":true,"spec_root":"%s","tree":[%s],"summary":{"total":%d,"charter":%d,"elaborated":%d,"in_progress":%d,"done":%d,"missing":%d},"briefing":"%s"}\n' \
    "$(esc "$ROOT_SPEC")" "$nodes_json" "$t_total" "$t_charter" "$t_elab" "$t_inprog" "$t_done" "$t_missing" "$(esc "$briefing")"
exit 0
