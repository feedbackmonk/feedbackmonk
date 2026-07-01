#!/usr/bin/env bash
# Run the full project verification-oracle suite (static probes).
#
# WHY THIS EXISTS (scrutiny 2026-07-01, finding P0-5):
#   CI previously ran exactly ONE of the project verification oracles
#   (multi-tenant-isolation-check). The other twelve — the anti-reward-hacking
#   *static* legs that catch what tests cannot (a dropped `.layer(cors)`, an
#   un-allowlisted `body_translated` read, a handler that stops calling
#   check_tier_quota, a board read that swaps its `approved` literal for a bound
#   param) — never gated a push. So detection-from-code drift accumulated
#   silently (the tier oracle was RED against an ops endpoint added months
#   earlier and nobody knew). This script is the single source of truth for
#   "the verification-oracle suite"; CI and scripts/ci-local.sh both call it.
#
# Runs each oracle's CHEAP STATIC probes (no --full). The `--full` behavioral
# probes duplicate `cargo test` and need a DB / built artifacts; run those in a
# nightly/pre-release job, not here.
#
# USAGE:  bash scripts/run-verification-oracles.sh
# Exit 0 iff every oracle passes; non-zero lists the failures.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Portable Python: pick the first candidate that ACTUALLY RUNS. (On Windows the
# `python3` name is often a Microsoft-Store stub that prints an install nag and
# exits non-zero; on CI/ubuntu `python3` is real and `python` may be absent — so
# probe by invoking, not by PATH presence.)
PY=""
for cand in python3 python py; do
  if command -v "$cand" >/dev/null 2>&1 && "$cand" -c 'import sys' >/dev/null 2>&1; then
    PY="$cand"; break
  fi
done
if [ -z "$PY" ]; then
  echo "::error::no working python interpreter found — cannot run verification oracles" >&2; exit 2
fi

# The project verification-oracle suite. Adding an oracle here is a reviewable
# surface — keep it in sync with .claude/oracles/INDEX.md § Verification Oracles.
ORACLES=(
  multi-tenant-isolation-check
  pii-scrub-audit
  cors-allowlist-enforcement
  approval-gate-enforcement
  public-board-moderation-gate
  solicitation-invariant-check
  translation-egress-q24-isolation
  feedback-erasure-completeness
  tier-enforcement-status
  widget-bundle-size
  selfhost-compose-smoke
  feedback-as-data-audit
)

fail=0
failed_list=()
for o in "${ORACLES[@]}"; do
  script=".claude/oracles/$o/oracle.py"
  if [ ! -f "$script" ]; then
    echo "::error::verification oracle missing: $script"; fail=1; failed_list+=("$o (missing)"); continue
  fi
  printf '\n--- oracle: %s ---\n' "$o"
  if ! "$PY" "$script"; then
    fail=1; failed_list+=("$o")
  fi
done

printf '\n=======================================================\n'
if [ "$fail" -eq 0 ]; then
  echo "✅ verification-oracle suite: all ${#ORACLES[@]} PASS"
else
  echo "❌ verification-oracle suite FAILED: ${failed_list[*]}"
fi
exit "$fail"
