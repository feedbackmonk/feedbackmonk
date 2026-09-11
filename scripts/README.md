# `scripts/` — repo-level gates and witnesses

## Purpose

The shell and PowerShell entry points a session runs against the whole repo: the CI-parity gate,
the verification-oracle suite it calls first, the end-to-end curl witnesses for the P0/P1 exit
gates, and the one-time GitCellar provisioning procedure. Nothing here is imported by the crates;
every file is a command.

## File index

| File | Purpose |
|---|---|
| `ci-local.sh`, `ci-local.ps1` | the CI-parity gate — runs the oracle suite, then offline `clippy --all-targets -D warnings`; `--tests` / `-Tests` adds the suite. Run before every push of Rust changes (`docs/dev-notes/rust-ci-parity.md`). |
| `run-verification-oracles.sh` | the single source of truth for "the verification-oracle suite": runs each `.claude/project-oracles/<name>/oracle.py` in its `ORACLES` array; CI job `verification-oracles` and `ci-local` both call it. |
| `e2e-p0-curl.sh`, `e2e-p0-curl.ps1` | the P0 exit-gate witness: signup → project → key-register → JWT-signed and anonymous submission → rate limit, against a running dev instance. |
| `e2e-p1-curl.sh` | the P1 exit-gate witness: extends P0 with the status-workflow + admin-reply pipeline. |
| `gen-ed25519.sh` | writes the Ed25519 keypair the curl witnesses sign with. |
| `provision-gitcellar.sh` | one-time provisioning of the GitCellar tenant, project and signing key — the procedure in `docs/integrations/gitcellar-adoption.md` § 3. |
| `smoke-test-p0.ps1` | drives the built binary end-to-end against the Mailpit + Postgres dev containers. |
| `i18n/` | the catalog tooling module — its own README is the index. |

## Public API

`bash scripts/ci-local.sh [--tests]` and `bash scripts/run-verification-oracles.sh` are the two
commands other documents name. Exit 0 iff green; a non-zero exit lists what failed.

## Constraints

- Adding an oracle to `run-verification-oracles.sh`'s `ORACLES` array widens the CI gate — a
  reviewable change, kept in sync with the Oracles table in `CLAUDE.md`.
- The witnesses need a running instance and `DATABASE_URL`; they are not unit tests and never run
  in CI.

## Relationships

- Consumed by `.github/workflows/ci.yml`, `/0-uldf-finalize`'s push step (through `ci-local`), and
  the runbooks under `docs/operations/`.
- Reads `.claude/project-oracles/` (the suite) and the crates (the gate).

## Decisions

**One runner script, not per-oracle CI steps.** CI once ran a single oracle by name and the other
twelve never gated a push; the suite drifted red for months unnoticed. `run-verification-oracles.sh`
exists so "the suite" has exactly one definition that CI and the local gate share.

**Witnesses live here, not under `crates/*/tests`.** They drive a deployed binary over HTTP with
curl and need a live database and mailer; keeping them out of `cargo test` keeps the unit suite
hermetic.
