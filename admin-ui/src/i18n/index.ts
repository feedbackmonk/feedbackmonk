// The SPA's i18next bootstrap (FR-FBR-36/38, Contract C35).
//
// WHAT THIS OWNS
//   - one i18next instance, initialised SYNCHRONOUSLY with the English
//     catalogs statically imported, so `t()` never renders a raw key or a
//     Suspense hole on first paint;
//   - lazy, code-split loading of the other 30 locales' catalogs;
//   - the re-export of `useTranslation`, so that importing a page ALWAYS
//     initialises i18n (a page-level unit test that renders a component in
//     isolation gets a working `t()` for free).
//
// WHAT IT DELIBERATELY DOES NOT DO
//   - No `i18next-browser-languagedetector`: the C34 resolver in `resolve.ts`
//     is the detection policy, and it is shared byte-for-byte with the widget
//     and the Rust backend. A second detector would be a second policy.
//   - No content translation. Feedback bodies, roadmap titles and replies are
//     rendered verbatim on every public surface (Q24 / DEC-FBR-15); this
//     module localises CHROME only.
//
// CATALOG PATHS: the catalogs live at the repo root (`i18n/locales/<code>/…`),
// outside `admin-ui/`, because three runtimes share them. The relative
// `../../../i18n/locales/…` shape is load-bearing for the dynamic import:
// Vite's `dynamic-import-vars` only rewrites specifiers that begin with `./`
// or `../` (an alias or bare specifier silently fails to code-split), and
// `server.fs.allow` in `vite.config.ts` is what lets the dev server read them.

import i18next, { type Resource } from "i18next";
import { initReactI18next, useTranslation } from "react-i18next";

import enPublic from "../../../i18n/locales/en/public.json";
import enAdmin from "../../../i18n/locales/en/admin.json";
import enStatus from "../../../i18n/locales/en/status.json";

export const NAMESPACES = ["public", "admin", "status"] as const;
export type Namespace = (typeof NAMESPACES)[number];

export const DEFAULT_NS: Namespace = "public";

type CatalogFile = Record<string, unknown>;

/**
 * `_meta` is catalog bookkeeping (C35 rule 2 — language endonym + translation
 * status), not a translatable key. Strip it so it can never be reached by a
 * `t('_meta.…')` typo or counted as a key by anything downstream.
 */
function strip(catalog: CatalogFile): CatalogFile {
  const { _meta, ...rest } = catalog as { _meta?: unknown };
  void _meta;
  return rest as CatalogFile;
}

const EN_RESOURCES: Resource = {
  en: {
    public: strip(enPublic),
    admin: strip(enAdmin),
    status: strip(enStatus),
  },
};

// One loader per namespace rather than one loader with two variables: it keeps
// Vite's generated glob to `i18n/locales/*/public.json` (etc.), so the widget
// and email catalogs — which this runtime never reads — stay out of the SPA
// bundle graph entirely.
const LOADERS: Record<Namespace, (code: string) => Promise<CatalogFile>> = {
  public: (code) =>
    import(`../../../i18n/locales/${code}/public.json`).then((m) => m.default),
  admin: (code) =>
    import(`../../../i18n/locales/${code}/admin.json`).then((m) => m.default),
  status: (code) =>
    import(`../../../i18n/locales/${code}/status.json`).then((m) => m.default),
};

if (!i18next.isInitialized) {
  void i18next.use(initReactI18next).init({
    lng: "en", // replaced by bootstrapLocale() once the DOM is available
    fallbackLng: "en",
    ns: [...NAMESPACES],
    defaultNS: DEFAULT_NS,
    resources: EN_RESOURCES,
    // React escapes every interpolated value already; escaping here would
    // double-encode apostrophes in French and German copy.
    interpolation: { escapeValue: false },
    // Per-key English fallback (C35 rule 6): a key missing from — or empty in
    // — a partially translated catalog renders the English value, never the
    // raw key.
    returnEmptyString: false,
    react: { useSuspense: false },
    initImmediate: false,
  });
}

const loaded = new Set<string>(["en"]);

/**
 * Load (once) every namespace catalog for a shipped locale code.
 *
 * The caller must pass a code that came out of `resolveLocale`/`validateLocale`
 * — this function is on the `import()` path, and validating here as well would
 * only hide a missing gate upstream. A catalog that fails to load is not fatal:
 * i18next keeps rendering English for the keys it does not have (C35 rule 6).
 */
export async function loadCatalogs(code: string): Promise<void> {
  if (loaded.has(code)) return;
  loaded.add(code);
  await Promise.all(
    NAMESPACES.map(async (ns) => {
      try {
        const catalog = await LOADERS[ns](code);
        i18next.addResourceBundle(code, ns, strip(catalog), true, true);
      } catch {
        // A missing or malformed catalog degrades to English for that
        // namespace. `i18n-catalog-integrity` is what makes this loud at
        // commit time; at runtime, silence beats a broken page.
      }
    }),
  );
}

export { i18next as i18n, useTranslation };
