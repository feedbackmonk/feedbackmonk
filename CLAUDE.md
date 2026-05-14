# feedbackmonk — Project Context for Claude Code

Project-specific context. The global ULDF framework guidance lives at `~/.claude/CLAUDE.md` and is the authoritative reference for framework commands, autonomy levels, propagation rules, and the agentic disciplines (Contexturgy, Oraculurgy, Probandurgy). This file ONLY documents what is specific to **feedbackmonk**.

> **Working name changed mid-arc**: project was named "Feedbackr" through P0 and most of P1. The name was changed to **feedbackmonk** on 2026-05-14 per DEC-FBR-11, enacting DEC-FBR-09's squat-contingency clause after `github.com/Feedbackr` and `feedbackr.com` were found taken. Identifier prefixes `DEC-FBR-*` and `FR-FBR-*` are stable — they do NOT rename (see DEC-FBR-11 § Identifier-stability rule). Code-level rename of `feedbackr-*` → `feedbackmonk-*` was completed in PF-RENAME-01 (commit on this branch). The on-disk working directory is still `E:\Developer\SourceControlled\Apps\Feedbackr` for now — working-dir rename is tracked in Pending Follow-Ups as PF-RENAME-02 (user action; Windows blocks renaming a CWD).

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

## Constraints not in spec artifacts

- **LICENSE** is now the full canonical AGPL-3.0 text (replaced 2026-05-13). Repo can be pushed publicly.
- **GitHub org + domain**: as of 2026-05-14, `github.com/feedbackmonk` and `feedbackmonk.com` are confirmed AVAILABLE but **not yet registered**. User action: register the org via `gh api orgs --method POST -f login=feedbackmonk` (or via web UI) and purchase `feedbackmonk.com` (~$10/yr) before first public push.
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

### PF-RENAME-02: Working-directory rename `Apps\Feedbackr` → `Apps\feedbackmonk` (requires user action)

**Trigger**: when the autopilot:continuous chain reaches quiescence (P1 fully finalized, no live PODS workers, no live LD session). The agent that recorded DEC-FBR-11 could not perform this rename — Windows does not allow renaming a directory that is any process's CWD, and the working dir was CWD for the recording agent and all live sibling sessions.

**Steps** (user-executable PowerShell):
1. Verify no live sessions: `~/.claude/oracles/dispatchable-sessions/run.ps1` should return `count: 0` (or only stale handoff-successors that can be killed).
2. Close all Claude Code terminals open in `E:\Developer\SourceControlled\Apps\Feedbackr`.
3. `Rename-Item "E:\Developer\SourceControlled\Apps\Feedbackr" "feedbackmonk"`
4. Update `~/.claude/MACHINE_CONFIG.md` Dev Port Registry: change the row's project path from `Apps\Feedbackr` to `Apps\feedbackmonk` (port numbers + project name unchanged).
5. If a git remote was set pointing at the old path, update it.
6. Re-open Claude Code at the new path.

### PF-REGISTER-01: Register `github.com/feedbackmonk` org + buy `feedbackmonk.com` (user action)

**Trigger**: before first public push.
- `gh api orgs --method POST -f login=feedbackmonk -f admin=<your-username>` (or web UI at https://github.com/account/organizations/new)
- Purchase `feedbackmonk.com` via Namecheap/Cloudflare/etc. (~$10-15/yr)
- Optionally also `feedbackmonk.app` and `feedbackmonk.dev` (both currently AVAILABLE per 2026-05-14 RDAP scan)

---

## License footer

feedbackmonk is AGPL-3.0-or-later. Contributors agree via DCO sign-off (no CLA per DEC-FBR-05). Self-host customers receive identical releases to SaaS; there is no proprietary fork.
