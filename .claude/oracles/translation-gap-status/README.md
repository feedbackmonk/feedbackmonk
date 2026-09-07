# translation-gap-status — Oracle (project-state, advisory)


## Synopsis

Answers the owner's release-gate question — *is a translation pass due?* — by wrapping `scripts/i18n/check-gaps.py --json` into one line: MISSING keys (never translated) and DRIFTED keys (English changed since, per the per-key SHA-256 sidecar) per locale and namespace, plus an approximate character count. Advisory and always exit 0; finalize reports it, never blocks on it.
## Assertion

> The owner can tell, before cutting a release, whether `/1-translate` needs
> to run and roughly how much it will cost.

Frozen in `manifest.json` at Task Zero (2026-09-06).

## What it does

A thin wrapper over `scripts/i18n/check-gaps.py --json` (the same detector
`/1-translate` Phase 2 and `validate.py`'s advisory `missing_key` class
consult), printed in the manifest's fixed schema:

```
translation-gap-status: <L> locales | <M> missing | <D> drifted | ~<C> chars | translation pass: DUE|NOT DUE
```

followed by a per-locale table. **Advisory only — exit 0 always** (2 only on
environment failure, e.g. `i18n/locales.json` fails to parse). This is
never a commit gate: DEC-FBR-17 makes translation a deliberate, owner-invoked
release step, and a partially-translated catalog is always shippable
(Contract C35 rule 6). Locales with a null DeepL target (`ga`, `fa`, `ml`,
`is`, `si` — English-fallback-by-design, Q28) are reported in their own
bucket and excluded from the top-line `missing`/`drifted` counts and from
the DUE/NOT DUE verdict: they will never be machine-translated, so a gap
there is never "due".

## Why MISSING and DRIFTED are counted together

A key can fail to need translation in two different ways — never translated
(MISSING), or translated once but the English source changed since
(DRIFTED, tracked via `i18n/source-hashes.json`). Both cost the same DeepL
characters to fix and both leave a user reading English (or stale English)
where their own language belongs, so both count toward "is a pass due" and
toward the character estimate. See `scripts/i18n/check-gaps.py`'s own
docstring for the exact MISSING/DRIFTED definitions.

## Self-test

`oracle.py --self-test` exercises `check-gaps.py`'s `scan()` directly
against a throwaway two-locale temp catalog tree (monkeypatches
`scripts/i18n/_catalog.py`'s path constants; the real `i18n/` tree is never
touched), proving: a key missing from a locale is detected, and a key
present in a locale but never recorded in `source-hashes.json` is reported
DRIFTED rather than silently trusted — the same "absent baseline never
equals a real hash" rule `check-gaps.py` documents.

## Graceful absence

Vacuous (0 locales, NOT DUE) when `i18n/locales/` does not exist yet.

## Known gaps

See `manifest.json` `assertion.known_gaps`: counts keys, not translation
quality; the character total is an upper bound (placeholders and
do-not-translate spans are sent inside DeepL's `ignore_tags` and still
count here); a key deleted from `en` but still present in a translated
file is an EXTRA key (`validate.py`'s `extra_key` class), not a gap this
oracle reports.
