#!/usr/bin/env bash
# retirement-candidates oracle (Unix) -- CTXY-04, DEC-226
#
# Verification Oracle (kind: verification). Answers: "which passages in this
# project's living durable artifacts carry a deterministic signal that they may
# no longer earn their place?"
#
# This is the CONTENT-level sibling of SWEEP-02 (planning-doc-staleness), which
# works at whole-file granularity. The bloat this targets lives INSIDE files that
# must themselves stay alive -- a 160 KB follow-ups file that every session is
# instructed to read, 39% of which is narration of finished work. No whole-file
# sweep will ever touch that.
#
# CRITICAL CONTRACT: the output is a WORKLIST, never a delete list. Every signal
# here is a proxy. A `DONE` marker measures that somebody typed DONE, not that
# the surrounding text stopped mattering -- and a trap note that prevents a
# repeat looks identical to a war story that does not. The agent judges each
# candidate against `segments/_retirement-test.md`. The oracle never deletes and
# never recommends deletion.
#
# Six signals:
#   exit-condition-satisfied  a declared exit condition whose ISO date has passed
#   exit-condition-declared   an exit condition present but not machine-evaluable
#   done-marker               a section carrying DONE/FIXED/RESOLVED/SUPERSEDED/...
#   self-supersession         a file whose head invalidates its own body
#   correction-strata         >=2 CORRECTION/UPDATE/PROGRESS layers (CTXY-08 shape)
#   provisional-no-exit       provisional-by-construction entry with no exit condition
#   no-inbound-refs           (file level) basename referenced nowhere else
#
# ADVISORY: status is "pass" or "warn"; a real run ALWAYS exits 0. Retirement is
# a judgment lane -- blocking a commit on a proxy signal would be exactly the
# confidently-wrong mechanism this framework treats as worse than none.
# `--self-test` asserts each detector fires on a synthetic positive and stays
# quiet on a synthetic negative (exit 1 if not).
set -u

MAX_FILES=200
MAX_FILE_BYTES=1048576
MAX_CANDIDATES=400
EXCERPT_CHARS=120

