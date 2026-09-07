//! Compiled-in catalogs: `t` / `t_args` / `t_plural` over
//! `i18n/locales/<code>/{email,status}.json` (Contract C40).
//!
//! **Why `include_str!` and not runtime file I/O** (DEC-FBR-IMPL-30): a
//! self-host image is one binary. Reading catalogs from disk would make the
//! rendered language depend on the deployment's working directory, turn a
//! missing file into a runtime error on the email path, and hand an operator a
//! way to change the product's text without changing the product. Compiled in,
//! a catalog that does not parse is a build failure, which is where that class
//! of bug belongs.
//!
//! Only the two server-side namespaces are embedded. `widget`, `public` and
//! `admin` are consumed by the TypeScript runtimes and would be dead weight in
//! the binary.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::Locale;

/// Namespaces this crate embeds. The first segment of every key it can resolve.
pub const NAMESPACES: &[&str] = &["email", "status"];

/// Embeds `(code, ns, raw json)` for every shipped locale × server namespace.
///
/// The code list appears ONCE here, and `catalog_codes_match_the_locale_table`
/// below fails the build's test run if it drifts from the generated `LOCALES`
/// table — which is the same drift-guard posture `gen-locales.py --check` gives
/// the two TypeScript tables. A `build.rs` could generate this instead; it would
/// buy nothing but a build script, since the guard is what makes hand-listing
/// safe, not the hand-listing that makes the guard necessary.
macro_rules! embed_catalogs {
    ($($code:literal),+ $(,)?) => {
        &[
            $(
                ($code, "email", include_str!(concat!("../../../i18n/locales/", $code, "/email.json"))),
                ($code, "status", include_str!(concat!("../../../i18n/locales/", $code, "/status.json"))),
            )+
        ]
    };
}

/// `(locale code, namespace, raw JSON)` for all 31 locales × 2 namespaces.
static RAW_CATALOGS: &[(&str, &str, &str)] = embed_catalogs!(
    "en", "de", "fr", "es", "pt-BR", "ja", "zh-CN", "ko", "zh-HK", "zh-TW", "ga", "nl", "lv", "ru",
    "uk", "pt-PT", "pl", "bg", "it", "fi", "tr", "cs", "sv", "el", "fa", "hu", "id", "ml", "is",
    "si", "sk",
);

/// Flattened catalogs, keyed by locale code, built once per process.
///
/// One map for both namespaces: keys are namespaced by construction (C35 rule
/// 3 — the first dotted segment IS the namespace), so `email.footer.signoff`
/// and `status.wontfix` cannot collide.
type Catalog = HashMap<String, String>;

static CATALOGS: OnceLock<HashMap<&'static str, Catalog>> = OnceLock::new();

fn catalogs() -> &'static HashMap<&'static str, Catalog> {
    CATALOGS.get_or_init(|| build_catalogs(RAW_CATALOGS))
}

/// Parse and flatten a raw catalog table.
///
/// Split out from [`catalogs`] so the fallback machinery can be exercised
/// against a SYNTHETIC table in tests — a half-translated German catalog is the
/// case that matters and, since the real catalogs are compiled in, there is no
/// other way to construct one without committing a translation (which
/// DEC-FBR-17 reserves for the owner's `/1-translate` step). The tests feed a
/// different table to the same parsing, flattening and lookup code the binary
/// runs; nothing about the mechanism is mocked.
fn build_catalogs(raw_table: &[(&'static str, &str, &str)]) -> HashMap<&'static str, Catalog> {
    let mut by_locale: HashMap<&'static str, Catalog> = HashMap::new();
    for (code, ns, raw) in raw_table {
        let entry = by_locale.entry(code).or_default();
        // A catalog that does not parse is a repo defect, not a runtime
        // condition: it was compiled in from a file under version control and
        // validated by the `i18n-catalog-integrity` oracle. Skipping it
        // degrades to English rather than taking the process down on the email
        // path.
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(value) => flatten_into(&value, String::new(), ns, entry),
            Err(e) => {
                debug_assert!(false, "catalog {code}/{ns}.json is not valid JSON: {e}");
            }
        }
    }
    by_locale
}

/// The C35 rule-6 chain over an explicit catalog map — the body of [`lookup`],
/// factored out so it can run against a synthetic table.
fn lookup_in<'a>(
    catalogs: &'a HashMap<&'static str, Catalog>,
    locale: Locale,
    key: &str,
) -> Option<&'a str> {
    let exact = |l: Locale| catalogs.get(l.code()).and_then(|c| c.get(key)).map(String::as_str);
    exact(locale)
        .or_else(|| locale.base().and_then(exact))
        .or_else(|| exact(Locale::EN))
}

