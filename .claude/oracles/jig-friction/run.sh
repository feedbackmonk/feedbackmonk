#!/bin/bash
# jig-friction oracle (Unix) — JIG-04, DEC-142.
#
# The "manufactured boredom" signal of the Development Jigs arc: a deterministic
# detector over the EXISTING command-usage telemetry that spots GRINDING
# signatures — an agent hand-repeating structurally-similar work within one
# session instead of building a jig (Fabricator, scenario-replayer, oracle …).
# It surfaces each cluster as an advisory, evidence-carrying briefing line.
#
# ADVISORY ONLY — never blocks a commit or a session (DEC-142). It informs.
#
# Substrate (NO new store, RECENCY precedent): the same JSONL invocation records
# written by hooks/command-usage-tracker.{sh,ps1} — one per Skill/Task/MCP call:
#   {"at":ISO,"cmd":NAME,"type":"skill|agent|mcp","session":ID,"project":P, ...}
# NOTE: command-usage records the invocation NAME (e.g. "mcp:playwright/
# browser_take_screenshot"), not a raw Bash command line — so the canonical
# grinding signature is N identical/similar NAMED invocations in one session
# (screenshot-loop, repeated navigate+evaluate, repeated same skill/subaction).
#
# HIGH-THRESHOLD BIAS (EPP-02 bounded-false-positive applied to the recognizer
# itself): a nagging detector is attention pollution — the resource Oraculurgy
# defends. Prefer missed-detection over false-fire; the finalize retrospective
# jig audit (JIG-05) is the backstop for what this misses. Hence minRepeats
# defaults HIGH (5) and the negative corpus MUST stay silent.
#
# Scope (the "session window"): resolution first-hit-wins —
#   1. --session <id>          (explicit; used by the smoke)
#   2. $CLAUDE_SESSION_ID       (the CURRENT live session — production briefing)
#   3. most-recent session in the data (deterministic fallback; single-session)
# windowMinutes (config, default 0 = whole session) optionally tightens scope to
# the last N minutes of that session's activity.
#
# Data directory resolution (first hit wins) — friction is a LIVE-session
# question, so the machine's live registry is PRIMARY (diverges deliberately
# from review-recency, whose historical question reads ./claude-usage first):
#   1. --dir <path>
#   2. ~/.claude/command-usage   (this machine's live registry)
#   3. ./claude-usage            (repo cross-machine aggregate — dogfood fallback)
#
# Output: single-line JSON, the FROZEN contract (GUIDE § 6.1):
#   {"status":"ok|signal",
#    "signals":[{"pattern":<shape sig>,"count":N,
#                "evidence":[<verbatim invocations>],
#                "suggestedArchetype":<slug from the frozen catalog list>}],
#    "briefing":<string>}
#   INVARIANT: status:"ok"  =>  briefing == ""   (quiet-path; the briefing
#   fan-out suppresses empty briefings automatically — RECENCY-04 precedent,
#   no session-start hook edit needed).
#
# Graceful absence: missing jq / missing registry / any internal failure ->
# emit the quiet JSON and exit 0 (NO-DATA is surfaced to stderr for on-demand
# visibility; a briefing oracle must NEVER crash the session-start fan-out).

set -uo pipefail

DIR=""
SESSION=""
MIN_REPEATS=""
WINDOW_MINUTES=""

while [ $# -gt 0 ]; do
    case "$1" in
        --dir)             DIR="${2:-}"; shift 2 ;;
        --dir=*)           DIR="${1#--dir=}"; shift ;;
        --session)         SESSION="${2:-}"; shift 2 ;;
        --session=*)       SESSION="${1#--session=}"; shift ;;
        --min-repeats)     MIN_REPEATS="${2:-}"; shift 2 ;;
        --min-repeats=*)   MIN_REPEATS="${1#--min-repeats=}"; shift ;;
        --window-minutes)  WINDOW_MINUTES="${2:-}"; shift 2 ;;
        --window-minutes=*) WINDOW_MINUTES="${1#--window-minutes=}"; shift ;;
        -h|--help)         grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "jig-friction: unknown argument: $1" >&2; shift ;;
    esac
done

