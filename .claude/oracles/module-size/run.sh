#!/usr/bin/env bash
# module-size oracle (Unix)
#
# Verification Oracle (kind: verification). The deterministic module-sizing
# detection surface the framework's own doctrines mandate -- Enforcement
# Placement Principle (detection is pure arithmetic => it belongs in code, not
# prose) + Oraculurgy Part 11 -- and that the scrutiny found conspicuously
# absent (P0-7: trajectory-cap ships a deterministic size-cap oracle for ONE
# markdown file while the doctrinal per-module token cap had none).
#
# What it does: estimates each ULADP module's token weight (byte-based, no file
# parsing) and reports modules OVER a soft band (default 4000 tokens, M2). A
# "module" = a directory bearing a README.md (the ULADP module unit); a
# directory WITHOUT a README but with >= 3 code files is reported in no_readme
# (a possible undocumented module -- surfaced, never skipped). Token estimate =
# sum(direct-file bytes)/4 -- one divisor everywhere, mirroring the existing
# convention in segments/-uladp/compliance_architecture.md (~:49).
#
# ADVISORY (M2, per the trajectory-cap precedent and the report's detection-
# first posture): over-band modules produce status "warn" and the script ALWAYS
# exits 0 on a real run. A hard cap invites split-for-the-metric micro-modules
# (perverse incentive, noted in README) -- this is a tripwire that says "look
# here", not a gate. Exit non-zero is reserved for execution error.
#
# NO-DATA honesty: not-a-readable-project-root / no analyzable files => status
# "no-data" (never a silent "pass"); unreadable paths land in no_data w/ reason.
#
# Output: single JSON object (schema frozen in README.md). git ls-tree fast path
# (tracked-only => respects .gitignore; both shells parse identical git output
# for byte-equivalent parity); find --printf fallback for non-git trees.
set -u

# ---- args -------------------------------------------------------------------
# --root <dir>  scan a specific tree (default: repo root three levels up).
#               Load-bearing for the smoke's synthetic-fixture legs.
ROOT_OVERRIDE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --root) ROOT_OVERRIDE="${2:-}"; shift 2 ;;
    --root=*) ROOT_OVERRIDE="${1#--root=}"; shift ;;
    *) shift ;;
  esac
done

# ---- band (soft, M2): overridable via .claude/config.json moduleSize.softBandTokens
DEFAULT_BAND=4000

