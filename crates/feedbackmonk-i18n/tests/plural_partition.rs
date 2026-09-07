//! The Rust half of the C35 rule-5 plural truth table.
//!
//! `i18n/plural-fixtures.json` is the SHARED contract, exactly as
//! `i18n/resolution-fixtures.json` is for the resolver: three runtimes
//! implement plural selection (this crate's `plural_category`, the widget's
//! `Intl.PluralRules`, and the catalog-authoring partition in
//! `scripts/i18n/_catalog.py`), and each is held to the same file by its own
//! suite. `catalogs.rs` states the goal outright — *"Two implementations of a
//! plural table drift; one that both sides are held to does not"* — and until
//! this file existed nothing actually held them.
//!
//! Read at TEST time rather than `include_str!`-embedded, for the same reason
//! the resolver fixtures are: editing the fixture must be able to turn this
//! red without anyone remembering to rebuild.

use std::collections::BTreeSet;
use std::path::PathBuf;

use feedbackmonk_i18n::catalogs::plural_category;
use feedbackmonk_i18n::{Locale, LOCALES};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Every integer count the fixture's reachable sets were computed over.
const COUNTS: std::ops::RangeInclusive<u64> = 0..=1000;

#[derive(Debug, Deserialize)]
struct Fixtures {
    locales: std::collections::BTreeMap<String, LocaleRow>,
    ledger: Ledger,
}

#[derive(Debug, Deserialize)]
struct LocaleRow {
    /// Suffixes `scripts/i18n/complete-plurals.py` writes for this locale.
    catalog_categories: Vec<String>,
    /// Categories `plural_category` can return over `COUNTS`.
    rust_reachable: Vec<String>,
    /// sha256 of the comma-joined per-count mapping over `COUNTS`.
    rust_map_sha256: String,
}

#[derive(Debug, Deserialize)]
struct Ledger {
    english_fallback_locales: LedgerEntry,
}

#[derive(Debug, Deserialize)]
struct LedgerEntry {
    codes: Vec<String>,
}

fn fixtures_path() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/feedbackmonk-i18n
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../i18n/plural-fixtures.json")
}

fn load() -> Fixtures {
    let path = fixtures_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} is not valid fixture JSON: {e}", path.display()))
}

/// The categories `plural_category` actually yields for `locale` over `COUNTS`.
fn reachable(locale: Locale) -> BTreeSet<String> {
    COUNTS
        .map(|n| plural_category(locale, n).suffix().trim_start_matches('_').to_string())
        .collect()
}

/// sha256 of the comma-joined `plural_category` result for every count in
/// `COUNTS` — the per-count MAPPING, not merely the reachable set. A rule
/// change that swaps two categories around without changing which ones are
/// reachable still turns this red, and it is what keeps the table transcribed
/// into `widget/src/plural-partition.test.ts` honest.
fn map_digest(locale: Locale) -> String {
    let seq: Vec<&str> = COUNTS
        .map(|n| plural_category(locale, n).suffix().trim_start_matches('_'))
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(seq.join(",").as_bytes());
    format!("{:x}", hasher.finalize())
}

#[test]
fn rust_per_count_mapping_matches_the_fixture_digest() {
    let f = load();
    let mut failures = Vec::new();
    for (code, row) in &f.locales {
        let locale = Locale::parse(code).unwrap();
        let got = map_digest(locale);
        if got != row.rust_map_sha256 {
            failures.push(format!(
                "  {code:<7} fixture={} plural_category={got}",
                row.rust_map_sha256
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "plural_category()'s per-count mapping changed for {} locale(s):
{}
         Regenerate i18n/plural-fixtures.json only if the rule change was intended.",
        failures.len(),
        failures.join("
")
    );
}

#[test]
fn fixture_covers_every_shipped_locale() {
    let f = load();
    let shipped: BTreeSet<&str> = LOCALES.iter().map(|l| l.code).collect();
    let fixed: BTreeSet<&str> = f.locales.keys().map(String::as_str).collect();
    assert_eq!(
        shipped, fixed,
        "i18n/plural-fixtures.json and the C34 locale table disagree about what ships"
    );
}

#[test]
fn rust_reachable_categories_match_the_fixture() {
    let f = load();
    let mut failures = Vec::new();
    for (code, row) in &f.locales {
        let locale = Locale::parse(code).unwrap_or_else(|| panic!("{code} is not a shipped code"));
        let got = reachable(locale);
        let want: BTreeSet<String> = row.rust_reachable.iter().cloned().collect();
        if got != want {
            failures.push(format!("  {code:<7} fixture={want:?} plural_category={got:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "plural_category() no longer matches i18n/plural-fixtures.json for {} locale(s):\n{}\n\
         If you changed the rule table on purpose, regenerate the fixture and say so in the commit.",
        failures.len(),
        failures.join("\n")
    );
}

/// The invariant that actually protects a reader: every category the resolver
/// can SELECT must be one the catalog authoring tool WRITES. When it is not,
/// `t_plural` misses the target catalog and `lookup`'s per-key fallback hands
/// back ENGLISH — a translated email rendering an English sentence at some
/// counts and the target language at others.
///
/// Six locales violate this today (measured 2026-09-07). They are pinned in
/// the fixture ledger so the set cannot grow silently; shrinking it is the goal.
#[test]
fn selected_category_is_one_the_catalog_ships_except_for_the_pinned_ledger() {
    let f = load();
    let pinned: BTreeSet<&str> = f
        .ledger
        .english_fallback_locales
        .codes
        .iter()
        .map(String::as_str)
        .collect();

    let mut violating = BTreeSet::new();
    let mut detail = Vec::new();
    for (code, row) in &f.locales {
        let locale = Locale::parse(code).unwrap();
        let ships: BTreeSet<&str> = row.catalog_categories.iter().map(String::as_str).collect();
        let unshipped: Vec<String> = reachable(locale)
            .into_iter()
            .filter(|c| !ships.contains(c.as_str()))
            .collect();
        if !unshipped.is_empty() {
            violating.insert(code.as_str());
            detail.push(format!(
                "  {code:<7} selects {unshipped:?} but the catalog only ships {ships:?}"
            ));
        }
    }

    let new_breaks: Vec<&&str> = violating.difference(&pinned).collect();
    assert!(
        new_breaks.is_empty(),
        "NEW English-fallback divergence in {new_breaks:?} — a plural lookup in these locales will \
         miss the target catalog and render English.\nAll current violations:\n{}\n\
         Fix plural_category() or scripts/i18n/_catalog.py so the two partitions agree; \
         extending the fixture ledger is a last resort and needs a reason.",
        detail.join("\n")
    );

    let fixed: Vec<&&str> = pinned.difference(&violating).collect();
    assert!(
        fixed.is_empty(),
        "{fixed:?} no longer diverge — remove them from ledger.english_fallback_locales.codes in \
         i18n/plural-fixtures.json so the ratchet keeps its teeth."
    );
}