emit_quiet() {
    printf '%s\n' '{"status":"ok","signals":[],"briefing":""}'
    exit 0
}

# jq is required; absent -> graceful quiet (never crash the fan-out).
command -v jq >/dev/null 2>&1 || { echo "jig-friction: jq unavailable — quiet" >&2; emit_quiet; }

# ---- config (CLI > .claude/config.json jigFriction.* > default) --------------
if [ -f ".claude/config.json" ]; then
    if [ -z "$MIN_REPEATS" ]; then
        MIN_REPEATS="$(jq -r '.jigFriction.minRepeats // empty' .claude/config.json 2>/dev/null)"
    fi
    if [ -z "$WINDOW_MINUTES" ]; then
        WINDOW_MINUTES="$(jq -r '.jigFriction.windowMinutes // empty' .claude/config.json 2>/dev/null)"
    fi
fi
[ -z "$MIN_REPEATS" ] && MIN_REPEATS=5
[ -z "$WINDOW_MINUTES" ] && WINDOW_MINUTES=0
case "$MIN_REPEATS"    in ''|*[!0-9]*) MIN_REPEATS=5 ;; esac
case "$WINDOW_MINUTES" in ''|*[!0-9]*) WINDOW_MINUTES=0 ;; esac

# ---- data dir resolution (live registry primary) ----------------------------
if [ -z "$DIR" ]; then
    if ls "$HOME/.claude/command-usage"/*.jsonl >/dev/null 2>&1; then
        DIR="$HOME/.claude/command-usage"
    elif [ -d "./claude-usage" ] && ls ./claude-usage/*.jsonl >/dev/null 2>&1; then
        DIR="./claude-usage"
    fi
fi
if [ -z "$DIR" ] || ! ls "$DIR"/*.jsonl >/dev/null 2>&1; then
    echo "jig-friction: NO-DATA — no *.jsonl registry (looked in --dir / ~/.claude/command-usage / ./claude-usage)" >&2
    emit_quiet
fi

ENV_SESSION="${CLAUDE_SESSION_ID:-}"

# ---- parse-boundary zone canonicalizer (DEC-302 / LEDGER-ZONE-02) -----------
# This oracle measures the window RELATIVE TO THE LEDGER'S OWN MAX ROW, which is
# why DEFER-139 called it the only zone-correct consumer. That is true only over
# a UNIFORMLY-zoned ledger: from DEC-302 the field is UTC-with-`Z` and older rows
# are BARE (legacy LOCAL), so one file holds both across the transition and a
# ledger-relative window silently spans a full offset at the boundary. Resolving
# both forms to one epoch below is what keeps the relative measure honest.
# PER-FILE COPY ON PURPOSE (DEC-279/DEC-251).
TZOFF_SECS="$(date +%z 2>/dev/null | awk '{s=substr($0,1,1); h=substr($0,2,2)+0; m=substr($0,4,2)+0; v=h*3600+m*60; if (s=="-") v=-v; print v}')"
[ -n "$TZOFF_SECS" ] || TZOFF_SECS=0

# ---- detect ------------------------------------------------------------------
# tr -d '\r': native-Windows writers emit CRLF (SHARED-CSI-07 lineage).
# fromjson? // empty: malformed lines are skipped, never fatal.
RESULT="$(cat "$DIR"/*.jsonl 2>/dev/null | tr -d '\r' | jq -R 'fromjson? // empty' | jq -s -c \
    --arg session "$SESSION" \
    --arg envSession "$ENV_SESSION" \
    --argjson minRepeats "$MIN_REPEATS" \
    --argjson tzoff "${TZOFF_SECS:-0}" \
    --argjson windowMinutes "$WINDOW_MINUTES" '

  # at_epoch -- one comparable instant for both zone forms (see the note above).
  # mktime reads the broken-down time AS UTC: a marked value is already UTC once
  # the Z is sliced off; a bare value is a LOCAL wall clock W at offset O, whose
  # true UTC is (W - O). Returns null on anything unparseable (fail-open below).
  def at_epoch:
    if (type != "string") or . == "" then null
    elif endswith("Z") then (try (.[0:19] | strptime("%Y-%m-%dT%H:%M:%S") | mktime) catch null)
    else (try (((strptime("%Y-%m-%dT%H:%M:%S") | mktime) - $tzoff)) catch null)
    end;

  def archetype($c):
    ($c | ascii_downcase) as $l
    | if   ($l | test("screenshot|snapshot|navigate|browser|click|_type|playwright|scroll|hover|press_key")) then "scenario-replayer"
      elif ($l | test("log|grep|tail|distill")) then "log-distiller"
      elif ($l | test("diff|compare")) then "diff-summarizer"
      elif ($l | test("harvest|scrape|collect")) then "corpus-harvester"
      elif ($l | test("state")) then "state-fabricator"
      elif ($l | test("reset|teardown|provision|setup|restart|clean|env")) then "environment-resetter"
      elif ($l | test("generat|fabricat|mock|sample|seed|fixture|synth")) then "fabricator"
      else "scenario-replayer" end;

  # verbatim invocation string, truncated to ~200 chars (evidence for a glance).
  def estr:
    ( .at + " " + .cmd
      + (if (.arg1 // "") != "" then " " + .arg1 else "" end)
      + (if ((.flags // []) | length) > 0 then " [" + (.flags | join(",")) + "]" else "" end)
    ) as $s
    | (if ($s | length) > 200 then ($s[0:197] + "...") else $s end);

  # normalized shape signature: type|cmd|arg1|flags, lowercased, digit-runs->N.
  def sig:
    ( (.type // "?") + "|" + .cmd + "|" + (.arg1 // "") + "|" + ((.flags // []) | join(",")) )
    | ascii_downcase | gsub("[0-9]+"; "N");

  [ .[] | select(type == "object" and (.cmd? | type == "string") and (.session? | type == "string"))
        | . + {"_ate": (.at | at_epoch)} ] as $recs
  | if ($recs | length) == 0 then {status:"ok", signals:[], briefing:""}
    else
      # --- scope session (first hit wins) ---
      ( if   $session != "" then $session
        elif ($envSession != "" and ($recs | any(.session == $envSession))) then $envSession
        else ($recs | max_by(._ate // 0) | .session) end ) as $scope
      | [ $recs[] | select(.session == $scope) ] as $scoped0
      # --- optional windowMinutes tightening (fail-open on parse error) ---
      # Uses _ate (zone-canonical epoch), not the raw string: a mixed-zone ledger
      # would otherwise put the window edge a full offset out (DEC-302).
      | ( if $windowMinutes > 0 and ($scoped0 | length) > 0 then
            ( ([$scoped0[]._ate | select(. != null)] | max) as $mx
              | if $mx == null then null else ($mx - ($windowMinutes * 60)) end ) as $cut
            | if $cut == null then $scoped0
              else [ $scoped0[] | select( (._ate // ($cut + 1)) >= $cut ) ]
              end
          else $scoped0 end ) as $scoped
      # --- cluster by shape signature ---
      | ( [ $scoped[] | . + {"_sig": sig} ]
          | group_by(._sig)
          | map(select(length >= $minRepeats))
          | map( (sort_by(._ate // 0)) as $g
                 | ($g[0]) as $rep
                 | { pattern: ( ($rep._sig) | gsub("\""; "") | gsub("[|]+$"; "") ),
                     count: ($g | length),
                     evidence: ( ($g | map(estr))[0:20]
                                 + (if ($g | length) > 20 then ["...(+" + (($g|length)-20|tostring) + " more)"] else [] end) ),
                     suggestedArchetype: archetype($rep.cmd) } )
          | sort_by(-.count) ) as $signals
      | if ($signals | length) == 0 then {status:"ok", signals:[], briefing:""}
        else
          ($signals[0]) as $top
          | ( ($signals | length | tostring) + " repetition signal(s) this session -- top: "
              + ($top.count | tostring) + "x " + $top.pattern
              + " (consider a " + $top.suggestedArchetype + " jig; see JIG_CATALOG.md)"
              | gsub("\""; "") ) as $brief
          | {status:"signal", signals:$signals, briefing:$brief}
        end
    end
' 2>/dev/null)"

if [ -z "$RESULT" ]; then
    echo "jig-friction: detector produced no output — quiet" >&2
    emit_quiet
fi

printf '%s\n' "$RESULT"
exit 0
