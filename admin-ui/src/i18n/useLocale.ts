// Which language this browser is reading the page in, and how it changes.
//
// PRECEDENCE (FR-FBR-36, task W-B step 3), highest first:
//   1. `?lang=` — wins for this page view, and is NOT persisted. A link can
//      therefore show someone a page in a chosen language without silently
//      re-assigning the language of every later visit.
//   2. `localStorage['fbm_lang']` — the visitor's own choice on this origin.
//      Choosing English persists exactly like any other choice; "no stored
//      value" and "stored `en`" are different states.
//   3. The tenant's configured locale (admin surfaces only, Contract C38) —
//      applied by `useAdminLocaleDefault` and only when 1 and 2 are absent.
//   4. `navigator.languages`, resolved through the C34 table.
//   5. `en`.
//
// EVERY EXTERNAL VALUE IS VALIDATED (`validateLocale`) before it reaches
// `<html lang>`, `dir`, `changeLanguage` or a catalog `import()`. An unknown
// value is ignored, never echoed and never concatenated into a path.
//
// NO AUTO-REDIRECT, EVER. The language changes in place; the URL is the
// visitor's, not ours.

import { useCallback, useEffect, useSyncExternalStore } from "react";
import { DEFAULT_LOCALE, localeByCode, type Dir } from "./locales.gen";
import { resolveLocale, validateLocale } from "./resolve";
import { i18n, loadCatalogs } from "./index";

export const LOCALE_STORAGE_KEY = "fbm_lang";

/** `localStorage` throws in Safari private mode and under a strict CSP sandbox. */
export function readStoredLocale(): string | null {
  try {
    return validateLocale(window.localStorage.getItem(LOCALE_STORAGE_KEY));
  } catch {
    return null;
  }
}

function writeStoredLocale(code: string): void {
  try {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, code);
  } catch {
    // A visitor who blocks storage still gets the language they picked for
    // this page view; only the memory of it is lost.
  }
}

/** Forget the visitor's stored choice — "follow the browser" again. */
export function clearStoredLocale(): void {
  try {
    window.localStorage.removeItem(LOCALE_STORAGE_KEY);
  } catch {
    // Nothing was stored if storage is unavailable.
  }
}

/** `?lang=` for the CURRENT page view. Not persisted (see precedence above). */
export function readQueryLocale(search: string = window.location.search): string | null {
  try {
    return validateLocale(new URLSearchParams(search).get("lang"));
  } catch {
    return null;
  }
}

/** What the browser alone would give us, resolved onto a shipped code. */
export function browserLocale(): string {
  return resolveLocale(navigatorCandidates());
}

function navigatorCandidates(): string[] {
  const nav = typeof navigator === "undefined" ? undefined : navigator;
  if (!nav) return [];
  const langs = nav.languages;
  if (langs && langs.length > 0) return [...langs];
  return nav.language ? [nav.language] : [];
}

/**
 * The locale this page view should open in, given the stored/query/browser
 * inputs. Pure — takes no action; `bootstrapLocale` applies the answer.
 */
export function resolveInitialLocale(opts: { tenantLocale?: string | null } = {}): string {
  return (
    readQueryLocale() ??
    readStoredLocale() ??
    validateLocale(opts.tenantLocale) ??
    resolveLocale(navigatorCandidates())
  );
}

export function dirOf(code: string): Dir {
  return localeByCode(code)?.dir ?? "ltr";
}

/**
 * Reflect the active locale onto the document element.
 *
 * `lang` is what a screen reader switches voice on; `dir` is what makes an RTL
 * locale readable at all. Both are set from the SHIPPED table, never from a
 * raw input string.
 */
export function applyDocumentLocale(code: string): void {
  if (typeof document === "undefined") return;
  document.documentElement.lang = code;
  document.documentElement.dir = dirOf(code);
}

/**
 * Switch language. Loads the catalogs first so the page never flashes a
 * half-translated frame, then flips i18next (which re-renders every consumer).
 *
 * `persist: false` is for defaults the visitor did not choose (`?lang=`, the
 * tenant default, the browser's own preference) — those must not overwrite an
 * explicit earlier choice.
 */
export async function setLocale(
  input: string,
  opts: { persist?: boolean } = {},
): Promise<void> {
  const code = validateLocale(input);
  if (!code) return;
  const { persist = true } = opts;
  await loadCatalogs(code);
  if (i18n.language !== code) await i18n.changeLanguage(code);
  if (persist) writeStoredLocale(code);
  applyDocumentLocale(code);
}

let bootstrapped = false;

/**
 * Apply the initial locale once per page load. Idempotent — safe to call from
 * a StrictMode double-invoked effect.
 */
export async function bootstrapLocale(): Promise<void> {
  if (bootstrapped) return;
  bootstrapped = true;
  const initial = resolveInitialLocale();
  // A `?lang=` view and a browser-preference default are both "not the
  // visitor's stored choice", so neither writes `fbm_lang`.
  await setLocale(initial, { persist: false });
}

/** Test seam: forget that `bootstrapLocale` already ran. */
export function resetLocaleBootstrapForTests(): void {
  bootstrapped = false;
}

function subscribe(onChange: () => void): () => void {
  i18n.on("languageChanged", onChange);
  return () => i18n.off("languageChanged", onChange);
}

function activeLocale(): string {
  return validateLocale(i18n.language) ?? DEFAULT_LOCALE;
}

export interface UseLocale {
  /** Always a shipped C34 code — safe for `lang`, `dir` and catalog paths. */
  locale: string;
  dir: Dir;
  setLocale: (code: string) => void;
}

export function useLocale(): UseLocale {
  const locale = useSyncExternalStore(subscribe, activeLocale, () => DEFAULT_LOCALE);

  useEffect(() => {
    void bootstrapLocale();
  }, []);

  // Keeps <html lang/dir> true even if something else changed the language
  // (the settings page saving, a test calling setLocale directly).
  useEffect(() => {
    applyDocumentLocale(locale);
  }, [locale]);

  const set = useCallback((code: string) => {
    void setLocale(code);
  }, []);

  return { locale, dir: dirOf(locale), setLocale: set };
}
