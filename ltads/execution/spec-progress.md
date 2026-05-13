# Spec Progress — Feedbackr v1 Arc (P0 → P4)

| FR | Description | Phase | Stage | Status | Witness |
|---|---|---|---|---|---|
| FR-FBR-01 | Multi-tenant data model + tenant-scoped repository | P0 | S1 | **DONE** | Stage 1 commit `dbbe04a`; 19 → 21 repo tests pass (incl. DEC-PODS-001's 3 new cross-tenant binding tests); `multi-tenant-isolation-check` oracle GREEN; Contract C1 frozen |
| FR-FBR-02 | Customer signup + onboarding | P0 | S2 (Worker A) | **DONE** | Stage 2 collab-20260513-221600; signup/verify-email/projects/signing-keys endpoints live; argon2 + HMAC-signed admin session; lettre Mailpit/SMTP mailer; 17 unit + 13 integration tests; `EmailVerificationRepo` widening (DEC-PODS-002) ratified |
| FR-FBR-03 | Submission API (JWT + anonymous) | P0 | S2 (Worker B) | **DONE** | Stage 2; `POST /api/v1/projects/{id}/feedback` with auth-mode dispatch; Contract C3 response shape; 11 handler unit tests; `ProjectRepo::open_for_submission` widening (DEC-PODS-001) ratified |
| FR-FBR-05 | JWT EdDSA verification | P0 | S2 (Worker B) | **DONE** | Stage 2; `crates/feedbackr-jwt/` enforces all 6 Contract C2 hard invariants (alg-allowlist EdDSA-only, alg-none + HS256-confusion rejection, wrong-aud, expired, missing-claim, oversize-metadata); JWT fixture corpus 24 named tests (Task Zero, all 8 cases a-h + boundary/leeway/RS256) hermetic-deterministic |
| FR-FBR-06 | Anonymous submission mode | P0 | S2 (Worker B) | **DONE** | Stage 2; `crates/feedbackr-anon/` AnonGate over governor keyed limiter; BLAKE3 domain-separated hash with `feedbackr-anon-v1` prefix; 22-char opaque base64url cookie; 11 tests covering determinism + domain separation + 11th-call 429 boundary |
| FR-FBR-18 | Health + structured logging | P0 | S3 | **DONE** | Stage 3 — `/health` + `/health/ready` per Contract C5 (`SqlxHealthCheck` ping, JSON body, 200/503 liveness/readiness split); `tracing` JSON formatter + `tower-http::trace::TraceLayer` + `x-request-id` propagation; e2e P0-exit-gate witness `scripts/e2e-p0-curl.sh` **PASS** end-to-end (7/7 steps incl. signup → verify → project → key-register → JWT submit → anon submit → 11-burst-429); FEEDBACKR_JWT_LEEWAY_SECONDS now actually consumed (critic C-001 resolved). |
| FR-FBR-04 | Embeddable widget (<30KB) | P2 | — | DEFERRED | — |
| FR-FBR-07..09 | Status workflow, drawer, replies | P1 | — | DEFERRED | — |
| FR-FBR-10 | PII scrub | P1 | — | DEFERRED | — |
| FR-FBR-11..14 | Public roadmap, voting, tiers | P2/P3 | — | DEFERRED | — |
| FR-FBR-15..17 | Marketing site, self-host | P4 | — | DEFERRED | — |

## P0 Stage 2 — Summary

- **Collaboration**: PODS session `collab-20260513-221600`, 2 workers (CLAUDE-A signup/onboarding, CLAUDE-B submission path)
- **Aggregate test count**: 116 (Stage 1 19 → Stage 2 116; +97 net-new tests across 8 crates, zero Stage-1 assertions modified)
- **Self-mediated decisions (LD-ratified)**:
  - DEC-PODS-001 — `ProjectRepo::open_for_submission(project_id)` allowlisted pre-auth boundary widening
  - DEC-PODS-002 — `EmailVerificationRepo` + `migrations/00002_email_verifications.sql` schema addition
- **Quality gate**: cargo build clean, `clippy --workspace -- -D warnings` clean, 116 tests pass, multi-tenant-isolation-check oracle GREEN
- **P0 exit gate progress**: **100% — P0 COMPLETE**. Stage 3 shipped `/health` + structured logging; e2e-p0-curl.sh PASS end-to-end against live binary on :14304 + Postgres :5433 + Mailpit :1025/:8025. Test count 116 → 118 (Stage 3 added 2 health unit tests). multi-tenant-isolation-check oracle GREEN. Two late Stage-3 fixes during e2e: `RegisterKeyRequest` serde alias `public_key_base64` to match Contract C4 (impl had short name `public_key_b64`); `axum::serve` now uses `into_make_service_with_connect_info::<SocketAddr>()` so the submission handler's IP-based anon-mode hash works.

## Active Stage Reference
- **P0 closed**. Next: P1 (Closes the Loop) via fresh `/0-uldf-ldis-plan "Feedbackr P1 — Closes the Loop"`.
- **Arc plan**: `docs/planning/plans/20260513T185711-feedbackr-v1-build-arc.md`
- **P0 plan**: `docs/planning/plans/20260513T210133-feedbackr-p0-foundation.md` (Stage 1 + 2 + 3 all DONE)
