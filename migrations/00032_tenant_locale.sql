-- 00032_tenant_locale.sql -- the tenant's Language setting (FR-FBR-38,
-- DEC-FBR-IMPL-31, Contract C38).
--
--   tenants.locale             -- the tenant's chosen UI language. Two jobs, one
--                                 value (DEC-FBR-IMPL-31): it is the admin
--                                 console's language, AND the fallback language
--                                 for an email whose recipient has no
--                                 `feedback.submitter_locale` of their own.
--                                 NULLABLE = "never chosen"; the admin UI then
--                                 follows the browser and emails fall to
--                                 English. A NULL is not 'en': one is an absent
--                                 preference, the other a stated one, and only
--                                 the first should start following the browser
--                                 when the admin travels.
--
--   tenants.translate_outbound -- RESERVED for FR-FBR-40 (Stage 2: machine
--                                 translation of outbound replies). INERT in
--                                 this stage: written and read by the C38
--                                 settings endpoint so the setting persists,
--                                 consulted by nothing. It ships now because the
--                                 alternative is a second `tenants` migration in
--                                 four weeks for one boolean, and DEFAULT false
--                                 means an un-migrated behaviour and a migrated
--                                 one are indistinguishable until W-E wires it.
--
-- Both additive; `locale` nullable, `translate_outbound` NOT NULL DEFAULT false
-- (an off switch is the safe default for anything that could cause egress --
-- the same posture as the FR-FBR-30 provider defaulting to `off`).
--
-- The CHECK bounds length only, for the same reason as 00031: the shipped-locale
-- vocabulary is additive data (`i18n/locales.json`), not schema.

ALTER TABLE tenants
    ADD COLUMN locale TEXT
        CHECK (locale IS NULL OR char_length(locale) BETWEEN 2 AND 35),
    ADD COLUMN translate_outbound BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN tenants.locale IS
    'C34 UI-locale code for this tenant: admin console language AND the email fallback language when the recipient has no submitter_locale (DEC-FBR-IMPL-31). NULL = never chosen (follow the browser; emails fall to English).';

COMMENT ON COLUMN tenants.translate_outbound IS
    'RESERVED for FR-FBR-40 (Stage 2 outbound machine translation). Persisted by the C38 settings endpoint; consulted by no code path in this stage.';