# Default corpus: living artifacts that accumulate provisional content. Roots are
# repo-relative; missing ones are skipped silently. Override for tests with
# CLAUDE_RETIREMENT_CORPUS (colon-separated globs).
#
# The follow-ups file gets THREE globs, not one. Dogfooding on a second project
# found an 82 KB `docs/pending-followups.md` -- 41 entries, the exact artifact
# class this oracle exists for -- invisible because the literal path did not
# match. Shell globs are case-sensitive, so kebab/snake/upper spellings each
# need their own pattern. A miss here is silent and reads as `pass`.
DEFAULT_CORPUS='CLAUDE.md
docs/PENDING_FOLLOW_UPS.md
docs/*[Ff]ollow*[Uu]p*.md
docs/*FOLLOW*UP*.md
docs/specs/DISCOVERIES.md
docs/specs/OPEN_QUESTIONS.md
docs/planning/deferred/*.md
docs/pending/*.md
docs/reviews/*.md
NEXT_SESSION*.md
docs/NEXT_SESSION*.md'

# Files whose content is provisional BY CONSTRUCTION -- an entry here with no
# declared exit condition is a CTXY-07 authoring gap, not a judgment call.
# Spelling-tolerant for the same reason as the corpus globs above.
PROVISIONAL_RE='(docs/planning/deferred/|docs/pending/|[Pp][Ee][Nn][Dd][Ii][Nn][Gg][-_ ]?[Ff][Oo][Ll][Ll][Oo][Ww][-_ ]?[Uu][Pp]|NEXT_SESSION)'

today="$(date -u +%Y-%m-%d)"

# ---------------------------------------------------------------- scan_file
# Emits one TSV record per candidate:
#   path \t line_start \t line_end \t signal \t heading \t excerpt
#
# ONE awk invocation over the WHOLE corpus, never one per file. Process spawn on
# Windows/MINGW costs ~150-250ms, so a 74-file corpus paid ~15s in fork overhead
# alone under the obvious per-file shape. Per-file state resets on FNR==1 and the
# END-of-file work is done by comparing FILENAME against the previous record's.
scan_all() {
  local today="$1" prov="$2" exc="$3"; shift 3
  [ "$#" -eq 0 ] && return 0
  awk -v TODAY="$today" -v PROV="$prov" -v EXC="$exc" '
    function jesc(s) {
      gsub(/\\/, "\\\\", s); gsub(/"/, "\\\"", s)
      gsub(/\t/, " ", s);    gsub(/\r/, "", s)
      return s
    }
    function trunc(s) { return (length(s) > EXC) ? substr(s, 1, EXC) "..." : s }
    function emit(fn, ls, le, sig, head, exc) {
      printf "%s\t%d\t%d\t%s\t%s\t%s\n", fn, ls, le, sig, jesc(head), jesc(trunc(exc))
    }
    # Close the open block, deciding its signals.
    function flush(endline,   sig) {
      if (bstart == 0) return
      if (b_exit_sat)        emit(CURF, bstart, endline, "exit-condition-satisfied", bhead, b_exit_txt)
      else if (b_exit_decl)  emit(CURF, bstart, endline, "exit-condition-declared",  bhead, b_exit_txt)
      if (b_done)            emit(CURF, bstart, endline, "done-marker",              bhead, b_done_txt)
      if (b_corr >= 2)       emit(CURF, bstart, endline, "correction-strata",        bhead, b_corr_txt)
      bstart = 0; b_exit_sat = 0; b_exit_decl = 0; b_done = 0; b_corr = 0
      b_exit_txt = ""; b_done_txt = ""; b_corr_txt = ""
    }
    # End-of-file work for CURF, using the line count it ended on.
    function closefile(lastline) {
      if (CURF == "") return
      flush(lastline)
      # provisional-no-exit is a FILE-level verdict, not a per-heading one. A
      # deferred brief is one artifact with many headings; firing per heading
      # turned a 74-file scan into 312 "candidates" -- noise in a worklist
      # costume. One artifact, one authoring gap.
      if (CURF ~ PROV) { is_prov_file = 1; if (!prov_line) { prov_line = 1; prov_head = "(whole file)" } }
      if (is_prov_file && !file_has_exit)
        emit(CURF, prov_line, lastline, "provisional-no-exit", prov_head, "provisional artifact with no declared exit condition (CTXY-07)")
      if (supersede_seen && lastline - supersede_line >= 40)
        emit(CURF, supersede_line, lastline, "self-supersession", "(file head)", supersede_txt)
      bstart = 0; incode = 0; file_has_exit = 0; is_prov_file = 0
      prov_line = 0; prov_head = ""; supersede_seen = 0; supersede_line = 0; supersede_txt = ""
    }
    BEGIN { bstart = 0; incode = 0; file_has_exit = 0; is_prov_file = 0; CURF = "" }

    # New file: close the previous one out, then adopt this one.
    FNR == 1 { closefile(PREVFNR); CURF = FILENAME }
    { PREVFNR = FNR }

    # Fenced code blocks are never prose -- their markers are examples, not claims.
    /^[ \t]*(```|~~~)/ { incode = !incode; next }
    incode { next }

    # ---- self-supersession: a supersession banner in the first 15 lines of a
    # file that continues for another 40+ lines. The banner invalidates a body
    # that is still sitting there being read.
    FNR <= 15 && /(^|[^A-Za-z])(SUPERSEDED|TOMBSTONE|OBSOLETE|DEPRECATED)([^A-Za-z]|$)/ {
      if (!supersede_seen) { supersede_seen = 1; supersede_line = FNR; supersede_txt = $0 }
    }
    /kept for (provenance|lineage|history)/ {
      if (!supersede_seen) { supersede_seen = 1; supersede_line = FNR; supersede_txt = $0 }
    }

    # ---- block boundaries (markdown headings, ANY level 1-6).
    #
    # Levels 2-4 was the original range and it produced a SILENT FALSE PASS: a
    # file whose only heading is a level-1 title never opens a block, `bstart`
    # stays 0, every content line hits the `bstart == 0 { next }` guard below,
    # and an 82 KB follow-ups file scans to zero candidates and reports `pass`.
    # Verified on a second project, 2026-08-02: the same file with `#` changed to
    # `##` yields done-marker + exit-condition-satisfied. Heading DEPTH is a
    # formatting choice; it must not decide whether text is scanned at all.
    /^#{1,6}[ \t]/ {
      flush(FNR - 1)
      bstart = FNR; bhead = $0
      sub(/^#+[ \t]*/, "", bhead)
      # A "Pending Follow-Ups"-shaped section is provisional wherever it lives.
      if (bhead ~ /[Pp]ending [Ff]ollow-?[Uu]ps?/) { is_prov_file = 1; prov_line = FNR; prov_head = bhead }
      next
    }
    bstart == 0 { next }

    # ---- exit conditions. TWO STRENGTHS, and the distinction is load-bearing.
    #
    # STRONG = an actual directive ("Remove this entry once X"). Only a STRONG
    # match, carrying a past ISO date, outside a markdown table, can be reported
    # as SATISFIED -- i.e. as an actionable candidate.
    #
    # WEAK = a mere mention of the concept ("none of its four exit conditions
    # had fired"). A WEAK match can only ever be `declared`, which is surfaced
    # and never actionable.
    #
    # Why: without the split, prose ABOUT exit conditions that happens to carry
    # any date fires as satisfied. Dogfooding produced three such hits and all
    # three were false -- including the CLAUDE.md section describing THIS oracle.
    # DEFER-044 records the identical defect in `governing-doc-consistency` ("a
    # stale date appears anywhere on the line"), where the prescribed remedy
    # would have written a lie into the docs. The trap was documented before this
    # oracle existed; the guard lives here so the prose does not have to.
    #
    # NOTE for editors: this awk program is inside a single-quoted shell string.
    # An apostrophe anywhere in these comments terminates it and breaks the
    # script with a bash syntax error, not an awk one. Do not write possessives.
    #
    # Table rows are excluded from SATISFIED for the same reason: a `|`-delimited
    # row is reference data about conditions, not a declaration of one.
    /(^|[^A-Za-z])([Rr]emove|[Dd]elete|[Dd]rop|[Rr]etire|[Pp]rune)[^.]{0,60}(once|when|after|upon)/ {
      d = ""
      if (match($0, /[0-9]{4}-[0-9]{2}-[0-9]{2}/)) d = substr($0, RSTART, RLENGTH)
      file_has_exit = 1
      is_table = ($0 ~ /^[ \t]*\|/)
      if (d != "" && d < TODAY && !is_table) { b_exit_sat = 1; b_exit_txt = $0 }
      else if (!b_exit_sat)                  { b_exit_decl = 1; b_exit_txt = $0 }
      next
    }
    /([Ee]xit condition)|([Ss]uperseded when)/ {
      file_has_exit = 1
      if (!b_exit_sat && !b_exit_decl) { b_exit_decl = 1; b_exit_txt = $0 }
      next
    }

    # ---- done markers. Uppercase tokens and the check emoji only: lowercase
    # "done"/"fixed" appear in ordinary prose constantly and are pure noise.
    /(^|[^A-Za-z])(DONE|COMPLETE|COMPLETED|FIXED|RESOLVED|SUPERSEDED|DISCHARGED|SHIPPED|LANDED|CLOSED)([^A-Za-z]|$)/ {
      if (!b_done) { b_done = 1; b_done_txt = $0 }
      next
    }
    # POSIX awk has no \x escapes and reads \b as backspace -- the check emoji is
    # matched by its literal UTF-8 bytes via index(), and every word boundary
    # below is spelled out as a character class.
    index($0, "\342\234\205") > 0 { if (!b_done) { b_done = 1; b_done_txt = $0 } ; next }

    # ---- correction strata (CTXY-08): appended layers under a stale head.
    /^[ \t>*-]*[ \t]*(CORRECTION|UPDATE|PROGRESS|AMENDMENT|REVISION|ADDENDUM|POSTSCRIPT|POST-SCRIPT)([^A-Za-z]|$)/ {
      b_corr++
      if (b_corr_txt == "") b_corr_txt = $0
      next
    }
    END { closefile(PREVFNR) }
  ' "$@" 2>/dev/null
}

