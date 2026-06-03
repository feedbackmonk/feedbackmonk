//! CORS policy for feedbackmonk's public, credentialed widget endpoints
//! (feedback submission + attachment upload).
//!
//! ## Why this exists
//!
//! The embeddable widget runs on a *customer* origin (e.g. `https://gitcellar.com`)
//! and calls the feedbackmonk API cross-origin. For the JSON `POST` the browser
//! sends `Content-Type: application/json`, which is **not** a CORS-safelisted
//! request-header value, so the browser first issues a preflight `OPTIONS`.
//! Without a CORS layer that preflight reaches a `POST`-only route and is
//! answered `405 Method Not Allowed` with no `Access-Control-*` headers, so the
//! browser blocks the real request — disabling cross-origin embed, the widget's
//! entire purpose. This layer answers the preflight and decorates responses with
//! the required `Access-Control-*` headers.
//!
//! This is the long-planned implementation of DEC-FBR-04's "Domain allowlist for
//! widget embed (CORS …)" — enforced at the **submission** endpoint, never on
//! `widget-config` (which stays `*`-public; see [`crate::handlers::widget_config`]).
//!
//! ## Credentials (the load-bearing constraint)
//!
//! The **anonymous** submission path uses `fetch(credentials: "include")` so the
//! `X-Feedbackmonk-Anon-Cookie` (dedup + rate limiting, FR-FBR-06) travels. A
//! credentialed CORS response **MUST** echo the *specific* request origin (never
//! the `*` wildcard) and set `Access-Control-Allow-Credentials: true`. Both are
//! guaranteed here by `allow_credentials(true)` combined with an explicit origin
//! allowlist — `tower_http` *panics at construction* if credentials are combined
//! with a wildcard, which is the spec-correct guard. The companion change is the
//! anon cookie's `SameSite=None; Secure` attributes (see
//! [`crate::handlers::feedback`]), required for the cookie to be stored and sent
//! in a third-party context. See DEC-FBR-IMPL-09.
//!
//! The **authenticated** path uses `credentials: "omit"` + an `Authorization:
//! Bearer` header. The same layer covers it: `authorization` is in the allowed
//! request headers and the echoed origin satisfies the cross-origin read check.
//!
//! ## Allowlist
//!
//! Origins come from `FEEDBACKMONK_CORS_ORIGINS` (comma-separated, e.g.
//! `https://gitcellar.com,https://www.gitcellar.com`). Unset / empty ⇒ **no**
//! cross-origin origin is allowed: the secure default. A preflight from a
//! non-allowlisted origin still gets a response, but without an
//! `Access-Control-Allow-Origin` header, so the browser blocks it.

use std::time::Duration;

use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderValue, Method};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Preflight cache lifetime advertised via `Access-Control-Max-Age` (10 min).
// `from_secs(600)` is intentional — the value is a CORS max-age in *seconds*;
// the seconds-based constructor mirrors the wire semantics.
#[allow(clippy::duration_suboptimal_units)]
const PREFLIGHT_MAX_AGE: Duration = Duration::from_secs(600);

/// Parse a comma-separated origin list (the value of `FEEDBACKMONK_CORS_ORIGINS`)
/// into trimmed, non-empty entries. Order is preserved; no de-duplication
/// (the allowlist is matched by membership, so duplicates are harmless).
#[must_use]
pub fn parse_origins(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Build the credentialed CORS layer for the public widget endpoints
/// (submission + attachments).
///
/// - Allowed origins: exactly `allowed_origins` (echoed back per-request; never
///   `*`). Entries that are not valid header values are skipped.
/// - Allowed methods: `POST`, `OPTIONS`.
/// - Allowed request headers: `content-type`, `authorization`.
/// - Credentials: allowed (`Access-Control-Allow-Credentials: true`).
///
/// An empty `allowed_origins` yields a layer that allows no cross-origin origin
/// — the secure default when `FEEDBACKMONK_CORS_ORIGINS` is unset.
pub fn public_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins: Vec<HeaderValue> = allowed_origins
        .iter()
        .filter_map(|o| o.parse::<HeaderValue>().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION])
        .allow_credentials(true)
        .max_age(PREFLIGHT_MAX_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_origins_trims_and_drops_empties() {
        let got = parse_origins(" https://a.example , ,https://b.example ");
        assert_eq!(
            got,
            vec!["https://a.example".to_string(), "https://b.example".to_string()]
        );
    }

    #[test]
    fn parse_origins_empty_input_is_empty() {
        assert!(parse_origins("").is_empty());
        assert!(parse_origins("   ").is_empty());
        assert!(parse_origins(", ,").is_empty());
    }

    #[test]
    fn parse_origins_single_value() {
        assert_eq!(
            parse_origins("https://gitcellar.com"),
            vec!["https://gitcellar.com".to_string()]
        );
    }

    #[test]
    fn public_cors_layer_builds_with_empty_and_nonempty_allowlist() {
        // Must not panic in either configuration (the credentials+wildcard
        // panic guard only fires for `Any`, which we never construct).
        let _empty = public_cors_layer(&[]);
        let _one = public_cors_layer(&["https://gitcellar.com".to_string()]);
    }
}
