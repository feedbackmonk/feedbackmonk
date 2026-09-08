---
name: 1-translate
description: "Owner-only: run the pre-release machine-translation pass over i18n/locales (DEC-FBR-17)."
disable-model-invocation: true
metadata:
  uldf_subsystem: project
---

# /1-translate — feedbackmonk translation propagation

**Only the owner invokes this.** Finalize, CI, hooks and worker sessions call
`scripts/i18n/check-gaps.py` at most — never `translate.py` or
`complete-plurals.py` for real. This is DEC-FBR-17: translation happens
deliberately, at release time, on the owner's word. `translate.py` and
`complete-plurals.py` enforce this themselves (they refuse to run without an
interactive TTY unless `--i-am-the-owner` is passed) — this skill does not
add a second gate, it sequences the tools correctly.

## Prerequisites

- **DeepL API key** (`DEEPL_API_KEY` env var; a `:fx`-suffixed key selects
  the free-tier endpoint automatically). Account + key location:
  `docs/operations/TRANSLATION.md` § Provider account.
- All four tools are Python 3.8+ stdlib, run from the repo root:
  `python scripts/i18n/{check-gaps,translate,validate,complete-plurals}.py`
  (or the `.sh`/`.ps1` shims beside them).

## Phase 1 — Quota preflight (do this first)

```
python scripts/i18n/translate.py --dry-run
```

Prints the plan (per DeepL-target group × namespace, key counts, character
totals) and — if `DEEPL_API_KEY` is set — the live quota (`used`/`limit`/
`remaining`). **A run that cannot finish must not be started**: both
`translate.py` and `complete-plurals.py` refuse on their own if the total
characters needed exceed the remaining quota (exit 2). Narrow scope with
`--locale`/`--namespace` if quota is tight, or wait for the monthly reset.

## Phase 2 — Gap detection

```
python scripts/i18n/check-gaps.py
```

Reports MISSING (never translated) and DRIFTED (English changed since) keys
per locale/namespace, plus the character estimate. Locales with a null
DeepL target (`ga`, `fa`, `ml`, `is`, `si` — English-fallback by design) are
reported in their own bucket, never as gaps to close. `--json` for
machine-readable output; `--strict` to exit 1 if anything is due (useful in
a pre-release checklist script, never in CI).

If nothing is MISSING or DRIFTED: report "All translations up to date" and
stop — Phases 3-4 have nothing to do.

## Phase 3 — Translation

```
python scripts/i18n/translate.py --i-am-the-owner
python scripts/i18n/complete-plurals.py --i-am-the-owner
```

`translate.py` sends every MISSING+DRIFTED key to DeepL (batches of ≤50,
`tag_handling=xml` with `{{placeholders}}`/URLs/brand names/HTML tags
wrapped in `<x>…</x>` so DeepL passes them through unchanged), writes the
result, sets `_meta.status` to `machine-translated — community review
welcome`, and refreshes `i18n/source-hashes.json` for exactly the keys it
wrote. `zh-HK`/`zh-TW` share DeepL's `ZH-HANT` target and are translated
together (one call, not two) since both map to the same target in
`i18n/locales.json`.

`complete-plurals.py` then fills the CLDR plural categories a locale needs
but English does not have (`_few`/`_many` for ru/uk/pl/cs/sk, `_zero` for
lv) by instantiating the English `_other` value with a selecting numeral,
translating, and swapping the numeral back for `{{count}}`. A form is
refused (never written) if the numeral does not survive translation — a
missing category falls back to English, which is safer than a plural form
with no count placeholder.

Useful flags on both: `--locale`, `--namespace`, `--formality
prefer_less|prefer_more` (DeepL's default returns formal register in
languages that distinguish it; consider this for informal product copy).

**Every write is round-trip-verified before and after** — a catalog file
that does not byte-round-trip through the JSON parser is skipped, never
force-rewritten, and reported. Neither tool ever touches `i18n/locales/en/`.

## Phase 4 — Validation + baseline refresh

```
python scripts/i18n/validate.py
python scripts/i18n/check-gaps.py --update-baseline
```

`validate.py` checks the nine defect classes (structural errors: extra
keys, placeholder mismatches, empty values, mojibake, leaked HTML entities,
plural-category gaps; advisory warnings: untranslated-authored, stripped
diacritics). Fix any **error** before committing; a **warning** is a
signal to look, not a blocker.

`check-gaps.py --update-baseline` is **mandatory**, not optional — it
re-snapshots `i18n/source-hashes.json` from the current `en/` values,
recording that the translations just written were produced from the
*current* English. Skipping this makes every key just translated report as
DRIFTED again on the next check.

Then: review the diff (translation output deserves a human read, even
machine-translated), and commit.

## Rules (restated from `i18n/README.md` Contract C35)

- Never edit `i18n/locales/en/**` here — only propagates, never creates.
- Never machine-translate legal/policy text before its English is frozen.
- `zh-HK`/`zh-TW` share one DeepL call; do not run them as two separate
  passes expecting independent results.
- If a script's quota/TTY/round-trip refusal fires, that is the tool
  working as designed — do not bypass it by hand-editing catalog files.

## Reference

- `i18n/README.md` — Contract C35 (the full catalog rules)
- `docs/operations/TRANSLATION.md` — the release step, provider account,
  five-language English fallback, how to add a language
- `.claude/oracles/translation-gap-status/` — the same MISSING/DRIFTED
  detection as a project-state oracle, for the session-start briefing
- `.claude/project-oracles/i18n-catalog-integrity/` — the commit-time structural
  gate this pass's output must pass
