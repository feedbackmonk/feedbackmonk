# Ideation Notes — UI localization across every human-facing surface
**Source**: owner request 2026-09-06 ("make sure our foreign languages implementation is correct, comprehensive, robust")
**Generated**: 2026-09-06T12:00:00
**Project**: feedbackmonk (feature exploration — locale-aware product; 31-locale parity with GitCellar)

## The Idea

Every user of feedbackmonk — an end-user in the embedded widget, a visitor on a tenant's public
board or roadmap, a submitter receiving a status email, a tenant admin in the triage console —
interacts with the product in the language they chose, or failing a choice, the language their
browser is set to. The language set is exactly GitCellar's 31 (English + 30, Gitea's upstream
set). Behind the scenes nothing changes for the machine: feedback is still canonicalised to
English (FR-FBR-30) so clustering, dedupe, sentiment and search work over one language. The two
axes — **UI locale** (what the human sees) and **canonical content language** (what the pipeline
reads) — are separate and must stay separate.

Translation of the UI catalogs is a **release step run only on the owner's word**, never a
development-time step, so strings can churn freely between releases without paying for
retranslation twice.

## What exists today (measured 2026-09-06)

### The backend half is built: FR-FBR-30

- Feedback is language-detected and translated to English by a background worker after accept;
  the verbatim `body` is never overwritten (Q24). `body_translated` + `source_lang` sit alongside.
- Search (`body_tsv`), clustering (`list_member_bodies_for_cluster`) and the P5 analyst read the
  English translation with fallback to the original. Admin has an original↔translation toggle.
- Provider is pluggable and **defaults OFF** (`FEEDBACKMONK_TRANSLATION_PROVIDER=off|deepl|libretranslate`),
  guarded by the `translation-egress-q24-isolation` oracle.
- **Production reality**: `curl https://feedback.gitcellar.com/api/v1/capabilities` still reports
  `"version":"0.2.0"` (deployed 2026-06-03, *before* FR-FBR-30 landed on 2026-06-21). No record in
  `docs/planning/feedbackmonk-deploy-state.md` or `docs/operations/RAILWAY_GITCELLAR.md` of a
  translation provider being configured. So the "process everything as English" promise is
  **inert on the only live instance** — it ships with the blocked redeploy (DEFER-009) / the
  DEC-FBR-14 SaaS cutover, and needs the provider env set at that moment.

### The UI half does not exist — anywhere

| Surface | State |
|---|---|
| `admin-ui/` (React) | No i18n library (`package.json`: react-query, axios, react, react-dom only). Every string is an inline English literal. `index.html:2` `<html lang="en">` static. The 15 enum→label maps in `shared/types.gen.ts` (`STATUS_LABELS`, `KIND_LABELS`, …) are the only centralised strings, and they live in a **generated** file. |
| Public board / roadmap | `pages/board/PublicBoard.tsx`, `pages/roadmap/PublicRoadmap.tsx` — same bundle as the admin console (`TenantHostLanding.tsx` renders them on tenant hosts). English literals; inherit `lang="en"`. |
| `widget/` | ~40 English literals across `ui.ts`, `widget.ts`, `attachments.ts`, `redact.ts`. Accepts `data-project-id`, `data-jwt`, `data-api-base`, `data-theme`, `data-capture-console`, `data-fbm-no-auto-mount`, `data-feedback-open` — **no locale attribute**. Never sets `lang` on `.fbm-root`, so its English content inherits and misdeclares the host page's language (WCAG 3.1.2). Renders server `err.message` verbatim (`ui.ts:346`). |
| Emails | `crates/feedbackmonk-api/src/email/templates.rs` — plain-text English; `status_human` hard-codes English status words; `EmailTenantBrand` (migration 00005) has brand fields but no locale. Account emails (`mailpit.rs` `build_verify_email` / `build_password_reset_email`, shared with prod SMTP via `env_smtp.rs`) English. |
| `marketing/` (Astro) | No `i18n` block in `astro.config.mjs`; flat English pages; `BaseLayout.astro:28` `lang="en"`. |
| Schema | Zero locale/language/timezone preference columns across all 26 tables. `feedback.source_lang` (00019) is the provider-*detected* language of a body, not a preference. **There is no members/users table — `tenants` is the account**, so "per-admin language" is per-tenant unless new schema is added. |
| API | Zero `Accept-Language` handling. |
| Client persistence | Zero `localStorage`/cookie usage outside the HTTP-only auth cookie. |
| Date/number formatting | `admin-ui/src/shared/format.ts:6,19` uses `Intl.RelativeTimeFormat(undefined, …)` / `toLocaleString()` — silently localises to the browser while all chrome stays English (mixed-locale output today). |
| Spec record | `DECISIONS.md:332` lists "i18n / multi-language (English-only v1, Plausible took same path)" under *v1 OUT of scope (defer to v1.1+)* — a deferral, not a non-goal, and it never got a DEFER record or a re-entry trigger. |

