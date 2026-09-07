//! The Rust half of the C34 resolver truth table.
//!
//! `i18n/resolution-fixtures.json` is the SHARED contract: the widget's
//! `resolveLocale`, the SPA's `resolve.ts` and this crate's `resolve()` are each
//! held to the same cases by their own suite. The fixtures are read at TEST time
//! (not `include_str!`-embedded) deliberately — a fixture edit must be able to
//! turn this red without anyone remembering to rebuild, and a missing file must
//! fail loudly rather than silently testing an embedded stale copy.

use std::path::PathBuf;

use feedbackmonk_i18n::{resolve, Locale};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixtures {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    candidates: Vec<String>,
    expect: String,
}

fn fixtures_path() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/feedbackmonk-i18n
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../i18n/resolution-fixtures.json")
}

fn load() -> Fixtures {
    let path = fixtures_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} is not valid fixture JSON: {e}", path.display()))
}

#[test]
fn every_resolution_fixture_passes() {
    let fixtures = load();
    assert!(
        fixtures.cases.len() >= 30,
        "fixture file looks truncated: {} cases",
        fixtures.cases.len()
    );

    let mut failures: Vec<String> = Vec::new();
    for case in &fixtures.cases {
        let candidates: Vec<&str> = case.candidates.iter().map(String::as_str).collect();
        let got = resolve(&candidates);
        if got.code() != case.expect {
            failures.push(format!(
                "  {:<45} candidates={:?} expected={} got={}",
                case.name, case.candidates, case.expect, got
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} resolution fixtures failed:\n{}",
        failures.len(),
        fixtures.cases.len(),
        failures.join("\n")
    );
}

#[test]
fn a_resolver_never_returns_an_unshipped_code() {
    // The property behind the truth table: whatever a client sends, the result
    // is always a code this deployment actually ships a bundle for.
    let fixtures = load();
    for case in &fixtures.cases {
        let candidates: Vec<&str> = case.candidates.iter().map(String::as_str).collect();
        let got = resolve(&candidates);
        assert!(
            Locale::parse(got.code()).is_some(),
            "{}: resolver produced unshipped code {got}",
            case.name
        );
    }
}
