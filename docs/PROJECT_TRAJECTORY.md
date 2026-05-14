# Project Trajectory — feedbackmonk

Rolling high-level state. Auto-maintained by `/0-uldf-finalize` Phase 12. Cheap orientation for fresh sessions; for detail, go to `docs/specs/` and `docs/planning/`.

**Last updated**: 2026-05-14 (P3 Stage 1 CLOSED — backend tier model + cap-firing predicate + free-tier footer + admin tier-status endpoint + `tier-enforcement-status` Verification Oracle ship in one mid-arc-checkpoint commit; 302 workspace tests pass; all 4 Verification Oracles GREEN; Polar deferred per DEC-FBR-DEFER-01; Stage 2 admin UI ahead)

> **Working name**: "Feedbackr" through P0/P1; renamed to **feedbackmonk** on 2026-05-14 per DEC-FBR-11. Identifier prefixes `FR-FBR-*` / `DEC-FBR-*` are stable per the ID-stability rule. Code-level `feedbackr-*` → `feedbackmonk-*` rename committed in PF-RENAME-01 (commit `82a2e59`). Pending user actions: PF-RENAME-02 (working-dir rename) + PF-REGISTER-01 (org+domain registration).

---

## Current Focus

**P3 Stage 1 CLOSED — backend commercial-gate ships in one mid-arc-checkpoint commit.** Next: Stage 2 (admin UI surface for tier display + stub Upgrade button) auto-spawns via `/0-uldf-proceed` at this commit's tail.

P3 Stage 1 (orchestrator session-20260514-102233-006 → worker session, single-worker HANDOFF topology) delivers the full backend commercial-gate in one staged commit: (a) `Tier` enum + `tier_quotas()` const fn = single source of truth for the four-tier matrix (Contract C19); (b) `TierQuotaRepo::check_tier_quota(scope, resource)` chokepoint wired pre-INSERT in projects + feedback handlers (Contract C17); (c) `ApiError::TierCapExceeded` 402/409 mapping per Contract C18; (d) tier-aware `TenantRepo::get_widget_brand` enforcing FR-FBR-14 free-tier footer at the brand-render boundary; (e) `GET /api/v1/admin/tier` admin status endpoint; (f) `tier-enforcement-status` Verification Oracle with three probes (Probe A handler coverage AST + Probe B Contract-C19 shape + Probe C `--full` integration smoke trio actively-passing); (g) `migrations/00008_tenant_tier_check.sql` defense-in-depth schema CHECK; (h) `docs/operations/TIER_OVERRIDE.md` operator dogfood SQL helper; (i) `docs/deferred/polar-integration.md` deferred-stub for FR-FBR-15. Polar billing is **deferred** per DEC-FBR-DEFER-01 (user direction at plan boundary — no consumer-billing pressure on P3 exit).

- **P3 Stage 1 exit gate**: PASSED — 302 workspace tests pass (P2 closed at 271; delta +31 net-new: tier model + tier_quota + admin_tier + smoke trio), clippy clean (`--all-targets -- -D warnings`), all 4 Verification Oracles GREEN (`multi-tenant-isolation-check`, `pii-scrub-audit`, `widget-bundle-size`, `tier-enforcement-status` active-PASS including Probe C `--full` smoke trio), FR-FBR-14 backend portion DONE in `docs/specs/SPECIFICATION.md`, FR-FBR-15 DEFERRED.

P2 convergence (commit `9f1a28b`) delivered the customer-facing surface (widget + public roadmap + Q24 promote) — preserved here as historical context.

## Active Threads

