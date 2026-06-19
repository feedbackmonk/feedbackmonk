#![allow(clippy::doc_markdown)] // module-doc names header constants verbatim

//! Shared voter-resolution chokepoint for the public voting surfaces.
//!
//! Extracted verbatim from `roadmap.rs` (pure move + re-export — roadmap voting
//! behavior is byte-identical) so the public **roadmap** voting handlers
//! (Contract C15) and the public **board** voting handlers (Contract C30,
//! PF-BOARD-VOTING-01) resolve voters through ONE implementation.
//!
//! This is THE load-bearing "don't duplicate the security primitive" decision
//! (migration 00007/00018 invariant #2): the anon `voter_id =
//! hex(AnonGate::token_hash(ip, cookie, project_id))` is a canonical chokepoint
//! whose per-project hash domain prevents cross-project replay. Re-implementing
//! it per surface would be a security hole; both surfaces call [`resolve_voter`]
//! / [`resolve_voter_no_rate_limit`] here instead.
//!
//! ## Auth-mode resolution (Contract C15/C30)
//!
//! - `Authorization: Bearer <token>` present → `feedbackmonk_jwt::verify_with_leeway`
//!   against the project's Identity-class signing keys (P5b C25). `voter_id =
//!   claims.sub`, mode `Jwt`.
//! - Absent → read `X-Feedbackmonk-Anon-Cookie` (mint + `Set-Cookie` if absent),
//!   compute `AnonGate::token_hash(ip, cookie, project_id)`. `voter_id =
//!   hex(hash)`, mode `Anon`. [`resolve_voter`] pre-checks the rate limit;
//!   [`resolve_voter_no_rate_limit`] (retract path) does not.
//!
//! The success response shapes differ per surface (`item_slug` vs `short_code`),
//! so each handler builds its own; the ERROR responses are slug/code-agnostic
//! (`{"error": ...}`) and live here, shared.

use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_anon::{AnonGate, ANON_COOKIE_HEADER};
use feedbackmonk_core::{KeyClass, RoadmapVoterMode};
use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, JwtError};
use feedbackmonk_repository::ProjectScope;

use crate::error::ApiError;
use crate::state::AppState;

/// Cookie attributes for the anon cookie when minted by a vote request.
/// Same Max-Age / SameSite / HttpOnly as the submission endpoint.
const ANON_COOKIE_MAX_AGE_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Failure modes of voter resolution. Each maps to a distinct HTTP response
/// (the handlers translate these via the response builders below).
pub(crate) enum VoteResolveError {
    JwtError(JwtError),
    RateLimited(u64),
    Api(ApiError),
}

impl From<ApiError> for VoteResolveError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}
impl From<feedbackmonk_repository::RepoError> for VoteResolveError {
    fn from(e: feedbackmonk_repository::RepoError) -> Self {
        Self::Api(e.into())
    }
}

/// Resolve the voter for a vote CAST. JWT `sub` (Jwt mode) or
/// `hex(AnonGate::token_hash(...))` (Anon mode); the anon path pre-checks the
/// rate limit (429 on exceed). Returns `(voter_id, voter_mode, set_cookie)`.
pub(crate) async fn resolve_voter(
    state: &AppState,
    scope: &ProjectScope,
    project_id: Uuid,
    headers: &HeaderMap,
    addr: SocketAddr,
) -> Result<(String, RoadmapVoterMode, Option<HeaderValue>), VoteResolveError> {
    if let Some(token) = extract_bearer(headers) {
        // P5b (C25): identity-class keys only for end-user JWT verification.
        let active_keys = state
            .signing_keys
            .list_active_for_class(scope, KeyClass::Identity)
            .await?;
        let now_unix = current_unix_timestamp();
        let claims = jwt_verify_with_leeway(
            &token,
            project_id,
            &active_keys,
            now_unix,
            state.jwt_iat_leeway_seconds,
        )
        .map_err(VoteResolveError::JwtError)?;
        Ok((claims.sub, RoadmapVoterMode::Jwt, None))
    } else {
        let client_ip = addr.ip().to_string();
        let (cookie_value, set_cookie_header) = resolve_anon_cookie(headers);
        let token_hash = AnonGate::token_hash(&client_ip, &cookie_value, project_id);

        match state.anon_gate.check(&token_hash, project_id) {
            Ok(_) => {}
            Err(feedbackmonk_anon::RateLimitError::Exceeded { retry_after_seconds }) => {
                return Err(VoteResolveError::RateLimited(retry_after_seconds));
            }
        }
        let voter_id = hex_encode(&token_hash);
        Ok((voter_id, RoadmapVoterMode::Anon, set_cookie_header))
    }
}

