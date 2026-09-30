# migrations

## Synopsis

Ordered, append-only SQL migrations (`00001`–`00032`) that build feedbackmonk's Postgres schema from empty, run lexically by `sqlx-cli`. `00001_p0_schema.sql` is the authoritative source for the column names the `feedbackmonk-repository` crate hard-depends on at sqlx-macro-compile time. Open the File Index below for what each migration adds (email verification, status history, replies, email branding, roadmap items + votes, tier check, attachments, crash-event, full-text search).

## Purpose & Responsibilities

This directory holds the ordered SQL migration set that builds feedbackmonk's Postgres schema from empty. The numbering convention (`NNNNN_description.sql`) is stable; migrations are append-only — never edit a committed migration, always add a new one.

The migration runner is `sqlx-cli` (used implicitly by `sqlx::test` macros in the repository crate). Self-host distribution (P4) will ship a `migrate` binary that runs this directory against a fresh database.

## File Index

| File | Purpose |
|---|---|
| `00001_p0_schema.sql` | P0 Foundation schema: `tenants`, `projects`, `signing_keys`, `feedback`, `anon_submissions`, `rate_limit_counters`. Backs FR-FBR-01..06. |
| `00002_email_verifications.sql` | P1 (FR-FBR-02). `email_verifications` token table backing Worker A's signup → verify-email flow; opaque 32-byte base64url token stored as `TEXT PRIMARY KEY`; `used_at` marks first redemption (replay-window idempotency handled in API layer). |
| `00003_feedback_status_history.sql` | P1 Stage 1 (FR-FBR-08). Adds BOTH the `feedback.status` column (CHECK against the six canonical kebab-case Contract-C6 statuses) AND the `feedback_status_history` audit table (cohesion decision DEC-FBR-IMPL-PI-S1-01). |
| `00004_feedback_replies.sql` | P1 Stage 2 (FR-FBR-08 reply + FR-FBR-09 public-reply email). `feedback_replies` table backing Contract C7's reply endpoint; `visibility ∈ {public, internal}` (public triggers a `PublicReplyEmail` send; internal is admin-only). Body window mirrors `feedback.body` (1..16384). |
| `00005_tenant_email_brand.sql` | P1 Stage 1 (FR-FBR-09, Contract C10). Additive branding columns on `tenants` (`brand_name`, `email_subject_prefix`, `support_email`, nullable `unsubscribe_url`, `footer_signature`) consumed by the Stage 2 email-template renderers; defaults backfilled from the existing `tenants.email`. |
| `00006_roadmap_items.sql` | P2 (FR-FBR-11, Contract C13). Schema half of the public roadmap surface: `roadmap_items` with the `considering → planned → in-progress → shipped` status machine and the `origin_feedback_id` UNIQUE constraint backing promote-to-roadmap idempotency (FR-FBR-12). Vote table lands in `00007`. |
| `00007_roadmap_votes.sql` | P2 (FR-FBR-13, Contract C14). Companion to `00006`: `roadmap_votes` with the `(item_id, voter_id)` UNIQUE double-vote guard (duplicate INSERT → `RepoError::Conflict` → 409, never a silent upsert). Backs the voting aggregator. |
| `00008_tenant_tier_check.sql` | P3 Stage 1 (FR-FBR-14, Contract C19). Adds a CHECK constraint on `tenants.tier` enumerating the four canonical pricing-tier values — defense-in-depth pairing with the Rust `Tier` enum + `Tier::from_db_str` strict parser. |
| `00009_attachments.sql` | `attachments` table (screenshot + captured-log parts). GitCellar customer-#1 parity gap #1. Tenant/project-scoped; `feedback_id` FK; storage-backend-agnostic (URI + content metadata). |
| `00010_feedback_crash_event.sql` | Adds nullable first-class `crash_event_id` column to `feedback`. GitCellar parity gap #2. NOT stored via `external_metadata` — a real column so the pull-mode correlation worker can index/join on it. |
| `00011_feedback_fts.sql` | Full-text search: `tsvector` generated column + GIN index on `feedback`. GitCellar parity gap #3. Backs `GET /api/v1/admin/feedback/search` via `websearch_to_tsquery`. |
| `00012_tenant_widget_brand_overrides.sql` | Post-v1 (DEC-FBR-IMPL-11/12). Five nullable per-tenant widget brand-override columns on `tenants` (`footer_text_override`, `footer_url`, `widget_theme` (CHECK auto\|light\|dark), `widget_primary_color`, `widget_logo_url`); all NULL = fall through to tier/CSS default. Decouples badge visibility from tier and adds widget theming/branding. Written only via the ops endpoint. |
| `00013_feedback_clusters.sql` | P5a, Contract C23 — the analyst data model: `feedback_clusters`, `analysis_sweeps` (sweep provenance, FR-FBR-20), `recommendations`, and the nullable `feedback.cluster_id` pointer. |
| `00014_work_orders.sql` | P5a, Contracts C22/C23: `work_orders` (approved decision → dispatched job, FR-FBR-22; `owner_overrides` for tweak-before-approve) and the **append-only** `work_order_events` ledger the `approval-gate-enforcement` oracle reads. |
| `00015_runner_tokens.sql` | P5b, Contract C25: `signing_keys.key_class` (`identity` vs `runner`, existing rows default `identity`), the optional `runner_tokens` registry and the append-only `runner_token_revocations` denylist. feedbackmonk still holds only public keys (DEC-FBR-04). |
| `00016_feedback_moderation.sql` | Public board + moderation gate (Contracts C28/C29): `feedback.moderation_status` (`pending`/`approved`/`rejected`, orthogonal to triage status), the append-only `feedback_moderation_events` ledger, and `projects.public_board_enabled` / `board_requires_moderation`. |
| `00017_feedback_sentiment_and_solicitation.sql` | FR-FBR-28/29: nullable `feedback.sentiment` with body-or-sentiment CHECK (body becomes optional), and `feedback_solicitations` — per-user solicitation/suppression state keyed by JWT `sub` per project. |
| `00018_feedback_board_votes.sql` | Public board voting (PF-BOARD-VOTING-01, Contract C30): `feedback_board_votes`, the board sibling of `roadmap_votes` keyed on `feedback_id` — a separate table, not a generalization (DEC-FBR-IMPL-21). |
| `00019_feedback_translation.sql` | FR-FBR-30 (multilingual translation, DEC-FBR-IMPL-25/26). Nullable `feedback.body_translated` + `source_lang` + `translation_status` (CHECK pending\|translated\|skipped\|failed) + `translation_attempts`; **repoints** the `body_tsv` generated column to `to_tsvector('english', coalesce(body_translated, body))` so FTS indexes the translation with fallback to the original; partial worklist index. Store-both — the verbatim `body` is never overwritten (Q24). |
| `00020_feedback_severity.sql` | Optional first-class `feedback.severity` (`low`/`medium`/`high`/`blocker`), replacing the `external_metadata.severity` side-channel; matches `feedbackmonk-core::severity`. |
| `00021_submit_idempotency.sql` | `submit_idempotency`: dedupes a retried `POST …/feedback` on `(project_id, idempotency_key)`, recording the created `feedback_id` (cascade-deleted with it). |
| `00022_idempotency_identity_scope.sql` | Security fix P1-3: adds `submitter_id` to the idempotency primary key (plus `content_hash` and a key-length CHECK) so two users choosing the same key no longer collide. |
| `00023_tenant_session_epoch.sql` | Security fix P1-1: `tenants.session_epoch`, folded into the signed admin cookie; bumping it (logout, password reset) revokes every outstanding session. |
| `00024_password_resets.sql` | Security fix P1-1: `password_resets` for the email-token reset flow — `token_hash` (sha256 of the wire token) is the primary key, with a short TTL. |
| `00025_feedback_short_code_per_project.sql` | Security fix P1-5: `feedback.short_code` uniqueness narrows from global to `(project_id, short_code)`, shrinking the collision space. |
| `00026_feedback_end_user_sub_index.sql` | Scrutiny P2-14: partial index on `(project_id, end_user_sub)` for the JWT-`sub`-scoped read and erasure hot path. |
| `00027_ops_audit_log.sql` | Scrutiny P1-12: append-only `ops_audit_log` recording every successful ops-token mutation (tier flips, brand overrides) against its target tenant. |
| `00028_work_orders_p6.sql` | P6 Autopilot (Contract C31): nullable provenance FKs for owner-authored work orders (with a both-or-neither CHECK) and `routing_label` for named-runner routing. |
| `00029_feedback_rating.sql` | Optional 1-5 `feedback.rating`, an additive sibling of `sentiment` rather than a widening of that published enum. |
| `00030_tenant_hosting.sql` | FR-FBR-32/33: globally unique nullable `tenants.subdomain` (with a shape CHECK) and the `tenant_domains` custom-domain registry — the host → tenant substrate DEC-FBR-13 assumes. |
| `00031_submitter_locale.sql` | FR-FBR-37, Contract C37: `feedback.submitter_locale`, the submitter's UI language at submit — deliberately distinct from `source_lang` (what the translator detected). |
| `00032_tenant_locale.sql` | FR-FBR-38, Contract C38: nullable `tenants.locale` (admin console language + email fallback) and `translate_outbound` (default false), reserved for FR-FBR-40 outbound translation. |

