# feedbackmonk-i18n

## Synopsis

Server-side locale resolution and catalog rendering (Contract C34/C40). The canonical 31-locale table, the C34 resolution algorithm, and compiled-in catalogs for emails and status words. Dependency-free so all runtimes (SPA, widget, Rust, tooling) read the same source.

## 1. Purpose & Responsibilities

The server-side half of UI localization (FR-FBR-34 / FR-FBR-37): the canonical locale vocabulary
(Contract C34), the locale resolver, and the compiled-in catalog reader that renders emails and
status words in the recipient's language (Contract C40). Depends on `http` (for `HeaderMap`) and
`serde_json` (catalog parsing) and nothing else; every other crate may depend on it.

## 2. File Index

| File | Purpose |
|---|---|
| `src/lib.rs` | `Dir`, `LocaleEntry`, `locale_by_code`; re-exports the table and the three modules |
| `src/locales.gen.rs` | **GENERATED** from `i18n/locales.json` by `scripts/i18n/gen-locales.py` — never edit |
| `src/locale.rs` | `Locale` newtype + the C34 resolution algorithm (`resolve` / `resolve_opt`) |
| `src/catalogs.rs` | `include_str!`-embedded catalogs; `t` / `t_args` / `t_plural` + the CLDR plural table |
| `src/accept_language.rs` | `Accept-Language` parsing (q-ordered, 200-byte DoS cap) |
| `tests/resolution_fixtures.rs` | Runs every case in `i18n/resolution-fixtures.json` against `resolve()` |
| `tests/plural_partition.rs` | Holds `plural_category()` to `i18n/plural-fixtures.json`: reachable set, per-count digest, and the ratchet asserting no category is selected that the catalogs never ship |

## 3. Public API & Usage

```rust
use feedbackmonk_i18n::{parse_accept_language, resolve, resolve_opt, t, t_args, Locale};

// A browser preference list -> a shipped locale (English when nothing matches).
let l = resolve(&["de-AT", "en"]);              // -> de
assert_eq!(l.code(), "de");

// `resolve_opt` keeps "we ship nothing for this" distinct from "English".
assert_eq!(resolve_opt(&["da"]), None);

// A stored canonical value: exact match only, never resolution.
assert!(Locale::parse("de-AT").is_none());

let _subject = t(l, "email.confirmation.subject");
let _line = t_args(l, "email.status.intro", &[("feedbackId", "FB-ABC123")]);
```

`parse_accept_language(&HeaderMap) -> Vec<String>` feeds `resolve` / `resolve_opt`;
`t_plural(locale, key, n, args)` selects `_zero/_one/_few/_many/_other` per
[`catalogs::plural_category`]. `english_keys()` enumerates the source vocabulary.

**Which function for which input** — the split matters and is mirrored in `admin-ui/src/i18n/resolve.ts`:

| Input | Function | Why |
|---|---|---|
| `navigator.languages`, `Accept-Language` | `resolve` / `resolve_opt` | a *preference list*: coerce `de-AT` → `de`, `pt` → `pt-BR` |
| a stored value, `?lang=`, a client-sent `locale` | `Locale::parse` | attacker-reachable or already-canonical: exact match, no coercion |

## 4. Constraints & Business Rules

- The table is **additive only** (DEC-FBR-15): a new language is a new row in `i18n/locales.json`
  followed by `python scripts/i18n/gen-locales.py`; codes never rename. `catalogs.rs` lists the
  codes once in `embed_catalogs!`, and a unit test fails if that list drifts from the table.
- Catalogs are compiled in (`include_str!`) so a self-host image is self-contained: no runtime file
  I/O, no egress (DEC-FBR-IMPL-30). **The paths are relative to `src/`
  (`../../../i18n/locales/<code>/{email,status}.json`), so the crate only builds inside the repo
  tree** — true for any normal clone, including CI's.
- Only the two SERVER namespaces are embedded (`email`, `status`). `widget` / `public` / `admin` are
  consumed by the TypeScript runtimes and would be dead weight in the binary.
- A language not in the table is never mapped to a neighbour; the resolver returns the default.
- Fallback is **per key**, not per file (C35 rule 6): active → base bundle → English. A raw key
  reaching a rendered surface is a bug, and `every_shipped_locale_renders_text_for_every_english_key`
  is the test that says so.
- **Nothing here can fail.** `t` returns the key rather than panicking; `parse_accept_language`
  returns fewer candidates rather than erroring. These run on the email path and the public submit
  path, where a locale must never be able to break the operation it decorates.

## 5. Relationships & Dependencies

- Consumed by `feedbackmonk-api` (email templates, submit handler locale capture, admin locale endpoint).
- Mirrors `admin-ui/src/i18n/locales.gen.ts` and `widget/src/locales.gen.ts` — all three are
  generated from one source; the `i18n-catalog-integrity` oracle runs `gen-locales.py --check`.

## 6. Decision Log

- **Own crate rather than a module in `feedbackmonk-core`**: the catalogs are large static strings
  and the crate must stay dependency-free so the widget/SPA build tooling can reason about the same
  catalog files without pulling the Rust workspace's dependency graph.
- **Generated table, not hand-mirrored**: GitCellar needed a drift-guard script between two
  hand-kept copies of the same table; generating removes that class.
- **`resolve` and `resolve_opt` are two functions, not one with a flag.** The submit path (C37) must
  record NULL when a browser offers only languages we do not ship, because "unknown" is fillable
  later and a wrong `en` is indistinguishable from a right one. Every other caller wants the English
  backstop. A boolean parameter would have made the important case the one you forget to ask for.
- **`macro_rules!` + a drift test, not a `build.rs`.** A build script could generate the
  `include_str!` list, but the thing that makes hand-listing safe is the test that compares it to
  `LOCALES` — and with that test present the build script buys only a build script.
- **One plural table, in one function.** `plural_category` covers exactly the categories C35
  declares and nothing more; `scripts/i18n/complete-plurals.py` fills the same partition. Two plural
  implementations drift; one that both sides are held to does not. Fractional CLDR categories are
  deliberately absent — nothing in this product pluralises a fraction, and unreachable rules are
  coverage theatre.
- **Emails use `email.status.value.*`, not the shared `status.*`.** The admin UI renders status
  chips in title case (`In Progress`); these emails have used sentence case (`In progress`) since
  P1. Sharing the keys would silently re-case two shipped emails. Prose and chip labels are
  different registers (LEAD ruling, MSG-004, 2026-09-06).
