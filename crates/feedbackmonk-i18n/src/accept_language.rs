//! `Accept-Language` parsing — the one new request-header surface this crate adds.
//!
//! Ported from GitCellar's `modules/web/middleware/locale.go`, including its DoS
//! guard: the header is truncated to [`MAX_ACCEPT_LANGUAGE_BYTES`] BEFORE any
//! parsing, so a client cannot make the server sort an unbounded list. The
//! guard is why this is a byte-cap rather than an item-cap — the cost being
//! bounded is the split, not just the sort.
//!
//! Nothing here can fail: a malformed header yields fewer candidates, never an
//! error and never a panic. A submit must not be rejectable by a header the
//! submitter's browser wrote.

use http::HeaderMap;

/// Bytes of `Accept-Language` considered. Anything beyond is discarded.
pub const MAX_ACCEPT_LANGUAGE_BYTES: usize = 200;

/// Candidate tags to hand [`crate::resolve`], most-preferred first.
///
/// Sorted by q-value descending (absent q = 1.0), stable within equal q so the
/// header's own order breaks ties — which is what RFC 9110 means by "order of
/// preference". `*` and `q=0` entries are dropped: a wildcard states no
/// preference, and `q=0` states an explicit *refusal*, so honouring either as a
/// preference would be a bug.
#[must_use]
pub fn parse_accept_language(headers: &HeaderMap) -> Vec<String> {
    let Some(raw) = headers.get(http::header::ACCEPT_LANGUAGE) else {
        return Vec::new();
    };
    let Ok(value) = raw.to_str() else {
        // Non-ASCII bytes in a header that is defined as ASCII: ignore it
        // wholesale rather than guess at an encoding.
        return Vec::new();
    };
    parse_accept_language_str(value)
}

/// [`parse_accept_language`] over a raw header string. Split out so the parser
/// is unit-testable without constructing a `HeaderMap`.
#[must_use]
pub fn parse_accept_language_str(value: &str) -> Vec<String> {
    // DoS guard FIRST — truncate on a char boundary so the cap holds for any
    // input without ever slicing mid-codepoint.
    let mut end = value.len().min(MAX_ACCEPT_LANGUAGE_BYTES);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let capped = &value[..end];

    let mut scored: Vec<(f32, usize, &str)> = Vec::new();
    for (index, part) in capped.split(',').enumerate() {
        let mut fields = part.split(';');
        let tag = fields.next().unwrap_or("").trim();
        if tag.is_empty() || tag == "*" {
            continue;
        }
        let mut q = 1.0_f32;
        for field in fields {
            let field = field.trim();
            if let Some(raw_q) = field.strip_prefix("q=").or_else(|| field.strip_prefix("Q=")) {
                // An unparseable q is treated as 1.0 rather than dropping the
                // tag: a client that mangles its own weights still gets a
                // language, which is the friendlier failure.
                q = raw_q.trim().parse::<f32>().unwrap_or(1.0);
            }
        }
        // `q=0` means "not acceptable" — an explicit refusal, not a weak
        // preference.
        if q <= 0.0 {
            continue;
        }
        scored.push((q, index, tag));
    }

    // Descending q; ties keep header order (the `index` tiebreak makes the sort
    // total, so it is deterministic without relying on sort stability).
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    scored.into_iter().map(|(_, _, tag)| tag.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, HeaderValue};

    fn headers(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(http::header::ACCEPT_LANGUAGE, HeaderValue::from_str(v).unwrap());
        h
    }

    #[test]
    fn absent_header_yields_nothing() {
        assert!(parse_accept_language(&HeaderMap::new()).is_empty());
    }

    #[test]
    fn simple_list_keeps_header_order() {
        assert_eq!(parse_accept_language(&headers("de,en")), vec!["de", "en"]);
    }

    #[test]
    fn q_values_order_the_list() {
        assert_eq!(
            parse_accept_language(&headers("en;q=0.5, de;q=0.9, fr")),
            vec!["fr", "de", "en"]
        );
    }

    #[test]
    fn equal_q_keeps_header_order() {
        assert_eq!(
            parse_accept_language(&headers("de;q=0.8, fr;q=0.8, es;q=0.8")),
            vec!["de", "fr", "es"]
        );
    }

    #[test]
    fn wildcard_and_zero_q_are_dropped() {
        assert_eq!(parse_accept_language(&headers("*")), Vec::<String>::new());
        assert_eq!(parse_accept_language(&headers("de, *;q=0.1")), vec!["de"]);
        assert_eq!(parse_accept_language(&headers("de;q=0, fr")), vec!["fr"]);
    }

    #[test]
    fn whitespace_and_casing_of_q_are_tolerated() {
        assert_eq!(
            parse_accept_language(&headers("  en-GB ; Q=0.2 ,  de-AT ;q=0.9 ")),
            vec!["de-AT", "en-GB"]
        );
    }

    #[test]
    fn unparseable_q_degrades_to_full_weight_not_a_drop() {
        assert_eq!(parse_accept_language(&headers("de;q=banana")), vec!["de"]);
    }

    #[test]
    fn header_is_truncated_at_the_dos_guard() {
        // 60 tags of 5 bytes each far exceeds the 200-byte cap; only the tags
        // that fit are considered, and nothing panics.
        let long = (0..60).map(|_| "de-AT").collect::<Vec<_>>().join(",");
        let got = parse_accept_language(&headers(&long));
        assert!(!got.is_empty());
        assert!(got.len() < 60, "cap did not apply: {} tags", got.len());
        let total: usize = got.iter().map(String::len).sum();
        assert!(total <= MAX_ACCEPT_LANGUAGE_BYTES);
    }

    #[test]
    fn truncation_never_splits_a_codepoint() {
        // A multi-byte tail straddling the cap must not panic.
        let v = format!("{}{}", "a".repeat(MAX_ACCEPT_LANGUAGE_BYTES - 1), "日本語");
        let _ = parse_accept_language_str(&v);
    }

    #[test]
    fn garbage_never_panics() {
        for v in [",,,", ";;;", "q=1", "  ", "-", "en;;q=;", ",;,;"] {
            let _ = parse_accept_language_str(v);
        }
    }
}
