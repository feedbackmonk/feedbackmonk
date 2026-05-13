# Project Trajectory — Feedbackr

Rolling high-level state. Auto-maintained by `/0-uldf-finalize` Phase 12. Cheap orientation for fresh sessions; for detail, go to `docs/specs/` and `docs/planning/`.

**Last updated**: 2026-05-13 (P0 Foundation CLOSE — Stages 1+2+3 all DONE; e2e P0-exit-gate witness PASS)

---

## Current Focus

**P1 — Closes the Loop** (next, not yet started). FR-FBR-07/08/09/10:
- **FR-FBR-07** — Admin UI: feedback list view + drawer detail + reply composer with public/internal visibility tabs + status transition controls. React port from `gitcellar-cloud/admin-ui/`.
- **FR-FBR-08** — Status workflow: 6-state machine (`submitted` → `triaged` → `in-progress` → `shipped`/`wontfix`/`duplicate`) with audit history in `feedback_status_history`.
- **FR-FBR-09** — Status emails (plain-text): confirmation, on-status-change, on-public-reply. FB-NNNNNN display IDs in subject. Footer parameterized per tenant brand.
- **FR-FBR-10** — PII scrubber with canonical 20-pattern regex set + drift-detection oracle (port verbatim from `gitcellar-service/src/feedback_logs/scrubber.rs`).

P1 entry point: fresh `/0-uldf-ldis-plan "Feedbackr P1 — Closes the Loop"` consuming the arc plan at `docs/planning/plans/20260513T185711-feedbackr-v1-build-arc.md`.

## Active Threads

- **P0 Foundation — COMPLETE**: all 5 P0 FRs DONE (FR-FBR-01/02/03/05/06/18); 118 tests pass; `multi-tenant-isolation-check` oracle GREEN; e2e P0-exit-gate witness `scripts/e2e-p0-curl.sh` PASS 7/7 end-to-end against live binary :14304 + Postgres :5433 + Mailpit :1025/:8025.
- **AGPL LICENSE pre-public-commit ratification gate — PENDING USER ACTION**: LICENSE file is still a stub; repo stays local-only until user replaces with full AGPL-3.0 text + finalizes GitHub org + domain registration. Not a P1 blocker; only blocks first public push.
- **LTADS S001** — autopilot:continuous, mid-arc. Arc grant in `.claude/session-state/task-arc-autonomy.json` valid until 2026-05-14T21:06:21Z; continues onto P1 phase.
- **GitCellar widget-embed touchpoint — DEFERRED to late P2 / early P3**. No P1 cross-repo blockers.

## Recent Decisions

- **DEC-PODS-001** (P0 Stage 2) — `ProjectRepo::open_for_submission(project_id) -> Result<ProjectScope>` allow-listed pre-auth-boundary widening to Contract C1. Self-mediated by CLAUDE-B under autopilot:continuous; LD-ratified. Pattern: pre-auth allowlist now proven as a repeatable mechanism for legitimate Contract-C1 widening (see DISCOVERIES.md D-FBR-05). 3 cross-tenant binding tests added.
- **DEC-PODS-002** (P0 Stage 2) — `EmailVerificationRepo` trait + `migrations/00002_email_verifications.sql` schema addition. Self-mediated by CLAUDE-A (structurally necessary — `multi-tenant-isolation-check` Probe A forbids raw SQL outside repo crate). LD-ratified. 5 sqlx-test integration tests.
- **Contract C1 frozen** (P0 Stage 1) — repository public surface stable; all subsequent stages worked within it without signature deviations.
- **Contract C2 frozen** (P0 Stage 2 Worker B) — JWT verifier 6 hard invariants enforced by `crates/feedbackr-jwt/` + witness-backed by 24 named tests in the fixture corpus.
- **Contract C5 frozen** (P0 Stage 3) — `/health` + `/health/ready` JSON shape with `SqlxHealthCheck` ping; 200/503 liveness/readiness split.
- **DEC-FBR-IMPL-01..04** (P0 Stage 1) — Contract C1 extensions; `scope_for` allowlist; Python-canonical oracle pattern; dev-port 5433 deconfliction.

## Risks

| Risk | Stage | Notes |
|---|---|---|
| **AGPL LICENSE stub** | Pre-public | User-action: replace `LICENSE` file with full AGPL-3.0 text from `https://www.gnu.org/licenses/agpl-3.0.txt` before first public push. Repo MUST stay local-only until then per DEC-FBR-05 + project CLAUDE.md `--skip-push` invariant. |
| **GitHub org + domain registration** | Pre-public | User-action: pending. Working name "Feedbackr" through P3; brand pass at P4 (DEC-FBR-09). |
| **In-memory anonymous rate-limiter loses state on restart** | P0 (deferred to v1.1) | Acceptable for P0 single-instance dogfood; `feedbackr-anon` API surface designed for non-breaking Redis backend swap in v1.1. See DISCOVERIES.md D-FBR-08. |
| **PII scrub drift in P1** | P1 | Mitigated by `pii-scrub-audit` oracle scheduled at P1 entry. Canonical 20-pattern set must be ported byte-for-byte from `gitcellar-service/src/feedback_logs/scrubber.rs`. |
| **GitCellar peer repo coordination** | Late P2 / early P3 | First cross-repo touchpoint when GitCellar embeds the widget. Forward-looking only; not a P1 blocker. |

## Next-Best-Steps

1. **User action** — replace `LICENSE` stub with full AGPL-3.0 text; register GitHub org + domain. (Pre-public-commit gate; orthogonal to P1 implementation.)
2. **`/0-uldf-proceed`** at P0 → P1 phase boundary. Topology selector will likely pick **HANDOFF** (fresh `/0-uldf-ldis-plan` session is the right shape — P1 is multi-FR, needs full planning treatment).
3. If HANDOFF chosen: **`/0-uldf-ldis-plan "Feedbackr P1 — Closes the Loop"`** consuming the arc plan; produces P1's intra-phase topology + interface contracts + Oracle Pre-Build Plan + Testability Gate findings.
4. P1 implementation likely PODS (admin UI + status workflow + emails + PII scrub are independent surfaces with parallel-friendly contracts).
5. **`pii-scrub-audit` oracle** must be built at P1 entry (port from GitCellar's existing oracle; drift-detection over canonical 20-pattern set).