- **P3 Stage 1 CLOSED — backend commercial-gate** (this commit): orchestrator HANDOFF to single worker; worker delivers tier model + cap-firing predicate + admin status endpoint + Verification Oracle + deferred-Polar stub. Mid-arc checkpoint per CSI-03 (Stage 2 ahead).
- **P3 Stage 2 — UPCOMING**: admin UI surface for tier display (read-only `GET /api/v1/admin/tier` consumer in `admin-ui/`) + stub "Upgrade" button (points at email until Polar lands). Auto-spawns via `/0-uldf-proceed` at this commit's tail.
- **P2 CLOSED — convergence commit** (commit `9f1a28b`): three-worker PODS fan-out converges. CLAUDE-A widget + Task Zero oracle + Contract C12; CLAUDE-B migrations 00006/00007 + repo layer + voting cache + 8 HTTP endpoints (C13/C14/C15); CLAUDE-C promote handler with Q24 byte-for-byte ports + admin-ui Roadmap pages (C16). All 4 FR-FBR P2 requirements flipped DONE.
- **PF-RENAME-01 — DONE** (commit `82a2e59`): code-level brand rename `feedbackr-*` → `feedbackmonk-*` at the P1→P2 quiescent boundary.
- **P1 CLOSED — arc-terminus** (commit `835fbf8`): Stage 3 e2e witness + critic C-002 + 4 module READMEs. 218 workspace tests; FR-FBR-07/08/09/10 all DONE.
- **P1 Stage 2 — DONE** (commit `d6f247a`): admin transition + reply HTTP (C7), admin list + detail HTTP (C8), `feedback_replies` migration 00004, three plain-text email templates (C10), Mailpit integration test, `admin-ui/` React+Vite directory + Playwright+axe a11y smoke.
- **P0 Foundation — COMPLETE** (commit `b9a672a`): all 5 P0 FRs DONE; e2e P0 PASS 7/7.
- **Pending pre-public-push gates (USER ACTION)**:
  - **PF-REGISTER-01** — Register `github.com/feedbackmonk` org + buy `feedbackmonk.com` (~$10/yr). Confirmed AVAILABLE 2026-05-14; not yet claimed. Push remains GATED until this clears.
  - **AGPL LICENSE**: full AGPL-3.0 text in `LICENSE` (replaced 2026-05-13) — gate CLEARED on the LICENSE side.
- **Pending follow-ups**:
  - **PF-RENAME-02 (USER ACTION, pending)** — Working-directory rename `Apps\Feedbackr` → `Apps\feedbackmonk`. Requires user action (Windows cannot rename CWD of live process). Surfaced at this quiescence boundary.
  - **DEC-PODS-LEAD-01 framework improvement** — `monitor-pods.ps1` regex should accept `CONVERGENCE-READY` alongside `COMPLETED` as a terminal status label (TIMEOUT artifact observed at convergence; not a real blocker).
  - **rollup-win32-x64-msvc npm bug 4828 workaround** — when widget CI lands, automate the manual binary-extraction fallback.
- **LTADS S001** — CONCLUDED at P1 close (commit `835fbf8`). P2 convergence ran outside LTADS (LTADS-not-active path through `/0-uldf-pods-converge --finalize`).
- **GitCellar widget-embed touchpoint — DEFERRED to late P2 / early P3**: first cross-repo integration when GitCellar embeds the feedbackmonk widget as customer #1.

## Recent Decisions

