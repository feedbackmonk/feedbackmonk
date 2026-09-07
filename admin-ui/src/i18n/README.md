# `src/i18n/` — the SPA's localization runtime (FR-FBR-34/36/38)

## Synopsis

One i18next instance, the C34 locale resolver, the locale state machine (`?lang=` → `fbm_lang` → tenant default → browser → `en`), the public language switcher, and the enum-label hook. Catalogs are NOT here — they live at the repo root in `i18n/locales/<code>/<ns>.json` (Contract C35), shared with the widget and the Rust backend.

## Purpose & Responsibilities

- **Bootstrap** (`index.ts`) — initialises i18next synchronously with the English catalogs statically imported, so no surface ever paints a raw key; lazily code-splits the other 30 locales.
- **Resolve** (`resolve.ts`) — turns a browser tag (`pt-AO`, `zh-Hant-CN`) into a locale we actually ship, per Contract C34. Also the **validation gate** for attacker-reachable input.
- **Locale state** (`useLocale.ts`) — decides which language this page view opens in, persists the visitor's choice, and keeps `<html lang>` / `<html dir>` true.
- **Tenant default** (`useAdminLocaleDefault.ts`) — applies the tenant's Contract C38 locale on admin routes only.
- **Switcher** (`LanguageSwitcher.tsx`) — the control public visitors use.
- **Enum labels** (`useLabels.ts`) — `status.*` / `kind.*` / `sentiment.*` / `roadmapStatus.*` from the shared `status` namespace.

## File Index

| File | Role |
|---|---|
| `index.ts` | i18next bootstrap, namespace list, `loadCatalogs(code)`, re-export of `useTranslation` (importing it is what initialises i18n) |
| `resolve.ts` | `resolveLocale` (preference list → shipped code), `resolveOne`, `canonicalise`, `validateLocale` (exact-only gate for external input) |
| `resolve.test.ts` | Runs every case in `i18n/resolution-fixtures.json` — the truth table shared with the widget and Rust resolvers |
| `useLocale.ts` | Precedence, persistence (`fbm_lang`), `setLocale`, `bootstrapLocale`, `applyDocumentLocale`, `browserLocale`, `dirOf` |
| `useLocale.test.ts` | Precedence, invalid-input rejection, `lang`/`dir` application, storage-throws survival |
| `useAdminLocaleDefault.ts` | Admin-only C38 read applied as a default (never an override) |
| `useLabels.ts` | Localized wire-enum labels with per-key English fallback (the four `status.json` SHARED families) |
| `useLabels.test.tsx` | Catalog value, per-key fallback, unknown-value fallback |
| `useAdminLabels.ts` | Localized wire-enum labels for the ADMIN-ONLY families (`admin.enum.*` — Stage 2 / W-D, LD ruling R-1); mirrors `useLabels.ts` exactly |
| `useAdminLabels.test.tsx` | Catalog value, per-key fallback, unknown-value fallback (mirrors `useLabels.test.tsx`) |
| `LanguageSwitcher.tsx` | Native `<select>` of the 31 endonyms; sets `lang` per option |
| `locales.gen.ts` | **GENERATED** from `i18n/locales.json` by `scripts/i18n/gen-locales.py` (C41) — never hand-edit |

## Public API & Usage

```tsx
import { useTranslation } from "../../i18n";        // t() — also guarantees init
import { useLocale } from "../../i18n/useLocale";   // { locale, dir, setLocale }
import { useLabels } from "../../i18n/useLabels";   // enum → localized label
import { LanguageSwitcher } from "../../i18n/LanguageSwitcher";

const { t } = useTranslation("public");
t("public.board.title");
t("public.roadmap.voteCount", { count: 3 });        // plural, C35 rule 5
```

Namespaces: `public` (public surfaces, W-B), `admin` (admin console — fully extracted in Stage 2 by W-D; `i18n-literal-ratchet` baseline is 0 for `admin-ui/src`), `status` (shared wire-enum labels, read by the Rust backend too).

