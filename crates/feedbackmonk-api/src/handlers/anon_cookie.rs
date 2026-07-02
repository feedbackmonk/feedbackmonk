//! Shared anon-cookie mint attributes for the public credentialed surfaces
//! (submission + voting). **One source of truth for the `Set-Cookie` shape.**
//!
//! Both the public submit path (`feedback.rs`) and the public voting path
//! (`voting_common.rs`) mint the same `X-Feedbackmonk-Anon-Cookie` for the same
//! anonymous dedup purpose (FR-FBR-06 / per-cookie voter uniqueness). They MUST
//! agree on the cookie attributes, because both are reached CORS-credentialed
//! (cross-site) from the embedded widget:
//!
//! - `SameSite=None; Secure` — the widget embeds cross-site (customer origin →
//!   feedbackmonk API) and fetches with `credentials: "include"`, so the cookie
//!   is only sent in that third-party context when it is `SameSite=None` (which
//!   the browser requires to be `Secure`). A `SameSite=Lax` cookie is silently
//!   dropped cross-site — the browser then never returns it, so every request
//!   mints a FRESH cookie → a fresh anon `voter_id` → the per-cookie uniqueness
//!   guard never sees a duplicate (unlimited ballot-stuffing; scrutiny P2-3).
//! - `HttpOnly` — unreadable to page JS (privacy; the widget never reads it).
//! - `Path=/api/v1`, `Max-Age=30d`.
//!
//! See DEC-FBR-IMPL-09. Previously each path inlined the format string; the vote
//! path drifted to `SameSite=Lax`, which is exactly the bug this shared helper
//! prevents from recurring.

use axum::http::HeaderValue;

use feedbackmonk_anon::ANON_COOKIE_HEADER;

/// `Max-Age` for the minted anon cookie (30 days).
pub(crate) const ANON_COOKIE_MAX_AGE_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Build the `Set-Cookie` header value for a freshly-minted anon cookie, with
/// the canonical cross-site attributes (`SameSite=None; Secure; HttpOnly`).
/// Returns `None` only if the value cannot form a valid header (never in
/// practice — the minted value is base64url).
pub(crate) fn set_cookie_header(minted: &str) -> Option<HeaderValue> {
    let set_cookie = format!(
        "{ANON_COOKIE_HEADER}={minted}; Path=/api/v1; Max-Age={ANON_COOKIE_MAX_AGE_SECONDS}; HttpOnly; Secure; SameSite=None"
    );
    HeaderValue::from_str(&set_cookie).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cookie_header_is_cross_site_safe() {
        let v = set_cookie_header("abc123").expect("valid header");
        let s = v.to_str().unwrap();
        assert!(s.contains("SameSite=None"));
        assert!(s.contains("Secure"));
        assert!(s.contains("HttpOnly"));
        assert!(!s.contains("SameSite=Lax"));
        assert!(s.contains("abc123"));
        assert!(s.contains(&format!("Max-Age={ANON_COOKIE_MAX_AGE_SECONDS}")));
    }
}
