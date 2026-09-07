# Translation — the `/1-translate` release step

feedbackmonk ships 31 locales (Contract C34/C35 — `i18n/README.md`). English
is authored by developers, in the same commit as the component that uses a
string; every other locale is filled by `/1-translate`, run only on the
owner's word (DEC-FBR-17). This document is the release-checklist version of
that skill; read `.claude/skills/1-translate/SKILL.md` for the full phase-by-
phase procedure.

## Why translation is a deliberate, separate step

Machine translation is fast, cheap, and occasionally wrong in ways that
matter — GitCellar shipped, then deleted, 120 machine-translated legal
documents because a policy update went out in 30 languages before anyone
reviewed the new English (see `i18n/README.md` rule 9). Coupling English
authorship to automatic translation removes the one gate that catches this:
a human reading the translated diff before it ships. So:

- Developers write and change strings **only** in `i18n/locales/en/<ns>.json`.
- `/1-translate` propagates to the other 30, on demand, reviewably.
- A partially translated catalog is always shippable (missing keys render
  the English value at runtime — Contract C35 rule 6) — there is never
  pressure to translate everything before merging a feature.

## Provider account

DeepL is the only translation provider (v1). Account + key are the owner's;
this file records **where**, not the credential itself:

- **Provider**: DeepL API (`https://api-free.deepl.com/v2` for a `:fx`-suffixed
  free-tier key, `https://api.deepl.com/v2` otherwise). `DEEPL_API_KEY` env
  var, or `--key` on any of the four scripts.
- **Free tier**: 500,000 characters/month. `translate.py --dry-run` and
  `complete-plurals.py --dry-run` print current usage and refuse a run that
  cannot finish within what remains.
- **DPA note**: catalog machine-translation sends **UI copy only** — never
  submitter-authored feedback content (that is `crates/feedbackmonk-i18n`'s
  separate FR-FBR-30 pipeline, gated by its own `translation-egress-q24-isolation`
  oracle and off by default). Still, sending product copy to a third-party
  API is a data flow worth disclosing to a self-host operator who asks —
  it is developer tooling, not a runtime dependency: a self-hosted instance
  that never runs `/1-translate` sends nothing to DeepL, ever.

## The five English-fallback-by-design locales

`ga`, `fa`, `ml`, `is`, `si` have no DeepL target (`deepl: null` in
`i18n/locales.json`, Q28). They render English at every key, always — this
is the intended behavior, not a gap to close. `check-gaps.py` and
`translation-gap-status` report them in their own bucket, excluded from the
MISSING/DRIFTED totals and the DUE/NOT DUE verdict.

## Adding a language

See `i18n/README.md` § "Adding a language" — add a row to
`i18n/locales.json`, regenerate the three runtime tables, run
`init-catalogs.py`, commit. Nothing in this file changes: `/1-translate`
picks up the new locale automatically from the table on its next run.

## Release checklist

1. `python scripts/i18n/translate.py --dry-run` — confirm quota covers the
   pass (or narrow scope).
2. `python scripts/i18n/check-gaps.py` — confirm what is MISSING/DRIFTED.
3. `/1-translate` (or its four phases run by hand — see the skill).
4. `python scripts/i18n/validate.py` — zero errors required; review warnings.
5. Review the diff. Machine translation is a first draft, not a final
   answer — a native-language reviewer catching an awkward phrase before
   release is cheaper than after.
6. Commit.

## Oracles

| Oracle | Kind | What it checks |
|---|---|---|
| `i18n-catalog-integrity` | verification | catalog structural soundness (parses, keys ⊆ en, placeholders match, CLDR plural categories present, no mojibake/leaked entities) — commit-time gate |
| `i18n-literal-ratchet` | verification | no new hard-coded English literal in `widget/src`/`admin-ui/src` beyond the frozen baseline — commit-time gate |
| `translation-gap-status` | project-state | MISSING/DRIFTED counts + character estimate — advisory, session-start/finalize briefing only |
