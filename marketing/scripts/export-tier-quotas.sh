#!/usr/bin/env bash
# DEC-FBR-IMPL-05 — Rust→JSON pricing SSOT export shim (POSIX).
#
# Runs the `feedbackmonk-core` example binary and writes the JSON output to
# `marketing/src/data/tier_quotas.json` (gitignored — generated, not source).
# Called by `marketing/scripts/run-export.mjs` (which Astro's `prebuild` invokes).

set -eu

# Resolve repo root from this script's location (marketing/scripts/ → repo).
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
OUT_PATH="${REPO_ROOT}/marketing/src/data/tier_quotas.json"

mkdir -p "$(dirname -- "${OUT_PATH}")"

cd -- "${REPO_ROOT}"
cargo run --quiet -p feedbackmonk-core --example export_tier_quotas > "${OUT_PATH}"

SIZE=$(wc -c < "${OUT_PATH}" | tr -d ' ')
echo "wrote marketing/src/data/tier_quotas.json (${SIZE} bytes)"
