-- 00012_tenant_widget_brand_overrides.sql -- post-v1 (DEC-FBR-IMPL-11 + DEC-FBR-IMPL-12)
--
-- Per-tenant widget brand overrides, surfaced by GitCellar's dogfooding of the
-- live widget. ADDITIVE and all-NULLABLE: every column defaults to NULL, which
-- means "fall through to the tier default (footer) or the widget's own CSS
-- default (theme/color/logo)". No existing tenant's widget-config changes until
-- an override is explicitly set, so there is NO backfill and NO behavior change
-- on deploy.
--
-- Column semantics:
--   footer_text_override  — NULL  ⇒ tier default (tier_quotas(tier).footer_text;
--                                    FR-FBR-14 path for external Free tenants)
--                           ''    ⇒ explicit SUPPRESS (widget renders no footer)
--                           text  ⇒ custom footer text (white-label)
--   footer_url            — NULL  ⇒ widget badge href defaults to
--                                    https://feedbackmonk.com
--                           url   ⇒ custom badge href (real marketing URL /
--                                    white-label target, no widget rebuild needed)
--   widget_theme          — NULL  ⇒ widget resolves 'auto'
--                           auto|light|dark ⇒ per-tenant default theme
--   widget_primary_color  — NULL  ⇒ widget uses its WCAG-AA-safe CSS default
--                                    (#2563eb); a value forces the accent
--                           '#rrggbb' ⇒ per-tenant accent
--   widget_logo_url       — NULL  ⇒ no logo in the modal header
--                           url   ⇒ per-tenant logo image
--
-- IMPORTANT (FR-FBR-14): footer_text_override is written ONLY via the ops
-- mutation endpoint (POST/PATCH /api/v1/ops/tenants/{id}, ops-token-guarded),
-- never tenant-self-serve. Tier default remains the source of truth for any
-- tenant whose override is NULL — so external Free tenants cannot remove the
-- "powered by feedbackmonk" badge themselves.
--
-- tier_quotas() (Contract C19) is UNCHANGED by this migration — the override is
-- a resolution layer above the tier default in
-- crates/feedbackmonk-repository/src/tenants.rs::get_widget_brand.
--
-- Append-only: this migration is 00012 (00011 was the last). Never edited.
--
-- Lineage:
--   DEC-FBR-IMPL-11 (footer/tier decoupling + ops endpoint)
--   DEC-FBR-IMPL-12 (theme knob + per-tenant primary_color/logo)
--   FR-FBR-14 (brand promise — preserved for external Free tenants)
--   Contract C12 (widget-config brand shape; gains footer_url + theme)

ALTER TABLE tenants
    ADD COLUMN footer_text_override TEXT,
    ADD COLUMN footer_url           TEXT,
    ADD COLUMN widget_theme         TEXT,
    ADD COLUMN widget_primary_color TEXT,
    ADD COLUMN widget_logo_url      TEXT;

-- Defense-in-depth: theme is a closed enum (mirrors the tenants_tier_check
-- pattern from 00008). NULL is allowed (means 'resolve auto' at the widget).
ALTER TABLE tenants
    ADD CONSTRAINT tenants_widget_theme_check
    CHECK (widget_theme IS NULL OR widget_theme IN ('auto', 'light', 'dark'));
