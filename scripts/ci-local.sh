#!/usr/bin/env bash
# CI-parity local gate — run BEFORE every push. Mirrors .github/workflows/ci.yml
# so a green run here means a green run there (modulo CI-runner-speed flakes).
#
# WHY THIS EXISTS (the failure it prevents):
#   CI compiles ALL targets in OFFLINE mode:
#     SQLX_OFFLINE=true cargo {build,test,clippy} --workspace --all-targets   (RUSTFLAGS=-D warnings)
#   A plain `cargo build --workspace` or a targeted `cargo test --test X` does
#   NOT compile every test target and does NOT deny warnings, so it silently
#   skips two whole classes of CI failure:
#     1. `sqlx::query!` macros in tests/ whose metadata was never cached
#        (a plain `cargo sqlx prepare --workspace` misses test targets — you must
#         prepare with `-- --all-targets`). Offline + --all-targets surfaces it.
#     2. clippy/rustc warnings-as-errors anywhere in the workspace.
#   The single `clippy --workspace --all-targets -- -D warnings` below catches
#   BOTH (clippy compiles every target, in offline mode, denying all warnings).
#
# USAGE:
#   bash scripts/ci-local.sh            # fast, DB-free: oracle + the compile/lint gate
#   bash scripts/ci-local.sh --tests    # also run the test suite (needs DATABASE_URL + Postgres)
#   bash scripts/ci-local.sh --deny     # also run cargo-deny (needs `cargo install cargo-deny`)
#   bash scripts/ci-local.sh --all      # everything
#
# If the gate fails with "no cached data for this query", regenerate the cache:
#   DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev \
#     cargo sqlx prepare --workspace -- --all-targets && git add .sqlx
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

export SQLX_OFFLINE=true   # match CI: compile against the .sqlx cache, no DB

run_tests=false
run_deny=false
for arg in "$@"; do
  case "$arg" in
    --tests) run_tests=true ;;
    --deny)  run_deny=true ;;
    --all)   run_tests=true; run_deny=true ;;
    -h|--help) sed -n '1,33p' "$0"; exit 0 ;;
    *) echo "unknown arg: $arg (try --tests, --deny, --all, --help)" >&2; exit 2 ;;
  esac
done

fail=0
step() { printf '\n=== %s ===\n' "$*"; }

step "Oracle: multi-tenant-isolation-check (CI job 1)"
bash .claude/oracles/multi-tenant-isolation-check/oracle.sh || fail=1

step "clippy --workspace --all-targets -- -D warnings  [offline]  (CI build+clippy)"
echo "(compiles EVERY target offline — the gate that catches missing test-target .sqlx + all warnings)"
if ! cargo clippy --workspace --all-targets -- -D warnings; then
  fail=1
  printf '\n>>> If the failure is "no cached data for this query", run:\n'
  printf '    DATABASE_URL=... cargo sqlx prepare --workspace -- --all-targets && git add .sqlx\n'
fi

if $run_tests; then
  step "cargo test --workspace  (CI test job; needs DATABASE_URL + Postgres)"
  if [ -z "${DATABASE_URL:-}" ]; then
    echo "DATABASE_URL not set — cannot run the suite. Export your dev Postgres URL, e.g.:"
    echo "  export DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev"
    fail=1
  else
    # Note: many #[sqlx::test] DBs at once can exhaust the local Postgres pool
    # (PoolTimedOut) — a local resource limit, not a code bug. Re-run a flaked
    # test in isolation, or lower parallelism: `... -- --test-threads=8`.
    cargo test --workspace -- --nocapture || fail=1
  fi
fi

if $run_deny; then
  step "cargo-deny check --workspace  (CI deny job)"
  if command -v cargo-deny >/dev/null 2>&1; then
    cargo deny check --workspace || fail=1
  else
    echo "cargo-deny not installed — skipping (cargo install cargo-deny to enable)"
  fi
fi

printf '\n'
if [ "$fail" -eq 0 ]; then
  echo "✅ CI-parity checks passed — safe to push."
else
  echo "❌ CI-parity checks FAILED — fix before pushing (this is what CI would report)."
fi
exit "$fail"