# ------------------------------------------------------------------ self-test
if [ "${1:-}" = "--self-test" ]; then
  td="$(mktemp -d)"; trap 'rm -rf "$td"' EXIT
  pos="$td/docs/pending"; mkdir -p "$pos"
  cat > "$pos/positive.md" <<'POS'
# Sample

## Entry one
Remove this entry once the installer ships after 2020-01-01.
Body text.

## Entry two
Status: DONE -- the migration landed.

## Entry three
Head statement.
**CORRECTION**: actually it was the other module.
**UPDATE**: reverted again.

## Entry four
Body with no exit condition of its own.
POS
  # provisional-no-exit is FILE-level, so the positive sample for it must be a
  # provisional file with NO exit condition anywhere -- positive.md declares one
  # in Entry one and correctly does not fire.
  cat > "$pos/no-exit.md" <<'NOEXIT'
# Deferred idea

## The idea
Ship a thing. Nothing here says what would make this brief deletable.
NOEXIT
  cat > "$td/clean.md" <<'NEG'
# Clean doc

## A real trap
Calling flush() before the lock is taken corrupts the index. There is no
regression test for this yet, so this paragraph is the guard.

## Why we rejected the queue
It serialized the writer, which is the whole point of the module.
NEG
  hits="$(scan_all "$today" "$PROVISIONAL_RE" "$EXCERPT_CHARS" "$pos/positive.md" "$pos/no-exit.md")"
  miss="$(scan_all "$today" "$PROVISIONAL_RE" "$EXCERPT_CHARS" "$td/clean.md")"
  need="exit-condition-satisfied done-marker correction-strata provisional-no-exit"
  missing=""
  for s in $need; do
    printf '%s\n' "$hits" | grep -q "	$s	" || missing="${missing}${s} "
  done
  noise="$(printf '%s\n' "$miss" | grep -c '[^[:space:]]' || true)"
  if [ -z "$missing" ] && [ "$noise" = "0" ]; then
    printf '{"status":"pass","details":{"self_test":true,"detectors_fired":true,"quiet_on_clean":true},"briefing":"Self-test PASS: all four block detectors fire on a synthetic positive and stay silent on a clean trap/rationale doc."}\n'
    exit 0
  fi
  printf '{"status":"fail","details":{"self_test":true,"missing_detectors":"%s","false_positives_on_clean":%s},"briefing":"Self-test FAIL: retirement detectors are silently broken."}\n' \
    "$missing" "$noise"
  exit 1
