# `.claude/project-oracles/` — this project's Verification Oracles

## Purpose

The seventeen static probes that defend feedbackmonk's code-level invariants — the
anti-reward-hacking legs that catch what tests cannot: a dropped `.layer(cors)`, a raw `sqlx::query`
outside the repository crate, a board read that swaps its `approved` literal for a bound param, a
handler that stops calling `check_tier_quota`.

**These are not ULDF framework oracles and do not share their contract.** The framework starter pack
is `oracle.json` carrying `"schema": "oracle/2"` plus a `run.py` exposing `run(ctx) -> verdict`, run
by the session-start hook; this project installs no copy of it, so those run from `~/.claude/oracles/`
in place and `.claude/oracles/` holds only its `INDEX.md` (DEC-538). These carry `manifest.json`, a canonical
`oracle.py`, `oracle.sh`/`oracle.ps1` shims and a `--full` flag, and are invoked directly. The
framework runner can answer a directory of this shape only `unknown`, which is why they live here.

## Index

One directory per oracle; `CLAUDE.md` § Oracles carries the one-line table of what each defends.

| Directory | Defends |
|---|---|
| `approval-gate-enforcement/` | no work order reaches ≥ `dispatched` without a prior owner-authored `approved` event |
| `cors-allowlist-enforcement/` | credentialed CORS stays wired into `build_app`, never wildcard |
| `feedback-as-data-audit/` | runner treats feedback as data: one prompt chokepoint, one egress sanitizer |
| `feedback-erasure-completeness/` | byte purge before row delete; every `REFERENCES feedback(id)` cascades |
| `i18n-catalog-integrity/` | catalog shape + generated locale tables (C35, C41) |
| `i18n-literal-ratchet/` | baseline 0 hard-coded user-facing literals in `widget/src`, `admin-ui/src` |
| `multi-tenant-isolation-check/` | the tenant-scoped repository layer is the sole query path (DEC-FBR-03) |
| `pii-scrub-audit/` | submitter PII does not escape into logs or public surfaces |
| `public-board-moderation-gate/` | no public board read or vote touches a non-`approved` row |
| `public-id-as-capability/` | a public id alone never authorizes; `authorize_submitter` gates first |
| `public-route-ceiling/` | every public router is wrapped by `apply_public_rate_limit` |
| `selfhost-compose-smoke/` | compose distribution + env-catalog SSOT `docs/operations/SELFHOST_ENV.md` |
| `solicitation-invariant-check/` | `opted_out` is terminal; writes route through the state machine |
| `submission-idempotency/` | idempotency keys are identity-scoped and 409 on content mismatch |
| `tier-enforcement-status/` | plan caps fire; `tier_quotas()` holds Contract C19's shape |
| `translation-egress-q24-isolation/` | provider defaults `off`; no public read of `body_translated` |
| `widget-bundle-size/` | page-load set ≤ 30,720 B, each locale chunk ≤ 4,096 B, no third-party trackers |

Each directory's own `manifest.json` + `README.md` is the authoritative record of its probes.

## Public API

`bash scripts/run-verification-oracles.sh` — the single source of truth for "the suite". It runs
each `oracle.py` cheap-static, exits 0 iff all seventeen pass, and is what CI job
`verification-oracles` and `scripts/ci-local.sh` both call. Adding an oracle means adding it to that
script's `ORACLES` array; nothing auto-discovers this directory.

Individually: `python .claude/project-oracles/<name>/oracle.py [--full]`. `--full` runs the
behavioral probes, which duplicate `cargo test` and need a DB or built artifacts — a
nightly/pre-release leg, not a per-commit one.

## Constraints

- **Never narrow a scan, weaken a threshold or add an allowlist entry to make a line go green.**
  `multi-tenant-isolation-check/allowlist.toml` and `tier-enforcement-status/allowlist.toml` each
  require a written `rationale`; a fourth pre-auth repository method is a reviewable widening, not a
  fix.
- Each `oracle.py` resolves the repo root by **depth** (`Path(__file__).resolve().parents[2]`).
  Moving one of these directories to a different nesting level breaks it silently.
- Consumers name these by path in Rust doc comments, module READMEs, `widget/vite.config.ts` and
  `crates/feedbackmonk-tracing/tests/scrubber_patterns.rs` (which reads
  `pii-scrub-audit/expected_hash.txt` through a hard relative path). Repoint every one in the same
  commit as any move.

## Relationships

- **Consumed by** `scripts/run-verification-oracles.sh` → `.github/workflows/ci.yml` job
  `verification-oracles`, and `scripts/ci-local.sh` / `ci-local.ps1` step 1.
- **Reads** `crates/`, `migrations/`, `deploy/docker/`, `widget/dist/`, `i18n/`, `admin-ui/src/`.
- **Sibling, not parent**: `.claude/oracles/` (the framework starter pack's home — different
  contract, different runner; no starter is installed there, see its `INDEX.md`).

## Decisions

**The two contracts get two directories.** These seventeen were installed under `.claude/oracles/`
alongside the framework pack. The v2 runner cannot read their manifests, so it reported `unknown`
for each — seventeen wasted rows in every session-start briefing, and a batch budget spent to
produce them. A 2026-09-07 migration read that as "retire the stale pack" and deleted all
seventeen, turning CI red, because nothing told it `scripts/run-verification-oracles.sh` invokes
`oracle.py` directly and never goes through the runner. Restoring them where they were would
reinstate the noise that caused the deletion; adding a `schema` key would make the runner look for a
`run.py` that does not exist and answer `unknown` anyway. Separate homes end the collision without
rewriting anything. See `docs/planning/deferred/DEFER-010_verification-oracle-pack-uninstalled.md`.

**Five of the seventeen still ship without their own README** — `approval-gate-enforcement`,
`feedback-as-data-audit`, `feedback-erasure-completeness`, `public-board-moderation-gate`,
`translation-egress-q24-isolation` — and `module-readme-parity` reports them as presence gaps. That
debt predates the move and was not created by it.
