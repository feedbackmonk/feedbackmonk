# feedbackmonk — Project Context for Claude Code

Project-specific context. The global ULDF framework guidance lives at `~/.claude/CLAUDE.md` and is the authoritative reference for framework commands, autonomy levels, propagation rules, and the agentic disciplines (Contexturgy, Oraculurgy, Probandurgy). This file ONLY documents what is specific to **feedbackmonk**.

> **Working name changed mid-arc**: project was named "Feedbackr" through P0 and most of P1. The name was changed to **feedbackmonk** on 2026-05-14 per DEC-FBR-11, enacting DEC-FBR-09's squat-contingency clause after `github.com/Feedbackr` and `feedbackr.com` were found taken. Identifier prefixes `DEC-FBR-*` and `FR-FBR-*` are stable — they do NOT rename (see DEC-FBR-11 § Identifier-stability rule). Code-level rename of `feedbackr-*` → `feedbackmonk-*` was completed in PF-RENAME-01. Working-directory rename `Apps\Feedbackr` → `Apps\feedbackmonk` completed in PF-RENAME-02 at the v1 arc-terminus (2026-05-14).

---

## What feedbackmonk is

Standalone open-source SaaS user-feedback platform: submission widget + status-workflow triage + public roadmap with voting + status emails. Multi-product per tenant.

- **Elevator pitch**: *Plausible Analytics for product feedback.*
- **License**: AGPL-3.0-or-later (see `LICENSE` — full canonical AGPL-3.0 text, replaced 2026-05-13).
- **Stage**: P1 Stage 2 complete; P1 finalize / P2 plan upcoming.

## Read first (always, for any session in this repo)

| File | What it tells you |
|---|---|
| `docs/specs/SPECIFICATION.md` | 18 functional requirements (FR-FBR-01..18) across phases P0–P4 |
| `docs/specs/DECISIONS.md` | DEC-FBR-01..11 + DEC-FBR-IMPL-* — load-bearing context for every implementation choice |
| `docs/specs/ARCHITECTURE.md` | System architecture |
| `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md` | Full build-arc plan: phase ordering, gates, interface contracts, Oracle Pre-Build Plan, Testability Gate findings |

The arc plan is the single most important downstream artifact — it pre-commits phase ordering and exit gates but defers intra-phase topology to each phase's own `/0-uldf-ldis-plan` round.

## Stack (load-bearing for tooling decisions)

- **Backend**: Rust. Reference implementation is `gitcellar-cloud/src/feedback/` in the peer GitCellar repo — read-only reference, NOT a base to extract from (DEC-FBR-07).
- **Admin UI / widget**: TypeScript. React for admin UI (P1, port pattern from `gitcellar-cloud/admin-ui/`); vanilla JS+CSS for the embeddable widget (P2, <30KB bundle cap per FR-FBR-04).
- **Marketing site**: Astro (P4 only).
- **Database**: PostgreSQL. Multi-tenant via `tenant_id` + `project_id` on every domain row; tenant-scoped repository layer is the **sole** query path (raw SQL is a security incident — DEC-FBR-03).
- **Distribution**: SaaS + self-host via `docker compose up` (P4).
- **Testing**: Vitest for unit tests, Playwright + axe-core for widget a11y (mandated by P2 Testability Gate finding).
- **Billing**: Polar (P3), pattern from GitCellar's existing setup.

## Dev Ports

