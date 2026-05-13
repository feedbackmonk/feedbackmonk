# Project Trajectory — Feedbackr

Rolling high-level state. Auto-maintained by `/0-uldf-finalize` Phase 12. Cheap orientation for fresh sessions; for detail, go to `docs/specs/` and `docs/planning/`.

**Last updated**: 2026-05-13 (P0 Stage 1 finalize)

---

## Current Focus

**P0 Foundation — Stage 2** (next). Two-worker PODS fan-out:
- **Worker A** — FR-FBR-02 customer signup + onboarding (login/session → create org → create first project → embed code display)
- **Worker B** — FR-FBR-03 + FR-FBR-05 + FR-FBR-06 submission path (Task Zero: JWT fixture corpus; then EdDSA verifier + anon-mode rate-limiter + POST /feedback handler)

Stage 2 consumes the **frozen Contract C1** repository surface that Stage 1 just landed. Worker briefs encoded in `docs/planning/plans/20260513T210133-feedbackr-p0-foundation.md` §Component Decomposition. Handoff state lives in `docs/planning/handoffs/stage1-to-stage2.md`.

## Active Threads

- **P0 Stage 1 — COMPLETE**: multi-tenant data model + tenant-scoped repository layer + Verification Oracle + 19 passing tests. FR-FBR-01 DONE.
- **Stage 2 fan-out — READY TO SPAWN**: `/0-uldf-pods-parallelize` is the next step. Both worker briefs are pre-encoded in the P0 plan.
- **LTADS S001** — ACTIVE, autopilot:continuous, mid-arc. Will checkpoint on Stage 1 commit; will continue through Stage 2 + Stage 3 + P0 exit gate.

## Recent Decisions

- **Contract C1 frozen** (P0 Stage 1 exit gate). Repository public surface = `TenantRepo` + `ProjectRepo` + `SigningKeyRepo` + `FeedbackRepo` traits + their sqlx impls + `TenantScope` / `ProjectScope` newtypes + `RepoError`. No signature deviations permitted in Stage 2 without a documented Contract C1 amendment.
- **DEC-FBR-IMPL-01** — `FeedbackRepo::submit_*` carry explicit `kind: FeedbackKind`; `list_recent(scope, limit)` is part of the trait (extensions of plan §C1, not widenings).
- **DEC-FBR-IMPL-02** — `TenantRepo::scope_for(Uuid)` allow-listed as the third pre-auth method (bridges verified session cookies → first `TenantScope`).
- **DEC-FBR-IMPL-03** — Verification Oracles default to **Python canonical + shell shims** when parsing crosses lines.
- **DEC-FBR-IMPL-04** — Dev Postgres on **port 5433**, not 5432 (deconflicts gitcellar-cloud).

## Risks

| Risk | Stage | Notes |
|---|---|---|
| **JWT verifier alg-confusion / aud-binding errors** | Stage 2 Worker B | Mitigated by Testability-Gate-mandated JWT fixture corpus (Worker B Task Zero) — six categories of fixture covering each known crypto-verifier failure mode. |
| **Anonymous-mode dedup correctness under clock skew** | Stage 2 Worker B | Deterministic-time fixtures planned; in-memory governor crate has good unit-test ergonomics per Stage 1 plan §Testability Gate findings (FR-FBR-06 composite 8). |
| **GitHub org + domain pre-registration** | Pre-public | User action pending; not a blocker for P0 implementation. AGPL LICENSE file is the full text (not a stub) — first public push gated only on org+domain registration per project CLAUDE.md. |
| **GitCellar peer repo coordination** | P2-P3 | First cross-repo touchpoint is late-P2 / early-P3 when GitCellar embeds the widget. Forward-looking only; no Stage 2 blockers. |

## Next-Best-Steps

1. **`/0-uldf-proceed`** at the Stage 1 → Stage 2 boundary. Topology selector will likely pick **PODS** (two workers, planned upfront, formal roles encoded in P0 plan).
2. If PODS is chosen: **`/0-uldf-pods-parallelize`** consuming the P0 plan; spawn Worker A + Worker B with the pre-encoded briefs.
3. Stage 2 exit gate: both workers green, JWT fixture corpus green, integration test (`curl signup → curl create-project → curl POST feedback`) green, multi-tenant-isolation-check oracle green.
4. Stage 3 (single session in converging tree) — FR-FBR-18 health + structured logging.
5. P0 exit gate → fresh `/0-uldf-ldis-plan` for P1.
