-- 00031_submitter_locale.sql -- the submitter's UI language, captured at submit
-- (FR-FBR-37, Contract C37).
--
-- WHY A NEW COLUMN AND NOT `source_lang` (00019)
--
-- The two look alike and mean opposite things, so conflating them would be a
-- privacy bug wearing a schema shortcut:
--
--   feedback.source_lang       -- what the TRANSLATION PROVIDER DETECTED the
--                                 body to be written in (FR-FBR-30). Derived
--                                 from content, after the fact, by a machine.
--
--   feedback.submitter_locale  -- what UI LANGUAGE the human was reading when
--                                 they submitted. A stated preference, captured
--                                 at the one moment it is knowable (the request
--                                 carries it; nothing later does). Used to pick
--                                 the language of the emails we send them back.
--
-- A German speaker filing in English has `source_lang = 'EN'` and
-- `submitter_locale = 'de'`. Reading either as the other sends the wrong
-- language or mis-attributes the content's language.
--
-- ADDITIVE AND NULLABLE. Every existing row stays NULL, which reads as "we do
-- not know this submitter's language" -- deliberately distinct from 'en'
-- ("this submitter reads English"). No backfill: inventing a locale for a past
-- submission would be a guess recorded as a fact.
--
-- VALUES are C34 canonical codes (`de`, `pt-BR`, `zh-Hans`-class tags resolved
-- BEFORE storage). The CHECK bounds length only -- the shipped-locale vocabulary
-- lives in `i18n/locales.json` and moves additively (DEC-FBR-15), so pinning it
-- into a DB constraint would make adding a language a migration. 35 is the
-- BCP-47 practical ceiling (RFC 5646 `language-script-region-variant`).
--
-- EXPOSURE: admin-only. `FeedbackDetailResponse` carries it; `FeedbackListItem`,
-- every board/roadmap projection and every public read do NOT (it is
-- PII-adjacent -- a locale narrows a population). The
-- `public-board-moderation-gate` oracle's no-PII wire scan enforces that.

ALTER TABLE feedback
    ADD COLUMN submitter_locale TEXT
        CHECK (submitter_locale IS NULL OR char_length(submitter_locale) BETWEEN 2 AND 35);

COMMENT ON COLUMN feedback.submitter_locale IS
    'C34 UI-locale code the submitter was reading at submit time (FR-FBR-37). A stated preference, NOT the provider-detected source_lang of the body. Admin-read only; never on a public projection.';
