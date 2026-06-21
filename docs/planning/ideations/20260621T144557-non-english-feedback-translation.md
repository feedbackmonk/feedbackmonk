# Ideation Notes
**Source**: /0-uldf-ldis-ideate
**Generated**: 2026-06-21T14:45:57
**Project**: feedbackmonk (feature exploration — non-English feedback handling)

## The Idea

feedbackmonk currently stores and processes feedback as a single verbatim `body TEXT`
column with no language awareness. GitCellar (customer #1) inherits Gitea's full set of
translations and serves a many-language userbase, so non-English feedback is expected in
production. Today such feedback is stored as-is, shown to the admin as-is, and — critically —
handed to downstream English-assuming consumers (FR-FBR-28 sentiment, the P5 agentic
resolution loop) as-is, where it degrades silently rather than failing loudly.

The idea: add a translation pipeline so incoming feedback in any language is made available
in a canonical language (English for v1) for both the human admin and the machine consumers,
while preserving the original verbatim text.

## Problem Space

- **Who feels it**: self-hosting tenants with international userbases (GitCellar first), and
  every downstream consumer that assumes English.
- **Why now**: P5a (agentic resolution loop, recommend-only) is live and FR-FBR-28 sentiment
  is recent — both silently assume English input. Non-English feedback erodes their quality
  with no error surfaced.

## Key Insights from Discussion

- **Nothing exists today.** Verified against code: `feedback` table (migration `00001`) has
  no language/locale/detected-language column; no translation pipeline, no language
  detection, no `Accept-Language` handling on the submit path. The only `translat*` hits in
  Rust are incidental doc-comment metaphors. Admin-UI `locale` references are all
  number/date formatting, not i18n.
- **The legal weight is about hand-off, not about us reading the text.** Feedback bodies are
  personal data (free text + stored `end_user_email`/`end_user_name`). Any external
  translation provider becomes a GDPR **data processor** → needs a DPA, privacy-policy
  disclosure, and (for US providers like Google) a cross-border transfer mechanism. DeepL
  (EU-based, offers a DPA, contractually does not train on Pro-API text) is materially
  cleaner than Google for EU data.
- **The real tension is self-host, not the SaaS operator.** Because feedbackmonk is AGPL and
  self-hosted, a hard external-API call would silently make *every self-hoster* a data
  exporter to a third party they never chose — colliding with the DEC-FBR-02 privacy brand
  promise. This makes "provider configurable + egress disclosed + a no-egress option"
  a design constraint, not a nicety.
- **Q24 forces store-both.** Roadmap promotion (FR-FBR-12 / Q24) reads `body` verbatim with
  no attribution. The original must remain untouched; the translation lives alongside it.

## Identified Feature Domains

- **Storage / schema**: keep `body` verbatim (Q24-safe); add a translated-English field +
  a detected-source-language field. New migration.
- **Translation pipeline**: async background job triggered after a feedback row is accepted
  (NOT synchronous on the public submit path). Language detection (skip translation when
  source is already the target language). Graceful "not yet translated" handling for
  consumers reading before the job completes.
- **Provider abstraction**: pluggable provider with three modes — **DeepL** (recommended
  cloud, EU), **local model** (no-egress, e.g. LibreTranslate/Argos-style), and **off**.
  Default **off** so a self-hoster makes a conscious egress choice. GitCellar sets DeepL.
- **Downstream consumer wiring**: FR-FBR-28 sentiment and the P5 agentic loop read the
  translated-English field. FTS (`00011`) and clustering (`00013`) decide original-vs-
  translation-vs-both indexing (deferred — see Open Questions).
- **Config / disclosure**: provider + credentials configuration; privacy-policy/self-host-env
  disclosure of the chosen processor (Contract C21 env-catalog SSOT touchpoint).

## Scope Thinking

- **MVP / Phase 1**:
  - Schema: verbatim `body` + translated-English column + detected-source-language column.
  - Async translate-after-accept job with language detection (skip when already English).
  - Provider abstraction with **DeepL** and **off** modes (default off).
  - Wire sentiment (FR-FBR-28) + P5 agentic loop to read the translated field, with
    graceful fallback to original when translation absent.
  - Egress disclosure in self-host env docs / privacy surface.
- **Future phases**:
  - **Local no-egress translation model** (LibreTranslate/Argos-style).
  - **Configurable primary / admin language** — per-project target language for the human
    admin (decided: future, not v1; v1 always targets English).
  - Admin-UI display of original-vs-translation toggle.
- **Out of scope**:
  - Synchronous at-submit translation (rejected — adds latency + a hard provider dependency
    to the rate-limited public widget endpoint).
  - Overwriting the verbatim body (rejected — Q24).

## Technical Considerations

- Async job model — reuse whatever background-processing pattern P5a / sentiment already use
  (to confirm during intake/spec).
- Translation cost is per-character; interacts with P3 tier enforcement — whether translation
  is a paid-tier feature or universal is an open question.
- Migration adds nullable columns; backfill strategy for existing rows (likely lazy / on next
  touch, or a one-shot backfill job) to confirm.

## Open Questions — ALL RESOLVED 2026-06-21 (formalized into spec)

Formalized as **FR-FBR-30** (SPECIFICATION.md) + **DEC-FBR-IMPL-25/26** (DECISIONS.md).

- **FTS + clustering indexing** → **RESOLVED: index the translation.** `body_tsv` (00011) is
  already `to_tsvector('english', body)` — hardcoded to the `'english'` config, so it currently
  mis-stems non-English bodies. Repointing FTS at the translated-English text makes the existing
  index *correct*; verbatim `body` stays stored for display + Q24. Clustering/analyst read the
  translated field with original fallback. (DEC-FBR-IMPL-25)
- **Tier interaction** → **RESOLVED: universal, not tier-gated** (user call). Correctness/quality
  feature, not a premium add-on; self-hosters use their own key. (DEC-FBR-IMPL-25)
- **Backfill** → **RESOLVED: lazy / no backfill** (user call). Applies only to feedback accepted
  after enablement; pre-existing rows keep NULL translation + fall back to original. Manual
  admin-triggered backfill possible later. (DEC-FBR-IMPL-25)
- **Local-model provider** → **RESOLVED: deferred to future**, LibreTranslate noted as the likely
  AGPL-compatible candidate. v1 ships DeepL + off only. (DEC-FBR-IMPL-26)

## Decisions Made (during ideation — to be ratified in spec)

- **D-XLATE-1 — Translate, don't just store**: non-English feedback is translated to a
  canonical language for admin + machine consumers. Rationale: silent quality degradation of
  sentiment + agentic loop on non-English input.
- **D-XLATE-2 — Store-both**: persist verbatim original + translated English + detected
  source language; original is never overwritten. Rationale: Q24 verbatim invariant +
  auditability.
- **D-XLATE-3 — Async after-accept timing**: translation runs as a background job after the
  feedback row is accepted, never synchronously on the public submit path. Rationale: avoid
  adding latency / a hard provider dependency to the rate-limited public widget endpoint.
- **D-XLATE-4 — Pluggable provider, default off, no-egress option required**: DeepL (cloud,
  EU) + local model + off; default off. Rationale: AGPL self-host means a hard external call
  would silently make every self-hoster a third-party data exporter — collides with
  DEC-FBR-02. Egress must be a conscious, disclosed choice.
- **D-XLATE-5 — English target for v1; configurable primary/admin language deferred**:
  machine consumers genuinely need English; per-project target language is a future
  enhancement. Rationale: keep v1 scope tight; the machine-consumer need is English-specific.

## Legal / Compliance Notes (load-bearing)

- Translation provider = GDPR data processor → DPA + privacy-policy disclosure required;
  US providers need a cross-border transfer mechanism (SCCs).
- DeepL preferred over Google for EU data (EU-based, DPA, no-training-on-Pro-text).
- Self-hosters become controllers and inherit these obligations — hence provider must be
  configurable, egress disclosed, and a no-egress path available.
