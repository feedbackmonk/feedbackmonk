# feedbackmonk-repository

<!-- agent-synopsis -->
The SOLE query path for feedbackmonk domain data. Tenant-scoped repository layer per DEC-FBR-03; raw SQL anywhere else is a security incident. Built in P0 Stage 1 per FR-FBR-01 + Contract C1.
<!-- /agent-synopsis -->

## Purpose & Responsibilities

This crate is **the** path from application code to the database. Every domain read or write goes through one of the four repository traits here (`TenantRepo`, `ProjectRepo`, `SigningKeyRepo`, `FeedbackRepo`). Per **DEC-FBR-03**, raw SQL anywhere outside this crate is a **security incident** — the `multi-tenant-isolation-check` Verification Oracle enforces this at AST grade on every commit.

The crate implements a three-leg defense against tenant-isolation drift:

| Leg | Mechanism | Where |
|---|---|---|
| **Leg 1** — type system | `TenantScope` / `ProjectScope` newtypes with `pub(crate)` constructors | `src/scope.rs` |
| **Leg 2** — AST oracle | `multi-tenant-isolation-check` greps for raw-SQL patterns + verifies first-arg discipline | `.claude/oracles/multi-tenant-isolation-check/` |
| **Leg 3** — lint baseline | `clippy::all = deny` workspace-wide; `clippy::pedantic` on this crate; `cargo-deny` on `deny.toml` | `Cargo.toml`, `deny.toml` |

## File Index

