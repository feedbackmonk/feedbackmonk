// Date/number formatting for humans.
//
// LOCALE IS AN ARGUMENT, NOT AN AMBIENT (D-FBR-31). `Intl` defaults to the
// BROWSER's locale, which is not the locale the page is being rendered in the
// moment a visitor picks a language in the switcher: a German-speaking visitor
// on an English browser used to read German chrome next to English-formatted
// dates. Pass the active locale from `useLocale()`.
//
// `locale` is REQUIRED (Stage 2 / W-D, D-FBR-31): every call site in the SPA
// now has a `useLocale()` in scope, so there is no remaining caller that needs
// the interim optional-parameter escape hatch.

export function formatRelative(
  iso: string,
  locale: string,
  now: Date = new Date(),
): string {
  const ts = new Date(iso).getTime();
  if (Number.isNaN(ts)) return iso;
  const diffSec = Math.round((ts - now.getTime()) / 1000);
  const abs = Math.abs(diffSec);
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (abs < 60) return rtf.format(diffSec, "second");
  if (abs < 3600) return rtf.format(Math.round(diffSec / 60), "minute");
  if (abs < 86400) return rtf.format(Math.round(diffSec / 3600), "hour");
  if (abs < 86400 * 30) return rtf.format(Math.round(diffSec / 86400), "day");
  if (abs < 86400 * 365)
    return rtf.format(Math.round(diffSec / (86400 * 30)), "month");
  return rtf.format(Math.round(diffSec / (86400 * 365)), "year");
}

export function formatAbsolute(iso: string, locale: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(locale);
}
