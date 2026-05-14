# Project Trajectory — feedbackmonk

Rolling high-level state. Auto-maintained by `/0-uldf-finalize` Phase 12. Cheap orientation for fresh sessions; for detail, go to `docs/specs/` and `docs/planning/`.

**Last updated**: 2026-05-14 (PF-RENAME-01 executed at the P1→P2 quiescent boundary — code-level `feedbackr-*` → `feedbackmonk-*` rename complete; 218 tests still pass; both Verification Oracles GREEN)

> **Working name**: "Feedbackr" through most of P1; renamed to **feedbackmonk** on 2026-05-14 per DEC-FBR-11. Identifier prefixes `FR-FBR-*` / `DEC-FBR-*` are stable and do NOT rename per the ID-stability rule. Code-level `feedbackr-*` → `feedbackmonk-*` rename **executed in this commit** (PF-RENAME-01 DONE). Remaining: PF-RENAME-02 (working-directory rename, user-action) + PF-REGISTER-01 (org+domain registration, user-action).

---

## Current Focus

**P1 CLOSED — autopilot:continuous arc reaches planned terminus.** Next phase: P2 (Customer-Facing) opens via fresh `/0-uldf-ldis-plan` after the deferred Cargo/env-var/working-dir rename (PF-RENAME-01 + PF-RENAME-02) executes in the next quiescent boundary.

Stage 3 delivered the P1 exit-gate triple in a single converging session: (a) `scripts/e2e-p1-curl.sh` witness extending `e2e-p0-curl.sh` with the full closes-the-loop pipeline (signup → submit → admin verify-email → list → transition → status-email-observed → public-reply → public-reply-email-observed; Mailpit assertions skip-gracefully when SMTP catcher unavailable); (b) `crates/feedbackmonk-api/tests/router_submission_integration.rs` covering 5 axum-Router-level cases (JWT happy path, anon happy path, **401 on `alg=none`** — Contract C2 invariant 1 load-bearing JWT-attack defense, 429 rate-limit via custom AnonGate, 400 empty body) — closing carry-forward critic C-002 from the P1 Stage 2 PODS verdict; (c) four module READMEs (`feedbackmonk-anon`, `feedbackmonk-jwt`, `feedbackmonk-api/src/auth`, `feedbackmonk-api/src/handlers`) with ULADP Agent Context Headers + Decision Logs capturing load-bearing invariants.

- **P1 exit gate**: PASSED — 218 workspace tests pass, clippy clean, both Verification Oracles GREEN, all P1 FRs (07/08/09/10) DONE in `docs/specs/SPECIFICATION.md`.

## Active Threads

- **P1 CLOSED — arc-terminus** (this commit): Stage 3 e2e-p1-curl.sh + router_submission_integration.rs (critic C-002) + 4 module READMEs land. 213 → 218 workspace tests (+5 router cases). Both Verification Oracles GREEN. FR-FBR-07/08/09/10 all DONE.
- **P1 Stage 2 — DONE** (commit `d6f247a`): admin transition + reply HTTP (C7), admin list + detail HTTP (C8), `feedback_replies` migration 00004, three plain-text email templates (C10), Mailpit integration test, `admin-ui/` React+Vite directory + Playwright+axe a11y smoke.
- **P1 Stage 1 — DONE** (commit `f63c66b`): `pii-scrub-audit` oracle, `feedbackmonk-tracing` crate, migrations 00003/00005, repository extensions, frozen contracts handoff doc.
- **P0 Foundation — COMPLETE** (commit `b9a672a`): all 5 P0 FRs DONE; e2e P0 PASS 7/7.
- **Pending pre-public-push gates (USER ACTION)**:
  - **PF-REGISTER-01** — Register `github.com/feedbackmonk` org + buy `feedbackmonk.com` (~$10/yr). Confirmed AVAILABLE 2026-05-14; not yet claimed.
  - **AGPL LICENSE**: full AGPL-3.0 text now in `LICENSE` (replaced 2026-05-13) — gate CLEARED on the LICENSE side; remaining gate is org/domain registration.
- **Pending follow-ups**:
  - **PF-RENAME-01 — DONE (this commit)** — Cargo workspace + crate names + env-var prefix rename `feedbackr-*` → `feedbackmonk-*` executed at the P1→P2 quiescent boundary. 100+ files changed atomically; 218 tests still pass; both Verification Oracles GREEN; sqlx offline cache regenerated.
  - **PF-RENAME-02 (USER ACTION, pending)** — Working-directory rename `Apps\Feedbackr` → `Apps\feedbackmonk`. Requires user action (Windows cannot rename CWD of live process).
- **LTADS S001 — CONCLUDED at this commit** (arc-terminus): autopilot:continuous arc ends at planned scope (P1 close). Phase 9 CSI-06 flips `current-session.md` Status → CONCLUDED + BoundConsent expired=true + emits `Concluded-By: <hash> via complete-arc-flag`.
- **GitCellar widget-embed touchpoint — DEFERRED to late P2 / early P3**.

## Recent Decisions

