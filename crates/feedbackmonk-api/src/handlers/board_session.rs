#![allow(clippy::unused_async)] // axum handlers must be async even when the body has no .await branch
#![allow(clippy::doc_markdown)] // module-doc references HTTP paths / cookie names verbatim

//! Hosted-board sign-in handoff and board session — Contract C43 (FR-FBR-43/44,
//! DEC-FBR-21). Wire contract: `docs/planning/handoffs/prioritization-contracts.md`.
//!
//! The routes register inside `board::board_router`, so `build_app` and the
//! `host-tenant-binding` classification do not change:
//!
//! ```text
//!   POST   /api/v1/projects/{project_id}/board/session   exchange_board_session
//!   GET    /api/v1/projects/{project_id}/board/session   get_board_session
//!   DELETE /api/v1/projects/{project_id}/board/session   end_board_session
//! ```
//!
//! The session (FR-FBR-44): the board posts the fragment-carried JWT once;
//! the exchange verifies it with `feedbackmonk_jwt::verify_with_leeway`
//! (audience = project), mints a random token, stores only its sha256 hex
//! ([`hash_board_session_token`]) and the `sub` via
//! `state.board_votes.sessions().create_board_session(..)`, and sets
//! [`board_session_cookie_name`] HttpOnly; Secure; SameSite=Lax; Path=/;
//! no Domain (host-only); Max-Age = 8 h. The token is read from that POST body
//! and nowhere else: no query string, path or other header is ever exchanged.
//!
//! A board viewer is resolved by [`resolve_board_viewer`]: a bearer JWT first,
//! else the session cookie, else anonymous. A session is refused once its
//! `expires_at` has passed — in SQL, and again here by this process's clock.
//!
//! The session routes run the board-enabled check (a disabled board 404s) but
//! never the sign-in check: they are how an anonymous viewer becomes signed in
//! (critic C-001, collab-20261013-145347).

use axum::extract::{Path, State};
use axum::http::header::{CACHE_CONTROL, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use feedbackmonk_core::{KeyClass, BOARD_SESSION_MAX_AGE_SECS};
use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, JwtError};
use feedbackmonk_repository::ProjectScope;

use crate::error::ApiError;
use crate::handlers::board::ensure_board_enabled;
use crate::handlers::voting_common::{
    current_unix_timestamp, extract_bearer, hex_encode, jwt_error_response,
};
use crate::state::AppState;

/// Cookie-name prefix; the full name carries the project id so one browser can
/// hold sessions for several projects' boards without collision.
pub const BOARD_SESSION_COOKIE_PREFIX: &str = "fbm_board_";

/// The board-session cookie name for one project (`fbm_board_<uuid simple>`).
#[must_use]
pub fn board_session_cookie_name(project_id: Uuid) -> String {
    format!("{BOARD_SESSION_COOKIE_PREFIX}{}", project_id.simple())
}

/// sha256 lowercase hex of a cookie token — the only form stored server-side
/// (migration 00034). 64 chars.
#[must_use]
pub fn hash_board_session_token(token: &str) -> String {
    let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    hex_encode(&digest)
}

/// `POST …/board/session` body: the end-user JWT taken from the URL fragment.
#[derive(Debug, Clone, Deserialize)]
pub struct BoardSessionExchangeRequest {
    pub token: String,
}