Widget bundle at HEAD: `widget.js` 15,041 B + `redact.js` 2,887 B + `widget.css` 5,322 B =
**23,250 B of the 30,720 B cap** — ~7.4 KB headroom. Bundling 30 catalogs is impossible;
lazy per-locale chunks (the `redact.js` precedent) are the only viable shape.

### What GitCellar does (the parity target)

Canonical source: `../GitCellar/apps/gitcellar-landing/src/i18n/locale-map.ts` (three codes per
locale — short `url` code used everywhere, Forge long form, `hreflang`; plus `dir` and endonym).
Desktop short-code vocabulary: `apps/gitcellar-desktop/src-web/src/supported-languages.ts`.

| Aspect | GitCellar |
|---|---|
| Set | 31: en, de, fr, es, pt-BR, ja, zh-CN, ko, zh-HK, zh-TW, ga, nl, lv, ru, uk, pt-PT, pl, bg, it, fi, tr, cs, sv, el, **fa (rtl)**, hu, id, ml, is, si, sk |
| Desktop | i18next + react-i18next + browser-languagedetector; nested JSON per locale, `_meta.status` provenance, `{{name}}` interpolation, CLDR plural suffixes (`_few/_many/_zero` completed per locale — i18next falls through to **English**, not `_other`, when a category is missing) |
| Detection | Desktop: `localStorage` → `navigator`, both run through a 5-step resolver (`locale-resolution.ts`: exact → override table (`zh-Hans→zh-CN`, `pt-AO→pt-PT`…) → base language → regional default (`pt→pt-BR`, `zh→zh-CN`) → leave alone/English). Forge: `?lang=` → `lang` cookie (1 year) → `Accept-Language`. Landing: **suggest, never redirect** (DEC-LS-28); geo-IP is used for sanctions + storage region only, **never for language** |
| Persistence | Desktop `localStorage` `gitcellar_language` + `settings.json`, synced bidirectionally to the Forge `user.language` (last-write-wins); landing `gc-lang-choice` / `gc-lang-suggest-dismissed` |
| Admin UI / cloud API / emails | **English only, deliberately** (staff tool); emails have no recipient locale |
| RTL | Landing yes (`dir` on `<html>`), Desktop **no** — a shipped gap |
| Tooling | DeepL Free (500k chars/mo); `check-translation-gaps.ps1` (MISSING + **DRIFTED** via per-key SHA-256 baseline `translation-source-hashes.json`); `retranslate-drifted.py` (quota preflight, refuses partial runs, byte-for-byte round-trip safety); `validate-translations.ps1` (9 classes incl. mojibake, stripped diacritics, untranslated-authored); `complete-plural-categories.py`; ratchet `i18n-audit.mjs`; 3 oracles; `/1-translate` skill; gap check in finalize, translation only on explicit invocation |
| DeepL coverage | 24/30 — `ga, fa, ml, is, si` fall back to English; `zh-HK/zh-TW` via `ZH-HANT` |
| Widget embed today | landing `ForgeIntegratedLayout.astro:411` + Forge `custom/footer.tmpl`, vendored **by copy** into two locations; no locale passed; a German visitor on `/de/pricing` gets an all-English modal |
| Lessons on record | do not MT legal text until the English is frozen (4×30 legal docs shipped then deleted); "a guard not mechanically invoked at a defined trigger is documentation, not a guard" |

## Key Insights from Discussion

- **Two axes, one product.** FR-FBR-30's target language stays English for the machine; the UI
  locale is the human's. A Japanese admin sees Japanese chrome, the verbatim Japanese body, and the
  English translation toggle — the canonical language is a pipeline detail they never have to
  think about. Conflating the two (a per-admin "target language") is the explicitly deferred
  D-XLATE-5 and stays deferred.
