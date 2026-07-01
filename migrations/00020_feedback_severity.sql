-- 00020_feedback_severity.sql -- first-class severity on a feedback row
--
-- GitCellar parity — first-class severity, Phase A; replaces the
-- `external_metadata.severity` side-channel. A 4-point impact signal supplied
-- by the submitter (or the embedding product) at submit time. Designed
-- tenant-generically: severity is OPTIONAL (nullable) — it is never required,
-- and NULL simply means "no severity supplied".
--
-- The domain type lives in `feedbackmonk-core::severity::Severity`; its
-- `as_db_str()` values match this CHECK constraint byte-for-byte
-- (`low | medium | high | blocker`), mirroring how `sentiment` (migration
-- 00017) pairs with `feedbackmonk-core::sentiment::Sentiment`.
--
-- Lineage:
--   docs/planning/plans/20260701T161200-feedbackmonk-phase-a-gitcellar-contract.md
--     (Stage 0 S0.3 migration reservation; consumed by stream A4)
--   DEC-FBR-03 (sole query path through feedbackmonk-repository; `feedback`
--     already carries denormalized tenant_id + project_id — this adds a column
--     to that table, no new table / no new scope columns needed)
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

-- First-class 4-point severity. Nullable; CHECK mirrors `sentiment` (00017).
ALTER TABLE feedback ADD COLUMN severity TEXT NULL
    CHECK (severity IN ('low', 'medium', 'high', 'blocker'));