/// Like [`resolve_voter`] but skips the rate-limit check. Retract is not a new
/// submission; rate-limiting it would block users from undoing accidental votes
/// during a burst.
pub(crate) async fn resolve_voter_no_rate_limit(
    state: &AppState,
    scope: &ProjectScope,
    project_id: Uuid,
    headers: &HeaderMap,
    addr: SocketAddr,
) -> Result<(String, RoadmapVoterMode, Option<HeaderValue>), VoteResolveError> {
    if let Some(token) = extract_bearer(headers) {
        // P5b (C25): identity-class keys only for end-user JWT verification.
        let active_keys = state
            .signing_keys
            .list_active_for_class(scope, KeyClass::Identity)
            .await?;
        let now_unix = current_unix_timestamp();
        let claims = jwt_verify_with_leeway(
            &token,
            project_id,
            &active_keys,
            now_unix,
            state.jwt_iat_leeway_seconds,
        )
        .map_err(VoteResolveError::JwtError)?;
        Ok((claims.sub, RoadmapVoterMode::Jwt, None))
    } else {
        let client_ip = addr.ip().to_string();
        let (cookie_value, set_cookie_header) = resolve_anon_cookie(headers);
        let token_hash = AnonGate::token_hash(&client_ip, &cookie_value, project_id);
        let voter_id = hex_encode(&token_hash);
        Ok((voter_id, RoadmapVoterMode::Anon, set_cookie_header))
    }
}

pub(crate) fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let stripped = value.strip_prefix("Bearer ")?;
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

pub(crate) fn resolve_anon_cookie(headers: &HeaderMap) -> (String, Option<HeaderValue>) {
    if let Some(existing) = headers.get(ANON_COOKIE_HEADER).and_then(|v| v.to_str().ok()) {
        if !existing.is_empty() {
            return (existing.to_string(), None);
        }
    }
    let minted = AnonGate::mint_cookie();
    let set_cookie = format!(
        "{ANON_COOKIE_HEADER}={minted}; Path=/api/v1; Max-Age={ANON_COOKIE_MAX_AGE_SECONDS}; HttpOnly; SameSite=Lax"
    );
    let header_value = HeaderValue::from_str(&set_cookie).ok();
    (minted, header_value)
}

pub(crate) fn current_unix_timestamp() -> i64 {
    #[allow(clippy::cast_possible_wrap)]
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

pub(crate) fn hex_encode(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").expect("write to String never fails");
    }
    s
}

// ===========================================================================
// Shared error-response builders (slug/code-agnostic — `{"error": ...}`)
// ===========================================================================

pub(crate) fn jwt_error_response(err: &JwtError) -> Response {
    let body = Json(json!({ "error": err.variant_name() }));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

pub(crate) fn rate_limited_response(retry_after_seconds: u64) -> Response {
    let body = Json(json!({
        "error": "RateLimitExceeded",
        "retry_after_seconds": retry_after_seconds,
    }));
    let mut response = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
    if let Ok(v) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
        response.headers_mut().insert("Retry-After", v);
    }
    response
}

pub(crate) fn already_voted_response() -> Response {
    let body = Json(json!({ "error": "AlreadyVoted" }));
    (StatusCode::CONFLICT, body).into_response()
}

pub(crate) fn vote_not_found_response() -> Response {
    let body = Json(json!({ "error": "VoteNotFound" }));
    (StatusCode::NOT_FOUND, body).into_response()
}

pub(crate) fn retraction_window_expired_response() -> Response {
    let body = Json(json!({ "error": "RetractionWindowExpired" }));
    (StatusCode::FORBIDDEN, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encode_is_lowercase_64_chars() {
        let bytes = [0xAB; 32];
        let s = hex_encode(&bytes);
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(&s[..4], "abab");
    }

    #[test]
    fn extract_bearer_present_and_empty() {
        let mut h = HeaderMap::new();
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_str("Bearer abc.def.ghi").unwrap(),
        );
        assert_eq!(extract_bearer(&h).as_deref(), Some("abc.def.ghi"));

        let mut h2 = HeaderMap::new();
        h2.insert(AUTHORIZATION, HeaderValue::from_str("Bearer ").unwrap());
        assert!(extract_bearer(&h2).is_none(), "empty token rejected");

        let h3 = HeaderMap::new();
        assert!(extract_bearer(&h3).is_none(), "absent rejected");
    }
}
