//! feedbackmonk-i18n — the server-side locale vocabulary and the compiled-in catalog
//! reader for emails and status words (FR-FBR-34/37, Contracts C34/C40).
//!
//! ```rust
//! use feedbackmonk_i18n::{resolve, t, Locale};
//! let l = resolve(&["de-AT", "en"]);      // -> de
//! let _ = t(l, "email.confirmation.subject");
//! ```
//!
//! Three concerns, one crate: the generated locale TABLE (`locales.gen.rs`), the
//! RESOLVER that maps a browser's preference list onto it (`locale.rs`), and the
//! CATALOGS compiled in from `i18n/locales/<code>/{email,status}.json`
//! (`catalogs.rs`). `accept_language.rs` is the one request-header surface.
//!
//! **The catalogs are read at COMPILE time from paths relative to this file**
//! (`../../../i18n/locales/…`), so the crate is only buildable inside the repo
//! tree — which is also what makes a self-host binary self-contained: no runtime
//! file I/O, no egress, nothing an operator can repoint (DEC-FBR-IMPL-30).
//!
//! `locales.gen.rs` is GENERATED from `i18n/locales.json` by `scripts/i18n/gen-locales.py`.
//! Never edit it by hand; the `i18n-catalog-integrity` oracle runs `--check` on every commit.

#![forbid(unsafe_code)]

/// Text direction of a locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Ltr,
    Rtl,
}

impl Dir {
    /// The value to put in an HTML `dir` attribute.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Dir::Ltr => "ltr",
            Dir::Rtl => "rtl",
        }
    }
}

/// One row of the C34 locale table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleEntry {
    /// Canonical short code (`de`, `pt-BR`, `zh-CN`, …).
    pub code: &'static str,
    /// Native display name (endonym).
    pub name: &'static str,
    pub dir: Dir,
    /// DeepL `target_lang`, or `None` when DeepL has no target (English fallback by design).
    pub deepl: Option<&'static str>,
}

#[path = "locales.gen.rs"]
mod locales_gen;
pub use locales_gen::{BARE_DEFAULTS, DEFAULT_LOCALE, LOCALES, TAG_OVERRIDES};

pub mod accept_language;
pub mod catalogs;
pub mod locale;

pub use accept_language::{parse_accept_language, MAX_ACCEPT_LANGUAGE_BYTES};
pub use catalogs::{english_keys, t, t_args, t_plural, PluralCategory, NAMESPACES};
pub use locale::{default_locale_code, resolve, resolve_opt, Locale};

/// Look up a locale entry by its canonical code (exact match, no resolution).
#[must_use]
pub fn locale_by_code(code: &str) -> Option<&'static LocaleEntry> {
    LOCALES.iter().find(|l| l.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_31_locales_english_first() {
        assert_eq!(LOCALES.len(), 31);
        assert_eq!(LOCALES[0].code, "en");
        assert_eq!(DEFAULT_LOCALE, "en");
    }

    #[test]
    fn only_persian_is_rtl() {
        let rtl: Vec<&str> = LOCALES.iter().filter(|l| l.dir == Dir::Rtl).map(|l| l.code).collect();
        assert_eq!(rtl, vec!["fa"]);
    }

    #[test]
    fn overrides_and_bare_defaults_point_at_shipped_codes() {
        for (_, target) in TAG_OVERRIDES.iter().chain(BARE_DEFAULTS.iter()) {
            assert!(locale_by_code(target).is_some(), "{target} is not shipped");
        }
    }

    #[test]
    fn codes_are_unique() {
        let mut codes: Vec<&str> = LOCALES.iter().map(|l| l.code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), LOCALES.len());
    }
}
