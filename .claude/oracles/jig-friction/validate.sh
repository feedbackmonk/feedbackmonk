#!/bin/bash
# jig-friction oracle self-test (Unix) — JIG-04, DEC-142.
# Self-contained: synthesizes a tiny in-temp corpus (no dependency on the
# smoke-tests fixtures, which are NOT deployed to target projects) and asserts
# BOTH output branches — the `signal` schema and the quiet-path invariant.
# Uses jq (the oracle's own hard dependency); avoids python (the WindowsApps
# python3 shim is broken here — PW-005 lineage).
set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
RUN="$ORACLE_DIR/run.sh"

fail() { echo "FAIL: $1" >&2; [ -n "${2:-}" ] && echo "$2" >&2; exit 1; }

# --- signal case: 5 identical named invocations in one session --------------
SIG_DIR="$(mktemp -d)"
QUIET_DIR="$(mktemp -d)"
trap 'rm -rf "$SIG_DIR" "$QUIET_DIR"' EXIT
{
  for t in 00 01 02 03 04; do
    printf '{"at":"2026-07-05T09:00:%s","cmd":"mcp:playwright/browser_take_screenshot","type":"mcp","session":"sess-v","project":"P","machine":"M"}\n' "$t"
  done
} > "$SIG_DIR/x.jsonl"
SIGNAL_OUT="$(bash "$RUN" --dir "$SIG_DIR" --session sess-v 2>/dev/null)"
QUIET_OUT="$(bash "$RUN" --dir "$QUIET_DIR" 2>/dev/null)"

# --- quiet-path shape (grep — works even without jq) ------------------------
printf '%s' "$QUIET_OUT" | grep -q '"status":"ok"' || fail "quiet output missing status:ok" "$QUIET_OUT"
printf '%s' "$QUIET_OUT" | grep -qE '"briefing":[[:space:]]*""' || fail "QUIET-PATH INVARIANT violated: status=ok but briefing non-empty" "$QUIET_OUT"

if ! command -v jq >/dev/null 2>&1; then
    echo "PASS (partial): jig-friction quiet-path invariant holds; jq absent — signal-schema assertions skipped" >&2
    echo "PASS: jig-friction oracle validates (quiet-path invariant; jq-gated signal check skipped)"
    exit 0
fi

# --- full assertions via jq -------------------------------------------------
FROZEN='["fabricator","state-fabricator","scenario-replayer","log-distiller","diff-summarizer","environment-resetter","corpus-harvester"]'

printf '%s' "$SIGNAL_OUT" | jq -e --argjson frozen "$FROZEN" '
    . as $r
    | ($r.status == "signal")
    and ($r.signals | length >= 1)
    and ($r.signals[0] | has("pattern") and has("count") and has("evidence") and has("suggestedArchetype"))
    and ($r.signals[0].count == 5)
    and ($r.signals[0].evidence | type == "array" and length == 5)
    and (($frozen | index($r.signals[0].suggestedArchetype)) != null)
    and ($r.briefing | type == "string" and length > 0)
' >/dev/null || fail "signal-branch schema/values assertion failed" "$SIGNAL_OUT"

printf '%s' "$QUIET_OUT" | jq -e '
    (.status == "ok") and (.signals | type == "array" and length == 0) and (.briefing == "")
' >/dev/null || fail "quiet-branch assertion failed" "$QUIET_OUT"

echo "PASS: jig-friction oracle validates (signal schema + quiet-path invariant)"
