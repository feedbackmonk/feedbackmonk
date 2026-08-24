-- 00029_feedback_rating.sql -- first-class 1-5 rating on a feedback row
--
-- A finer-grained sibling of `sentiment` (migration 00017), requested by the
-- GitCellar in-app solicitation card, whose prompt moved from a 3-point tap to
-- a 1-5 scale.
--
-- WHY A NEW COLUMN RATHER THAN WIDENING `sentiment`
--
-- The obvious move -- widen the `sentiment` CHECK from three values to five --
-- was deliberately rejected:
--
--   * `sentiment` is a PUBLISHED wire enum on a public submit API with more
--     than one consumer (the embeddable widget, the admin UI, and every
--     adopting product). Widening it breaks all of them at once.
--   * Every stored row carries the 3-point value, and the admin
--     sentiment-trend aggregation
--     (GET /api/v1/admin/feedback/sentiment-trend, index
--     `feedback_project_sentiment_idx`) emits a fixed three-bucket shape.
--     Widening strands that history behind a backfill whose mapping is a guess.
--
-- So this is ADDITIVE. A submission may carry a rating, a sentiment, both, or
-- neither. When a rating is present and a sentiment is not, the API derives the
-- sentiment from the rating (1-2 => negative, 3 => neutral, 4-5 => positive;
-- `feedbackmonk-core::rating::Rating::to_sentiment`) and writes BOTH columns --
-- so a 5-point client keeps populating the 3-point column, every existing
-- reader keeps working untouched, and no backfill exists to get wrong.
--
-- The domain type is `feedbackmonk-core::rating::Rating`, whose MIN/MAX match
-- this CHECK constraint exactly, mirroring how `sentiment` (00017) pairs with
-- `Sentiment` and `severity` (00020) with `Severity`.
--
-- NOTE ON THE BODY-OR-SENTIMENT INVARIANT: `feedback_body_or_sentiment_check`
-- (00017) is deliberately NOT relaxed to accept a bare rating. Because the API
-- derives and stores `sentiment` whenever a rating is supplied, a rating-only
-- submission always lands with `sentiment` populated and satisfies the existing
-- constraint as-is. Leaving the constraint alone keeps that derivation
-- structurally enforced rather than merely conventional -- a rating that
-- somehow reached the DB without its derived sentiment would be rejected.
--
-- Idempotency: standard sqlx migrator semantics -- runs exactly once.

-- First-class 1-5 rating. Nullable; CHECK mirrors `Rating::MIN`/`Rating::MAX`.
ALTER TABLE feedback ADD COLUMN rating SMALLINT NULL
    CHECK (rating IS NULL OR rating BETWEEN 1 AND 5);

-- Partial index mirroring `feedback_project_sentiment_idx` (00017): scoped
-- scans bucketed by time (`accepted_at`, the table's only timestamp) and filtered
-- to rows that carry a rating -- what a rating-distribution or rating-trend
-- admin view scans.
CREATE INDEX feedback_project_rating_idx
    ON feedback (project_id, accepted_at DESC)
    WHERE rating IS NOT NULL;
