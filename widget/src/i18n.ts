import type { Dir } from "./locales.gen.js";
import {
  BARE_DEFAULTS,
  DEFAULT_LOCALE,
  ENGLISH_FALLBACK,
  LOADERS,
  RTL,
  TAG_OVERRIDES,
} from "./locale-chunks.js";
import EN from "./catalog-en.js";

// Runtime i18n for the feedbackmonk widget (FR-FBR-35, Contracts C34/C35/C42).
//
// No i18n library and no dependency (DEC-FBR-IMPL-30) — every byte here counts
// against the 30 KiB page-load cap defended by the widget-bundle-size oracle.
//
//   - `en` is INLINED (statically imported above), so an English page load
//     fetches nothing extra and a failed/absent locale chunk degrades to
//     English rather than to raw keys.
//   - every other locale is a same-origin lazy chunk `dist/locales/<code>.js`
//     (`loadLocale`), which is what keeps the cap holdable at 31 locales.
//   - lookup is per-KEY: active catalog → `en` → the key itself. A partially
//     translated catalog is always shippable (C35 rule 6).

export type Catalog = Readonly<Record<string, string>>;
export type Args = Record<string, string | number>;
export type Translate = (key: string, args?: Args) => string;

/**
 * OWN-property lookup on the three generated tables.
 *
 * `k in TABLE` and `TABLE[k]` both walk the prototype chain, so `constructor` --
 * the one `Object.prototype` name that survives canonicalisation (`__proto__` ->
 * `--proto--`, `toString` -> `tostring`) -- passed the shipped-code gate AND made
 * `BARE_DEFAULTS[base]` hand back the `Object` constructor itself, which
 * `resolveOne` then returned as if it were a locale code. R-SEC finding,
 * collab-20260907-034037; widget bytes authorised by the LD.
 */
function own(table: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(table, key);
}

/**
 * Is `code` one of the 31 shipped locales (C34)?
 *
 * The shipped set is read off the generated loader map, so "a locale we ship"
 * and "a locale we can actually fetch a catalog for" are the same fact rather
 * than two tables that can disagree. See `locale-chunks.ts` for why the widget
 * projects the C34 table instead of importing `locales.gen.ts`.
 */
function isShipped(code: string): boolean {
  return code === DEFAULT_LOCALE || own(LOADERS, code);
}

let loc: string = DEFAULT_LOCALE;
let active: Catalog = EN;
let pluralRules: Intl.PluralRules | null = null;

// ---------------------------------------------------------------------------
// Resolver (Contract C34) — PORTED VERBATIM in semantics from
// `admin-ui/src/i18n/resolve.ts`, which landed first (CLAUDE-B, MSG 22:38;
// LEAD 22:42: "copy it rather than re-deriving"). Same function names, same
// order, same edge cases, so a reader can diff the two files line for line.
//
// The ONE difference is where the tables come from: the SPA imports
// `locales.gen.ts`; the widget cannot afford to (see `locale-chunks.ts`), so
// the same three tables arrive as a build-time projection of the same
// `i18n/locales.json`. Both files run `i18n/resolution-fixtures.json` as a
// test, and the fixtures decide who is right.
//
// WHAT THIS DELIBERATELY DOES NOT DO: a language we do not ship passes through
// unresolved. Mapping `da` to `sv` because the languages are close would show a
// Danish reader Swedish and present it as their language — worse than English,
// which at least does not pretend.
// ---------------------------------------------------------------------------

/** BCP-47 canonical casing: `pt_br` → `pt-BR`, `ZH-HANT` → `zh-Hant`. */
export function canonicalise(tag: string): string {
  return tag
    .trim()
    .replace(/_/g, "-")
    .split("-")
    .map((part, i) => {
      if (i === 0) return part.toLowerCase();
      if (part.length === 4)
        return part[0].toUpperCase() + part.slice(1).toLowerCase();
      if (part.length === 2 || /^\d{3}$/.test(part)) return part.toUpperCase();
      return part.toLowerCase();
    })
    .join("-");
}

/**
 * Resolve ONE candidate tag onto a shipped locale code, or `null`.
 * Order (C34): exact → longest-prefix override → base language → bare default.
 */
export function resolveOne(candidate: string): string | null {
  if (!candidate || !candidate.trim()) return null;
  const tag = canonicalise(candidate);
  if (isShipped(tag)) return tag;
  // Longest prefix first, so `zh-Hans-CN` hits `zh-Hans`; script beats region,
  // so `zh-Hant-CN` is Traditional despite the CN.
  const parts = tag.split("-");
  for (let n = parts.length; n >= 2; n--) {
    const prefix = parts.slice(0, n).join("-");
    if (own(TAG_OVERRIDES, prefix)) return TAG_OVERRIDES[prefix];
  }
  const base = parts[0];
  if (isShipped(base)) return base;
  return own(BARE_DEFAULTS, base) ? BARE_DEFAULTS[base] : null;
}