# ---- excluded path segments (never modules; also cheap non-git guard) -------
# Directories whose contents are build output, dependencies, or framework
# runtime state -- not ULADP modules. git ls-tree already drops .gitignore'd
# paths; this belt-and-suspenders catches tracked-but-not-a-module dirs.
is_excluded() {
  case "/$1/" in
    */.git/*|*/node_modules/*|*/dist/*|*/build/*|*/coverage/*|*/__pycache__/*) return 0 ;;
    */.next/*|*/.expo/*|*/target/*|*/.cargo/*|*/vendor/*) return 0 ;;
    */.claude/collaboration/*|*/.claude/session-state/*) return 0 ;;
  esac
  return 1
}

emit_no_data() { # $1 = reason
  local r; r=$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')
  printf '{"status":"no-data","modules_scanned":0,"over_band":[],"over_band_total":0,"no_readme":[],"no_readme_total":0,"no_data":[{"path":".","reason":"%s"}],"no_data_total":1,"band":{"softTokens":%d,"source":"default"},"enum_mode":"none","scan_duration_ms":0,"briefing":"module-size: NO-DATA (%s) -- could not analyze; not a silent pass."}\n' \
    "$r" "$DEFAULT_BAND" "$r"
  exit 0
}

# ---- resolve root -----------------------------------------------------------
if [ -n "$ROOT_OVERRIDE" ]; then
  ROOT="$ROOT_OVERRIDE"
else
  ROOT="$(cd "$(dirname "$0")/../../.." 2>/dev/null && pwd)"
fi
[ -n "$ROOT" ] && [ -d "$ROOT" ] && [ -r "$ROOT" ] || emit_no_data "project root not readable"

start_ms=$(($(date +%s%N)/1000000))

# ---- band from config -------------------------------------------------------
BAND=$DEFAULT_BAND
BAND_SOURCE="default"
CFG="$ROOT/.claude/config.json"
if [ -f "$CFG" ] && command -v jq >/dev/null 2>&1; then
  _b="$(jq -r '.moduleSize.softBandTokens // empty' "$CFG" 2>/dev/null)"
  case "$_b" in
    ''|*[!0-9]*) : ;;                       # absent / non-numeric -> keep default
    *) BAND="$_b"; BAND_SOURCE="config" ;;
  esac
fi

# ---- enumerate files as "SIZE\tPATH" (repo-relative, forward-slash) ---------
ERRFILE="$(mktemp 2>/dev/null || echo "/tmp/msz.$$")"
LISTFILE="$(mktemp 2>/dev/null || echo "/tmp/mszl.$$")"
: > "$ERRFILE"; : > "$LISTFILE"

ENUM_MODE=""
if git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1 \
   && git -C "$ROOT" rev-parse --verify HEAD >/dev/null 2>&1; then
  # Fast path: sizes come straight from the tree object -- ONE git process, no
  # per-file stat. Reflects HEAD (committed tracked state); uncommitted working-
  # tree deltas are not reflected -- acceptable for an advisory size tripwire.
  ENUM_MODE="git"
  git -C "$ROOT" ls-tree -r -l HEAD 2>>"$ERRFILE" \
    | awk -F'\t' 'NF==2 { n=split($1,f," "); if (n>=4 && f[4] ~ /^[0-9]+$/) printf "%s\t%s\n", f[4], $2 }' \
    > "$LISTFILE"
else
  # Fallback: filesystem walk (non-git tree, or git repo with no commit yet).
  # find --printf is ONE process (no per-file stat spawn); prunes the big dirs.
  ENUM_MODE="find"
  ( cd "$ROOT" && find . \
      \( -name .git -o -name node_modules -o -name dist -o -name build \
         -o -name coverage -o -name __pycache__ -o -name .next -o -name .expo \
         -o -name target -o -name .cargo -o -name vendor \) -prune -o \
      -type f -printf '%s\t%p\n' 2>>"$ERRFILE" ) \
    | sed 's/\t\.\//\t/' > "$LISTFILE"
fi

if [ ! -s "$LISTFILE" ]; then
  rm -f "$LISTFILE"
  # If find/ls-tree wrote only errors (unreadable tree) that is still NO-DATA.
  rm -f "$ERRFILE"
  emit_no_data "no analyzable files under root"
fi

# ---- aggregate + classify + build JSON (all inside awk; ONE process) --------
# Per-entry sed/subshell spawns are catastrophically slow on Git Bash
# (~100ms each => tens of seconds), so ALL string escaping, exclusion, sizing,
# capping and JSON-fragment assembly happens in awk. Pipeline is two processes:
# aggregator awk -> sort (by est desc, so the cap keeps the worst) -> builder
# awk. The builder emits five \x1f-delimited fields; bash reads them once.
CAP=50
US="$(printf '\037')"   # ASCII unit separator: field delimiter (never in paths)

BUILT="$(awk -F'\t' '
  function dirof(p,   i){ i=length(p); while(i>0 && substr(p,i,1)!="/") i--; return (i==0)?".":substr(p,1,i-1) }
  function base(p,   i){ i=length(p); while(i>0 && substr(p,i,1)!="/") i--; return substr(p,i+1) }
  NF==2 {
    size=$1+0; p=$2
    if (p=="") next
    d=dirof(p); b=base(p)
    bytes[d]+=size; seen[d]=1
    if (b=="README.md") readme[d]=1
    if (b ~ /\.(sh|ps1|ts|tsx|js|jsx|mjs|cjs|py|go|rs|java|cs|cpp|cc|c|h|hpp|rb|php|swift|kt|scala|lua|pl|r)$/) code[d]++
  }
  END{ for (d in seen) printf "%s\t%d\t%d\t%d\n", d, bytes[d]+0, readme[d]+0, code[d]+0 }
' "$LISTFILE" \
  | sort -t"$(printf '\t')" -k2,2nr \
  | awk -F'\t' -v band="$BAND" -v cap="$CAP" -v US="$US" '
  function esc(s){ gsub(/\\/,"\\\\",s); gsub(/"/,"\\\"",s); return s }
  function excluded(d){
    if (d ~ /(^|\/)(\.git|node_modules|dist|build|coverage|__pycache__|\.next|\.expo|target|\.cargo|vendor)(\/|$)/) return 1
    if (d ~ /(^|\/)\.claude\/(collaboration|session-state)(\/|$)/) return 1
    return 0
  }
  {
    d=$1; bytes=$2+0; hasreadme=$3+0; codec=$4+0
    if (d=="" || excluded(d)) next
    est=int(bytes/4)
    if (hasreadme==1) {
      modules++
      if (est>band) {
        overtotal++
        if (overtotal<=cap)
          overjson = overjson (overjson=="" ? "" : ",") "{\"path\":\"" esc(d) "\",\"est_tokens\":" est ",\"band\":" band "}"
      }
    } else if (codec>=3) {
      nrtotal++
      if (nrtotal<=cap)
        nrjson = nrjson (nrjson=="" ? "" : ",") "{\"path\":\"" esc(d) "\",\"est_tokens\":" est ",\"code_files\":" codec "}"
    }
  }
  END{ printf "%d%s%d%s%d%s%s%s%s\n", modules+0, US, overtotal+0, US, nrtotal+0, US, overjson, US, nrjson }
')"

IFS="$US" read -r MODULES OVER_TRUNC NOREADME_TRUNC OVER_JSON NOREADME_JSON <<EOF
$BUILT
EOF
MODULES="${MODULES:-0}"; OVER_TRUNC="${OVER_TRUNC:-0}"; NOREADME_TRUNC="${NOREADME_TRUNC:-0}"

# ---- no_data from unreadable paths (best-effort; find/ls-tree stderr) -------
# esc() here runs only for the (usually empty) error set and the fixed briefing
# string -- not per module -- so its sed spawns don't threaten the speed budget.
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
NODATA_JSON=""; NODATA_TRUNC=0
if [ -s "$ERRFILE" ]; then
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    fp="$(printf '%s' "$line" | sed -E "s/^(find|wc|xargs|fatal)[^:]*: //; s/: [^:]*$//; s/^['\"]//; s/['\"]$//")"
    reason="$(printf '%s' "$line" | sed -E 's/.*: ([^:]*)$/\1/')"
    [ -z "$fp" ] && fp="(unknown)"
    [ -z "$reason" ] && reason="unreadable"
    if [ "$NODATA_TRUNC" -lt "$CAP" ]; then
      NODATA_JSON="${NODATA_JSON:+$NODATA_JSON,}{\"path\":\"$(esc "$fp")\",\"reason\":\"$(esc "$reason")\"}"
    fi
    NODATA_TRUNC=$(( NODATA_TRUNC + 1 ))
  done < "$ERRFILE"
fi
rm -f "$ERRFILE" "$LISTFILE"

dur=$(( $(($(date +%s%N)/1000000)) - start_ms ))

# ---- status + briefing ------------------------------------------------------
if [ "$OVER_TRUNC" -gt 0 ]; then
  status="warn"
  if [ "$OVER_TRUNC" -eq 1 ]; then noun="module"; else noun="modules"; fi
  briefing="module-size: $OVER_TRUNC $noun over the ~${BAND}-token soft band (advisory; est=bytes/4). Consider decomposition or run /0-uldf-uladp-compliance --architecture. Soft band, not a cliff (M2)."
else
  status="pass"
  briefing=""
fi
briefing_esc="$(esc "$briefing")"

printf '{"status":"%s","modules_scanned":%d,"over_band":[%s],"over_band_total":%d,"no_readme":[%s],"no_readme_total":%d,"no_data":[%s],"no_data_total":%d,"band":{"softTokens":%d,"source":"%s"},"enum_mode":"%s","scan_duration_ms":%d,"briefing":"%s"}\n' \
  "$status" "$MODULES" "$OVER_JSON" "$OVER_TRUNC" "$NOREADME_JSON" "$NOREADME_TRUNC" \
  "$NODATA_JSON" "$NODATA_TRUNC" "$BAND" "$BAND_SOURCE" "$ENUM_MODE" "$dur" "$briefing_esc"

# Advisory oracle: always exit 0 on a real run (warn does not block).
exit 0
