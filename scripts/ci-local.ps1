#!/usr/bin/env pwsh
# CI-parity local gate — run BEFORE every push. Mirrors .github/workflows/ci.yml.
#
# WHY: CI compiles ALL targets in OFFLINE mode
#   (SQLX_OFFLINE=true cargo {build,test,clippy} --workspace --all-targets, RUSTFLAGS=-D warnings).
# A plain `cargo build` / targeted `cargo test` skips two CI failure classes:
#   1. sqlx::query! macros in tests/ never cached (prepare needs `-- --all-targets`);
#   2. clippy/rustc warnings-as-errors.
# The single `clippy --all-targets -- -D warnings` (offline) below catches BOTH.
#
# USAGE:
#   pwsh scripts/ci-local.ps1            # fast, DB-free: oracle + compile/lint gate
#   pwsh scripts/ci-local.ps1 -Tests     # also run the suite (needs DATABASE_URL + Postgres)
#   pwsh scripts/ci-local.ps1 -Deny      # also run cargo-deny
#   pwsh scripts/ci-local.ps1 -All
#
# Fix for "no cached data for this query":
#   $env:DATABASE_URL="postgres://postgres:dev@localhost:5433/feedbackmonk_dev"
#   cargo sqlx prepare --workspace -- --all-targets ; git add .sqlx
param(
  [switch]$Tests,
  [switch]$Deny,
  [switch]$All
)
$ErrorActionPreference = 'Continue'
Set-Location (Join-Path $PSScriptRoot '..')
$env:SQLX_OFFLINE = 'true'   # match CI: compile against the .sqlx cache, no DB
if ($All) { $Tests = $true; $Deny = $true }

$fail = 0
function Step($m) { Write-Host "`n=== $m ===" }

Step "Oracle: multi-tenant-isolation-check (CI job 1)"
bash .claude/oracles/multi-tenant-isolation-check/oracle.sh
if ($LASTEXITCODE -ne 0) { $fail = 1 }

Step "clippy --workspace --all-targets -- -D warnings  [offline]  (CI build+clippy)"
Write-Host "(compiles EVERY target offline — catches missing test-target .sqlx + all warnings)"
cargo clippy --workspace --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) {
  $fail = 1
  Write-Host "`n>>> If the failure is 'no cached data for this query', run:"
  Write-Host "    cargo sqlx prepare --workspace -- --all-targets ; git add .sqlx"
}

if ($Tests) {
  Step "cargo test --workspace  (CI test job; needs DATABASE_URL + Postgres)"
  if (-not $env:DATABASE_URL) {
    Write-Host "DATABASE_URL not set — cannot run the suite. Set it, e.g.:"
    Write-Host '  $env:DATABASE_URL="postgres://postgres:dev@localhost:5433/feedbackmonk_dev"'
    $fail = 1
  } else {
    cargo test --workspace -- --nocapture
    if ($LASTEXITCODE -ne 0) { $fail = 1 }
  }
}

if ($Deny) {
  Step "cargo-deny check --workspace  (CI deny job)"
  if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    cargo deny check --workspace
    if ($LASTEXITCODE -ne 0) { $fail = 1 }
  } else {
    Write-Host "cargo-deny not installed — skipping (cargo install cargo-deny to enable)"
  }
}

Write-Host ''
if ($fail -eq 0) {
  Write-Host "[OK] CI-parity checks passed — safe to push."
} else {
  Write-Host "[FAIL] CI-parity checks FAILED — fix before pushing (this is what CI would report)."
}
exit $fail