## Constraints & Business Rules

- **Every external locale value is validated before use.** `?lang=`, `localStorage['fbm_lang']` and the tenant's stored locale all pass `validateLocale` (exact shipped code only) before they reach `<html lang>`, `dir`, `changeLanguage` or a catalog `import()`. An unknown value is ignored — never echoed, never concatenated into a path.
- **The dynamic import path is a static template over the generated code list.** `import(\`../../../i18n/locales/${code}/public.json\`)` — the `../` prefix is load-bearing (Vite's `dynamic-import-vars` does not rewrite aliases), and `code` only ever comes from the shipped table.
- **CHROME is localized; CONTENT is not.** Feedback bodies, roadmap titles and replies render verbatim on every public surface (Q24 / DEC-FBR-15). No public surface reads `body_translated` — the `translation-egress-q24-isolation` oracle enforces this.
- **`en` is the only catalog developers edit** (C35 rule 1). The other 30 are written by `scripts/i18n/translate.py` at the owner's release step (DEC-FBR-17), so a missing key must always fall back to English rather than render a raw key.
- **No `i18next-browser-languagedetector`.** The C34 resolver is the detection policy, shared byte-for-byte with the widget and the Rust backend; a second detector would be a second policy.
- **Changing language never navigates.** No locale path segment, no `?lang=` rewrite, no redirect.

## Relationships & Dependencies

- **Consumes**: `i18n/locales.json` → `locales.gen.ts` (C34/C41, LD-owned); `i18n/locales/en/{public,admin,status}.json` (C35); `i18n/resolution-fixtures.json`; `shared/localeApi.ts` (C38).
- **Consumed by**: `pages/board/PublicBoard.tsx`, `pages/roadmap/PublicRoadmap.tsx`, `pages/public/TenantHostLanding.tsx`, `pages/settings/LanguageSettings.tsx`, `App.tsx`, and — since Stage 2 (W-D) — every remaining `admin-ui/src/pages/**` and `components/**` file.
- **Sibling implementations** that must not drift: `widget/src/i18n.ts` (same resolver), `crates/feedbackmonk-i18n` (same resolver + the `status` namespace).

## Decision Log

- **Why a second, stricter validator (`validateLocale`) beside `resolveLocale`.** They answer different questions. `resolveLocale` is generous on purpose — a browser offering `pt-AO` should get Portuguese. But that generosity is wrong for `?lang=` and `localStorage`, where the input is attacker-reachable and coercion widens what an attacker can steer. Splitting them makes the gate auditable in one place instead of implicit in a call site.
- **Why the catalogs live outside `admin-ui/`.** Three runtimes read the same strings; one source is the only way `status.wontfix` cannot mean two different words in an email and on a board. The cost is a relative import that reaches above the Vite root, paid once in `vite.config.ts` (`server.fs.allow`).
- **Why English is statically imported and the rest are lazy.** English is the fallback for every key in every locale (C35 rule 6), so it must be present before first paint; the other 30 are one small chunk each, fetched only if someone actually reads that language.
- **Why `useTranslation` is re-exported from `index.ts` rather than imported from `react-i18next` directly.** Importing a page in a unit test must not require remembering to initialise i18n first. Routing every consumer through this module makes initialisation a property of importing the code that needs it.
- **Why the tenant default is admin-only.** `shared/ApiClient`'s 401 interceptor redirects to `/login`; issuing the C38 read from a public board would bounce anonymous visitors out of a page they are entitled to read. Ruled and confirmed by the LD (session `collab-20260906-215851`, MSG-001).
- **Why `bootstrapLocale` does not persist what it resolved.** A language the visitor was *given* (browser preference, `?lang=` link, tenant default) is not a language they *chose*. Persisting it would make a shared link permanently re-language someone's site.
