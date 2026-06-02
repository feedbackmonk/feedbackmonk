-- 00010_feedback_crash_event.sql -- feedbackmonk Gap #2 (GitCellar customer-#1
-- parity): crash-event correlation.
--
-- Adds a first-class, nullable `crash_event_id` to `feedback` so an AUTH-MODE
-- submission can link to an external crash event (GitCellar uses Glitchtip,
-- a Sentry-compatible error tracker). The id is the value GitCellar Desktop
-- captured from the Glitchtip SDK at crash time and forwards with the feedback.
--
-- DELIBERATELY a column, NOT a key inside `external_metadata` (PODS decision
-- LEAD @ 12:30, channels/decisions.md): it is queried directly (correlation
-- lookup path) and is part of the integration contract, so it earns a column.
--
-- Correlation itself is best-effort/pull (channels/decisions.md, BRAVO Task
-- Zero): the schema only stores the link key here. Resolving it to crash
-- detail (title/culprit/permalink) is an off-hot-path read handled by the
-- `crash_correlation` worker module; Glitchtip being down never blocks submit.
--
-- Lineage:
--   docs/integrations/gitcellar-adoption.md §8 gap #2
--   docs/planning/plans/20260602T121500-gitcellar-customer-1-enablement.md (Gap 2)
--   Frozen migration number 00010 (collaboration GUIDE §6).

ALTER TABLE feedback
    ADD COLUMN crash_event_id TEXT
        CHECK (crash_event_id IS NULL OR length(crash_event_id) BETWEEN 1 AND 128);

-- Correlation lookup path: "which feedback links to this crash event?".
-- Partial index keeps it tiny -- only the (rare) auth-mode crash-linked rows
-- are indexed; the overwhelming anonymous/non-crash majority is excluded.
CREATE INDEX feedback_crash_event_id_idx
    ON feedback (crash_event_id)
    WHERE crash_event_id IS NOT NULL;
