# feedbackmonk — Project Context for Claude Code

Project-specific context only. The ULDF framework itself — commands, autonomy switches, propagation rules, the three disciplines — is at `~/.claude/CLAUDE.md`, which is authoritative wherever this file is silent.

> **Identifier stability**: the project was renamed Feedbackr → feedbackmonk on 2026-05-14 (DEC-FBR-11). Code, env vars (`FEEDBACKMONK_*`), crates, containers and paths are fully renamed; the identifier prefixes `DEC-FBR-*` and `FR-FBR-*` are **not**, and historical documents keep the old spelling deliberately — do not "fix" them.

---

## What feedbackmonk is

Standalone open-source SaaS user-feedback platform: submission widget + status-workflow triage + public roadmap with voting + status emails. Multi-product per tenant.

- **Elevator pitch**: *Plausible Analytics for product feedback.*
- **License**: AGPL-3.0-or-later (see `LICENSE` — full canonical AGPL-3.0 text).
- **Where it runs**: one self-host instance is LIVE at `feedback.gitcellar.com`, on GitCellar's Railway. Treat it as production. `feedbackmonk.com` is purchased and points at nothing.
- **Where the work stands**: v1 (P0–P4) shipped; post-v1 phases run against the arc plan below, which is the authority on what is done and what is next.

## Read first (always, for any session in this repo)

| File | What it tells you |
|---|---|
| `docs/specs/SPECIFICATION.md` | the functional requirements, FR-FBR-01..41 (v1 was FR-01..18 across P0–P4) |
| `docs/specs/DECISIONS.md` | DEC-FBR-* + DEC-FBR-IMPL-* — load-bearing context for every implementation choice |
| `docs/specs/ARCHITECTURE.md` | System architecture |
| `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md` | Full build-arc plan: phase ordering, gates, interface contracts, Oracle Pre-Build Plan, Testability Gate findings |

The arc plan is the single most important downstream artifact — it pre-commits phase ordering and exit gates but defers intra-phase topology to each phase's own `/0-uldf-ldis-plan` round.

## Where things live

| Area | What it is |
|---|---|
| `crates/` | the Rust workspace — `feedbackmonk-{api,core,repository,jwt,anon,i18n,runner,tracing}`; index in `crates/README.md` |
| `admin-ui/` | React + Vite admin console, plus the public board and roadmap pages; `admin-ui/README.md` |
| `widget/` | the embeddable vanilla-JS widget, under a hard byte cap; `widget/README.md` |
| `i18n/` | the locale catalogs — that README **is** Contract C35 |
| `migrations/` | SQL migrations, applied in order; `migrations/README.md` |
| `deploy/`, `scripts/` | the self-host compose image, and `scripts/ci-local.sh`, the CI-parity gate |
| `docs/operations/` | runbooks: `LOCAL_DEV.md`, `SELFHOST*.md`, `SAAS_HOSTING.md`, `RAILWAY_GITCELLAR.md`, `TRANSLATION.md` |
| `docs/integrations/gitcellar-adoption.md` | the frozen wire contract customer #1 consumes — the authority on the signing-key / EdDSA-JWT / widget-embed handshake |
| `docs/dev-notes/` | facts true for one area, delivered at the call that trips them rather than read up front |
| `docs/pending/`, `docs/planning/deferred/` | the bodies behind the follow-up stubs below, and DEFER briefs |

## Stack (load-bearing for tooling decisions)

