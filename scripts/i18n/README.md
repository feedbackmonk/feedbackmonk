# `scripts/i18n/` — catalog tooling (Contract C35 enforcement)

## Synopsis

Deterministic Python tooling for catalog management: generates runtime locale tables, detects translation gaps and drift, runs machine-translation passes, completes CLDR plural categories, and validates catalog structure. Everything is read-only against English or write-only against the other 30 locales (English is authored by developers only).

## 1. Purpose & Responsibilities

Deterministic, stdlib-only Python tooling that operates on the `i18n/`
catalog tree: generates the three runtime locale tables, seeds skeleton
catalog files, detects translation gaps/drift, runs the DeepL machine-
translation pass, completes CLDR plural categories, and validates catalog
structure against nine defect classes. Everything here is either read-only
against `i18n/locales/en/**` or write-only against the other 30 locale
directories — `en/` is authored by developers directly (Contract C35 rule 1).

## 2. File Index

| File | Purpose |
|---|---|
| `gen-locales.py` (+ `.sh`/`.ps1`) | Regenerates the three runtime locale tables from `i18n/locales.json`. **LEAD-owned/frozen** — do not edit. |
| `init-catalogs.py` | Creates missing skeleton catalog files for every locale × namespace. **LEAD-owned/frozen** — do not edit. |
| `_catalog.py` | Shared library: locale table access, CLDR plural-category table, JSON flatten/unflatten, SHA-256 hashing, placeholder extraction, byte-round-trip-verified catalog I/O. Every other script here imports it. |
| `check-gaps.py` (+ shims) | MISSING/DRIFTED report per locale × namespace; `--json`, `--strict`, `--update-baseline`. |
| `translate.py` (+ shims) | DeepL machine-translation pass. Owner-only (DEC-FBR-17): refuses without an interactive TTY unless `--i-am-the-owner`; quota-preflights and refuses a run it cannot finish; round-trip-verifies every write. |
| `validate.py` (+ shims) | Nine-class structural + quality validation (see its own docstring). Exit 1 on any error; warnings are advisory. |
| `complete-plurals.py` (+ shims) | Fills CLDR plural categories (`_few`/`_many`/`_zero`) English never authors, by numeral-instantiating and re-translating `_other`. Shares `translate.py`'s owner-only/dry-run/quota rules. |
| `tests/` | `python -m unittest discover scripts/i18n/tests` — fixture catalogs + a mocked in-process DeepL server (`tests/mock_deepl.py`); no live API key ever needed. |

## 3. Public API & Usage

All four Stage-1 tools are CLI entry points; run with `python
scripts/i18n/<name>.py --help` or the `.sh`/`.ps1` shims beside them
(delegate to the same Python file, Windows/Unix-portable). `_catalog.py` is
the one importable module — see its docstrings for `flatten`/`unflatten`,
`plural_categories(code)`, `sha256_text`, `placeholders`,
`load_catalog_verified`/`write_catalog_verified`.

Release-time sequencing of all four tools: `.claude/skills/1-translate/SKILL.md`.

## 4. Constraints & Business Rules

- **Never edit `i18n/locales/en/**`, `i18n/locales.json`,
  `i18n/resolution-fixtures.json`, `gen-locales.py`, `init-catalogs.py`**
  from any script here — those are developer-authored or LEAD-frozen
  surfaces (Contract C35 rule 1; see `i18n/README.md`).
- **`translate.py`/`complete-plurals.py` must never run unattended.** No
  interactive TTY and no `--i-am-the-owner` ⇒ refuse (exit 3). Neither
  script has a "just do it anyway" override beyond that flag.
- **Every catalog write is round-trip-verified**, both the file being
  overwritten (before) and the new content (after) — a file that does not
  byte-round-trip through `json.dumps(..., indent=2, ensure_ascii=False) +
  "\n"` is skipped, never force-rewritten.
- **Stdlib only** (Python 3.8+): `json`, `hashlib`, `re`, `urllib`,
  `http.server`, `unittest`, `argparse`. No `requests`, no `pytest`, no
  third-party dependency of any kind.
- **The CLDR plural-category table in `_catalog.py` matches Contract C35
  exactly**, not full CLDR — Irish/Icelandic/Sinhala's genuinely richer
  plural systems are deliberately simplified to `{one, other}` here because
  C35 doesn't ask for more. Widening that is a C35 amendment, not a change
  to make in this module.

## 5. Relationships & Dependencies

- Consumes `i18n/locales.json` (the frozen 31-locale table) and
  `i18n/locales/<code>/<ns>.json` (the catalog tree).
  Produces/maintains `i18n/source-hashes.json` (drift baseline),
  `i18n/literal-baseline.json` (format defined by
  `.claude/oracles/i18n-literal-ratchet/`), `i18n/validate-baseline.json`
  (stripped-diacritics ratchet, `validate.py`'s own).
- Consumed by three oracles: `.claude/oracles/i18n-catalog-integrity/`
  (delegates its Probes C/D/E to `validate.py`'s own defect classes — one
  implementation of "what is a well-formed catalog", not two),
  `.claude/oracles/translation-gap-status/` (wraps `check-gaps.py --json`),
  `.claude/oracles/i18n-literal-ratchet/` (its own scanner, over
  `widget/src`/`admin-ui/src`, not this directory's catalog tree).
- Consumed by `.claude/skills/1-translate/SKILL.md`, the owner-invoked
  release sequencing of all four tools.

## 6. Decision Log

- Per-namespace catalog files (`i18n/locales/<code>/<ns>.json`) exist so no
  two localization workers ever write the same file — a Stage-1 parallel-
  work decision, not a tooling one, but it shapes every path in this
  directory (`_catalog.catalog_path(code, ns)`).
- `check-gaps.py`'s DRIFTED definition treats an absent baseline hash as
  "never equal to a real hash" rather than "trust it" — a key present in
  both `en` and a locale file but never recorded in
  `i18n/source-hashes.json` reports drifted, not clean, so a hand-added
  translation outside the pipeline is never silently assumed current.
- `translate.py` groups shipped locales by their DeepL **target** rather
  than translating per locale code: `zh-HK` and `zh-TW` both map to
  `ZH-HANT` in `i18n/locales.json`, so they are translated once (the union
  of keys either needs) and each locale file receives only the keys it
  individually needed — no wasted characters, no locale silently left with
  a stale sibling value.
- `complete-plurals.py` instantiates a numeral into English's `_other`
  value rather than asking DeepL for a grammatical category directly (DeepL
  has no such request shape) — and refuses to write a form whose numeral
  did not survive translation, because a plural form with no `{{count}}`
  placeholder is worse than the English fallback it would replace.