/// Response of every board-session route. `expires_at` is present exactly when
/// `signed_in` is true. The `sub` is never echoed.
#[derive(Debug, Clone, Serialize)]
pub struct BoardSessionResponse {
    pub signed_in: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Who is looking at the board (FR-FBR-43/46).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardViewer {
    Anonymous,
    /// A verified end-user identity: the JWT `sub` (bearer token or session).
    SignedIn { sub: String },
}

impl BoardViewer {
    #[must_use]
    pub fn sub(&self) -> Option<&str> {
        match self {
            Self::Anonymous => None,
            Self::SignedIn { sub } => Some(sub.as_str()),
        }
    }
}

/// Why a viewer could not be resolved. A bad bearer token is the existing JWT
/// error response (`401 {"error":"<variant>"}`), never a silent downgrade to
/// anonymous; anything else is an ordinary API error.
#[derive(Debug)]
pub enum BoardViewerError {
    Jwt(JwtError),
    Api(ApiError),
}

impl From<ApiError> for BoardViewerError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

impl From<feedbackmonk_repository::RepoError> for BoardViewerError {
    fn from(e: feedbackmonk_repository::RepoError) -> Self {
        Self::Api(e.into())
    }
}

impl IntoResponse for BoardViewerError {
    fn into_response(self) -> Response {
        match self {
            Self::Jwt(e) => jwt_error_response(&e),
            Self::Api(e) => e.into_response(),
        }
    }
}

/// The refusal for an anonymous viewer on a `signed_in` board (FR-FBR-43):
/// `401 {"error":"SignInRequired"}` — distinct from the board-disabled 404 so
/// the UI renders a sign-in prompt.
#[must_use]
pub fn sign_in_required_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "SignInRequired" })),
    )
        .into_response()
}

/// Verify an end-user JWT for this project (audience = project, identity-class
/// keys only, P5b C25) and return its `sub`.
#[allow(clippy::result_large_err)] // the error is a finished refusal response
async fn verify_end_user_jwt(
    state: &AppState,
    scope: &ProjectScope,
    token: &str,
) -> Result<String, BoardViewerError> {
    let keys = state
        .signing_keys
        .list_active_for_class(scope, KeyClass::Identity)
        .await?;
    let claims = jwt_verify_with_leeway(
        token,
        scope.project_id(),
        &keys,
        current_unix_timestamp(),
        state.jwt_iat_leeway_seconds,
    )
    .map_err(BoardViewerError::Jwt)?;
    Ok(claims.sub)
}

/// The raw board-session token this request carries for this project, if any.
fn session_cookie_token(headers: &HeaderMap, project_id: Uuid) -> Option<String> {
    let jar = CookieJar::from_headers(headers);
    let value = jar.get(&board_session_cookie_name(project_id))?.value().to_string();
    (!value.is_empty()).then_some(value)
}

/// The live session behind this request's cookie, refused once `expires_at`
/// has passed by this process's clock as well as the database's.
async fn live_session(
    state: &AppState,
    scope: &ProjectScope,
    headers: &HeaderMap,
) -> Result<Option<feedbackmonk_repository::BoardSession>, ApiError> {
    let Some(token) = session_cookie_token(headers, scope.project_id()) else {
        return Ok(None);
    };
    let session = state
        .board_votes
        .sessions()
        .resolve_board_session(scope, &hash_board_session_token(&token))
        .await?;
    Ok(session.filter(|s| s.expires_at > Utc::now()))
}

/// Resolve the board viewer: a valid `Authorization: Bearer` JWT (audience =
/// project) wins; else a live board-session cookie for this project; else
/// anonymous. An invalid bearer token is an error, never a silent downgrade.
/// The token is never read from a query string or path.
#[allow(clippy::result_large_err)] // the error is a finished refusal response
pub async fn resolve_board_viewer(
    state: &AppState,
    scope: &ProjectScope,
    headers: &HeaderMap,
) -> Result<BoardViewer, BoardViewerError> {
    if let Some(token) = extract_bearer(headers) {
        let sub = verify_end_user_jwt(state, scope, &token).await?;
        return Ok(BoardViewer::SignedIn { sub });
    }
    Ok(match live_session(state, scope, headers).await? {
        Some(session) => BoardViewer::SignedIn {
            sub: session.user_sub,
        },
        None => BoardViewer::Anonymous,
    })
}

