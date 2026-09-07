// Contract C34 — resolving a browser locale tag onto a locale we actually ship.
//
// THE SAME SEMANTICS RUN IN THREE PLACES: here (SPA), `widget/src/i18n.ts`
// (embeddable widget) and `crates/feedbackmonk-i18n` (Rust, for emails and
// `Accept-Language`). All three are pinned by ONE truth table —
// `i18n/resolution-fixtures.json` — which each of them runs as a test. If you
// change a rule here, change it there, and the fixtures decide who is right.
//
// WHY A TABLE RATHER THAN "PICK THE FIRST REGIONAL VARIANT" (ported reasoning
// from GitCellar's `locale-resolution.ts`): choosing between `pt-BR` and
// `pt-PT`, or between Simplified and Traditional Chinese, is a judgement about
// readers, not a lookup. Making it implicit in array order means a reordered
// list silently reassigns a hundred million readers to a different orthography.
// Every entry in `TAG_OVERRIDES` / `BARE_DEFAULTS` (i18n/locales.json, C34) is
// a decision someone can disagree with in review.
//
// WHAT THIS DELIBERATELY DOES NOT DO: a language we do not ship passes through
// unresolved. Mapping `da` to `sv` because the languages are close would show a
// Danish reader Swedish and present it as their language — worse than English,
// which at least does not pretend.

import {
  BARE_DEFAULTS,
  DEFAULT_LOCALE,
  TAG_OVERRIDES,
  isShippedLocale,
} from "./locales.gen";

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
 * Resolve ONE candidate tag onto a shipped locale code, or `null` when we ship
 * nothing for it.
 *
 * Order (C34): exact shipped code → longest-prefix override → base language →
 * bare-language default → nothing.
 */
export function resolveOne(candidate: string): string | null {
  if (!candidate || !candidate.trim()) return null;
  const tag = canonicalise(candidate);

  // 1. We ship exactly this tag.
  if (isShippedLocale(tag)) return tag;

  // 2. An explicit decision covers this tag (or a prefix of it). Longest
  //    prefix first, so `zh-Hans-CN` hits `zh-Hans`; script beats region, so
  //    `zh-Hant-CN` is Traditional despite the CN.
  //    OWN properties only: a plain `TABLE[key]` read walks the prototype
  //    chain exactly as `in` does, so `constructor` used to come back as the
  //    `Object` constructor itself — truthy, and returned as if it were a
  //    locale code, breaking this function's ALWAYS-a-shipped-code contract
  //    (R-SEC finding + the widget ratchet, collab-20260907-034037).
  const parts = tag.split("-");
  for (let n = parts.length; n >= 2; n--) {
    const prefix = parts.slice(0, n).join("-");
    if (Object.prototype.hasOwnProperty.call(TAG_OVERRIDES, prefix)) return TAG_OVERRIDES[prefix];
  }

  // 3. The base language ships its own catalog: de-AT → de.
  const base = parts[0];
  if (isShippedLocale(base)) return base;

  // 4. The base ships only regional catalogs: pt → pt-BR, zh → zh-CN.
  if (Object.prototype.hasOwnProperty.call(BARE_DEFAULTS, base)) return BARE_DEFAULTS[base];

  // 5. Not a language we ship.
  return null;
}

/**
 * Resolve an ordered preference list (`navigator.languages`, or an
 * `Accept-Language` header sorted by q) onto a shipped locale code.
 *
 * Returns the FIRST candidate that resolves; `en` when none does. The return
 * value is ALWAYS a member of the shipped table — callers may pass it straight
 * to `<html lang>`, `dir`, `changeLanguage` and the catalog `import()` without
 * re-validating.
 */
export function resolveLocale(candidates: readonly string[]): string {
  for (const candidate of candidates) {
    const hit = resolveOne(candidate);
    if (hit) return hit;
  }
  return DEFAULT_LOCALE;
}

/**
 * Validate an EXTERNAL locale input (`?lang=`, `localStorage['fbm_lang']`, the
 * tenant's stored locale) — an exact shipped code, or nothing.
 *
 * Deliberately stricter than `resolveLocale`: an unrecognised value is ignored
 * rather than coerced, so a hostile `?lang=` can never be echoed anywhere, and
 * a stored value from a future (or tampered) catalog list cannot reach an
 * `import()` path. Casing/underscore canonicalisation IS applied, because
 * `?lang=PT_BR` is a user typo, not an attack.
 */
export function validateLocale(value: unknown): string | null {
  if (typeof value !== "string" || !value.trim()) return null;
  const tag = canonicalise(value);
  return isShippedLocale(tag) ? tag : null;
}
