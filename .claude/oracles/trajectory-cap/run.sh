#!/usr/bin/env bash
# trajectory-cap oracle (Unix)
#
# Verification Oracle (kind: verification). Auto-discovered by /0-uldf-finalize
# Phase 1a sec 1a.6. Detects when docs/PROJECT_TRAJECTORY.md has breached its
# documented size caps -- the signature of a Phase 12 (TRAJECTORY-02) run that
# APPENDED instead of reshaping. Phase 12's caps are prose-only (agent-executed),
# so a run that skipped pruning accumulates silently; this makes the breach
# deterministic and every-finalize.
#
# Four signals (any one => warn):
#   - total lines  > max_lines      (accumulation)
#   - total bytes  > max_bytes      (accumulation by content weight)
#   - longest line > max_line_chars (the giant-single-line append -- a line-count
#                                    check alone MISSES this; SessionHelm was only
#                                    173 lines with a 57k-char single line)
#   - mojibake markers present      (UTF-8/CP1252 round-trip corruption on write)
#
# Output: JSON ({status, details, briefing}). ADVISORY: status is "pass" or
# "warn" and the script ALWAYS exits 0 on a real run -- the breach is
# self-correcting within the same /0-uldf-finalize via the Phase 12 REPAIR path,
# so blocking the commit here would prevent the repair from running. `--self-test`
# asserts the detector fires on a synthetic bloated sample (exit 1 if it does not).
set -u

# --- Caps (mirror oracle.json config; edit here to change scope) ---
TARGET="docs/PROJECT_TRAJECTORY.md"
MAX_LINES=250
MAX_BYTES=32768
MAX_LINE_CHARS=2000

# Returns the violation list (one per line) for a given file on $1.
# Echoes "lines|bytes|longest_chars|longest_lineno|over_cap|mojibake" on fd 3-style
# via globals, and prints human-readable violations to stdout.
analyze() {
  local f="$1"
  A_LINES=$(wc -l < "$f" | tr -d ' ')
  A_BYTES=$(wc -c < "$f" | tr -d ' ')
  # longest line length + its 1-based number
  local lr; lr=$(awk '{ if (length > max) { max = length; ln = NR } } END { printf "%d %d", max+0, ln+0 }' "$f")
  A_LONGEST=${lr% *}
  A_LONGEST_LN=${lr#* }
  A_OVER=$(awk -v cap="$MAX_LINE_CHARS" 'length > cap { c++ } END { print c+0 }' "$f")
  # mojibake: UTF-8 byte sequence of "â€" (C3 A2 E2 82 AC) or lone "Ã" (C3 83),
  # both strong markers of a UTF-8<->CP1252 round-trip. Byte-literal grep.
  A_MOJI=$(LC_ALL=C grep -acE "$(printf '\xc3\xa2\xe2\x82\xac')|$(printf '\xc3\x83')" "$f" 2>/dev/null | tr -d ' ')
  [ -z "$A_MOJI" ] && A_MOJI=0

  VIOLATIONS=""
  [ "$A_LINES" -gt "$MAX_LINES" ]            && VIOLATIONS="${VIOLATIONS}lines:${A_LINES}>${MAX_LINES}\n"
  [ "$A_BYTES" -gt "$MAX_BYTES" ]            && VIOLATIONS="${VIOLATIONS}bytes:${A_BYTES}>${MAX_BYTES}\n"
  [ "$A_LONGEST" -gt "$MAX_LINE_CHARS" ]     && VIOLATIONS="${VIOLATIONS}longest_line:${A_LONGEST}>${MAX_LINE_CHARS}(line ${A_LONGEST_LN})\n"
  [ "$A_MOJI" -gt 0 ]                        && VIOLATIONS="${VIOLATIONS}mojibake:${A_MOJI} line(s)\n"
}

# ---- self-test ----
if [ "${1:-}" = "--self-test" ]; then
  tmp_bad="$(mktemp)"; tmp_ok="$(mktemp)"
  # Bad sample: few lines, but one giant single line (the real failure shape).
  { printf '# Project Trajectory\n\n## Current Focus\n\n'; head -c 60000 /dev/zero | tr '\0' 'x'; printf '\n'; } > "$tmp_bad"
  printf '# Project Trajectory\n\n## Current Focus\n\nShort and clean.\n' > "$tmp_ok"
  analyze "$tmp_bad"; bad_v="$VIOLATIONS"
  analyze "$tmp_ok";  ok_v="$VIOLATIONS"
  rm -f "$tmp_bad" "$tmp_ok"
  if [ -n "$bad_v" ] && [ -z "$ok_v" ]; then
    printf '{"status":"pass","details":{"self_test":true,"detector_fired_on_bloat":true,"detector_quiet_on_clean":true},"briefing":"Self-test PASS: detector flags a giant-single-line sample and stays quiet on a clean one."}\n'
    exit 0
  else
    printf '{"status":"fail","details":{"self_test":true,"detector_fired_on_bloat":%s,"detector_quiet_on_clean":%s},"briefing":"Self-test FAIL: cap detector is silently broken."}\n' \
      "$( [ -n "$bad_v" ] && echo true || echo false )" "$( [ -z "$ok_v" ] && echo true || echo false )"
    exit 1
  fi
fi

repo_root="$(cd "$(dirname "$0")/../../.." && pwd)"
start_ms=$(($(date +%s%N)/1000000))
f="$repo_root/$TARGET"

if [ ! -f "$f" ]; then
  dur=$(( $(($(date +%s%N)/1000000)) - start_ms ))
  printf '{"status":"pass","details":{"file_exists":false,"path":"%s","scan_duration_ms":%d},"briefing":"No %s -- nothing to check."}\n' \
    "$TARGET" "$dur" "$TARGET"
  exit 0
fi

analyze "$f"
dur=$(( $(($(date +%s%N)/1000000)) - start_ms ))

# Build JSON violations array from the newline-delimited VIOLATIONS.
viol_json=""
if [ -n "$VIOLATIONS" ]; then
  while IFS= read -r v; do
    [ -z "$v" ] && continue
    esc=$(printf '%s' "$v" | sed 's/"/\\"/g')
    viol_json="${viol_json:+$viol_json,}\"$esc\""
  done <<EOF
$(printf '%b' "$VIOLATIONS")
EOF
fi

if [ -z "$viol_json" ]; then
  status=pass
  briefing="Within caps: $A_LINES lines, $A_BYTES bytes, longest line $A_LONGEST chars."
else
  status=warn
  briefing="TRAJECTORY BLOAT: docs/PROJECT_TRAJECTORY.md breached its size caps (Phase 12 appended instead of reshaping). Phase 12 REPAIR mode will reshape it on the next /0-uldf-finalize (salvage parseable signal, discard bloat, full rewrite). Non-blocking."
fi

printf '{"status":"%s","details":{"file_exists":true,"path":"%s","lines":%d,"bytes":%d,"longest_line_chars":%d,"longest_line_number":%d,"lines_over_char_cap":%d,"mojibake_lines":%d,"violations":[%s],"caps":{"max_lines":%d,"max_bytes":%d,"max_line_chars":%d},"scan_duration_ms":%d},"briefing":"%s"}\n' \
  "$status" "$TARGET" "$A_LINES" "$A_BYTES" "$A_LONGEST" "$A_LONGEST_LN" "$A_OVER" "$A_MOJI" "$viol_json" \
  "$MAX_LINES" "$MAX_BYTES" "$MAX_LINE_CHARS" "$dur" "$briefing"

# Advisory oracle: always exit 0 on a real run (warn does not block the commit
# that runs the Phase 12 repair). Exit code is reserved for self-test only.
exit 0
