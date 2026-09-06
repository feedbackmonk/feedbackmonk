# feedbackmonk-i18n

## 1. Purpose & Responsibilities

The server-side half of UI localization (FR-FBR-34 / FR-FBR-37): the canonical locale vocabulary
(Contract C34), the locale resolver, and the compiled-in catalog reader that renders emails and
status words in the recipient's language (Contract C40). Depends on nothing; every other crate may
depend on it.

**Stage 0 (this commit) ships only the generated locale table + types.** The resolver, `t()` /
`t_args()`, `parse_accept_language()` and the `include_str!` catalogs are Stage 1 (worker W-C) —
see `docs/planning/plans/20260906T213706-ui-localization-31-locales-fr-fbr-34-40.md` § W-C and
§ Interface Contracts C40.

## 2. File Index

| File | Purpose |
|---|---|
| `src/lib.rs` | `Dir`, `LocaleEntry`, `locale_by_code`; re-exports the generated table |
| `src/locales.gen.rs` | **GENERATED** from `i18n/locales.json` by `scripts/i18n/gen-locales.py` — never edit |

## 3. Public API & Usage

```rust
use feedbackmonk_i18n::{LOCALES, DEFAULT_LOCALE, Dir, locale_by_code};
let fa = locale_by_code("fa").unwrap();
assert_eq!(fa.dir, Dir::Rtl);
```

Planned (C40, Stage 1): `Locale` newtype with `parse`/`EN`/`dir`/`base`; `resolve(&[&str]) -> Locale`;
`t(locale, key)`; `t_args(locale, key, &[(&str, &str)])`; `parse_accept_language(&HeaderMap)`.

## 4. Constraints & Business Rules

- The table is **additive only** (DEC-FBR-15): a new language is a new row in `i18n/locales.json`
  followed by `python scripts/i18n/gen-locales.py`; codes never rename.
- Catalogs are compiled in (`include_str!`) so a self-host image is self-contained: no runtime file
  I/O, no egress (DEC-FBR-IMPL-30).
- A language not in the table is never mapped to a neighbour; the resolver returns the default.

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
