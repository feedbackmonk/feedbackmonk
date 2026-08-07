#!/usr/bin/env bash
# tracked-docs-not-clobbered oracle (Unix)
#
# Verification Oracle (kind: verification). Auto-discovered by /0-uldf-finalize
# Phase 1a sec 1a.6. Detects when a load-bearing tracked doc (CLAUDE.md, root
# README.md, docs/specs/*.md) has been clobbered with raw
# dev-server / build log output -- the signature of a misdirected dev-server
# stdout redirect (e.g. `npm run dev > CLAUDE.md`, `cargo tauri dev > README.md`)
# whose detached child inherits the open file descriptor and re-writes the file
# on every HMR / rebuild reload, even after the launching shell dies.
#
# Output: JSON ({status, details, briefing}). Exit 0 when clean, 1 on
# contamination. `--self-test` asserts the detector fires on a synthetic sample.
set -u

ESC=$'\033'
BANNERS=(
  'VITE v[0-9]'
  'ready in [0-9]+ ?ms'
  'Compiling [^ ]+ v[0-9]'
  'Finished .?dev.? profile'
  'Running BeforeDevCommand'
  'Running .?beforeDevCommand'
  'Watching .+ for changes'
  'hmr update '
  'webpack compiled'
  'Compiled successfully'
  'webview2?: '
  '\[tauri\]'
  'Local: +https?://localhost'
)

# Returns 0 (contaminated) / 1 (clean) for the content on stdin.
detect() {
  local content; content="$(cat)"
  # ANSI escape -> decisive. Match the literal 2-byte ESC '[' sequence with a
  # fixed-string (`-F`) grep so this works under BOTH GNU and BSD/macOS grep.
  # A `grep -qP "\x1b\["` relies on PCRE (`-P`), which BSD/macOS grep lacks --
  # and /0-uldf-finalize runs this oracle as a non-interactive script (no
  # profile sourced), so a Homebrew GNU grep on the interactive PATH does NOT
  # rescue it. On macOS the PCRE check errored out and silently fell through to
  # the textual-banner path, leaving the "ANSI = decisive" guarantee dead while
  # --self-test stayed green (the combined sample also carries textual banners,
  # masking the break -- hence the separate ANSI-only sample below).
  # `LC_ALL=C` forces byte-wise matching of the ESC (0x1b) byte in any locale.
  if printf '%s' "$content" | LC_ALL=C grep -qF "$(printf '\033[')"; then return 0; fi
  local hits=0
  for pat in "${BANNERS[@]}"; do
    if printf '%s' "$content" | grep -qE "$pat"; then hits=$((hits+1)); fi
  done
  [ "$hits" -ge 2 ]
}

if [ "${1:-}" = "--self-test" ]; then
  # Combined sample (ANSI + textual banners) -- the original broad check.
  sample=$'Some heading\n'"${ESC}"$'[32mVITE v5.4.2'"${ESC}"$'[0m  ready in 412 ms\n  Local:   http://localhost:5173/\nCompiling app v0.1.0'
  # ANSI-ONLY sample -- carries an ESC '[' sequence but NO text matching any
  # BANNER pattern, so it can ONLY be caught by the ANSI-escape check. This
  # guards the "ANSI = decisive" path independently: if that path regresses
  # (e.g. a non-portable grep flag), the combined sample above would still pass
  # via textual banners and mask the break -- this one would not.
  ansi_only=$'Project Title\n'"${ESC}"$'[38;5;204msome arbitrary colored prose with no log keywords'"${ESC}"$'[0m\nmore ordinary documentation text'

  if ! printf '%s' "$sample" | detect; then
    printf '{"status":"fail","details":{"self_test":true,"detector_fired":false},"briefing":"Self-test FAIL: detector did not flag an obviously-clobbered sample -- gate silently broken."}\n'
    exit 1
  fi
  if ! printf '%s' "$ansi_only" | detect; then
    printf '{"status":"fail","details":{"self_test":true,"detector_fired":false},"briefing":"Self-test FAIL: ANSI-only sample not flagged -- the decisive ANSI-escape path is broken (e.g. a non-portable grep -P under BSD/macOS grep)."}\n'
    exit 1
  fi
  printf '{"status":"pass","details":{"self_test":true,"detector_fired":true},"briefing":"Self-test PASS: detector flags both a combined clobber sample and an ANSI-only sample (decisive ANSI path verified)."}\n'
  exit 0
fi

repo_root="$(cd "$(dirname "$0")/../../.." && pwd)"
start_ms=$(($(date +%s%N)/1000000))

targets=()
for rel in CLAUDE.md README.md; do
  [ -f "$repo_root/$rel" ] && targets+=("$rel")
done
for sub in docs/specs; do
  if [ -d "$repo_root/$sub" ]; then
    while IFS= read -r f; do targets+=("${f#"$repo_root/"}"); done \
      < <(find "$repo_root/$sub" -maxdepth 1 -name '*.md' -type f)
  fi
done
# De-duplicate (docs/specs entries can't also be docs top-level, but be safe).
IFS=$'\n' read -r -d '' -a targets < <(printf '%s\n' "${targets[@]}" | awk '!seen[$0]++' && printf '\0')

contam_json=""; contam_count=0; checked=0
for rel in "${targets[@]}"; do
  [ -z "$rel" ] && continue
  checked=$((checked+1))
  if detect < "$repo_root/$rel"; then
    excerpt="$(grep -aEm1 "$(IFS='|'; echo "${BANNERS[*]}")|$(printf '\x1b')\[" "$repo_root/$rel" | sed 's/\x1b/<ESC>/g; s/"/\\"/g' | tr -d '\r' | cut -c1-160)"
    contam_json="${contam_json:+$contam_json,}{\"path\":\"$rel\",\"excerpt\":\"$excerpt\"}"
    contam_count=$((contam_count+1))
  fi
done

dur=$(( $(($(date +%s%N)/1000000)) - start_ms ))
if [ "$contam_count" -eq 0 ]; then
  status=pass
  briefing="Clean: $checked tracked docs free of dev-server/build log contamination."
else
  status=fail
  briefing="CLOBBER DETECTED: $contam_count tracked doc(s) contain raw dev-server/build log output. A dev server was likely launched with stdout redirected into a tracked file. Do NOT commit -- restore from HEAD and kill any orphaned dev process holding the handle."
fi

printf '{"status":"%s","details":{"checked":%d,"contaminated_count":%d,"contaminated":[%s],"scan_duration_ms":%d},"briefing":"%s"}\n' \
  "$status" "$checked" "$contam_count" "$contam_json" "$dur" "$briefing"

[ "$status" = pass ] && exit 0 || exit 1