- **Backend**: Rust. A reference implementation lives in the peer GitCellar repo, under its `gitcellar-cloud` crate's feedback module — read-only reference, NOT a base to extract from (DEC-FBR-07).
- **Admin UI / widget**: TypeScript. React for admin UI (port pattern from that repo's admin-ui); vanilla JS+CSS for the embeddable widget (<30KB bundle cap per FR-FBR-04).
- **Marketing site**: Astro (deferred until `feedbackmonk.com` is live).
- **Database**: PostgreSQL. Multi-tenant via `tenant_id` + `project_id` on every domain row; tenant-scoped repository layer is the **sole** query path (raw SQL is a security incident — DEC-FBR-03).
- **Distribution**: SaaS + self-host via `docker compose up`.
- **Testing**: Vitest for unit tests, Playwright + axe-core for widget a11y (mandated by the P2 Testability Gate finding).
- **Billing**: Polar, pattern from GitCellar's existing setup.

## Dev Ports

- **Frontend**: `14204` (admin UI / React + Vite; bound with `strictPort: true`)
- **Backend API**: `14304` (`feedbackmonk-api` crate; default in `FEEDBACKMONK_PORT` env var)
- **Local Postgres**: `5433` (deconflicted from gitcellar-cloud's `5432`, per DEC-FBR-IMPL-04)

All registered in `~/.claude/MACHINE_CONFIG.md` Dev Port Registry.

## Build, run, push

- `bash scripts/ci-local.sh` (PowerShell: `pwsh scripts/ci-local.ps1`) is the CI-parity gate; add `--tests` / `-Tests` to run the suite too. **Never push Rust changes without it.** Why a plain `cargo build` is not enough, and the `sqlx prepare` incantation CI needs, are in `docs/dev-notes/rust-ci-parity.md` — delivered when you run cargo or push.
- `origin` is the real public GitHub remote, not this machine's local Gitea, and **a plain `git push` from an automated shell fails** — twice over, on HTTP/2 and on the credential-helper order. Both workarounds are in `docs/dev-notes/git-push-github.md`, delivered at the push.
- Local dev setup and the migration-ledger state: `docs/operations/LOCAL_DEV.md`.

## Workflow

- Use `/0-uldf-ldis-plan "feedbackmonk P<N> — <Phase Name>"` at each phase boundary.
- Use `/0-uldf-proceed` at phase boundaries — let it pick HERE / HANDOFF / PODS topology based on context budget and work shape.
- LTADS is **active** in this repo.
- Per DEC-FBR-07, this repo is greenfield — there is no source-level dependency on GitCellar. Do NOT modify GitCellar code from this working tree.

## Oracles

**Two directories, two contracts — do not put one in the other's home.**

- `.claude/oracles/` is the ULDF framework's starter pack: `oracle.json` carrying `"schema": "oracle/2"` plus a `run.py`. The session-start hook runs the every-session fast ones and emits an ORACLE BRIEFING — read it before investigating manually. Audit via `/0-uldf-oracle`.
- `.claude/project-oracles/` is this project's **Verification Oracle** suite, on its own older contract: `manifest.json` (+ sometimes `manifest.toml`), an `oracle.py` canonical, `oracle.sh`/`oracle.ps1` shims and a `--full` flag. Its consumer is `scripts/run-verification-oracles.sh`, which CI job `verification-oracles` and `scripts/ci-local.sh` both call. The framework runner never sees these. (Moved out of `.claude/oracles/` on 2026-09-08 — sharing the namespace bought one `unknown` row per oracle at every session start, DEFER-010.)

The table says only **what each oracle defends**, so you can tell which invariant your change is about to touch. Each oracle's own `manifest.json` + `README.md` is the authoritative record of its probes and self-test.

| Oracle | What it defends |
|---|---|
| `multi-tenant-isolation-check` | the tenant-scoped repository layer is the sole query path (DEC-FBR-03) |
| `pii-scrub-audit` | submitter PII does not escape into logs or public surfaces |
| `widget-bundle-size` | widget page-load set ≤ 30,720 B, each lazy `dist/locales/<code>.js` ≤ 4,096 B, no third-party trackers (FR-FBR-04, DEC-FBR-02) |
| `tier-enforcement-status` | plan caps fire, free-tier footer, `tier_quotas()` shape (FR-FBR-14, C19) |
| `selfhost-compose-smoke` | `docker compose up` distribution + env-catalog SSOT `docs/operations/SELFHOST_ENV.md` (FR-FBR-17, C21) |
| `cors-allowlist-enforcement` | credentialed CORS stays wired into `build_app`, never wildcard (DEC-FBR-IMPL-09) |
| `approval-gate-enforcement` | no work order reaches ≥ `dispatched` without a prior owner-authored `approved` event (FR-FBR-25a/22) |
| `public-board-moderation-gate` | no public board **read or vote** touches a non-`approved` row; board wire shape leaks no PII (C30) |
| `translation-egress-q24-isolation` | translation provider defaults `off`; no public read of `body_translated` (DEC-FBR-IMPL-25/26) |
| `i18n-catalog-integrity` | catalog shape + generated locale tables (C35, C41) — the exit gate of every localization stage |
| `i18n-literal-ratchet` | **baseline 0** — any new hard-coded user-facing literal in `widget/src` or `admin-ui/src` is a hard failure |

Four more are installed and not listed above: `feedback-erasure-completeness`, `public-route-ceiling`, `public-id-as-capability`, `submission-idempotency`. `bash scripts/run-verification-oracles.sh` runs all seventeen; `ls .claude/project-oracles/` tells you what is actually there.

> **Three oracles this project's prose still names do not exist in the tree**, and never came back with the pack: `host-tenant-binding` (DEC-FBR-13 — the install brief is DEFER-006), `translation-gap-status` (advisory translation-debt reporter, cited by `/1-translate` and `scripts/i18n/README.md`), and `feedback-parity-status` (cited by `docs/specs/SPECIFICATION.md` and `DECISIONS.md`). Treat every reference to them as an unbuilt intention, not a guard.

## Constraints not in spec artifacts

- **The repo is public** at https://github.com/feedbackmonk/feedbackmonk (AGPL-3.0-or-later, full canonical text). Nothing pushed here is private — scrub before you commit, and never paste a machine-local account name or credential into a tracked file (DEFER-003 records this repo doing exactly that once).
- **GitCellar is customer #1, not a dependency.** Never modify GitCellar code from this working tree; the peer repo is a read-only reference (DEC-FBR-07). Cross-repo work is filed as a DEFER brief in that repo.

## Privacy invariants (load-bearing — never silently relax)

- **No third-party trackers in the widget, ever** (no Segment, Mixpanel, GA, Intercom). DEC-FBR-02 brand promise.
- **JWT customer signs is the ONLY identity feedbackmonk ever has** for an end-user (DEC-FBR-04). No callbacks to customer auth providers; no long-lived bearer tokens.
- **Q24 invariant** (FR-FBR-12): a roadmap item promoted from feedback carries the feedback body verbatim, with NO submitter attribution and NO FB-ID reference. The byte-for-byte unit test ported from GitCellar's `roadmap_promote.rs` guards it and is untouchable — same test name, same assertions.

## Pending Follow-Ups

- **Trigger: before the next release** — run `/1-translate`. No translation has ever run, by design (DEC-FBR-17): every non-`en` catalog is a skeleton and every surface falls back to English per key. The gap is large and the `translation-gap-status` oracle reads DUE. This is the release gate; nothing else waits on it.
- **Trigger: your word — two env-var changes, now unblocked** — deploys to `feedback.gitcellar.com` work again and it runs `0.4.0` (`docs/planning/feedbackmonk-deploy-state.md` § Stage F/G), so these are decisions, not blockers: (1) `FEEDBACKMONK_TRANSLATION_PROVIDER` — which provider and whose key, then `POST /api/v1/ops/translation/backfill`; (2) `FEEDBACKMONK_STORAGE_BACKEND=s3` — needs a production bucket plus four variables, and the only object-storage credentials on the machine are a **test** R2 key (the `attachments` table is empty, so nothing is lost meanwhile). The third item that used to sit here, rotating the session secret and ops token, was **done on 2026-09-10** and verified live. Note for whoever does these: each `variableUpsert` triggers its own deploy, so set every variable first and deploy once.
- **Trigger: a cross-product decision — the nightly feedbackmonk backup has no dead-man switch** — since 2026-09-10 the `feedbackmonk` database is dumped nightly at 04:30 UTC by its own Railway cron (`deploy/backup/` is the source of truth, `docs/planning/feedbackmonk-deploy-state.md` § Stage H), encrypted to the same key that restores GitCellar's dumps and read-back-verified on every run. **What is still missing is an alarm**: GitCellar's verify cron watches only its own prefix, so if this job silently stops firing, nothing says so. Closing it means extending that cron or giving this one a heartbeat — filed to GitCellar, and it is their call as much as ours. The job also reuses GitCellar's bucket credentials, so a rotation there breaks this backup.
- **Trigger: your word — ops, not code** — provision `feedbackmonk.com` and cut GitCellar onto it (DEC-FBR-14). FR-FBR-32/33 are built, tested and live on `feedback.gitcellar.com`, but nowhere runs them under the `feedbackmonk.com` root domain. Details: `docs/pending/saas-standup.md`
- **Trigger: `feedbackmonk.com` is live** — FR-FBR-41 (Astro marketing site) is DEFERRED until then.
<!-- /0-uldf-schedule writes here -->

## License footer

feedbackmonk is AGPL-3.0-or-later. Contributors agree via DCO sign-off (no CLA per DEC-FBR-05). Self-host customers receive identical releases to SaaS; there is no proprietary fork.