## Constraints & Business Rules

- **Append-only.** Never edit a committed migration. To alter a table, add a new migration that performs the alteration (`ALTER TABLE` / data migration). Editing in place corrupts every downstream environment that has already run the migration.
- **Numbered prefix is load-bearing.** `sqlx-cli` runs migrations in lexical order by filename. Future migrations: `00002_*`, `00003_*`, etc. Skipping or reusing numbers is a bug.
- **Schema column names are an interface.** `crates/feedbackmonk-repository/src/*.rs` hard-depends on every column name in `00001_p0_schema.sql`. A rename here without a coordinated repository update breaks the entire build at sqlx-macro-compile time.
- **`gen_random_uuid()` requires `pgcrypto`.** The first migration enables the extension (`CREATE EXTENSION IF NOT EXISTS pgcrypto;`). Self-host environments need a Postgres build that includes pgcrypto; documented in `docs/operations/LOCAL_DEV.md`.

## Relationships & Dependencies

- **Authoritative for**: `crates/feedbackmonk-repository` query SQL, `crates/feedbackmonk-core` record-field names.
- **Run by**: `sqlx::test` macros at test time (against the dev container on port 5433), and the future self-host migrate runner (P4).
- **Triggered by oracle**: `multi-tenant-isolation-check` lists `migrations/**` as a freshness trigger — changes here invalidate the oracle and require a re-run.