- **The widget cannot own the language decision.** The host page already knows its user's language
  (GitCellar's Forge cookie / Desktop setting); the widget should follow the host (`data-locale`,
  else `<html lang>`, else `navigator.languages`) rather than run its own picker inside a modal.
- **Server text reaches end users on two paths** — API error messages in the widget, and emails.
  A client-only catalog leaves both English. Error messages are solved client-side (the widget
  already receives `err.code`; map code → localized string). Emails need a server-side catalog and
  a persisted **submitter locale**, captured at submit time — the one moment the language is knowable.
- **"Where the user resides" is better served by the browser's language than by geography.**
  Geo-IP needs an IP→country lookup (an egress or a database) and gets Switzerland, Belgium,
  Canada, expats and VPN users wrong; `navigator.languages` / `Accept-Language` is the user's own
  stated preference and needs no lookup. GitCellar uses geo for sanctions only and argues against
  language inference from it (DEC-LS-28). Raised as Q23 because the owner's wording was residency.
- **Partially translated catalogs must always be shippable.** Per-key English fallback at runtime
  is what makes "translate only before a release, on my word" safe: an untranslated key renders
  English, never a raw key or a crash. Drift detection (source hash per key) is what makes the
  release pass cheap — only changed keys are re-sent.
- **The bundle cap needs a definition, not a raise.** Counting 30 lazy locale chunks against the
  30 KB cap would fail by construction; the cap should measure what an English visitor's page
  loads (`widget.js` + `widget.css` + `redact.js`), with locale chunks under their own per-chunk
  cap. `oracle.py:200` says never silently raise the cap — this is scoping the measurement, and
  it is recorded as a decision so an agent does not just glob-exclude.

## Domains Identified

1. Locale model + resolution (shared vocabulary, resolver, persistence rules) — FR-FBR-34
2. Widget — FR-FBR-35
3. Public board / roadmap / tenant-host landing — FR-FBR-36
4. End-user + account emails — FR-FBR-37
5. Admin UI + tenant language setting — FR-FBR-38
6. Translation workflow tooling + owner-gated release step — FR-FBR-39
7. Outbound team-authored text translated into the submitter's language — FR-FBR-40 (proposed)
8. Marketing site — FR-FBR-41 (proposed)

## Scope Boundaries

- **In**: everything a user reads that feedbackmonk authors (chrome, labels, validation, toasts,
  emails, status words); `lang`/`dir` correctness; locale-aware `Intl` formatting; catalog tooling.
- **Out** (unchanged invariants): the verbatim `body` on every public surface (Q24 +
  `translation-egress-q24-isolation` Probe B) — board visitors read feedback in the language it was
  written in; the English canonical target (D-XLATE-5); the no-tracker / no-egress-by-default
  posture (DEC-FBR-02 / DEC-FBR-IMPL-26) — UI catalogs are static files, no runtime egress.
- **Out (future)**: per-viewer translation of board content; a per-admin target language;
  right-to-left layout audit beyond `dir` + logical properties on the widget and public pages.

## Open Questions — raised to the owner (spec domain at ask-major)

See `docs/specs/OPEN_QUESTIONS.md` Q23, Q25–Q29. Q24 is skipped as an identifier because "the Q24
invariant" is a reserved name in this repo (FR-FBR-12 / DEC-FBR-IMPL-25).

## Decisions (ratified in DECISIONS.md)

- DEC-FBR-15 — adopt GitCellar's 31-locale set and short-code vocabulary verbatim; RTL from day one.
- DEC-FBR-16 (PROPOSED, pending Q23) — initial language from the user's browser/OS preference; explicit choice wins and persists; no geo-IP.
- DEC-FBR-17 — machine translation of UI catalogs is an owner-authorised release step; runtime per-key English fallback keeps partial catalogs shippable.
- DEC-FBR-IMPL-30 — one catalog source, three runtimes; widget = lazy per-locale chunks, no i18n library; admin/public = i18next; Rust = compiled-in.
- DEC-FBR-IMPL-31 — locale persistence per surface; UI locale and FR-FBR-30 target language are separate axes.