/// Flatten nested JSON objects to dotted keys, dropping `_meta` and any
/// non-string leaf. `prefix` is the dotted path so far; `ns` is the namespace
/// the file belongs to, used only to reject keys that do not start with it.
fn flatten_into(value: &serde_json::Value, prefix: String, ns: &str, out: &mut Catalog) {
    let serde_json::Value::Object(map) = value else {
        return;
    };
    for (k, v) in map {
        if prefix.is_empty() && k == "_meta" {
            continue;
        }
        let path = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            serde_json::Value::String(s) => {
                // C35 rule 3: the first segment equals the namespace. A key
                // that violates it is ignored rather than silently shadowing
                // another namespace's key.
                if path.split('.').next() == Some(ns) {
                    out.insert(path, s.clone());
                }
            }
            serde_json::Value::Object(_) => flatten_into(v, path, ns, out),
            _ => {}
        }
    }
}

/// Look up one key in exactly one locale — no fallback.
fn lookup_exact(locale: Locale, key: &str) -> Option<&'static str> {
    catalogs()
        .get(locale.code())
        .and_then(|c| c.get(key))
        .map(String::as_str)
}

/// The C35 rule-6 fallback chain: active locale → its base bundle (when one is
/// shipped) → English. Per-KEY, not per-file: a half-translated catalog is
/// always shippable, and a raw key on screen is a bug rather than a state.
fn lookup(locale: Locale, key: &str) -> Option<&'static str> {
    lookup_in(catalogs(), locale, key)
}

/// Every key the embedded ENGLISH catalogs define — the source vocabulary.
///
/// Public because it is the honest answer to "what must a translator cover?",
/// and because it lets a consumer assert the fallback invariant over the whole
/// key space rather than over a hand-picked sample.
#[must_use]
pub fn english_keys() -> Vec<&'static str> {
    let mut keys: Vec<&str> = catalogs()
        .get(Locale::EN.code())
        .map(|c| c.keys().map(String::as_str).collect())
        .unwrap_or_default();
    keys.sort_unstable();
    keys
}

/// Translate `key` into `locale`.
///
/// Never panics and never returns empty: an unknown key yields the key itself,
/// which is loud in a rendered email and greppable in a log — the failure mode
/// a translator or a reviewer can actually see.
#[must_use]
pub fn t(locale: Locale, key: &str) -> Cow<'static, str> {
    match lookup(locale, key) {
        Some(v) => Cow::Borrowed(v),
        None => Cow::Owned(key.to_string()),
    }
}

/// [`t`] with `{{name}}` interpolation (C35 rule 4).
///
/// Named placeholders only — no formatting expressions, no nesting, matching
/// the subset the widget's `t()` and i18next implement. A placeholder with no
/// matching argument is left in the output verbatim, so a missing argument is
/// visible rather than silently rendering a hole.
#[must_use]
pub fn t_args(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let template = t(locale, key);
    interpolate(&template, args)
}

/// Replace every `{{name}}` in `template` from `args`.
#[must_use]
pub fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    if args.is_empty() || !template.contains("{{") {
        return template.to_string();
    }
    let mut out = template.to_string();
    for (name, value) in args {
        let needle = format!("{{{{{name}}}}}");
        if out.contains(&needle) {
            out = out.replace(&needle, value);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Plurals (C35 rule 5)
// ---------------------------------------------------------------------------

/// A CLDR cardinal plural category, as a catalog key suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralCategory {
    Zero,
    One,
    Few,
    Many,
    Other,
}

impl PluralCategory {
    /// The suffix appended to the base key (`items` → `items_one`).
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Zero => "_zero",
            Self::One => "_one",
            Self::Few => "_few",
            Self::Many => "_many",
            Self::Other => "_other",
        }
    }
}