/**
 * Resolve an ordered preference list onto a shipped locale code.
 *
 * For the widget the list is `[data-locale, <html lang>, ...navigator.languages]`
 * (Contract C36 precedence, assembled in `widget.ts`). Returns the FIRST
 * candidate that resolves; `en` when none does. The result is ALWAYS a member
 * of the shipped table, so callers may pass it straight to `lang`, `dir` and
 * the catalog loader without re-validating.
 */
export function resolveLocale(candidates: readonly (string | null | undefined)[]): string {
  for (const candidate of candidates) {
    const hit = candidate ? resolveOne(candidate) : null;
    if (hit) return hit;
  }
  return DEFAULT_LOCALE;
}

/**
 * Validate an EXTERNAL locale input — an exact shipped code, or nothing.
 * Stricter than `resolveLocale` on purpose: an unrecognised value is ignored
 * rather than coerced. The widget has no attacker-reachable locale input today
 * (`data-locale` is the embedder's own HTML, and it wants the lenient
 * resolution above — a host that writes `data-locale="de-AT"` means German);
 * this exists so the three runtimes expose one API, and for the day the widget
 * grows an input that is not the embedder's.
 */
export function validateLocale(value: unknown): string | null {
  if (typeof value !== "string" || !value.trim()) return null;
  const tag = canonicalise(value);
  return isShipped(tag) ? tag : null;
}

export function setLocale(code: string): void {
  loc = isShipped(code) ? code : DEFAULT_LOCALE;
  active = EN;
  pluralRules = null;
}

export function activeLocale(): string {
  return loc;
}

/**
 * Install a fetched catalog as the active one. Separate from `loadLocale` so a
 * chunk that lands after the locale changed again cannot win — and so the
 * per-key fallback chain is unit-testable without mocking a dynamic import.
 */
export function applyCatalog(code: string, map: Catalog): void {
  if (loc === code) active = map;
}

export function dir(): Dir {
  return RTL.indexOf(loc) >= 0 ? "rtl" : "ltr";
}

/**
 * The language the widget's words are actually WRITTEN IN — not always the
 * active locale (R-A11Y finding A-2).
 *
 * `ga, fa, ml, is, si` have no MT provider, so their catalogs stay English
 * permanently: `/1-translate` fills the other 25 and never these. Declaring
 * `lang="fa"` over English words makes a screen reader read English with
 * Persian phonology, and `fa` is the only RTL locale we ship. `dir()` is
 * deliberately NOT adjusted — the visitor still gets the mirrored layout their
 * locale asks for, announced in a voice that can pronounce what is on screen.
 */
export function contentLang(): string {
  return ENGLISH_FALLBACK.indexOf(loc) >= 0 ? DEFAULT_LOCALE : loc;
}

/**
 * True when `key` renders a real string (active catalog or the `en` source).
 *
 * OWN properties only, for the same reason as `own` above: `key in EN` is true
 * for `constructor` and every other `Object.prototype` name, so `t()` would
 * report a key it cannot render. Unreachable today — `t()` takes literal
 * developer-authored keys and the one constructed key is prefixed
 * (`"widget.error." + code`) — and closed anyway so widening `t()` to a dynamic
 * key later cannot reopen it (LD ruling, collab-20260907-034037: close the
 * class, not the reachable instances).
 */
export function hasKey(key: string): boolean {
  return own(active, key) || own(EN, key);
}

function category(n: number): string {
  try {
    if (!pluralRules) pluralRules = new Intl.PluralRules(loc);
    return pluralRules.select(n);
  } catch {
    // No `Intl.PluralRules` (or an exotic tag): English-shaped fallback.
    return n === 1 ? "one" : "other";
  }
}

/**
 * Translate `key`, interpolating `{{name}}` placeholders from `args`.
 * `args.count` additionally selects a CLDR plural suffix (`_one`, `_few`, …),
 * falling back to `_other` and then to the unsuffixed key (C35 rule 5).
 */
export const t: Translate = (key, args) => {
  let k = key;
  if (args && typeof args.count === "number") {
    const c = "_" + category(args.count);
    if (hasKey(key + c)) k = key + c;
    else if (hasKey(key + "_other")) k = key + "_other";
  }
  let s = own(active, k) ? active[k] : own(EN, k) ? EN[k] : key;
  if (args) {
    for (const name in args) {
      s = s.split("{{" + name + "}}").join(String(args[name]));
    }
  }
  return s;
};

/**
 * Fetch the per-locale catalog chunk (`dist/locales/<code>.js`, Contract C42).
 * Same-origin dynamic `import()` — the identical mechanism `redact.ts` already
 * uses under GitCellar's `script-src 'self'` CSP. Never throws: a missing,
 * blocked or slow chunk simply leaves the widget rendering English.
 * `en` is inlined and is deliberately never fetched.
 */
export async function loadLocale(code: string): Promise<void> {
  setLocale(code);
  const target = loc;
  const load = own(LOADERS, target) ? LOADERS[target] : undefined;
  if (!load) return;
  try {
    const mod = await load();
    if (mod && mod.default) applyCatalog(target, mod.default);
  } catch {
    // Stay English (C35 rule 6). A widget that renders raw keys is a bug;
    // a widget that renders English is a degraded but correct widget.
  }
}
