# Spec Progress — Feedbackr v1 Arc (P0 → P4)

| FR | Description | Phase | Stage | Status | Witness |
|---|---|---|---|---|---|
| FR-FBR-01 | Multi-tenant data model + tenant-scoped repository | P0 | S1 | **DONE** | Stage 1 commit; 19 tests pass incl. cross-tenant invariants; `multi-tenant-isolation-check` oracle GREEN; Contract C1 frozen for Stage 2 |
| FR-FBR-02 | Customer signup + onboarding | P0 | S2 (Worker A) | BLOCKED | awaits Stage 1 |
| FR-FBR-03 | Submission API (JWT + anonymous) | P0 | S2 (Worker B) | BLOCKED | awaits Stage 1 |
| FR-FBR-05 | JWT EdDSA verification | P0 | S2 (Worker B) | BLOCKED | awaits Stage 1 |
| FR-FBR-06 | Anonymous submission mode | P0 | S2 (Worker B) | BLOCKED | awaits Stage 1 |
| FR-FBR-18 | Health + structured logging | P0 | S3 | BLOCKED | awaits Stage 2 |
| FR-FBR-04 | Embeddable widget (<30KB) | P2 | — | DEFERRED | — |
| FR-FBR-07..09 | Status workflow, drawer, replies | P1 | — | DEFERRED | — |
| FR-FBR-10 | PII scrub | P1 | — | DEFERRED | — |
| FR-FBR-11..14 | Public roadmap, voting, tiers | P2/P3 | — | DEFERRED | — |
| FR-FBR-15..17 | Marketing site, self-host | P4 | — | DEFERRED | — |

## Active Stage Reference
- **Stage 1 plan**: `docs/planning/plans/20260513T210133-feedbackr-p0-foundation.md` §Stage 1
- **Arc plan**: `docs/planning/plans/20260513T185711-feedbackr-v1-build-arc.md`
