//! The `Locale` newtype and the C34 resolution algorithm.
//!
//! One resolver, three runtimes: this is the Rust half of the same truth table
//! `admin-ui/src/i18n/resolve.ts` and `widget/src/i18n.ts` implement. All three
//! are held to `i18n/resolution-fixtures.json` by their own test suites — a
//! divergence is a red test, not a support ticket.

use crate::{locale_by_code, Dir, LocaleEntry, BARE_DEFAULTS, DEFAULT_LOCALE, LOCALES, TAG_OVERRIDES};

/// A shipped locale. `Copy`, and no larger than a pointer pair: it wraps the
/// `&'static str` code out of [`LOCALES`], so constructing one is a table
/// lookup and passing one around is free.
///
/// There is deliberately no `Locale::from_str_unchecked` — every value in the
/// program came from [`Locale::parse`] or [`resolve`], so a `Locale` in hand is
/// a *shipped* locale by construction. That is what lets `t()` skip validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locale(&'static LocaleEntry);

/// Hash by code, matching the derived `Eq` (which compares the pointed-to row,
/// and codes are unique by construction — see `codes_are_unique` in `lib.rs`).
impl std::hash::Hash for Locale {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.code.hash(state);
    }
}

impl Locale {
    /// English — the source language, the default, and the last link of every
    /// fallback chain.
    pub const EN: Locale = Locale(&LOCALES[0]);

    /// Exact-match parse: the canonical code of a shipped locale, nothing else.
    /// `de` yes; `de-AT`, `DE`, `da` no. Use [`resolve`] when the input is a
    /// user/browser preference rather than a stored canonical value.
    #[must_use]
    pub fn parse(code: &str) -> Option<Locale> {
        locale_by_code(code).map(Locale)
    }

    /// The canonical code (`de`, `pt-BR`, `zh-CN`, …).
    #[must_use]
    pub const fn code(self) -> &'static str {
        self.0.code
    }

    /// Native display name (endonym).
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.0.name
    }

    /// Text direction — the value for an HTML `dir` attribute.
    #[must_use]
    pub const fn dir(self) -> Dir {
        self.0.dir
    }

    /// The base-language bundle to fall back to, when one is shipped.
    ///
    /// Only meaningful for a regional code whose bare language is itself a
    /// shipped bundle. Today no such pair exists (`pt`/`zh` ship only regional
    /// bundles and are handled by [`BARE_DEFAULTS`] in the other direction), so
    /// this returns `None` for every shipped code — but the fallback chain in
    /// `t()` reads it unconditionally so that adding, say, a `pt` bundle later
    /// makes `pt-BR` inherit from it with no code change (C34).
    #[must_use]
    pub fn base(self) -> Option<Locale> {
        let (lang, _) = self.0.code.split_once('-')?;
        Locale::parse(lang)
    }

    /// The generated table row, for callers that want `deepl` or want to
    /// enumerate.
    #[must_use]
    pub const fn entry(self) -> &'static LocaleEntry {
        self.0
    }

    /// Every shipped locale, in switcher order (English first).
    pub fn all() -> impl Iterator<Item = Locale> {
        LOCALES.iter().map(Locale)
    }
}

impl Default for Locale {
    fn default() -> Self {
        Self::EN
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.code)
    }
}

/// Canonicalise one BCP-47-ish tag to the casing the table uses:
/// trim, `_` → `-`, language lowercase, 4-letter script Titlecase, 2-letter or
/// 3-digit region UPPERCASE, everything else lowercase.
///
/// Garbage in is *not* an error — it simply fails to match anything downstream.
fn canonicalize(tag: &str) -> String {
    let trimmed = tag.trim();
    let mut out = String::with_capacity(trimmed.len());
    for (i, part) in trimmed.split(['-', '_']).enumerate() {
        if i > 0 {
            out.push('-');
        }
        if i == 0 {
            out.push_str(&part.to_ascii_lowercase());
        } else if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            // Script subtag: Titlecase (`hant` → `Hant`).
            let mut cs = part.chars();
            if let Some(first) = cs.next() {
                out.push(first.to_ascii_uppercase());
                out.push_str(&cs.as_str().to_ascii_lowercase());
            }
        } else if (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
            || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
        {
            // Region subtag: UPPERCASE (`de` → `DE`, `419` unchanged).
            out.push_str(&part.to_ascii_uppercase());
        } else {
            out.push_str(&part.to_ascii_lowercase());
        }
    }
    out
}

