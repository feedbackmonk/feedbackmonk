# Project Trajectory — Feedbackr

Rolling high-level state. Auto-maintained by `/0-uldf-finalize` Phase 12. Cheap orientation for fresh sessions; for detail, go to `docs/specs/` and `docs/planning/`.

**Last updated**: 2026-05-14 (P1 Stage 2 mid-arc checkpoint — Status Workflow + Admin UI DONE; Stage 3 e2e + ULADP cleanup pending)

---

## Current Focus

**P1 Stage 3** (single agent, in converging session) after Stage 2 mid-arc checkpoint commit.

Stage 2 PODS session `collab-20260514-001500` converged with critic PASS verdict. Workers delivered the full closes-the-loop slice: admin transition + reply endpoints (Contract C7), list + detail endpoints (Contract C8), `feedback_replies` migration 00004, three plain-text email templates (FR-FBR-09 via Contract C10), and the React+Vite admin UI on port 14204 (FR-FBR-07). Five self-mediated widenings ratified by LD.

- **Stage 3 brief**: `scripts/e2e-p1-curl.sh` witness extension (signup → submit → admin login → list → transition → status-email-observed → public-reply → public-reply-email-observed) + carry-forward critic C-002 (axum-Router-level submission integration tests) + 5 missing module READMEs (feedbackr-anon, feedbackr-jwt, feedbackr-api/{auth,handlers}; the new email module already shipped its README with Stage 2).
- **P1 exit gate**: e2e-p1-curl.sh PASS + both Verification Oracles GREEN + all P1 FRs (07/08/09/10) DONE in `docs/specs/SPECIFICATION.md`.

## Active Threads

- **P1 Stage 2 — DONE** (commit pending this finalize): admin status transition + reply HTTP (C7), admin list + detail HTTP (C8), `feedback_replies` migration 00004, three plain-text email templates with tenant-brand parameterization (C10), Mailpit integration test, new `admin-ui/` React+Vite directory with state-machine-aware UI + Playwright+axe a11y smoke. 185 → 213 backend tests (+28) + 13 admin-ui Vitest tests + 1 Playwright a11y smoke. Both Verification Oracles GREEN. Critic verdict PASS.
- **P1 Stage 3 pending**: `scripts/e2e-p1-curl.sh` + critic C-002 + 5 module READMEs.
- **P1 Stage 1 — DONE** (commit `f63c66b`): `pii-scrub-audit` oracle, `feedbackr-tracing` crate, migrations 00003/00005, repository extensions, frozen contracts handoff doc.
- **P0 Foundation — COMPLETE** (commit `b9a672a`): all 5 P0 FRs DONE; e2e P0 PASS 7/7.
- **AGPL LICENSE pre-public-commit ratification gate — PENDING USER ACTION**: LICENSE file still a stub; repo stays local-only until user replaces with full AGPL-3.0 text + finalizes GitHub org + domain registration. P1 work continues local-only.
- **LTADS S001** — autopilot:continuous, mid-arc. Arc grant valid until 2026-05-14T21:06:21Z; continues into P1 Stage 3.
- **GitCellar widget-embed touchpoint — DEFERRED to late P2 / early P3**.

## Recent Decisions

