# `i18n/` — the catalog tree (Contract C35)

One source for every human-facing string feedbackmonk authors, consumed by three runtimes
(widget, admin/public SPA, Rust). Requirements FR-FBR-34..40; decisions DEC-FBR-15/16/17,
DEC-FBR-IMPL-30/31. This README **is** Contract C35.

## Synopsis

The single source of truth for user-facing strings across all three runtimes. 31 locales (en + 30 translations), five namespaces per locale (widget, public, admin, email, status). Developers author English only; every other locale is machine-translated on the owner's command. Per-key fallback to English at runtime ensures no raw keys ever reach a user interface.

## Layout

```
i18n/
  locales.json                  the 31-locale table (C34) -- the ONLY hand-edited locale list
  resolution-fixtures.json      resolver truth table, run by both the TS and the Rust resolver
  plural-fixtures.json          plural truth table + divergence ratchet, run by all THREE runtimes
  source-hashes.json            per-key SHA-256 of the English value at last translation (drift baseline)
  literal-baseline.json         the i18n-literal-ratchet oracle baseline (produced by scripts/i18n/check-gaps.py)
  locales/<code>/<ns>.json      the catalogs: 31 codes x 5 namespaces
  README.md                     this contract
```

Namespaces and their consumers:

| `<ns>` | Consumer | Owner (Stage 1) |
|---|---|---|
| `widget` | `widget/` (`t()` in `widget/src/i18n.ts`; sliced per locale into `dist/locales/<code>.js` by `widget/scripts/slice-locales.mjs`, which also projects `locales.json` into the build-generated `widget/src/locale-chunks.ts` — the widget imports that, not `locales.gen.ts`, to stay under the 30 KB cap) | W-A (done) |
| `public` | public board / roadmap / tenant-host landing (i18next) | W-B |
| `admin` | admin console (i18next) | W-B (bootstrap), W-D (extraction) |
| `email` | `crates/feedbackmonk-i18n` (`include_str!`) — confirmation / status / reply / account emails; **includes `email.status.value.<wire>`, the email register of the status words (sentence case), deliberately separate from the UI `status.*` chip labels (title case) — MSG-004 ruling, not drift** | W-C (done) |
| `status` | shared enum labels (`status.*`, `kind.*`, `sentiment.*`, …) — SPA **and** Rust | W-B keys, W-C reads |

Per-namespace files exist so that no two workers ever write the same file.

## Rules (the contract)

1. **`en/` is the source of truth.** Developers add and change strings **only** in
   `locales/en/<ns>.json`, in the same commit as the component that uses them. The other 30
   directories are written by `scripts/i18n/translate.py` and by nobody else (DEC-FBR-17).
2. **Every file starts with `_meta`**: `{"_meta": {"language": "<endonym>", "status": "<status>"}}`.
   `status` ∈ `source` (English) · `untranslated` · `machine-translated — community review welcome` ·
   `english-fallback — provider unsupported` (`ga, fa, ml, is, si`).
3. **Keys are dotted, namespaced, camelCase leaves**: `widget.form.subject`, `email.status.subject`,
   `status.wontfix`. Nested JSON objects are allowed; the runtimes flatten to dotted keys. The
   first segment equals the file's namespace — **except `status.json`, the shared-enum namespace,
   whose families are `status.*`, `kind.*`, `sentiment.*` and `roadmapStatus.*`** (one value key
   per enum wire value; the SPA and Rust read the same four families). *Amended 2026-09-06 by the
   LD on MSG-001 of collab-20260906-215851.*
4. **Interpolation is `{{name}}`** (i18next syntax; the widget `t()` and the Rust `t_args` implement
   the same subset: named placeholders only, no formatting expressions).
5. **Plurals use suffixes**: `key_one` / `key_other`, plus `key_few` / `key_many` (ru, uk, pl, cs, sk)
   and `key_zero` (lv) where CLDR requires them. `scripts/i18n/complete-plurals.py` fills the extra
   categories; the `i18n-catalog-integrity` oracle checks they are present. Missing categories fall
   back to English — never to `_other` — so they must exist.
6. **Per-key English fallback at runtime.** A key missing from `<code>/<ns>.json` renders the `en`
   value. A partially translated catalog is always shippable; a raw key on screen is a bug.
7. **Drift is tracked per key.** `source-hashes.json[ns][key]` holds the SHA-256 of the English
   value at the last translation. `check-gaps.py` reports keys whose English changed since
   (DRIFTED) alongside keys never translated (MISSING). `translate.py` refreshes the hash for
   every key it writes.
8. **Do not translate**: brand names (feedbackmonk, GitCellar, DeepL), URLs, code identifiers,
   `{{placeholders}}`, HTML tags. `aria-label` and `alt` text **are** translatable.
9. **Legal / policy text is never machine-translated before its English is frozen** (GitCellar shipped,
   then deleted, 120 machine-translated legal documents for exactly this reason).
10. **Translation runs only on the owner's word** (`/1-translate`). Finalize, CI, hooks and workers
    call `check-gaps.py` at most. `translate.py` refuses to run without an interactive TTY or
    `--i-am-the-owner`.

## Tooling (`scripts/i18n/`)

| Script | Purpose | Stage |
|---|---|---|
| `gen-locales.py [--check]` | regenerate the three runtime locale tables from `locales.json` | 0 (done) |
| `init-catalogs.py` | create missing skeleton files for every locale × namespace (idempotent) | 0 (done) |
| `check-gaps.py` | MISSING + DRIFTED report, `--json`, `--strict` | 1 (done) |
| `translate.py` | DeepL pass with quota preflight and round-trip safety; owner-only | 1 (done) |
| `validate.py` | structural + quality validation (nine classes) | 1 (done) |
| `complete-plurals.py` | CLDR plural-category completion | 1 (done) |

Oracles: `i18n-catalog-integrity` (verification), `translation-gap-status` (project-state),
`i18n-literal-ratchet` (verification) — `.claude/oracles/`.

## Adding a language

Add a row to `locales.json` (code, endonym, dir, DeepL target or `null`), run
`python scripts/i18n/gen-locales.py` and `python scripts/i18n/init-catalogs.py`, commit all of it.
Nothing else changes: every runtime reads the generated table.