fi

# ---------------------------------------------------------------------- main
# Repo root. The three-level climb is correct for a PROJECT install
# (<proj>/.claude/oracles/<name>/run.sh -> <proj>) and for this repo's template
# copy (<repo>/claude-template/oracles/<name>/run.sh -> <repo>), and WRONG for
# the deployed global copy (~/.claude/oracles/<name>/run.sh -> $HOME), where it
# scanned the home directory and returned files_scanned:0 as a clean `pass`.
# A green that measured nothing is the failure class this framework treats as
# worse than no oracle at all, so resolve the CALLER's repo first and keep the
# climb only as the fallback. Measured 2026-08-09: deployed copy 0 files /
# status pass, template copy 195 files / 401 candidates, same cwd.
repo_root="$(git rev-parse --show-toplevel 2>/dev/null)"
[ -n "$repo_root" ] || repo_root="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$repo_root" || exit 0
start_ms=$(($(date +%s%N)/1000000))

corpus="${CLAUDE_RETIREMENT_CORPUS:-}"
if [ -n "$corpus" ]; then
  globs="$(printf '%s' "$corpus" | tr ':' '\n')"
else
  globs="$DEFAULT_CORPUS"
fi

raw=""; nraw=0; truncated=0
while IFS= read -r g; do
  [ -z "$g" ] && continue
  for f in $g; do
    [ -f "$f" ] || continue
    nraw=$((nraw + 1))
    # The cap is a cost bound, and silently hitting it under-reports the very
    # worklist this oracle exists to produce -- a shorter list reads exactly
    # like a cleaner corpus. Record it so truncation is LOUD; never raise the
    # cap to make a red go away (OVALID-05). Measured 2026-08-09: 195 of 200.
    [ "$nraw" -gt "$MAX_FILES" ] && { truncated=1; break 2; }
    raw="${raw}${f}
"
  done
done <<EOF
$globs
EOF

# Size filter in ONE wc call, not one per file (same fork-cost reason as the
# single-awk scan). `wc -c` on a list prints "<bytes> <path>" per line plus a
# "total" line when given more than one file.
files=""; nfiles=0
if [ -n "$raw" ]; then
  # shellcheck disable=SC2046
  sizes="$(printf '%s' "$raw" | grep -v '^$' | tr '\n' '\0' | xargs -0 wc -c 2>/dev/null)"
  files="$(printf '%s\n' "$sizes" | awk -v cap="$MAX_FILE_BYTES" '
    { n = $1; $1 = ""; sub(/^ /, "")
      if ($0 == "total" || $0 == "") next
      if (n + 0 <= cap) print }')"
  nfiles="$(printf '%s\n' "$files" | grep -c '[^[:space:]]' || true)"
fi