- **DEC-FBR-DEFER-01** (P3 Stage 1) — Polar billing (FR-FBR-15) **deferred** from P3 per user direction at plan boundary. P3 ships tier model + cap enforcement + free-tier footer + admin tier-status endpoint per FR-FBR-14 but NOT a Polar webhook receiver / customer subscription schema / self-service upgrade flow. Stage 2 admin "Upgrade" button is a stub. Operator promotion via SQL helper at `docs/operations/TIER_OVERRIDE.md` until Polar lands. Webhook envelope + event-mapping + schema migration shape captured in `docs/deferred/polar-integration.md` for future port from `gitcellar-cloud/src/billing/`.
- **D-FBR-19 / D-FBR-20 / D-FBR-21** (P3 Stage 1) — Three new generalizable patterns surfaced: schema-CHECK + Rust-codec defense-in-depth pairing for tier-shaped enumerated columns (catches structurally-different drift); eager Probe C smoke-test authoring upgrades vacuous-PASS oracle to active-PASS at marginal cost; test-mod artifact YAML `tests_modified[]` frontmatter as upfront enumeration + cross-check (active defense against D-FBR-17 missed-fixture-site failure mode). All three documented in `docs/specs/DISCOVERIES.md`.
- **Worker chose AND, not OR** (P3 Stage 1) — Plan offered "schema CHECK OR Rust strict parser." Worker chose both. Schema CHECK catches direct DB writes that bypass Rust; sqlx codec catches programmatic mistakes earlier in the loop. Documented in `migrations/00008_tenant_tier_check.sql` header + `feedbackmonk-core/README.md` Decision Log.
- **DEC-PODS-A-01 / A-02** (P2 convergence) — Widget tracker hostname list expanded 8 → 18 (additive only, sha256 `7823d6e6…`; defends DEC-FBR-02 brand promise against wider surface than seed list); widget e2e port 14206 deconflicted from CLAUDE-C's 14205 admin-ui e2e (DEC-FBR-IMPL-04 port-deconflict pattern). Both RATIFIED at convergence; port row propagated to `~/.claude/MACHINE_CONFIG.md` Dev Port Registry.
- **DEC-PODS-B-01 / B-02 / B-03** (P2 convergence) — Allowlist structural-mirror constructor entries for `SqlxRoadmapItemRepo::new` + `SqlxRoadmapVoteRepo::new`; AppState fixture extensions justified via `docs/test-modifications/20260514-p2-appstate-roadmap-fields.md` (extended at convergence to cover all 4 co-edits — 3 AppState extensions + 1 `TenantRepo::get_widget_brand` mock fill-in); cache + helper widenings (60s default retraction window, `LEFT JOIN` in `aggregate_vote_counts`, defense-in-depth `from_db_str` Anon fallback). All RATIFIED.
- **DEC-PODS-C-01 / C-02 / C-03 / C-04** (P2 convergence) — `RoadmapItem` TypeScript optional widenings (`origin_feedback_id?`, `voted_by_me?`); admin roadmap URL omits project segment (`/admin/roadmap`; sole-project resolution via `fetchAdminProjects`); promote module README inlined as Rust `//!` module-doc (single-file module convention); 4 net-new slug-helper tests beyond gitcellar's 6 Q24 byte-for-byte ports (orthogonal to Q24 — slug helper is feedbackmonk-only). All RATIFIED.
- **DEC-PODS-LEAD-01** (P2 convergence) — Convergence proceeds despite `monitor-pods.ps1` TIMEOUT (regex artifact, not real blocker — monitor matched only `COMPLETED` and missed CLAUDE-A's `CONVERGENCE-READY` label). All exit-gate items GREEN per worker reports. Framework-level improvement candidate documented as follow-up.
- **PF-RENAME-01 executed** (commit `82a2e59`) — Mechanical brand-rename at the P1→P2 quiescent boundary: kebab `feedbackr-*` → `feedbackmonk-*`, env prefix `FEEDBACKR_` → `FEEDBACKMONK_`, header `X-Feedbackr-Anon-Cookie` → `X-Feedbackmonk-Anon-Cookie`, session cookie `feedbackr_session` → `feedbackmonk_session`, BLAKE3 domain separator `b"feedbackr-anon-v1"` → `b"feedbackmonk-anon-v1"`. ID prefixes `DEC-FBR-*` / `FR-FBR-*` preserved per DEC-FBR-11. Crate renames via `git mv` (history preserved).

## Risks

| Risk | Stage | Notes |
|---|---|---|
| **GitHub org + domain registration** | Pre-public | User-action pending (PF-REGISTER-01). Push remains GATED on this until cleared. Both `github.com/feedbackmonk` and `feedbackmonk.com` confirmed AVAILABLE 2026-05-14; `gh api orgs --method POST -f login=feedbackmonk` + `feedbackmonk.com` purchase (~$10/yr). |
| **Q24 byte-for-byte invariant drift** | P2 → forever | **MITIGATED** — `promote.rs::tests` module has `#[allow(clippy::uninlined_format_args, clippy::doc_markdown)]` to preserve byte-for-byte invariant against future lint drift; ULADP module-doc explicitly marks render functions + 6 ported tests as UNTOUCHABLE. Drift is unlikely without deliberate edit. |
| **In-memory anonymous rate-limiter loses state on restart** | P0 (deferred to v1.1) | Acceptable for single-instance dogfood; non-breaking Redis backend swap planned for v1.1. See DISCOVERIES.md D-FBR-08. |
| **Voting cache cold-start cost** | P2 → P3 | First request for a project after restart triggers lazy warming; 60s thereafter cached. If P3 launches show cold-start latency issues, consider eager warming for top-N projects at boot. |
| **Widget node_modules rollup npm bug 4828** | P2 dev / CI | **MITIGATED locally** — manual binary extraction via `npm pack` documented in CONVERGENCE_REPORT. CI will need automation when widget e2e CI lands. |
| **PII scrub drift** | P1+ | **MITIGATED** — `pii-scrub-audit` oracle GREEN at every convergence. |
| **Multi-tenant isolation drift** | P0+ | **MITIGATED** — `multi-tenant-isolation-check` oracle GREEN; allowlist additions are structural-mirror constructor entries only. |
| **`tenant_users` table doesn't exist yet** | P1/P2 carry-forward | Migration 00004's `author_user_id` is bare UUID with no FK. Add FK with `ALTER TABLE … ADD CONSTRAINT … NOT VALID; VALIDATE CONSTRAINT …` when `tenant_users` lands (P3 multi-admin work, FR-FBR-15). |
| **GitCellar peer repo coordination** | Late P2 / early P3 | First cross-repo touchpoint imminent when GitCellar embeds the feedbackmonk widget as customer #1. Forward-looking only; not a P2 blocker. |

## Next-Best-Steps

1. **P3 Stage 2 (auto-spawn via `/0-uldf-proceed`)** — Admin UI surface: read-only tier display (consumes `GET /api/v1/admin/tier`), per-tier capability matrix presentation, stub "Upgrade" button (points at email contact per DEC-FBR-DEFER-01 until Polar lands). Plan reference: `docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md` §Stage 2. Handoff brief: `docs/planning/handoffs/p3-stage1-to-stage2.md` (Contracts C17/C18/C19 frozen verbatim + TS starter kit).
2. **PF-REGISTER-01 (USER ACTION)** — Register `github.com/feedbackmonk` org and purchase `feedbackmonk.com` before any first public push. Both confirmed AVAILABLE 2026-05-14; `gh api orgs --method POST -f login=feedbackmonk` or web UI.
3. **PF-RENAME-02 (USER ACTION)** — Working-directory rename `Apps\Feedbackr` → `Apps\feedbackmonk`. Must run with no live Claude Code sessions in the directory. PowerShell: `Rename-Item "E:\Developer\SourceControlled\Apps\Feedbackr" "feedbackmonk"`. Update `~/.claude/MACHINE_CONFIG.md` Dev Port Registry row afterward.
4. **P4 plan** — `/0-uldf-ldis-plan "feedbackmonk P4 — Go-Public"` after P3 closes. Scope: marketing site (FR-FBR-16 Astro), self-host distribution (FR-FBR-17 docker compose), Polar resurrection if consumer-billing pressure emerges (FR-FBR-15 — port from `docs/deferred/polar-integration.md`).
5. **Widget CI hardening** — automate the rollup-win32-x64-msvc fallback when widget e2e CI lands (npm bug 4828).
