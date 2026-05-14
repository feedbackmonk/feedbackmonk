-- 00008_tenant_tier_check.sql -- P3 Stage 1 Phase 1 (Commercial Gate)
--
-- Adds a CHECK constraint on `tenants.tier` enumerating the canonical
-- four pricing-tier values (DEC-FBR-03). Defense-in-depth pairing with
-- the Rust `Tier` enum + `Tier::from_db_str` strict parser
-- (`feedbackmonk-core::tier`).
--
-- Lineage:
--   FR-FBR-14 (tier enforcement)
--   DEC-FBR-03 (pricing tier matrix)
--   Contract C19 (P3 plan §Interface Contracts) — `tier_quotas()` keys
--
-- Three-leg defense (DEC-FBR-IMPL-* style):
--   leg 1 = `Tier` enum (compile-time exhaustiveness in match)
--   leg 2 = THIS schema CHECK (runtime DB rejection of bad writes)
--   leg 3 = `tier-enforcement-status` oracle Probe B (config-shape drift)
--
-- The existing `tier TEXT NOT NULL DEFAULT 'free'` column from migration
-- 00001 is augmented — no data change required at deploy time because
-- every existing row has 'free' (the default). Future migrations that
-- introduce a new tier value MUST update this CHECK constraint in the
-- same migration as adding the `Tier::<Variant>` variant.

ALTER TABLE tenants
    ADD CONSTRAINT tenants_tier_check
    CHECK (tier IN ('free', 'starter', 'pro', 'self_host'));