# Inbound-reference probe -- ONE pass over the whole tree, not one per file.
# The per-file shape measured 30s on this repo (74 x git grep); a single
# multi-pattern grep plus an awk fold measures under 2s. An oracle consulted
# from a briefing budget cannot afford the naive shape.
#
# Result: INBOUND_YES, a newline-delimited set of basenames that some OTHER file
# mentions. A name absent from it has zero inbound references.
INBOUND_YES=""
build_inbound_index() {
  local names="$1"
  [ -z "$names" ] && return 0
  local pat_args=""
  while IFS= read -r n; do
    [ -z "$n" ] && continue
    pat_args="$pat_args -e $n"
  done <<EOF
$(printf '%s' "$names" | sort -u)
EOF
  [ -z "$pat_args" ] && return 0

  local hits
  if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    # shellcheck disable=SC2086
    hits="$(git grep -I -F -n $pat_args -- . 2>/dev/null)"
  else
    # shellcheck disable=SC2086
    hits="$(grep -rIn -F --exclude-dir=.git --exclude-dir=node_modules $pat_args . 2>/dev/null | sed 's|^\./||')"
  fi

  INBOUND_YES="$(printf '%s\n' "$hits" | awk -v NAMES="$names" '
    BEGIN {
      n = split(NAMES, arr, "\n")
      for (i = 1; i <= n; i++) if (arr[i] != "") want[arr[i]] = 1
    }
    {
      # "path:lineno:content" -- the path is everything before the first colon
      # that is followed by digits and another colon.
      p = $0
      if (match(p, /^[^:]*:[0-9]+:/)) { path = substr(p, 1, RSTART + RLENGTH - 1) } else next
      sub(/:[0-9]+:$/, "", path)
      base = path; sub(/^.*\//, "", base)
      for (nm in want) if (nm != base && index($0, nm) > 0) seen[nm] = 1
    }
    END { for (nm in seen) print nm }
  ')"
}
# NOTE: no per-file has_inbound() helper. Calling one costs a basename+grep pair
# of subshells per file, which on a 74-file corpus was ~9s of pure fork overhead
# -- two thirds of the oracle's runtime, for a set-membership test. The whole
# question is answered once, below, as a set difference.

cand_json=""; ncand=0
surf_json=""; nsurf=0
declare_counts=""

# One tree-wide pass, before the per-file loop.
basenames="$(printf '%s' "$files" | sed 's|^.*/||' | grep -v '^$' | sort -u)"
build_inbound_index "$basenames"

# Block-level signals: ONE awk over the whole corpus.
# shellcheck disable=SC2046
scan_out="$(scan_all "$today" "$PROVISIONAL_RE" "$EXCERPT_CHARS" $(printf '%s' "$files" | grep -v '^$' | tr '\n' ' '))"

# DEFER-083 / DEC-279: `IFS=$'\t' read` COLLAPSES empty fields (tab is IFS
# whitespace). `head` is an empty-defaultable MIDDLE field -- empty whenever a
# candidate block precedes ANY heading in its file -- so the EXCERPT rendered in
# the `heading` JSON field and `excerpt` rendered empty. Advisory worklist output
# only (the candidate is still found, counted and reported), which is why this
# was Tier 3 and deliberately parked; it is fixed here because the field is
# machine-read by consumers of the emitted JSON.
#
# Empty-preserving split, bash-3.2 safe (no arrays/readarray/namerefs). The awk
# producer emits jesc()'d values that cannot contain a literal tab, so a tab is
# always a field boundary. The `${r#*<tab>}` strip is GUARDED -- that expansion
# returns the WHOLE string when no tab is present (DEC-232, same class).
_rc_split_tsv() {
  local _r="$1" _t
  _t=$(printf '\t')
  case "$_r" in (*"$_t"*) RC_F1="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) RC_F1="$_r"; _r="" ;; esac
  case "$_r" in (*"$_t"*) RC_F2="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) RC_F2="$_r"; _r="" ;; esac
  case "$_r" in (*"$_t"*) RC_F3="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) RC_F3="$_r"; _r="" ;; esac
  case "$_r" in (*"$_t"*) RC_F4="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) RC_F4="$_r"; _r="" ;; esac
  case "$_r" in (*"$_t"*) RC_F5="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) RC_F5="$_r"; _r="" ;; esac
  RC_F6="$_r"
}
while IFS= read -r _rc_rec; do
  _rc_split_tsv "$_rc_rec"
  p="$RC_F1"; ls="$RC_F2"; le="$RC_F3"; sig="$RC_F4"; head="$RC_F5"; exc="$RC_F6"
  [ -z "${sig:-}" ] && continue
  if [ "$sig" = "exit-condition-declared" ]; then
    nsurf=$((nsurf + 1))
    surf_json="${surf_json:+$surf_json,}{\"path\":\"$p\",\"line\":$ls,\"signal\":\"$sig\",\"heading\":\"$head\",\"excerpt\":\"$exc\",\"note\":\"exit condition present but not machine-evaluable -- verify against the system, not against the entry's own claim\"}"
    continue
  fi
  ncand=$((ncand + 1))
  [ "$ncand" -gt "$MAX_CANDIDATES" ] && break
  cand_json="${cand_json:+$cand_json,}{\"path\":\"$p\",\"line_start\":$ls,\"line_end\":$le,\"signal\":\"$sig\",\"heading\":\"$head\",\"excerpt\":\"$exc\"}"
  declare_counts="${declare_counts}${sig}
"
done <<EOF
$scan_out
EOF

# File-level: zero inbound references anywhere in the tree. One awk fold over
# (corpus paths) x (referenced basenames) -- no subshell per file.
orphans="$(printf '%s\n' "$files" | awk -v REFD="$INBOUND_YES" '
  BEGIN { n = split(REFD, r, "\n"); for (i = 1; i <= n; i++) if (r[i] != "") refd[r[i]] = 1 }
  /^[ \t]*$/ { next }
  $0 == "CLAUDE.md" { next }              # read by convention, not by reference
  $0 ~ /^docs\/specs\// { next }          # ditto
  { base = $0; sub(/^.*\//, "", base); if (!(base in refd)) print $0 }')"

while IFS= read -r f; do
  [ -z "$f" ] && continue
  ncand=$((ncand + 1))
  [ "$ncand" -gt "$MAX_CANDIDATES" ] && break
  cand_json="${cand_json:+$cand_json,}{\"path\":\"$f\",\"line_start\":1,\"line_end\":0,\"signal\":\"no-inbound-refs\",\"heading\":\"(whole file)\",\"excerpt\":\"no other tracked file mentions this filename\"}"
  declare_counts="${declare_counts}no-inbound-refs
"
done <<EOF
$orphans
EOF

by_signal=""
if [ -n "$declare_counts" ]; then
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    s="${line%% *}"; c="${line##* }"
    by_signal="${by_signal:+$by_signal,}\"$s\":$c"
  done <<EOF
$(printf '%s' "$declare_counts" | grep -v '^$' | sort | uniq -c | awk '{print $2" "$1}')
EOF
fi

dur=$(( $(($(date +%s%N)/1000000)) - start_ms ))

trunc_note=""
if [ "$truncated" -eq 1 ]; then
  trunc_note=" TRUNCATED: the corpus hit the ${MAX_FILES}-file scan cap, so this worklist is a FLOOR, not a census -- files past the cap were never opened. Narrow the corpus (CLAUDE_RETIREMENT_CORPUS) or archive terminal artifacts; do not read a shorter list as a cleaner one."
fi

if [ "$ncand" -eq 0 ]; then
  # A truncated scan that found nothing has not established anything, so it must
  # not render as an unqualified clean pass.
  if [ "$truncated" -eq 1 ]; then
    status=warn
    briefing="RETIREMENT: 0 candidates, but the scan was capped at $MAX_FILES files.$trunc_note"
  else
    status=pass
    briefing=""
  fi
else
  status=warn
  briefing="RETIREMENT: $ncand candidate passage(s) across $nfiles living artifact(s) carry a deterministic retirement signal. These are a WORKLIST, not a delete list -- judge each against segments/_retirement-test.md. Acted on by /0-uldf-finalize Phase 8.8 and /1-uldf-finalize Phase 3.5; sweep with /0-uldf-context-audit --retire.$trunc_note"
fi

printf '{"status":"%s","details":{"files_scanned":%d,"candidate_count":%d,"surfaced_count":%d,"truncated":%s,"scan_cap":%d,"by_signal":{%s},"scan_duration_ms":%d,"contract":"worklist -- the oracle never deletes and never recommends deletion; every signal is a proxy for utility, which is not machine-visible"},"candidates":[%s],"surfaced":[%s],"briefing":"%s"}\n' \
  "$status" "$nfiles" "$ncand" "$nsurf" "$([ "$truncated" -eq 1 ] && echo true || echo false)" "$MAX_FILES" "$by_signal" "$dur" "$cand_json" "$surf_json" "$briefing"

exit 0