/// Resolve ONE canonicalised tag, or `None` when this deployment ships nothing
/// for it. The C34 per-candidate ladder:
/// exact → longest-prefix [`TAG_OVERRIDES`] → base language → [`BARE_DEFAULTS`].
fn resolve_one(canonical: &str) -> Option<Locale> {
    if canonical.is_empty() || canonical == "*" {
        return None;
    }

    // 1. Exact shipped code.
    if let Some(l) = Locale::parse(canonical) {
        return Some(l);
    }

    // 2. Longest-prefix override. `zh-Hant-CN` must reach `zh-Hant` (script)
    //    rather than being decided by its region, so the LONGEST matching
    //    override key wins.
    let mut best: Option<(&str, &str)> = None;
    for (prefix, target) in TAG_OVERRIDES {
        let matches = canonical == *prefix
            || (canonical.len() > prefix.len()
                && canonical.starts_with(prefix)
                && canonical.as_bytes()[prefix.len()] == b'-');
        let longer_than_best = match best {
            None => true,
            Some((p, _)) => prefix.len() > p.len(),
        };
        if matches && longer_than_best {
            best = Some((prefix, target));
        }
    }
    if let Some((_, target)) = best {
        return Locale::parse(target);
    }

    // 3. Base language, when the bare code is itself shipped (`de-AT` → `de`).
    let lang = canonical.split('-').next().unwrap_or(canonical);
    if let Some(l) = Locale::parse(lang) {
        return Some(l);
    }

    // 4. Bare default, when the language ships only regional bundles
    //    (`pt` → `pt-BR`). Reached by a bare `pt` AND by a regional `pt-US`
    //    whose region matches no override.
    for (bare, target) in BARE_DEFAULTS {
        if lang == *bare {
            return Locale::parse(target);
        }
    }

    None
}

/// Resolve an ordered preference list (`navigator.languages`, or
/// `Accept-Language` sorted by q) to a shipped locale.
///
/// Returns the FIRST candidate that resolves; [`Locale::EN`] when none does.
/// A locale never resolves to a linguistic "neighbour" — an unshipped language
/// gets English, not something merely nearby (C34, and the reason `da` does not
/// silently become `sv`).
#[must_use]
pub fn resolve(candidates: &[&str]) -> Locale {
    resolve_opt(candidates).unwrap_or(Locale::EN)
}

/// [`resolve`] without the English backstop: `None` means *nothing the caller
/// offered is a language we ship*, which is a different fact from *the caller
/// wants English*.
///
/// The submit path (C37) needs exactly that distinction — a browser that sent
/// `Accept-Language: da` stores NULL, not `en`, so a later feature can tell
/// "we do not know this submitter's language" from "this submitter reads
/// English".
#[must_use]
pub fn resolve_opt(candidates: &[&str]) -> Option<Locale> {
    candidates
        .iter()
        .find_map(|c| resolve_one(&canonicalize(c)))
}

/// The default locale's code — `en`, from the generated table.
#[must_use]
pub const fn default_locale_code() -> &'static str {
    DEFAULT_LOCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_is_the_default_and_the_first_row() {
        assert_eq!(Locale::EN.code(), "en");
        assert_eq!(Locale::default(), Locale::EN);
        assert_eq!(default_locale_code(), "en");
    }

    #[test]
    fn parse_is_exact_only() {
        assert_eq!(Locale::parse("de").map(Locale::code), Some("de"));
        assert_eq!(Locale::parse("pt-BR").map(Locale::code), Some("pt-BR"));
        // Not canonicalised, not resolved — `parse` is for stored canonical values.
        assert!(Locale::parse("de-AT").is_none());
        assert!(Locale::parse("DE").is_none());
        assert!(Locale::parse("da").is_none());
        assert!(Locale::parse("").is_none());
    }

    #[test]
    fn canonicalize_matches_the_c34_rules() {
        assert_eq!(canonicalize("  ja  "), "ja");
        assert_eq!(canonicalize("pt_br"), "pt-BR");
        assert_eq!(canonicalize("ZH-HANT"), "zh-Hant");
        assert_eq!(canonicalize("De-de"), "de-DE");
        assert_eq!(canonicalize("es-419"), "es-419");
        assert_eq!(canonicalize("zh-hans-cn"), "zh-Hans-CN");
    }

    #[test]
    fn dir_and_name_come_from_the_table() {
        let fa = Locale::parse("fa").unwrap();
        assert_eq!(fa.dir(), Dir::Rtl);
        assert_eq!(fa.dir().as_str(), "rtl");
        assert_eq!(Locale::EN.dir(), Dir::Ltr);
        assert_eq!(fa.name(), "فارسی");
    }

    #[test]
    fn base_is_none_while_no_bare_language_bundle_ships() {
        // Documents today's table: `pt`/`zh` ship only regional bundles, so no
        // regional code has a shipped base. If a `pt` bundle is ever added this
        // assertion flips and `t()` gains a real middle fallback link for free.
        for l in Locale::all() {
            assert_eq!(l.base(), None, "{} unexpectedly has a base bundle", l.code());
        }
    }

    #[test]
    fn resolve_opt_distinguishes_unshipped_from_english() {
        assert_eq!(resolve_opt(&["da"]), None);
        assert_eq!(resolve_opt(&[]), None);
        assert_eq!(resolve_opt(&["*"]), None);
        assert_eq!(resolve_opt(&["en-GB"]), Some(Locale::EN));
        // …while `resolve` collapses both onto English.
        assert_eq!(resolve(&["da"]), Locale::EN);
        assert_eq!(resolve(&["en-GB"]), Locale::EN);
    }

    #[test]
    fn all_yields_the_whole_table_in_order() {
        let codes: Vec<&str> = Locale::all().map(Locale::code).collect();
        assert_eq!(codes.len(), LOCALES.len());
        assert_eq!(codes[0], "en");
    }

    #[test]
    fn long_garbage_tag_does_not_panic() {
        let long = "x".repeat(4096);
        assert_eq!(resolve(&[&long]), Locale::EN);
        assert_eq!(resolve(&["!!", "not a tag", "-", "--", "a-"]), Locale::EN);
    }
}