| File | Purpose |
|---|---|
| `src/lib.rs` | Crate root. Re-exports the public surface (the four traits, their sqlx impls, the scope newtypes, the error type). |
| `src/scope.rs` | `TenantScope` and `ProjectScope` newtypes. Constructors are `pub(crate)` — the type-system half of leg 1. |
| `src/error.rs` | `RepoError` (incl. `TenantProjectMismatch` for cross-tenant `ProjectRepo::open` rejection) and `Result` alias. |
| `src/tenants.rs` | `TenantRepo` trait + `SqlxTenantRepo` impl. **Three pre-auth allow-listed methods**: `create`, `find_by_email`, `scope_for`. 4 tests including cross-tenant lookup invariants. |
| `src/projects.rs` | `ProjectRepo` trait + `SqlxProjectRepo` impl. `open(&TenantScope, project_id)` is the **sole** `ProjectScope` constructor in the public API. 4 tests including `open_rejects_cross_tenant_project`. |
| `src/signing_keys.rs` | `SigningKeyRepo` trait + `SqlxSigningKeyRepo` impl. Always takes `&ProjectScope` first. 2 tests. |
| `src/feedback.rs` | `FeedbackRepo` trait + `SqlxFeedbackRepo` impl. P0: `submit_authenticated`, `submit_anonymous`, `list_recent`. **P1 Stage 1 additions**: `list_for_admin(scope, status_filter, limit, offset)` + `get_with_history(scope, feedback_id)` (Contract C6 backing). **P1 Stage 2 addition**: `update_status_in_executor(scope, conn, feedback_id, new_status)` — same-txn `feedback.status` UPDATE companion to `feedback_status_history::append_in_executor` (Contract C6 Hard Invariant #4). Also defines `FeedbackListItem` and `StatusHistoryRow` value types. ~8 tests including cross-tenant negatives for the new methods. |
| `src/feedback_status_history.rs` | **P1 Stage 1**. `FeedbackStatusHistoryRepo` trait + `SqlxFeedbackStatusHistoryRepo` impl. `append(scope, feedback_id, from, to, reason?, duplicate_of?, transitioned_by)` + `list_for_feedback(scope, feedback_id)`. Both `&ProjectScope`-first. 4 sqlx::test cases including cross-tenant negatives + duplicate_of cross-tenant rejection. **P1 Stage 2 addition**: `append_in_executor(scope, conn, ...)` — executor-aware overload so transition handlers can compose same-transaction writes (Contract C6 Hard Invariant #4). |
| `src/feedback_replies.rs` | **P1 Stage 2 (migration 00004)**. `FeedbackReplyRepo` trait + `SqlxFeedbackReplyRepo` impl backing Contract C7's `POST /api/v1/admin/feedback/{id}/reply` endpoint. `create(scope, feedback_id, body, visibility, author_user_id)` + `list_for_feedback(scope, feedback_id)` + `count_for_feedback(scope, feedback_id)`. Defines `FeedbackReply` value type + `ReplyVisibility` enum (`Public` / `Internal`). All methods `&ProjectScope`-first. 4 sqlx::test cases incl. cross-tenant negatives + visibility-enum round trip. |
| `src/email_verifications.rs` | `EmailVerificationRepo` trait + `Redemption` value type + `SqlxEmailVerificationRepo` impl. `create` / `redeem` / `mark_used`. **5 tests** including round-trip, unknown-token, mark-used, duplicate-token conflict, cascade-delete. **`redeem` is pre-auth allow-listed (the verify token IS the credential at redemption time)**; see DEC-PODS-002 below. Added in P0 Stage 2 by CLAUDE-A. |
| `src/tenants.rs` (P1 Stage 1 extension) | `TenantRepo` trait now includes `get_brand` + `update_brand` plus the new `EmailTenantBrand` value type (Contract C10). The pre-existing `create` was extended to populate brand-column defaults inline so new signups land identically to migration 00005's backfilled rows. |
| `src/tenants.rs` (P3 Stage 1 extension) | `TenantRepo` trait gains `get_tier(&TenantScope) -> Tier`, `count_projects(&TenantScope) -> i64`, `count_feedback_in_window(&TenantScope, days) -> i64`. `get_widget_brand` is now **tier-aware** — Free tier returns `footer_text: Some("powered by feedbackmonk")`, all paid tiers (`Starter`, `Pro`, `SelfHost`) return `None`. This is the FR-FBR-14 free-tier-footer enforcement at the brand-render boundary. Inherent test seam `set_tier_for_test` is allow-listed in `multi-tenant-isolation-check`. 4 net-new sqlx::test cover tier round-trip, footer flip per tier, project-count, rolling-window feedback-count. |
| `src/tier_quota.rs` | **P3 Stage 1**. `TierQuotaRepo` trait + `SqlxTierQuotaRepo` impl backing Contract C17. `check_tier_quota(&TenantScope, ResourceKind) -> QuotaStatus` is the **single chokepoint** for tier-cap enforcement — every domain-write handler under `crates/feedbackmonk-api/src/handlers/` consults it before its first INSERT. Sibling `get_tier_status(&TenantScope) -> TierStatus` returns the current tier + quotas + live usage in one round (consumed by `GET /api/v1/admin/tier`). 6 sqlx::test cover Free/Starter/Pro/SelfHost cap-firing semantics + rolling-window edge cases. The `tier-enforcement-status` Verification Oracle Probe A asserts handler coverage at AST level. |
| `src/attachments.rs` | **GitCellar parity gap #1 (migration 00009).** `AttachmentRepo` trait + `SqlxAttachmentRepo` impl. `resolve_feedback_uuid`, `count_images`, `insert`, `list_for_feedback` — all `&ProjectScope`-first (Probe B compliant; constructor allow-listed in `multi-tenant-isolation-check`). Backs the `attachments` table; storage-backend-agnostic (stores URI + content metadata, not bytes). sqlx::test coverage in `tests/`. |
| `tests/` | Integration tests for the repository surface (incl. attachment-repo cross-tenant negatives). |
| `Cargo.toml` | Adds `clippy::pedantic` on top of the workspace's `clippy::all = deny`. Depends on `sqlx`, `uuid`, `chrono`, `async-trait`, `thiserror`, `serde_json`, `feedbackmonk-core`. |

## Public API & Usage

```rust
use feedbackmonk_repository::{TenantRepo, ProjectRepo, FeedbackRepo, TenantScope, ProjectScope};

// Stage 2+ login handler:
let tenant = tenant_repo.find_by_email(email).await?;          // pre-auth (allow-listed)
verify_password(&tenant.password_hash, password)?;             // proves identity
let scope: TenantScope = tenant_repo.scope_for(tenant.id).await?;  // pre-auth bridge (allow-listed)

// Now every downstream call is type-checked tenant-isolated:
let project_scope: ProjectScope = project_repo.open(&scope, project_id).await?;
let feedback = feedback_repo.submit_authenticated(&project_scope, body, kind, ...).await?;
let recent = feedback_repo.list_recent(&project_scope, 20).await?;
```

The contract is **frozen** for Stage 2 consumption — Workers A and B treat the trait set as a library surface. See `docs/planning/handoffs/stage1-to-stage2.md`.

## Constraints & Business Rules

- **Constructor discipline (leg 1)**: `TenantScope::new` and `ProjectScope::new` are `pub(crate)`. `ProjectRepo::open` is the **sole** `ProjectScope` constructor in the public API.
- **Allowlist discipline (leg 2)**: The three pre-auth methods on `TenantRepo` (`create`, `find_by_email`, `scope_for`) are the **only** methods that may legitimately take a non-`&TenantScope` first argument. Adding a fourth requires updating `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` with a documented rationale.
- **Pedantic clippy (leg 3)**: this crate runs `clippy::pedantic` in addition to the workspace `clippy::all = deny`.
- **`FeedbackRepo::submit_*` accept `kind: FeedbackKind`**: this is an EXTENSION of plan §C1 (additional info), not a WIDENING (both methods still take `&ProjectScope` first). The schema declares `kind` with CHECK constraint and FR-FBR-03 Contract C3 accepts optional `kind`.
- **`FeedbackRepo::list_recent` exists beyond plan §C1 enumeration**: used by 3/4 feedback tests as round-trip read path and by Stage 2 Worker A's forward-looking admin-feedback-list endpoint. Same scope discipline.

## Relationships & Dependencies

- **Consumes**: `feedbackmonk-core` (every query returns a `feedbackmonk_core::*` record).
- **Consumed by**: `feedbackmonk-api` (Stage 2+ HTTP handlers — Workers A and B), `feedbackmonk-jwt` (Stage 2 — signing-key lookup), `feedbackmonk-anon` (Stage 2 — rate-limit counters), future health crate, future admin UI backend.
- **Schema source of truth**: `migrations/00001_p0_schema.sql`. Every query in this crate hard-depends on those column names; schema column renames require a follow-up migration AND a coordinated change here.
- **Oracle**: `.claude/oracles/multi-tenant-isolation-check/` polices the layer boundary on every commit; CI gates the build on its exit code.

## Decision Log

### Constructors are `pub(crate)`; `ProjectRepo::open` is the sole `ProjectScope` constructor

**Decision**: `TenantScope::new` and `ProjectScope::new` are crate-private. The only public path to a `ProjectScope` is `ProjectRepo::open(&TenantScope, project_id)`, which enforces tenant→project ownership at the type-system boundary.

**Rationale**: `DEC-FBR-03` declares any raw SQL outside this crate a security incident. The type system half of the defense (leg 1) requires that *just having a `ProjectScope` value* proves the caller has already passed a tenant-ownership check. Public constructors would let any caller fabricate a `ProjectScope` from arbitrary `Uuid`s, defeating the type-system guarantee and reducing leg 1 to a naming convention. The `pub(crate)` discipline is what makes `ProjectScope` a real proof-carrying type rather than a tagged tuple.

**Trade-offs**: Slightly inconvenient for external consumers — they cannot mint a scope for testing without going through `open`. Mitigated by the in-crate tests living inside `crates/feedbackmonk-repository/src/**.rs` (`pub(crate)` is reachable), and by the fact that integration tests in higher crates SHOULD use the real `open` path anyway (that's how production code obtains a scope).

**Implementation**: `src/scope.rs` — `pub(crate) fn new(...)` on both newtypes. `src/projects.rs::ProjectRepo::open` is the sole public constructor of `ProjectScope`. Multi-tenant-isolation-check oracle enforces no `ProjectScope { ... }` struct literals outside this crate.

### `TenantRepo::scope_for(Uuid)` is allow-listed pre-auth

**Decision**: `TenantRepo::scope_for(uuid) -> Result<TenantScope>` is the third allow-listed pre-auth method (alongside `create` and `find_by_email`). It bridges a verified session-cookie tenant_id to a fresh `TenantScope`. Documented in `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`.

**Rationale**: The pre-authentication boundary necessarily mints the **first** `TenantScope` from a verified caller. The `TenantScope` constructor is `pub(crate)`, so without `scope_for` (or an equivalent), Stage 2 Worker A's login handler would have no path from "I've validated this session cookie" to "...therefore here is a `TenantScope` for downstream calls." Naming the boundary explicitly — and gating it through a single method documented in the allowlist — is more honest than back-channels.

**Trade-offs**: Adds a third entry to the pre-auth allowlist. The risk is that the allowlist grows over time without corresponding tightening. Mitigated by requiring per-entry rationale in `allowlist.toml` and by the oracle freshness contract triggering on allowlist changes.

**Implementation**: `src/tenants.rs` — `async fn scope_for(&self, tenant_id: Uuid) -> Result<TenantScope>`. Returns `RepoError::NotFound` for unknown tenant_id (covered by `scope_for_unknown_tenant_returns_not_found` test). Allowlist entry at `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` lines 32-35 carries the rationale.

### `FeedbackRepo::submit_*` accept `kind`; `list_recent` exists

**Decision**: Both `submit_authenticated` and `submit_anonymous` carry an explicit `kind: FeedbackKind` parameter, and `FeedbackRepo` includes a `list_recent(scope, limit)` method beyond the plan §C1 enumeration.

**Rationale**: Plan §C1 sketched the submit methods without explicit `kind`, but the schema declares `kind` with a CHECK constraint and FR-FBR-03 Contract C3 accepts optional `kind`. Forcing every caller to omit `kind` or default it at the DB layer pushes a fundamental piece of feedback metadata through the wrong seam. Same principle for `list_recent`: 3/4 of the feedback unit tests need a round-trip read path, and Stage 2 Worker A's admin-feedback-list endpoint will consume it.

**Trade-offs**: Mild departure from the plan's literal §C1 sketch. Both additions are EXTENSIONS (additional info / additional method) rather than WIDENINGS (the `&ProjectScope` first-arg discipline is preserved on every method). If the orchestrator ratifies a Contract C1 amendment in the other direction (default `kind` at DB layer; drop `list_recent`), both are trivially removable.

**Implementation**: `src/feedback.rs` trait definitions. The orchestrator has explicitly noted these as forward-evolutions of C1 in the Stage 1 completion brief and in `docs/specs/DECISIONS.md` (DEC-FBR-IMPL-01).

### Pedantic clippy on the repository crate

**Decision**: `crates/feedbackmonk-repository/Cargo.toml` declares `[lints.clippy] pedantic = { level = "warn", priority = -1 }` on top of the workspace `clippy::all = deny`.

**Rationale**: This crate IS the security boundary for tenant isolation. The opportunity cost of letting an avoidable footgun ship here is much higher than in `-core` (pure data) or `-api` (HTTP plumbing). Pedantic clippy catches several classes of subtle issue (lossy casts, awkward error propagation, unnecessarily-loose types) that show up in DB-touching code disproportionately.

**Trade-offs**: Some pedantic lints are noisy; we'll allow individual lints case-by-case rather than blanket-disable. The cost is mild lint-management overhead.

**Implementation**: `Cargo.toml` `[lints.clippy]` block. Verified GREEN on this commit via `cargo clippy --workspace --all-targets -- -D warnings`.

### `ProjectRepo::open_for_submission(project_id)` is allow-listed pre-auth (DEC-PODS-001)

**Decision**: `ProjectRepo::open_for_submission(project_id) -> Result<ProjectScope>` is the fourth allow-listed pre-auth method. It mints a `ProjectScope` from a raw URL-path `project_id` WITHOUT a `TenantScope`. Used ONLY by the public submission endpoint (`POST /api/v1/projects/{project_id}/feedback`, FR-FBR-03). Documented in `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`.

**Rationale**: The submission endpoint is PUBLIC by design — the end-user JWT it carries identifies an end-user, not a tenant (DEC-FBR-04). There is no admin session, so there is no `TenantScope` flowing into the handler. After body validation + JWT-or-anon dispatch, the handler must call `FeedbackRepo::submit_authenticated` / `submit_anonymous`, both of which require a `ProjectScope`. Without this method, the handler would have no legitimate way to mint one.

The method body looks up `tenant_id` from the `projects` table and mints both `TenantScope` and `ProjectScope` inside the repository crate where the `pub(crate)` constructors are accessible. Authentication of the end-user happens DOWNSTREAM via the JWT verifier (Contract C2) or anonymous rate-limit gate (FR-FBR-06).

**Trade-offs**: Adds a fourth entry to the pre-auth allowlist. Different from the existing three in that the `Uuid` argument is *not* an already-verified identifier — it's a public URL-path parameter. This is intentional: the method ONLY mints a scope, it does NOT authorize any writes; authorization happens at the JWT/anon layer downstream. The contract is: "I am a public endpoint by design; please mint me a scope so I can write through the tenant-isolated path, and the rest of the protection lives in the JWT verifier."

**Implementation**: `src/projects.rs` — `async fn open_for_submission(&self, project_id: Uuid) -> Result<ProjectScope>`. Returns `RepoError::NotFound` for unknown `project_id`. Three integration tests cover existing-project mint, unknown-project, and cross-tenant binding invariant (`open_for_submission_binds_scope_to_correct_tenant_across_tenants`). Allowlist entry under "pre-authentication boundary" rationale citing DEC-FBR-04 + DEC-PODS-001.

### `TenantRepo::get_brand` is its own method, not a widening of `find_by_email` (P1 Stage 1)

**Decision**: `TenantRepo::get_brand(&TenantScope) -> Result<EmailTenantBrand>` is a new tenant-scoped method that reads the brand columns added by migration 00005. We did NOT widen the pre-auth allow-listed `find_by_email` to return brand fields.

**Rationale**: `find_by_email` is one of the three pre-auth allow-listed methods (alongside `create` and `scope_for`). Pre-auth surface discipline is load-bearing — every column added to a pre-auth return type expands the attack surface available to a caller that has only proven they know an email address. Brand fields are NOT load-bearing pre-auth (they're consumed by email-template rendering, which happens deep inside an authenticated request lifecycle). Adding a separate `get_brand(&TenantScope)` keeps the pre-auth payload minimal and routes brand reads through normal scope-discipline.

**Trade-offs**: One extra DB round-trip for handlers that need both auth and brand. Mitigated by the fact that admin handlers are rarely auth-then-brand in the same call path (auth happens at session-middleware time; brand is read by the email-emit code path, which has its own scope). Stage 2 Worker A may revisit if profiling shows the extra round-trip is hot.

**Implementation**: `src/tenants.rs` — `async fn get_brand(&self, scope: &TenantScope) -> Result<EmailTenantBrand>` + `async fn update_brand(&self, scope: &TenantScope, brand: &EmailTenantBrand) -> Result<()>`. `EmailTenantBrand` value type lives in `src/tenants.rs` and is re-exported from `lib.rs`. Pre-existing `TenantRepo::create` was extended to populate the brand-column defaults inline so new signups land identically to migration 00005's backfilled rows. Cross-tenant negatives + defaults-match-migration tests cover the new surface.

### `EmailVerificationRepo` + `redeem` allow-listed pre-auth (DEC-PODS-002)

**Decision**: Added `EmailVerificationRepo` trait + `SqlxEmailVerificationRepo` impl + `migrations/00002_email_verifications.sql` to back FR-FBR-02's verify-email idempotency. The `redeem(token)` method is allow-listed pre-auth (the verify token IS the credential at redemption time). `create` and `mark_used` both take `&TenantScope` — scope-discipline-clean.

**Rationale**: The signup → verify-email flow requires durable token storage for idempotency (a double-clicked verify link must succeed both times within a replay window). Two paths existed: (a) inline `sqlx::query` in `feedbackmonk-api`, which would FAIL the `multi-tenant-isolation-check` oracle Probe A (forbidden patterns outside repository crate); (b) add the trait + impl to this crate with `redeem` allow-listed. Path (a) is structurally impossible — the CI gate blocks it. Path (b) follows the established `TenantRepo::find_by_email` precedent: at redemption time, the tenant is in `pending_verification` state (`verified_at IS NULL`), and the token itself is the credential establishing tenant identity. The API layer chains `redeem` → `TenantRepo::scope_for` to mint a real `TenantScope` for `mark_used` + `TenantRepo::mark_verified` downstream calls.

**Trade-offs**: Adds a fifth entry to the pre-auth allowlist (4 trait methods + 1 inherent constructor). The pattern is structurally identical to existing allow-listed entries; oracle-enforced first-arg discipline + allowlist rationale review is the canonical mechanism. The `email_verifications` table cascades from `tenants` (`ON DELETE CASCADE`), so tenant deletion cleans up orphaned tokens.

**Implementation**: `src/email_verifications.rs` — `create` / `redeem` / `mark_used` + 5 sqlx-test integration tests (round-trip, unknown-token, mark-used round-trip, duplicate-token Conflict, cascade-delete). `migrations/00002_email_verifications.sql` adds the table with token PK, FK to `tenants` ON DELETE CASCADE, `expires_at NOT NULL`, `used_at` nullable, `created_at` default, and an index on `tenant_id`. Allowlist entries: `EmailVerificationRepo::redeem` (rationale: "Pre-auth: opaque verify-email token IS the credential. At redemption time the tenant is in pending-verification state and no TenantScope can exist. Mirrors the rationale for `TenantRepo::find_by_email`.") + inherent `SqlxEmailVerificationRepo::new`.

### `check_tier_quota(scope, resource)` is the single chokepoint for tier enforcement (P3 Stage 1)

**Decision**: `TierQuotaRepo::check_tier_quota(&TenantScope, ResourceKind) -> QuotaStatus` is the **sole** authoritative path for tier-cap enforcement. Every domain-write handler under `crates/feedbackmonk-api/src/handlers/` MUST consult it BEFORE its first INSERT for chargeable resources. The `tier-enforcement-status` Verification Oracle Probe A enforces this invariant at AST level — handlers either call the predicate or appear in the oracle's allowlist with a documented rationale.

**Rationale**: Tier enforcement is a load-bearing commercial-gate invariant (FR-FBR-14) and a load-bearing brand-promise surface (the free-tier footer is what underwrites *"Plausible Analytics for product feedback"* — DEC-FBR-02). Distributing the cap-check across handlers — each implementing its own count + threshold + error-mapping — creates two distinct drift surfaces: (a) per-resource arithmetic divergence (one handler counts feedback differently), (b) cap-bypass holes (a new handler gets added and forgets to check). The single-chokepoint pattern collapses both into one auditable surface that the oracle can prove coverage over. This is structurally identical to `pii-scrub-audit`'s "tracing-subscriber setup outside `feedbackmonk-tracing/` is forbidden" defense — the AST oracle is what makes "always consult X" a code-level invariant rather than a code-review hope.

**Trade-offs**: Adding a chargeable-resource means: (a) extending `ResourceKind` enum, (b) extending `tier_quotas()` to populate the new limit per tier, (c) wiring `check_tier_quota(scope, ResourceKind::NewThing)` into the INSERT-site handler. Probe A enforces the third lest a writer ship without it. The trade is favorable — chargeable-resource additions are deliberately rare and high-stakes, and the constraint forces them through the audit-trail-bearing path.

**Implementation**: `src/tier_quota.rs` — `TierQuotaRepo` trait + `SqlxTierQuotaRepo` impl. Pre-write call sites: `crates/feedbackmonk-api/src/handlers/projects.rs` (project-create) and `crates/feedbackmonk-api/src/handlers/feedback.rs` (feedback-submission). Error mapping: `allowed = false` → `ApiError::TierCapExceeded { tier, resource, current, limit, upgrade_hint }` per Contract C18 (HTTP 402 for volume caps, 409 for project caps). Defense-in-depth siblings: `Tier` enum (compile-time exhaustiveness), schema CHECK in `migrations/00008_tenant_tier_check.sql` (runtime DB rejection), `tier-enforcement-status` Probe B (Contract C19 shape).

### `TenantRepo::get_widget_brand` is tier-aware (FR-FBR-14 free-tier-footer enforcement)

**Decision**: `TenantRepo::get_widget_brand(&TenantScope) -> WidgetBrand` reads `tenants.tier` alongside the brand columns and returns `footer_text: Some("powered by feedbackmonk")` when the tier is `Free`, `None` otherwise. Free-tier tenants cannot opt out; paid tenants always opt out (the footer is the free-tier brand-promise enforcement surface, not a per-tenant preference).

**Rationale**: FR-FBR-14 specifies the free-tier footer as a **product invariant**, not a configuration value. Implementing it at the brand-render boundary — the same query that returns `display_name`, `accent_color`, `logo_url` — keeps the policy at the data-shape edge, where the widget consumes it without knowing tier. Alternative locations considered: (a) the widget JS computes footer from a `tier` field added to its config endpoint — rejected because it puts policy in the client (a forked widget could omit the footer; brand promise becomes unenforceable); (b) the widget-config HTTP handler adds a tier-conditional branch — rejected because it duplicates tier knowledge across handler + repository, and Probe B's "single source of truth in `tier_quotas()`" defense doesn't extend to handler-side branches.

**Trade-offs**: Two: (1) Paid tenants who *want* to attribute (rare but not zero) cannot. Mitigation: this is a brand-promise design choice ratified at FR-FBR-14 spec time; if commercial demand emerges, the seam to revisit is `TierQuotas.show_footer: bool` (already a field on the config struct, currently hardcoded). (2) Brand reads now fan out to one extra column on `tenants`. Sub-microsecond cost; not measurable.

**Implementation**: `src/tenants.rs::get_widget_brand`. Returns `WidgetBrand { display_name, accent_color, logo_url, footer_text: Option<String> }`. Tier-flip is enforced via a sqlx::test that seeds two tenants (one Free, one Pro), reads `get_widget_brand` on each, asserts `footer_text` is `Some("powered by feedbackmonk")` and `None` respectively. Probandurgy lineage: `tier-enforcement-status` Probe C `--full` integration smoke includes the tier-flip end-to-end through the widget-config handler.