- **PF-RENAME-01 executed** (P1→P2 quiescent boundary, this commit) — Mechanical brand-rename: kebab `feedbackr-*` → `feedbackmonk-*`, snake `feedbackr_*` → `feedbackmonk_*`, env prefix `FEEDBACKR_` → `FEEDBACKMONK_`, header `X-Feedbackr-Anon-Cookie` → `X-Feedbackmonk-Anon-Cookie`, session cookie `feedbackr_session` → `feedbackmonk_session`, db names `feedbackr_dev`/`feedbackr_test` → `feedbackmonk_dev`/`feedbackmonk_test`, BLAKE3 domain separator `b"feedbackr-anon-v1"` → `b"feedbackmonk-anon-v1"` (pre-launch anon-cookie invalidation acceptable), plan-file paths renamed. ID prefixes `DEC-FBR-*` / `FR-FBR-*` preserved per DEC-FBR-11. Crate renames via `git mv` (history preserved). Local-dev container `feedbackr-pg-dev` not renamed (LOCAL_DEV.md documents `feedbackmonk-pg-dev` as the new name for next container recreation; not user-required immediately). All four verification gates GREEN: `cargo build --workspace` ✓, `cargo clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` 218/218 ✓, both Verification Oracles PASS.
- **Critic C-002 closure decision** (P1 Stage 3) — Router-level submission integration test injects `ConnectInfo<SocketAddr>` via request extensions (since `tower::ServiceExt::oneshot` doesn't populate it like the production `into_make_service_with_connect_info` does); test-harness limitation documented in helper docstring. `alg=none` case (Contract C2 invariant 1) substituted for the brief's "401 missing-cookie" entry (likely typo — submission endpoint is public and cookie-free at the public surface). Justification artifact at `docs/test-modifications/20260514-stage3-critic-c-002.md`.
- **Stage 3 Decision Logs authored in-place** (P1 Stage 3) — Four new module READMEs ship with Decision Log sections capturing load-bearing invariants: feedbackmonk-anon (BLAKE3 domain-separation prefix, in-memory governor as P0/P1 model), feedbackmonk-jwt (EdDSA-only allowlist defeats alg=none AND HMAC-confusion at header-parse, wrong-audience precedes signature check for information-leak hardening — Contract C2 invariant 3), feedbackmonk-api/src/auth (cookie name `feedbackmonk_session` reconciled vs P1-plan misnomer `feedbackmonk_admin_session` — Contract C11), feedbackmonk-api/src/handlers (scope-bound writes, same-transaction status update + audit row insert via `_in_executor` overloads).
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
| **GitHub org + domain registration** | Pre-public | User-action pending. Working name "feedbackmonk" through P3; brand pass at P4 (DEC-FBR-09). |
| **In-memory anonymous rate-limiter loses state on restart** | P0 (deferred to v1.1) | Acceptable for P0 single-instance dogfood; non-breaking Redis backend swap planned for v1.1. See DISCOVERIES.md D-FBR-08. |
| **PII scrub drift** | P1+ | **MITIGATED** — `pii-scrub-audit` oracle GREEN. Stage 2 status-emit paths inherited the chokepoint automatically (verified by oracle Probe A clean run at convergence). |
| **Admin UI display-id format hard-coded** | P1 → P3 | Stage 2 kept P0's `FB-XXXXXX` random alphanumeric `short_code`. If P3 ops feedback drives a switch to numeric `FB-NNNNNN`, mirror in `admin-ui/src/shared/types.gen.ts` comments simultaneously. |
| **`tenant_users` table doesn't exist yet** | P1 carry-forward | Migration 00004's `author_user_id` is bare UUID with no FK. Document at Stage 3 finalize; add FK with `ALTER TABLE … ADD CONSTRAINT … NOT VALID; VALIDATE CONSTRAINT …` when `tenant_users` lands (P3 multi-admin work, FR-FBR-15). |
| **GitCellar peer repo coordination** | Late P2 / early P3 | First cross-repo touchpoint when GitCellar embeds the widget. Forward-looking only; not a P1 blocker. |

## Next-Best-Steps

1. **PF-REGISTER-01 (USER ACTION)** — Register `github.com/feedbackmonk` org and purchase `feedbackmonk.com` before any first public push. Both confirmed AVAILABLE 2026-05-14; `gh api orgs --method POST -f login=feedbackmonk` or web UI.
2. **PF-RENAME-02 (USER ACTION)** — Working-directory rename `Apps\Feedbackr` → `Apps\feedbackmonk`. Must run with no live Claude Code sessions in the directory. PowerShell: `Rename-Item "E:\Developer\SourceControlled\Apps\Feedbackr" "feedbackmonk"`. Update `~/.claude/MACHINE_CONFIG.md` Dev Port Registry row afterward.
3. **P2 plan** — `/0-uldf-ldis-plan "feedbackmonk P2 — Customer-Facing"`. P2 scope: embeddable widget (FR-FBR-04 — <30KB cap, vanilla JS+CSS), `widget-bundle-size` Verification Oracle (defends the cap as a contract), public roadmap with voting (FR-FBR-11), promote-to-roadmap with Q24 byte-for-byte body invariant (FR-FBR-12 — port verbatim test from gitcellar `roadmap_promote.rs`).