## Decision Log

### `tenants.tier` ships at P0 with default `'free'` even though enforcement is P3

**Decision**: The `tenants` table includes a `tier TEXT NOT NULL DEFAULT 'free'` column from migration `00001`, even though tier-cap enforcement (FR-FBR-14) is a P3 feature.

**Rationale**: Adding a column to a table with many rows (post-launch) is significantly more expensive than including it at greenfield. The feedbackmonk arc plan flags FR-FBR-14 as P3 work that will land months after P0; backfilling `tier` on every existing tenant at P3 would either require a downtime window or a careful zero-downtime ADD COLUMN dance. Shipping the column at P0 — with the inert default `'free'` — costs effectively nothing now and avoids a real cost later. This is forward-looking ripple-from-arc-plan, not premature optimization: the cost asymmetry is large and the implementation is one column declaration.

**Trade-offs**: A column whose only value is the default for ~6 months. The cost is one extra byte per tenant row in pg_class metadata; effectively zero.

**Implementation**: `00001_p0_schema.sql` line 23 (`tier TEXT NOT NULL DEFAULT 'free'`). FR-FBR-14 (P3) reads/writes this column; until then it is inert.

### Schema column renames require coordinated repository updates

**Decision**: Treat the column-name set in `00001_p0_schema.sql` as an interface contract with `crates/feedbackmonk-repository`. Renames require a follow-up migration AND a coordinated repository change in the same commit (or sequenced commits with a sqlx-cache regeneration in between).

**Rationale**: `feedbackmonk-repository` uses `sqlx::query!` / `sqlx::query_as!` macros that compile-check against the live schema (or against `.sqlx/` cached metadata in offline mode). A column rename without a coordinated update breaks the build at the macro-compile stage — loud, but cross-cutting. Documenting the contract explicitly here saves the next developer 15 minutes of "why does cargo build fail when the SQL parses fine."

**Trade-offs**: None — this is a documented invariant, not new policy.

**Implementation**: Inline comments in `00001_p0_schema.sql` lines 1-13 already state this; this Decision Log entry preserves the WHY beyond the file's lifetime.