/// The CLDR cardinal category for `n` in `locale`.
///
/// **This is the whole plural rule table, deliberately in one function.** It
/// covers exactly the categories Contract C35 declares — `_one`/`_other`
/// everywhere, plus `_few`/`_many` for ru/uk/pl/cs/sk and `_zero` for lv — and
/// it is the same partition `scripts/i18n/complete-plurals.py` fills in. Two
/// implementations of a plural table drift; one that both sides are held to
/// does not. Only integer counts are modelled: nothing in this product
/// pluralises a fraction, and CLDR's fractional categories (`v != 0`) would be
/// unreachable rules pretending to be coverage.
#[must_use]
#[allow(clippy::match_same_arms)] // the language groups are the documentation
pub fn plural_category(locale: Locale, n: u64) -> PluralCategory {
    let n100 = n % 100;
    let n10 = n % 10;
    match locale.code() {
        // Latvian: an explicit zero category, plus a "one" that excludes 11.
        "lv" => {
            if n10 == 0 || (11..=19).contains(&n100) {
                PluralCategory::Zero
            } else if n10 == 1 && n100 != 11 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        // East Slavic: 1/21/31… one; 2-4/22-24… few; everything else many
        // (including 0 and the 11-14 exception band).
        "ru" | "uk" => {
            if n10 == 1 && n100 != 11 {
                PluralCategory::One
            } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
                PluralCategory::Few
            } else {
                PluralCategory::Many
            }
        }
        // Polish: only a literal 1 is "one"; the 2-4 band is few; the rest many.
        "pl" => {
            if n == 1 {
                PluralCategory::One
            } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
                PluralCategory::Few
            } else {
                PluralCategory::Many
            }
        }
        // Czech/Slovak: 1 one, 2-4 few, everything else other. CLDR's `many` is
        // the fractional category only, so it is unreachable for integer counts
        // — `complete-plurals.py` still writes the key, and a translator filling
        // it costs nothing.
        "cs" | "sk" => {
            if n == 1 {
                PluralCategory::One
            } else if (2..=4).contains(&n) {
                PluralCategory::Few
            } else {
                PluralCategory::Other
            }
        }
        // Everything else in the table: the two-category default.
        _ => {
            if n == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
    }
}