fn session_cookie(project_id: Uuid, value: String, max_age_secs: i64) -> HeaderValue {
    let cookie = Cookie::build((board_session_cookie_name(project_id), value))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(max_age_secs))
        .build();
    // Name and value are hex/ASCII by construction.
    HeaderValue::from_str(&cookie.to_string()).expect("ASCII cookie")
}

fn session_response(body: BoardSessionResponse, set_cookie: Option<HeaderValue>) -> Response {
    let mut resp = (StatusCode::OK, Json(body)).into_response();
    let headers = resp.headers_mut();
    // A session answer is per browser; no cache may keep it.
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(c) = set_cookie {
        headers.insert(SET_COOKIE, c);
    }
    resp
}

/// `POST …/board/session` — the one-time token exchange (FR-FBR-44).
pub async fn exchange_board_session(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(req): Json<BoardSessionExchangeRequest>,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    ensure_board_enabled(&state, &scope).await?;

    let sub = match verify_end_user_jwt(&state, &scope, &req.token).await {
        Ok(sub) => sub,
        Err(e) => return Ok(e.into_response()),
    };

    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = hex_encode(&bytes);
    let session = state
        .board_votes
        .sessions()
        .create_board_session(
            &scope,
            &hash_board_session_token(&token),
            &sub,
            BOARD_SESSION_MAX_AGE_SECS,
        )
        .await?;

    Ok(session_response(
        BoardSessionResponse {
            signed_in: true,
            expires_at: Some(session.expires_at),
        },
        Some(session_cookie(project_id, token, BOARD_SESSION_MAX_AGE_SECS)),
    ))
}

/// `GET …/board/session` — whether this browser holds a live session. Reads
/// the cookie only; a token anywhere in the request is ignored.
pub async fn get_board_session(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    ensure_board_enabled(&state, &scope).await?;
    let session = live_session(&state, &scope, &headers).await?;
    Ok(session_response(
        BoardSessionResponse {
            signed_in: session.is_some(),
            expires_at: session.map(|s| s.expires_at),
        },
        None,
    ))
}

/// `DELETE …/board/session` — sign out of the board: ends the session and
/// expires the cookie.
pub async fn end_board_session(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    ensure_board_enabled(&state, &scope).await?;
    if let Some(token) = session_cookie_token(&headers, project_id) {
        state
            .board_votes
            .sessions()
            .end_board_session(&scope, &hash_board_session_token(&token))
            .await?;
    }
    Ok(session_response(
        BoardSessionResponse {
            signed_in: false,
            expires_at: None,
        },
        Some(session_cookie(project_id, String::new(), 0)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_lowercase_sha256_hex() {
        let h = hash_board_session_token("abc");
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn cookie_name_is_project_scoped() {
        let a = board_session_cookie_name(Uuid::nil());
        assert_eq!(a, "fbm_board_00000000000000000000000000000000");
        assert_ne!(a, board_session_cookie_name(Uuid::from_u128(1)));
    }

    #[test]
    fn session_cookie_is_host_only_httponly_secure_lax() {
        let c = session_cookie(Uuid::nil(), "t".into(), BOARD_SESSION_MAX_AGE_SECS);
        let s = c.to_str().unwrap();
        assert!(s.starts_with("fbm_board_00000000000000000000000000000000=t"));
        for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/", "Max-Age=28800"] {
            assert!(s.contains(attr), "{attr} missing: {s}");
        }
        assert!(!s.contains("Domain"), "host-only: {s}");
    }

    #[test]
    fn session_token_is_read_from_this_projects_cookie_only() {
        let pid = Uuid::from_u128(7);
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_str(&format!(
                "other=1; {}=abc; {}=zzz",
                board_session_cookie_name(pid),
                board_session_cookie_name(Uuid::from_u128(8))
            ))
            .unwrap(),
        );
        assert_eq!(session_cookie_token(&h, pid).as_deref(), Some("abc"));
        assert!(session_cookie_token(&HeaderMap::new(), pid).is_none());
    }
}
