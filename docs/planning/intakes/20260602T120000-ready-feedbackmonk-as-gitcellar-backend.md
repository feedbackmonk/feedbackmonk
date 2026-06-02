# Intake Assessment
**Source**: /0-uldf-ldis-intake
**Generated**: 2026-06-02T12:00:00
**Task**: Ready feedbackmonk to be GitCellar's feedback backend + close customer-#1 parity gaps.

---

## Phase 0 — Prior Context Recovered

- **GitCellar adoption intake** (peer repo): `../GitCellar/docs/planning/intakes/20260602T104026-adopt-feedbackmonk-as-feedback-system.md` — Path C (enhance → converge → replace), hard no-feature-loss constraint, 5-item PARITY CHECKLIST, 3 decisions resolved (D1 deploy at feedbackmonk.com w/ GitCellar as tenant #1; D2 forge bridge disable-not-delete; D3 no data migration).
- **feedbackmonk spec**: `docs/specs/SPECIFICATION.md` — all 18 FR-FBR done through P4 Stage 2; v1 content-complete. PF-DEPLOY-01 open (not deployed anywhere).
- **feedbackmonk extraction intake**: `docs/planning/intakes/20260512T221154-...` — GitCellar is named "customer #1"; DEC-FBR-07 mandates API+widget integration, NO Rust-crate coupling.

═══════════════════════════════════════════════════════════════
       LEAD DEVELOPER INTELLIGENCE ASSESSMENT
═══════════════════════════════════════════════════════════════

TASK: Ready feedbackmonk as GitCellar's feedback backend + close parity gaps 1–4.

───────────────────────────────────────────────────────────────
PERCEPTION
───────────────────────────────────────────────────────────────

Type: **Enhancement (Feature Addition × 4)** + **Deployment/Ops** leg, in service of a cross-repo Migration owned by GitCellar.
Scope: **MEDIUM–LARGE** — 4 independent feature gaps (each a distinct subsystem) + one deploy workstream. Weeks of FTE if all built; gaps parallelize cleanly.
Risk: **MEDIUM**. No-regression acceptance contract; attachments (#1) is the heaviest (storage, redaction, PII scrub, multipart). Deployment is net-new ops, not code risk.

Professional Assessment:
The binding work is on the feedbackmonk side, exactly as GitCellar's intake predicts. All 4 build-gaps are genuinely absent (code-verified, not taken on trust). The data model is friendly to the cheapest gaps (#4 my-feedback needs no schema change; #3 search is an index + route; #2 is one column + a worker). Gap #1 (attachments) is the real build. Deployment is unblocked operationally (domain owned, stack built + smoke-tested) — it just needs a host.

───────────────────────────────────────────────────────────────
VERIFICATION OF THE 5 PARITY ITEMS (against code at 78aca1e)
───────────────────────────────────────────────────────────────

1. **Attachments** — **CONFIRMED MISSING.** No attachment table/columns in any migration (00001–00008); no multipart/file-upload handling in any handler; no canvas-redaction or service-log-capture code. (`FEEDBACKMONK_S3_*` is explicitly deferred to v1.1 in SELFHOST_ENV.md.) → BUILD.
2. **Crash-event correlation** — **CONFIRMED MISSING.** `feedback` table has no `crash_event_id`; no Glitchtip integration; no correlation worker. (Only grep hits were a PII-scrubber pattern + planning docs.) → BUILD.
3. **Admin full-text search** — **CONFIRMED MISSING.** No search route under `/api/v1/admin/feedback`; admin list is filter/paginate only; no tsvector/GIN index. → BUILD.
4. **End-user my-feedback list + reply-thread read API** — **CONFIRMED MISSING (GitCellar's belief is correct).** Full route table verified: the only public end-user endpoint is `POST /api/v1/projects/:id/feedback` (submit). All read/list/detail/reply routes are under `/api/v1/admin/feedback`, gated by the admin-session cookie — none scoped by JWT `sub`. Public replies reach submitters by email, not an in-app thread. Data model supports closing it with no schema change (`feedback.end_user_sub` stored; `feedback_replies.visibility ∈ {public,internal}`). → BUILD.
5. **Forge issue bridge** — **N/A (informational).** GitCellar is dropping it; DEC-FBR-06 already dropped Forge on the feedbackmonk side. → NO ACTION.

**Bonus discovery (D-FBR, small):** server-emitted `embed_snippet` is stale — emits `data-project="<slug>"`, but the shipped widget reads `data-project-id` (UUID) + `data-api-base`. Copy-paste of the emitted snippet would not work. Fix on feedbackmonk side; canonical embed documented in the integration contract §4/§7.

───────────────────────────────────────────────────────────────
DEPLOYMENT REQUIREMENTS (PF-DEPLOY-01)
───────────────────────────────────────────────────────────────

- Stack is BUILT + smoke-tested (FR-FBR-17, `selfhost-compose-smoke` GREEN to `/health/ready`). Domain owned. Self-host == SaaS artifact (DEC-FBR-05). Only a running host is missing.
- **Recommended host: Railway** (matches GitCellar infra). Service map: api → `api.feedbackmonk.com`; admin-ui → `app.feedbackmonk.com`; widget static → `cdn.feedbackmonk.com`; managed Postgres; migrate one-shot; marketing (optional) → `www`.
- Min env: `DATABASE_URL`, `FEEDBACKMONK_PUBLIC_URL`, `FEEDBACKMONK_SESSION_SECRET` (+ `FEEDBACKMONK_BIND_ADDR=0.0.0.0`, SMTP vars for real email). Full catalog: `docs/operations/SELFHOST_ENV.md` (C21).
- Provisioning GitCellar tenant/project/key: signup → verify-email → `POST /api/v1/projects` (returns `project_id` UUID = JWT `aud`) → `POST /api/v1/projects/:id/signing-keys` (register GitCellar's 32-byte Ed25519 public key, Contract C4). Procedure detailed in the integration contract §3.

───────────────────────────────────────────────────────────────
CALIBRATION
───────────────────────────────────────────────────────────────

Required Spec Level: **Thorough** (production cutover, no-regression contract). Current: **Near-thorough** — parity checklist is a ready-made requirements contract; gaps are well-bounded.

VERDICT: **SUFFICIENT to plan** (this is an orient/verify/prep session; heavy build gated on go-ahead). Route to `/0-uldf-ldis-plan` for the gap-closing topology after go-ahead.

───────────────────────────────────────────────────────────────
ORACLE CANDIDATES
───────────────────────────────────────────────────────────────

• **feedback-parity-status** (proposed by GitCellar's intake) — answers "which of parity gaps 1–4 are closed, is the Phase-3 cutover gate open?" deterministic ✓ | recurrent ✓ (every phase boundary, both repos) | freshness-contractable ✓ (checklist + FR statuses) | gracefully-absent ✓. Build timing: cheap; build when gap-closing starts so it gates GitCellar's cutover.

───────────────────────────────────────────────────────────────
COLLABORATION ASSESSMENT
───────────────────────────────────────────────────────────────

Scope: MEDIUM–LARGE. Subdivisible: YES — gaps 1–4 are independent subsystems.
Value: Specialization 4/5, Quality 4/5 (no-regression), Discovery 3/5, Speed 4/5. Friction: Coupling 4/5, Boundary Clarity 5/5 (parity checklist = boundaries).
Net: ~13 → **PODS-suitable for the build phase** (one worker per gap; #1 attachments largest). Deploy is a separate sequential workstream. Gate cutover on `feedback-parity-status`.

───────────────────────────────────────────────────────────────
PLAN TO CLOSE GAPS 1–4 (sketch; finalize in /0-uldf-ldis-plan after go-ahead)
───────────────────────────────────────────────────────────────

- **Deploy (parallel, unblocks GitCellar Phase 1 immediately)**: stand up Railway stack; provision GitCellar tenant/project/Ed25519 key; fill real `project_id` into the integration contract; GitCellar embeds anonymous widget on gitcellar.com. Zero feedbackmonk feature work needed for this.
- **Gap 4 (smallest, do first)**: 2 JWT-scoped read routes (`GET …/me/feedback`, `GET …/me/feedback/:fb/thread`) reusing existing repo + JWT verify; public replies only; no schema change.
- **Gap 3**: Postgres FTS (`tsvector` generated column + GIN index migration) + `GET /api/v1/admin/feedback/search?q=` + admin UI debounced search box.
- **Gap 2**: add `crash_event_id` column (migration) + accept on submit (auth-mode `external_metadata` or dedicated field) + correlation worker against Glitchtip + Desktop crash-link banner contract.
- **Gap 1 (largest)**: attachment storage (S3-compatible; `FEEDBACKMONK_S3_*`), ≤4 images ≤5MB multipart, canvas redaction tool (widget), service-log + console-log capture with PII scrubbing (reuse `feedbackmonk-tracing` 20-pattern scrubber). Respect 30KB widget cap — redaction UI may need lazy-load.
- **Then**: build `feedback-parity-status` oracle; GitCellar runs Phase 3 cutover gated on it.

───────────────────────────────────────────────────────────────
INTEGRATION CONTRACT
───────────────────────────────────────────────────────────────

Authored to `docs/integrations/gitcellar-adoption.md` (DRAFT, pre-deploy). Covers: deploy/host
recommendation, API base, tenant/project/signing-key provisioning, anonymous widget embed (gitcellar.com),
full EdDSA JWT minting spec (claims/alg/TTL/errors), signing-key registration (Contract C4), the
not-yet-built gap-#4 read API (§6), and the stale-embed-snippet discrepancy (§7).

───────────────────────────────────────────────────────────────
RECOMMENDED NEXT STEPS
───────────────────────────────────────────────────────────────

1. **Report to user + GitCellar; WAIT for go-ahead** (per session brief — no heavy build yet).
2. On go-ahead: `/0-uldf-ldis-plan "feedbackmonk — GitCellar customer-#1 enablement"` → STAGED/PODS topology (deploy ∥ gap-closing; cutover gated).
3. Deploy + provisioning can run immediately on go-ahead — it unblocks GitCellar's Phase 1 (anonymous website feedback) with zero feedbackmonk feature work.

═══════════════════════════════════════════════════════════════
