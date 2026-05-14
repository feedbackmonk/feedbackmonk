-- 00005_tenant_email_brand.sql -- P1 Stage 1 (FR-FBR-09 Contract C10)
--
-- Extends the `tenants` table with branding parameters used by Worker A's
-- email-template renderers in Stage 2. The columns are deliberately
-- ADDITIVE — no P0 consumer breaks. All non-`unsubscribe_url` columns are
-- NOT NULL with sensible defaults backfilled from the existing
-- `tenants.email` row's local-part.
--
-- Schema rationale (Contract C10):
--   - brand_name           — "Acme" — used everywhere brand identity appears
--   - email_subject_prefix — prefix in `[<prefix> #FB-XXXXXX] subject`
--   - support_email        — defaults to tenants.email; reply-to + footer
--   - unsubscribe_url      — NULLABLE; None ⇒ no unsubscribe footer line
--   - footer_signature     — closing line, e.g. "— The Acme team"
--
-- Backfill strategy: the local-part of the existing `tenants.email`
-- (substring before '@') is used as the default `brand_name` and
-- `email_subject_prefix`. `support_email` defaults to the full
-- `tenants.email`. `footer_signature` defaults to `"— The <local-part>
-- team"`. Tenants can update these via `TenantRepo::update_brand` (added
-- in this Stage 1 commit) once an admin UI surface lands (P1 Stage 2
-- Worker B; Stage 3 e2e doesn't exercise the brand-edit path).
--
-- Migration 00004 is reserved for Stage 2 Worker A (feedback_replies);
-- this is migration 00005 so Stage 2's commit lands cleanly next to
-- Stage 1's two without a renumbering merge.
--
-- Idempotency: each migration runs once via sqlx migrator. Re-running is
-- a no-op (sqlx_migrations table tracks applied versions).
--
-- Lineage:
--   FR-FBR-09 (status emails)
--   Contract C10 (P1 plan §Interface Contracts)
--   GitCellar peer reference: gitcellar-cloud/src/feedback/email_templates.rs

ALTER TABLE tenants
    ADD COLUMN brand_name           TEXT,
    ADD COLUMN email_subject_prefix TEXT,
    ADD COLUMN support_email        TEXT,
    ADD COLUMN unsubscribe_url      TEXT,
    ADD COLUMN footer_signature     TEXT;

-- Backfill from the existing email column. Uses split_part for the
-- local-part extraction.
UPDATE tenants
SET
    brand_name           = COALESCE(brand_name,           split_part(email, '@', 1)),
    email_subject_prefix = COALESCE(email_subject_prefix, split_part(email, '@', 1)),
    support_email        = COALESCE(support_email,        email),
    footer_signature     = COALESCE(footer_signature,     '— The ' || split_part(email, '@', 1) || ' team');

-- After backfill, tighten the non-NULL columns. `unsubscribe_url` stays
-- nullable (Contract C10 explicit: "None → no unsubscribe footer line").
ALTER TABLE tenants
    ALTER COLUMN brand_name           SET NOT NULL,
    ALTER COLUMN email_subject_prefix SET NOT NULL,
    ALTER COLUMN support_email        SET NOT NULL,
    ALTER COLUMN footer_signature     SET NOT NULL;
