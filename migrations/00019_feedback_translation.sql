-- 00019_feedback_translation.sql -- multilingual feedback translation (FR-FBR-30)
--
-- Adds an async translate-after-accept pipeline so non-English feedback is made
-- available in a canonical language (English, v1) for English-assuming consumers
-- (FR-FBR-28 sentiment, the P5 agentic loop / clustering, admin full-text search)
-- WITHOUT ever overwriting the verbatim `body`.
--
-- Decisions (load-bearing — do NOT silently relax):
--   DEC-FBR-IMPL-25 — store-both (verbatim `body` is NEVER overwritten; Q24
--     preserved); async translate-AFTER-accept (NEVER on the public submit path);
--     English canonical target (v1); universal / not tier-gated; lazy backfill
--     (pre-existing rows keep NULL translation and fall back to the original);
--     FTS indexes the translation.
--   DEC-FBR-IMPL-26 — provider pluggable, DEFAULT OFF; egress is a conscious,
--     disclosed choice.
--
-- Design:
--   * Three nullable columns sit ALONGSIDE `body` (store-both):
--       - body_translated  — the canonical-language (English, v1) translation.
--                            NULL until the worker writes it (or for a skipped /
--                            failed / disabled / pre-feature row).
--       - source_lang      — the provider-detected source language (BCP-47 /
--                            ISO-639-1, e.g. `de`, `pt-BR`). NULL until detected.
--       - translation_status — NULL | pending | translated | skipped | failed
--                            (see the CHECK below for the meaning of each).
--     plus a bounded-retry counter:
--       - translation_attempts — number of provider attempts that FAILED for
--                            this row. Bounds auto-retry of `failed` rows (the
--                            worker re-claims `failed` rows whose attempts are
--                            below a code-side cap; DEC-FBR-IMPL-25 D2).
--
--   * translation_status lifecycle:
--       - NULL       = pre-feature row, sentiment-only row, or a row accepted
--                      while the provider was OFF. Lazy-backfill: NEVER touched
--                      by the worker. Reads fall back to `body`.
--       - pending    = stamped at submit when the provider is ENABLED and the
--                      row has a non-empty body. The worker's claim set.
--       - translated = the worker wrote `body_translated` + `source_lang`.
--       - skipped    = the provider detected the source already equals the
--                      target (no translation needed); `body_translated` stays
--                      NULL so reads fall back to the (already-English) `body`.
--       - failed     = the provider errored. Re-pollable until the attempts cap.
--
--   * FTS REPOINT (DEC-FBR-IMPL-25 "FTS indexes the translation"): `body_tsv`
--     (migration 00011) was `GENERATED ALWAYS AS (to_tsvector('english', body))`
--     — hardcoded to the `'english'` config, so it mis-stems non-English bodies.
--     Repoint the generated column at `coalesce(body_translated, body)` so the
--     existing `'english'`-config index becomes CORRECT for non-English rows once
--     translated, and falls back to today's behaviour (`english(body)`) for
--     untranslated rows. Because it stays `GENERATED ALWAYS`, the vector
--     auto-recomputes when the worker UPDATEs `body_translated` — ZERO extra
--     maintenance, no UPDATE trigger (mirrors 00011's / 00006's convention).
--
-- Tenant isolation (DEC-FBR-03): this migration adds NO new query path outside
-- the repository crate. The translate-after-accept worker's claim/set/skip/fail
-- queries are tenant-AGNOSTIC transport keyed on the immutable `feedback.id`
-- primary key — they read a row's `body`, translate it, and write the result
-- back to the SAME row; no data crosses tenants and nothing is returned to a
-- request surface. Those four `FeedbackRepo` methods are documented pre-auth
-- exceptions in the `multi-tenant-isolation-check` allowlist. The columns +
-- indexes here are tenant-agnostic storage.
--
-- Lineage:
--   docs/specs/SPECIFICATION.md FR-FBR-30
--   docs/specs/DECISIONS.md DEC-FBR-IMPL-25 / DEC-FBR-IMPL-26
--   docs/planning/plans/20260621T160022-fr-fbr-30-multilingual-translation.md (Stream A)
--   DEC-FBR-03 (sole query path through feedbackmonk-repository)
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

-- ============================================================================
-- Store-both columns (additive, all nullable except the retry counter).
-- ============================================================================

ALTER TABLE feedback
    ADD COLUMN body_translated TEXT
        CHECK (body_translated IS NULL OR char_length(body_translated) BETWEEN 1 AND 65536);

ALTER TABLE feedback
    ADD COLUMN source_lang TEXT
        CHECK (source_lang IS NULL OR char_length(source_lang) BETWEEN 1 AND 35);

ALTER TABLE feedback
    ADD COLUMN translation_status TEXT
        CHECK (translation_status IS NULL
               OR translation_status IN ('pending', 'translated', 'skipped', 'failed'));

-- Number of FAILED provider attempts for this row. Bounds auto-retry: the worker
-- re-claims `failed` rows whose attempts are below a code-side cap, then stops.
ALTER TABLE feedback
    ADD COLUMN translation_attempts SMALLINT NOT NULL DEFAULT 0
        CHECK (translation_attempts >= 0);

-- ============================================================================
-- FTS repoint: index the translation (coalesce to the original as fallback).
-- ============================================================================

-- Drop the GIN index + the generated column, then re-create the column over
-- coalesce(body_translated, body) and the index. `to_tsvector('english', …)` of
-- a coalesce over two IMMUTABLE-safe text columns is itself IMMUTABLE, which is
-- the requirement for a STORED generated column.
DROP INDEX IF EXISTS feedback_body_tsv_idx;
ALTER TABLE feedback DROP COLUMN body_tsv;
ALTER TABLE feedback
    ADD COLUMN body_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('english', coalesce(body_translated, body))) STORED;
CREATE INDEX feedback_body_tsv_idx
    ON feedback USING GIN (body_tsv);

-- ============================================================================
-- Worker claim index (partial — covers only the small non-terminal working set).
-- ============================================================================

-- The worker claims `pending` rows and re-claims `failed` rows (bounded retry).
-- Index both so the claim query is an index scan over the tiny non-terminal set,
-- never a full-table scan. Terminal rows (translated/skipped, and failed rows
-- past the attempts cap) are excluded from `pending`/`failed` or filtered by the
-- attempts predicate in the query; the count of permanently-failed rows is
-- expected to be negligible.
CREATE INDEX feedback_translation_worklist_idx
    ON feedback (accepted_at)
    WHERE translation_status IN ('pending', 'failed');