- **DEC-PODS-003 / DEC-PODS-004** (P1 Stage 2) — Executor-aware overloads `FeedbackStatusHistoryRepo::append_in_executor` + `FeedbackRepo::update_status_in_executor` for Contract C6 Hard Invariant #4 (atomic audit row + status column in same transaction). LD-ratified at convergence; both methods stay scope-first (Probe B clean). DEC-PODS-004's `FOR UPDATE` lock adds TOCTOU defense — candidate for D-FBR-12 promotion at Stage 3 finalize.
- **DEC-PODS-005** (P1 Stage 2) — Inline `insta` snapshots for the 6 email template fixtures (3 templates × 2 brand fixtures), not file-on-disk. Functionally identical; improves PR-review locality.
- **DEC-PODS-006** (P1 Stage 2) — `SqlxFeedbackReplyRepo::new` added to `multi-tenant-isolation-check/allowlist.toml`. Structural mirror of Stage 1's pre-authorized `SqlxFeedbackStatusHistoryRepo::new` entry (constructor stores `PgPool`; no queries). LD-ratified at convergence.
- **Stage 2 same-branch (not worktree)** — `project-runtime-state` oracle reported `antiFitScore=0` (fit-positive); same-branch chosen per the suggestion-only invariant (user did not pass `--worktrees`). Workers' file-touches were perfectly disjoint (Worker A in `crates/`, Worker B in `admin-ui/`); zero file conflicts as predicted.
- **Contracts C6 / C7 / C8 / C9 / C10 / C11 frozen** (P1 Stage 1) — captured verbatim in `docs/planning/handoffs/p1-stage1-to-stage2.md`; Stage 2 consumers preserved them byte-for-byte (critic confirmed Rust ↔ TS mirror parity at convergence).
- **Write-boundary scrubbing chokepoint** (P1 Stage 1) — `ScrubbingMakeWriter` instead of `tracing_subscriber::Layer`. Stage 2 status-emit paths inherited scrubbing automatically per design.

## Risks

| Risk | Stage | Notes |
|---|---|---|
| **AGPL LICENSE stub** | Pre-public | User-action: replace `LICENSE` file with full AGPL-3.0 text before first public push. Repo MUST stay local-only until then per DEC-FBR-05 + project CLAUDE.md `--skip-push` invariant. |
| **GitHub org + domain registration** | Pre-public | User-action pending. Working name "Feedbackr" through P3; brand pass at P4 (DEC-FBR-09). |
| **In-memory anonymous rate-limiter loses state on restart** | P0 (deferred to v1.1) | Acceptable for P0 single-instance dogfood; non-breaking Redis backend swap planned for v1.1. See DISCOVERIES.md D-FBR-08. |
| **PII scrub drift** | P1+ | **MITIGATED** — `pii-scrub-audit` oracle GREEN. Stage 2 status-emit paths inherited the chokepoint automatically (verified by oracle Probe A clean run at convergence). |
| **Admin UI display-id format hard-coded** | P1 → P3 | Stage 2 kept P0's `FB-XXXXXX` random alphanumeric `short_code`. If P3 ops feedback drives a switch to numeric `FB-NNNNNN`, mirror in `admin-ui/src/shared/types.gen.ts` comments simultaneously. |
| **`tenant_users` table doesn't exist yet** | P1 carry-forward | Migration 00004's `author_user_id` is bare UUID with no FK. Document at Stage 3 finalize; add FK with `ALTER TABLE … ADD CONSTRAINT … NOT VALID; VALIDATE CONSTRAINT …` when `tenant_users` lands (P3 multi-admin work, FR-FBR-15). |
| **GitCellar peer repo coordination** | Late P2 / early P3 | First cross-repo touchpoint when GitCellar embeds the widget. Forward-looking only; not a P1 blocker. |

## Next-Best-Steps

1. **User action** — replace `LICENSE` stub with full AGPL-3.0 text; register GitHub org + domain. (Orthogonal to P1 implementation.)
2. **Stage 3 in converging session** — author `scripts/e2e-p1-curl.sh` extending `scripts/e2e-p0-curl.sh` with 8-step closes-the-loop pipeline (signup → submit → admin login → list contains FB-id → transition to triaged → poll Mailpit for status-change email → reply public → poll Mailpit for public-reply email). Co-locate `crates/feedbackr-api/tests/router_submission_integration.rs` (carry-forward critic C-002, ~3-5 tests at axum Router level). Author 5 module READMEs: `crates/feedbackr-anon/`, `crates/feedbackr-jwt/`, `crates/feedbackr-api/src/auth/`, `crates/feedbackr-api/src/handlers/` (and verify `crates/feedbackr-api/src/email/README.md` already shipped). Stage 3 routes via `/0-uldf-proceed` per autopilot:continuous chain.
3. **P1 exit gate** → `/0-uldf-finalize --skip-push --complete-arc` (arc-terminus this time) → `/0-uldf-ltads-stop`.
4. After P1 close: `/0-uldf-ldis-plan "Feedbackr P2 — Customer-Facing"` (widget on 14204, bundle-size oracle).
