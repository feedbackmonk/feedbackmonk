#!/usr/bin/env bash
# The landing check's fast lane (`finalize.fast_command` in `.claude/config.json`):
# compile every Rust target the way CI does, OFFLINE against the committed
# `.sqlx/` query data (docs/dev-notes/rust-ci-parity.md), and type-check the two
# front ends. Offline is what makes it runnable with no Postgres up: an online
# `cargo check` fails every `sqlx::query!` with "error communicating with
# database", which says nothing about the change being landed.
set -euo pipefail
cd "$(dirname "$0")/.."

SQLX_OFFLINE=true cargo check --workspace --all-targets
(cd widget && npm run --silent gen:locales && npx tsc -b)
(cd admin-ui && npx tsc -b)