- **Frontend**: `14204` (admin UI / React + Vite; bound with `strictPort: true`)
- **Backend API**: `14304` (`feedbackmonk-api` crate; default in `FEEDBACKMONK_PORT` env var)
- **Local Postgres**: `5433` (deconflicted from gitcellar-cloud's `5432`, per DEC-FBR-IMPL-04)

All registered in `~/.claude/MACHINE_CONFIG.md` Dev Port Registry.

## Workflow

- Use `/0-uldf-ldis-plan "feedbackmonk P<N> — <Phase Name>"` at each phase boundary.
- Use `/0-uldf-proceed` at phase boundaries — let it pick HERE / HANDOFF / PODS topology based on context budget and work shape.
- LTADS is **active** in this repo (initialized during P0 Stage 1 auto-init via spec-presence detection).
- Per DEC-FBR-07, this repo is greenfield — there is no source-level dependency on GitCellar. Do NOT modify GitCellar code from this working tree.

## Oracles

This project has `.claude/oracles/` with the universal starter pack + project-specific Verification Oracles. The session-start hook runs every-session fast oracles and emits an ORACLE BRIEFING (git state, LTADS state, project type, pending follow-ups, etc.) — read it before investigating manually. Audit via `/0-uldf-oracle`.

Verification Oracles built so far + scheduled:

| Oracle | Phase | Status |
|---|---|---|
| `multi-tenant-isolation-check` | P0 Task Zero | ✅ LIVE (built P0 Stage 1) |
| `pii-scrub-audit` | P1 | ✅ LIVE (built P1 Stage 1) |
| `widget-bundle-size` | P2 (start) | ✅ LIVE (built P2 Task Zero) — defends <30KB cap (FR-FBR-04) + DEC-FBR-02 no-trackers brand promise as code-level invariants; active-PASS against built `widget/dist/` at 16,829B / 30,720 cap (45% headroom) |
| `tier-enforcement-status` | P3 (start) | ✅ LIVE (built P3 Stage 1 Task Zero) — defends cap-firing + free-tier footer (FR-FBR-14) + Contract C19 `tier_quotas()` shape as code-level invariants; three-probe (AST handler coverage + config-shape + integration smoke gated behind `--full`); active-PASS with Probe C smoke trio (Free 2nd project → 409, Free 51st feedback → 402, widget-config footer flip Free/Pro) |
| `selfhost-compose-smoke` | P4 (start) | ✅ LIVE (built P4 Stage 2 Task Zero) — defends FR-FBR-17 `docker compose up` distribution + Contract C21 env-catalog SSOT (`docs/operations/SELFHOST_ENV.md`) as code-level invariants; three-probe (yaml-lint + env-doc cross-reference against C21 + `--full` clean-state smoke against `/health/ready`); cold-start vacuous-PASS; active-PASS post-Phase-1 with compose env-refs ⊆ C21 catalog + Probe C `/health/ready` 200 in <90s |
| `cors-allowlist-enforcement` | post-v1 (DEC-FBR-IMPL-09) | ✅ LIVE (built 2026-06-03) — defends the credentialed CORS posture on the public widget endpoints (DEC-FBR-IMPL-09 / DEC-FBR-04) as code-level invariants; closes the gap that `tests/cors_preflight.rs` tests the layer in isolation and can't catch `.layer(cors)` wiring removal from `build_app`; two static probes (A: `main.rs` wires the layer from `FEEDBACKMONK_CORS_ORIGINS` to submit + attachments; B: `cors.rs` keeps `allow_credentials` + `AllowOrigin::list`, never wildcard) + `--full` runs the `cors_preflight` integration test; active-PASS |

## Constraints not in spec artifacts

- **LICENSE** is now the full canonical AGPL-3.0 text (replaced 2026-05-13). Repo can be pushed publicly.
- **GitHub org + domain**: **DONE** (PF-REGISTER-01). The `github.com/feedbackmonk` org is registered (2026-05-16) and the public repo `github.com/feedbackmonk/feedbackmonk` is live with `main` pushed (last push 2026-05-17; local `main` in sync with `origin/main`). `feedbackmonk.com` is purchased. First public push is **no longer gated** — `/0-uldf-finalize` no longer requires `--skip-push`, and normal propagation consent rules apply. Remaining is operational, not registration: the domain is not yet pointed at a running deployment (see PF-DEPLOY-01 below).
- **GitCellar peer repo** is in pre-launch hardening. feedbackmonk work neither blocks on nor modifies GitCellar; the only cross-repo touchpoint is late P2 / early P3 when GitCellar embeds feedbackmonk's widget as customer #1 (forward-looking integration, NOT extraction).

## Privacy invariants (load-bearing — never silently relax)

- **No third-party trackers in the widget, ever** (no Segment, Mixpanel, GA, Intercom). DEC-FBR-02 brand promise.
- **JWT customer signs is the ONLY identity feedbackmonk ever has** for an end-user (DEC-FBR-04). No callbacks to customer auth providers; no long-lived bearer tokens.
- **Q24 invariant** (FR-FBR-12, P2): roadmap items promoted from feedback contain the feedback body verbatim with NO submitter attribution and NO FB-ID reference. Port the byte-for-byte unit test from GitCellar's `roadmap_promote.rs` — same test name, same assertions. Document as untouchable in the module README.

## Pending Follow-Ups

<!-- /0-uldf-schedule writes here -->

### ~~PF-RENAME-01: Cargo / env-var / package-name rename `feedbackr-*` → `feedbackmonk-*`~~ — DONE

Completed in a single atomic commit at the P1-finalize → P2-plan boundary. Scope delivered:
- Cargo workspace + all 6 member crate `[package].name` + every `[dependencies]` path reference
- Env var prefix `FEEDBACKR_` → `FEEDBACKMONK_` across code, scripts, docs, `.env.example`
- HTTP header constant `X-Feedbackr-Anon-Cookie` → `X-Feedbackmonk-Anon-Cookie` (`feedbackmonk-anon::ANON_COOKIE_HEADER`)
- Session cookie name `feedbackr_session` → `feedbackmonk_session`
- `admin-ui/package.json` name + Vite/CI db name (`feedbackr_test` / `feedbackr_dev` → `feedbackmonk_*`)
- `.sqlx/` offline cache regenerated and re-committed
- Both Verification Oracles GREEN after path updates (`multi-tenant-isolation-check` + `pii-scrub-audit`)
- Plan-file rename: `20260513T185711-feedbackr-v1-build-arc.md` → `…-feedbackmonk-v1-build-arc.md` (+ P0/P1 plan files)
- ID stability preserved: `DEC-FBR-*` and `FR-FBR-*` left untouched per DEC-FBR-11.

### ~~PF-RENAME-02: Working-directory rename `Apps\Feedbackr` → `Apps\feedbackmonk`~~ — DONE

Executed at the v1 arc-terminus (2026-05-14). Scope delivered:
- `Rename-Item "E:\Developer\SourceControlled\Apps\Feedbackr" "feedbackmonk"` (user-action; Windows blocks renaming a CWD, so executed after closing the last Claude session in the directory).
- `~/.claude/MACHINE_CONFIG.md` Dev Port Registry row path updated `Apps\Feedbackr` → `Apps\feedbackmonk` (port numbers + project name unchanged).
- Living docs path references updated in the same commit (CLAUDE.md banner, SPECIFICATION.md Repository home, ARCHITECTURE.md, PROJECT_TRAJECTORY.md Next-Best-Steps).
- Historical records left intact per DEC-FBR-11 identifier-stability rule (planning/intakes, commit-log, decision-record narrative, OPEN_QUESTIONS resolution narrative).
- No git remote existed at rename time (PF-REGISTER-01 still pending), so no remote-URL update required.

### ~~PF-RENAME-03: Local dev container rename `feedbackr-*-dev` → `feedbackmonk-*-dev`~~ — DONE

Executed 2026-05-15 post-arc-terminus. Scope delivered:
- `docker rename feedbackr-pg-dev feedbackmonk-pg-dev` (Postgres dev container on port 5433; `DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev` unchanged).
- `docker rename feedbackr-mailpit-dev feedbackmonk-mailpit-dev` (Mailpit SMTP-capture dev container on ports 1025/8025; ad-hoc dev container originally created during P1 status-emails work, not under `deploy/docker/docker-compose.yml` control).
- `ltads/execution/development-brief.md` constraint row updated to reflect new container name (the row had explicitly flagged the rename as a future item).
- `docs/operations/LOCAL_DEV.md` already prescribed `feedbackmonk-pg-dev` (updated in PF-RENAME-01); the rename brings live state into agreement with the doc.
- Concluded LTADS session records (`current-session.md`, `commit-log.md`, etc.) left intact per append-only history rule — they correctly describe the container name as it was during the concluded session.
- Stale gitignored routing artifacts cleaned up: `.claude/handoff/handoff-*.md` (14 unpinned files referencing dead `crates/feedbackr-*` paths) and `.claude/session-state/finalize-session-files-S001-*.json` / `-S002-*.json` / `-p4-stage1.json` (per-session caches referencing pre-rename paths). All gitignored — local hygiene only, no commit churn.

### ~~Documentation rename fixup (PF-RENAME-FIXUP)~~ — DONE

Executed 2026-05-15 in commit `b73a7b4`. Fixed two categories of issues introduced by PF-RENAME-02's path-rename sweep:
- **Over-rename** (6 fixes): historical "Feedbackr"/`github.com/Feedbackr`/`FEEDBACKR_*` references in `README.md`, `DECISIONS.md` DEC-FBR-11, `OPEN_QUESTIONS.md` Q9 had been corrupted to `feedbackmonk`/`github.com/feedbackmonk`/`FEEDBACKMONK_*`, inverting the meaning of the squat-contingency narrative.
- **Stale forward-references** (6 fixes): `feedbackr.com` → `feedbackmonk.com` (public roadmap URL in DECISIONS.md, Cloudflare deploy landing, scope-table row 16, P4 exit-gate line in arc plan); `feedbackr-tier-quotas` oracle name → `feedbackmonk-tier-quotas` (SPECIFICATION.md); planned P3 webhook signing headers `x-feedbackr-*` → `x-feedbackmonk-*` (DISCOVERIES.md D-FBR-07).

### ~~PF-REGISTER-01: Register `github.com/feedbackmonk` org + buy `feedbackmonk.com`~~ — DONE

Completed by user action (verified 2026-06-02 via `gh api`):
- `github.com/feedbackmonk` org registered 2026-05-16.
- Public repo `github.com/feedbackmonk/feedbackmonk` created (public, default branch `main`) and pushed — last push 2026-05-17; local `main` (`5bf9878`) in sync with `origin/main` (ahead 0 / behind 0). The `origin` remote is configured locally.
- `feedbackmonk.com` purchased.
- **Effect**: the first-public-push gate is cleared. `/0-uldf-finalize` no longer needs `--skip-push`; normal propagation-consent rules apply.

### PF-DEPLOY-01: Stand up a reachable feedbackmonk instance for the GitCellar integration (decision + ops)

**Trigger**: when wiring GitCellar (customer #1) to embed the feedbackmonk widget.

The v1 code is content-complete and the full embed→auth→submit loop is built and tested — GitCellar needs **no further feedbackmonk feature work**. What it needs is a *running, reachable* instance. Two hosting models (decision pending — depends on infra preference):

- **Self-host (recommended for GitCellar-ASAP)**: GitCellar runs the stack via `docker compose up` (FR-FBR-17, smoke-tested to `/health/ready`) on a GitCellar-controlled host (e.g. `feedback.gitcellar.com`). Widget `<script src>` + API base point there. **Does NOT require `feedbackmonk.com` to be live.** Runbook: `docs/operations/SELFHOST.md` + `SELFHOST_ENV.md`.
- **SaaS**: deploy feedbackmonk behind `api.feedbackmonk.com` + `cdn.feedbackmonk.com`; point `feedbackmonk.com` DNS at it. More infra/ops; needed only if feedbackmonk is offered as a hosted service rather than self-hosted by GitCellar.

Integration handshake either way (all built): customer signs up → gets `project_id` → registers an Ed25519 **public** key (`POST /api/v1/projects/{id}/signing-keys`, Contract C4) → mints EdDSA JWTs (`sub`/`iat`/`exp`/`aud`=project_id; Contract C2) → embeds widget with `data-project-id` + `data-jwt`.

The separate Astro **marketing site** (`feedbackmonk.com` landing page, FR-FBR-16) is product marketing — not required for GitCellar's functional integration.

---

## License footer

feedbackmonk is AGPL-3.0-or-later. Contributors agree via DCO sign-off (no CLA per DEC-FBR-05). Self-host customers receive identical releases to SaaS; there is no proprietary fork.
