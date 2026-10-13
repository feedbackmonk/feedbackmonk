#![allow(clippy::unused_async)] // axum handlers must be async even when the body has no .await branch
#![allow(clippy::doc_markdown)] // module-doc references HTTP paths / cookie names verbatim

//! Hosted-board sign-in handoff and board session — Contract C43 (FR-FBR-43/44,
//! DEC-FBR-21). Wire contract: `docs/planning/handoffs/prioritization-contracts.md`.
//!
//! **Contract stub.** The `contracts` piece froze the names, wire shapes and
//! helper signatures below; the `public-api` piece writes the handler bodies
//! and registers the routes inside `board::board_router` (so `build_app` and
//! the `host-tenant-binding` classification do not change):
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
//! no Domain (host-only); Max-Age ≤ 8 h. A board viewer is then resolved by
//! [`resolve_board_viewer`]: a bearer JWT first, else the session cookie.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use feedbackmonk_repository::ProjectScope;

use crate::error::ApiError;
use crate::handlers::voting_common::hex_encode;
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

/// Resolve the board viewer: a valid `Authorization: Bearer` JWT (audience =
/// project) wins; else a live board-session cookie for this project; else
/// anonymous. An invalid bearer token is an error, never a silent downgrade.
///
/// Contract stub: returns `Anonymous` until `public-api` implements it.
pub async fn resolve_board_viewer(
    state: &AppState,
    scope: &ProjectScope,
    headers: &HeaderMap,
) -> Result<BoardViewer, ApiError> {
    let _ = (state, scope, headers);
    Ok(BoardViewer::Anonymous)
}

/// `POST …/board/session` — the one-time token exchange (FR-FBR-44).
/// Contract stub: 501 until `public-api` implements it.
pub async fn exchange_board_session(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(req): Json<BoardSessionExchangeRequest>,
) -> Response {
    let _ = (state, project_id, req);
    StatusCode::NOT_IMPLEMENTED.into_response()
}

/// `GET …/board/session` — whether this browser holds a live session.
/// Contract stub: 501 until `public-api` implements it.
pub async fn get_board_session(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let _ = (state, project_id, headers);
    StatusCode::NOT_IMPLEMENTED.into_response()
}

/// `DELETE …/board/session` — sign out of the board (ends the session and
/// clears the cookie). Contract stub: 501 until `public-api` implements it.
pub async fn end_board_session(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let _ = (state, project_id, headers);
    StatusCode::NOT_IMPLEMENTED.into_response()
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
}
