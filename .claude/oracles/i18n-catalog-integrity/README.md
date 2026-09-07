# i18n-catalog-integrity — Verification Oracle


## Synopsis

Verifies the shared catalog tree is structurally sound and the three generated locale tables still equal `i18n/locales.json`: every `i18n/locales/<code>/<ns>.json` parses and carries `_meta`, keys are a subset of `en`, `{{placeholder}}` sets match per key, CLDR plural categories are present, and no mojibake or leaked entities. Probes C/D/E delegate to `scripts/i18n/validate.py` so the C35 contract has exactly one implementation.
## Assertion

> Every runtime (widget, SPA, Rust) will read a consistent catalog: no key can
> render as a raw key, a broken placeholder, or a wrong-locale string because
> of a malformed or drifted catalog file, and all three runtimes agree on
> which 31 locales exist.

Frozen in `manifest.json` at Task Zero (2026-09-06), before any probe beyond
A/B existed. See `manifest.json` for the full `measures` / `known_gaps` text.

## Probes

| Probe | What it proves | Source |
|---|---|---|
| **A** — generated-table drift | `admin-ui/src/i18n/locales.gen.ts`, `widget/src/locales.gen.ts`, `crates/feedbackmonk-i18n/src/locales.gen.rs` are byte-identical to a fresh render of `i18n/locales.json` | `scripts/i18n/gen-locales.py --check` |
| **B** — parse + `_meta` shape | every `i18n/locales/*/*.json` parses and starts with a well-formed `_meta` (`language` + a known `status`) | this oracle, directly |
| **C** — keys ⊆ en + placeholders | every non-en key is a subset of `en`'s, and the `{{name}}` token set matches per key | delegates to `scripts/i18n/validate.py`'s `extra_key` + `placeholder_mismatch` classes |
| **D** — CLDR plural categories | every translated plural base carries the categories its locale's CLDR rule set requires | delegates to `validate.py`'s `plural_category_gap` class |
| **E** — mojibake / entities | no `Ã`/`â€`/`Â` byte-corruption sequences, no leaked `&amp;`/`&#NN;` | delegates to `validate.py`'s `mojibake` + `leaked_entity` classes |

C/D/E deliberately do not re-derive the catalog-shape contract — they call
`scripts/i18n/validate.py`'s own `run()` and filter by defect class, so there
is exactly one implementation of "what does a well-formed catalog look like"
consulted by both the on-demand tool and the commit-time gate.

## Why C/D/E are not the same as `validate.py`'s own exit code

`validate.py` also reports `missing_key` (info), `untranslated_authored`
(warning) and `stripped_diacritics` (warning) — deliberately advisory classes
that do not gate a commit (an unfinished translation pass is always
shippable, per DEC-FBR-17 / C35 rule 6). The oracle only elevates the
**structural** classes (extra keys, placeholder drift, plural-category gaps,
encoding corruption) to a FAIL, because those are the ones a raw or broken
string in front of an end user traces back to.

## Adversarial self-test (performed 2026-09-06, before bumping to 1.0.0)

`oracle.py --self-test` (either shim, same flag) mechanizes exactly this
demonstration, so it re-runs on demand rather than being a one-time manual
record: it builds a throwaway scratch mirror of `i18n/` + `scripts/i18n/`
(Probe C leg) and a second scratch mirror of the three generated-table
outputs (Probe A leg) — never the live working tree, so no other worker's
in-flight catalog edits are ever at risk — and asserts both sabotages are
caught:

| Sabotage | Result |
|---|---|
| baseline (unmodified scratch mirror) | Probe C **PASS**; Probe A **PASS** |
| drop `{{name}}` from a translated `de/widget.json` value whose `en` source has it | Probe C **FAIL**, `placeholder_mismatch` |
| append a trailing comment to a scratch copy of `admin-ui/src/i18n/locales.gen.ts` | Probe A **FAIL**, names the file, prints the regen command |
| both reverted (scratch discarded) | back to PASS |

Probe D and E are exercised the same way via `scripts/i18n/tests/` (see
`test_validate.py::test_plural_category_gap_for_russian` and
`::test_mojibake_is_error`, plus the static fixtures under
`scripts/i18n/tests/fixtures/{plural-missing,mojibake}/`) rather than a
third scratch-mirror leg in `--self-test` — they are the same `validate.py`
delegation path as C, so one adversarial demonstration of the delegation
mechanism (C) plus unit coverage of each class's own detection logic is the
actual coverage, not three redundant reproductions of one mechanism.

## Known gaps

See `manifest.json` `assertion.known_gaps` — translation *quality* is
out of scope (that is `scripts/i18n/validate.py`'s advisory classes and
`translation-gap-status`'s MISSING count); the oracle checks the working
tree, not HEAD; placeholder equality is set-equality of `{{name}}` tokens
only; Probe D checks presence of plural categories, not their grammatical
correctness.

## Graceful absence

Vacuous-PASS when `i18n/locales/` does not exist (pre-Stage-0).