/// Pluralised lookup: `t_plural(l, "email.reply.count", 3)` resolves
/// `email.reply.count_few` in Russian, `…_other` in English.
///
/// `{{count}}` is interpolated automatically; extra arguments go through
/// [`t_args`]'s rules. When the target locale's category is missing, the
/// fallback lands on ENGLISH's own category for the same count (C35 rule 5:
/// never silently onto `_other` in the target language, which would render a
/// grammatically wrong string that looks translated).
#[must_use]
pub fn t_plural(locale: Locale, key: &str, n: u64, args: &[(&str, &str)]) -> String {
    let count = n.to_string();
    let category = plural_category(locale, n);
    let suffixed = format!("{key}{}", category.suffix());

    let template = lookup(locale, &suffixed).or_else(|| {
        // English's partition is one/other, so a Russian `_few` has no English
        // twin; ask English what IT would use for this count.
        let en_suffixed = format!("{key}{}", plural_category(Locale::EN, n).suffix());
        lookup_exact(Locale::EN, &en_suffixed)
    });

    let mut merged: Vec<(&str, &str)> = Vec::with_capacity(args.len() + 1);
    merged.push(("count", count.as_str()));
    merged.extend_from_slice(args);

    match template {
        Some(tpl) => interpolate(tpl, &merged),
        None => suffixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LOCALES;

    #[test]
    fn catalog_codes_match_the_locale_table() {
        // The drift guard that makes hand-listing the codes in `embed_catalogs!`
        // safe: adding a row to `i18n/locales.json` without extending the macro
        // fails here (and vice versa).
        let mut embedded: Vec<&str> = RAW_CATALOGS.iter().map(|(c, _, _)| *c).collect();
        embedded.sort_unstable();
        embedded.dedup();
        let mut table: Vec<&str> = LOCALES.iter().map(|l| l.code).collect();
        table.sort_unstable();
        assert_eq!(
            embedded, table,
            "catalogs.rs `embed_catalogs!` drifted from the generated LOCALES table"
        );
        assert_eq!(RAW_CATALOGS.len(), LOCALES.len() * NAMESPACES.len());
    }

    #[test]
    fn every_embedded_catalog_parses() {
        for (code, ns, raw) in RAW_CATALOGS {
            serde_json::from_str::<serde_json::Value>(raw)
                .unwrap_or_else(|e| panic!("{code}/{ns}.json is not valid JSON: {e}"));
        }
    }

    #[test]
    fn meta_is_not_a_translatable_key() {
        assert!(lookup_exact(Locale::EN, "_meta.language").is_none());
        assert!(lookup_exact(Locale::EN, "_meta").is_none());
    }

    #[test]
    fn unknown_key_returns_the_key_itself() {
        assert_eq!(t(Locale::EN, "email.nope.nothing"), "email.nope.nothing");
        let de = Locale::parse("de").unwrap();
        assert_eq!(t(de, "email.nope.nothing"), "email.nope.nothing");
    }

    #[test]
    fn interpolation_replaces_named_placeholders_only() {
        assert_eq!(interpolate("Hi {{name}}!", &[("name", "Ada")]), "Hi Ada!");
        // Unknown placeholder is left verbatim — a missing argument must be visible.
        assert_eq!(interpolate("Hi {{name}}!", &[]), "Hi {{name}}!");
        // Repeated placeholder, and a value containing braces, are both safe.
        assert_eq!(
            interpolate("{{a}}-{{a}}", &[("a", "{{b}}")]),
            "{{b}}-{{b}}"
        );
    }

    #[test]
    fn plural_categories_follow_the_c35_partition() {
        let en = Locale::EN;
        assert_eq!(plural_category(en, 0), PluralCategory::Other);
        assert_eq!(plural_category(en, 1), PluralCategory::One);
        assert_eq!(plural_category(en, 2), PluralCategory::Other);

        let ru = Locale::parse("ru").unwrap();
        assert_eq!(plural_category(ru, 1), PluralCategory::One);
        assert_eq!(plural_category(ru, 21), PluralCategory::One);
        assert_eq!(plural_category(ru, 11), PluralCategory::Many);
        assert_eq!(plural_category(ru, 3), PluralCategory::Few);
        assert_eq!(plural_category(ru, 13), PluralCategory::Many);
        assert_eq!(plural_category(ru, 0), PluralCategory::Many);

        let pl = Locale::parse("pl").unwrap();
        assert_eq!(plural_category(pl, 1), PluralCategory::One);
        assert_eq!(plural_category(pl, 21), PluralCategory::Many); // not `one`, unlike ru
        assert_eq!(plural_category(pl, 22), PluralCategory::Few);

        let cs = Locale::parse("cs").unwrap();
        assert_eq!(plural_category(cs, 1), PluralCategory::One);
        assert_eq!(plural_category(cs, 3), PluralCategory::Few);
        assert_eq!(plural_category(cs, 5), PluralCategory::Other);

        let lv = Locale::parse("lv").unwrap();
        assert_eq!(plural_category(lv, 0), PluralCategory::Zero);
        assert_eq!(plural_category(lv, 11), PluralCategory::Zero);
        assert_eq!(plural_category(lv, 1), PluralCategory::One);
        assert_eq!(plural_category(lv, 21), PluralCategory::One);
        assert_eq!(plural_category(lv, 2), PluralCategory::Other);

        // The five languages with extra categories are exactly the C35 set.
        for l in crate::Locale::all() {
            let extras = (0..=100u64)
                .map(|n| plural_category(l, n))
                .any(|c| matches!(c, PluralCategory::Few | PluralCategory::Many | PluralCategory::Zero));
            let expected = matches!(l.code(), "ru" | "uk" | "pl" | "cs" | "sk" | "lv");
            assert_eq!(extras, expected, "{} plural partition", l.code());
        }
    }

    /// A half-translated German catalog: ONE key translated, one absent.
    ///
    /// This is the state every non-English catalog will be in for most of its
    /// life, and the state the real embedded catalogs cannot be put into from a
    /// test (they are compiled in, and DEC-FBR-17 reserves writing translations
    /// for the owner's `/1-translate` step). Everything below this line runs the
    /// production parse/flatten/lookup code — only the input table differs.
    fn half_translated_table() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            (
                "en",
                "email",
                r#"{"_meta":{"language":"English","status":"source"},
                    "email":{"reply":{"subject":"Reply from the team",
                                      "intro":"The {{brandName}} team replied."}}}"#,
            ),
            (
                "de",
                "email",
                r#"{"_meta":{"language":"Deutsch","status":"machine-translated — community review welcome"},
                    "email":{"reply":{"subject":"Antwort vom Team"}}}"#,
            ),
        ]
    }

    #[test]
    fn fallback_is_per_key_not_per_file() {
        let cat = build_catalogs(&half_translated_table());
        let de = Locale::parse("de").unwrap();

        // The translated key renders German…
        assert_eq!(
            lookup_in(&cat, de, "email.reply.subject"),
            Some("Antwort vom Team")
        );
        // …while the sibling key in the SAME half-translated file renders
        // English rather than a raw key. This is what makes a partial catalog
        // shippable (C35 rule 6).
        assert_eq!(
            lookup_in(&cat, de, "email.reply.intro"),
            Some("The {{brandName}} team replied.")
        );
        // A key in neither catalog is absent, not empty — `t()` turns this into
        // the key itself.
        assert_eq!(lookup_in(&cat, de, "email.reply.nope"), None);
        // English itself is unaffected by the German catalog's existence.
        assert_eq!(
            lookup_in(&cat, Locale::EN, "email.reply.subject"),
            Some("Reply from the team")
        );
    }

    #[test]
    fn every_shipped_locale_renders_text_for_every_english_key() {
        // The durable form of "a raw key on screen is a bug": whatever state
        // the 30 translated catalogs are in — all skeletons today, fully
        // translated after `/1-translate`, anything in between — every locale
        // must produce real text for every source key. This is the assertion
        // that keeps a half-run translation from shipping raw keys.
        let keys = english_keys();
        assert!(!keys.is_empty(), "the English catalogs define no keys");
        for locale in Locale::all() {
            for key in &keys {
                let rendered = t(locale, key);
                assert!(!rendered.is_empty(), "{locale}/{key} rendered empty");
                assert_ne!(
                    rendered.as_ref(),
                    *key,
                    "{locale} renders the raw key {key} — the fallback chain is broken"
                );
            }
        }
    }

    #[test]
    fn the_english_catalog_covers_both_namespaces() {
        let keys = english_keys();
        assert!(keys.iter().any(|k| k.starts_with("email.")), "no email.* keys");
        assert!(keys.iter().any(|k| k.starts_with("status.")), "no status.* keys");
    }

    #[test]
    fn a_key_in_the_wrong_namespace_file_is_ignored() {
        // C35 rule 3: the first dotted segment IS the file's namespace. A
        // `status.*` key smuggled into `email.json` must not shadow the real
        // one — the two files have different owners, so cross-namespace writes
        // are a coordination failure, not a feature.
        let cat = build_catalogs(&[
            ("en", "email", r#"{"status":{"shipped":"WRONG"},"email":{"ok":"right"}}"#),
            ("en", "status", r#"{"status":{"shipped":"Shipped"}}"#),
        ]);
        assert_eq!(lookup_in(&cat, Locale::EN, "status.shipped"), Some("Shipped"));
        assert_eq!(lookup_in(&cat, Locale::EN, "email.ok"), Some("right"));
    }

    #[test]
    fn t_plural_interpolates_count_and_degrades_to_the_key() {
        // No plural keys ship yet; the degradation is the observable behaviour.
        assert_eq!(t_plural(Locale::EN, "email.nope", 1, &[]), "email.nope_one");
        assert_eq!(t_plural(Locale::EN, "email.nope", 2, &[]), "email.nope_other");
    }
}
