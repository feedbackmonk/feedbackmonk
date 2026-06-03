//! `POST /api/v1/projects/{project_id}/feedback` -- public submission
//! endpoint (FR-FBR-03 + FR-FBR-05 + FR-FBR-06; Contract C3).
//!
//! ## Auth-mode dispatch
//!
//! The handler examines the `Authorization` header:
//!
//! - Present (`Authorization: Bearer <token>`) -> auth mode. The token is
//!   verified via `feedbackmonk_jwt::verify` against the project's active
//!   signing keys; on success, `submit_authenticated` writes a row with
//!   `end_user_sub` populated.
//! - Absent -> anonymous mode. The handler reads (or mints) an
//!   `X-Feedbackmonk-Anon-Cookie` cookie, computes `token_hash(ip, cookie,
//!   project_id)`, and asks the rate-limit gate; on success,
//!   `submit_anonymous` writes a row with `anon_token_hash` populated.
//!
//! ## Project scope (DEC-PODS-001)
//!
//! The endpoint is public. There is no admin session and therefore no
//! `TenantScope`. `ProjectRepo::open_for_submission(project_id)` mints a
//! `ProjectScope` directly from the URL path's `project_id` -- the
//! allowlisted pre-auth-boundary method that resolves the project's owning
//! tenant inside the repository crate.
//!
//! ## Response shape (Contract C3)
//!
//! ```json
//! {
//!   "feedback_id": "FB-XXXXXX",
//!   "accepted_at": "2026-05-13T21:00:00Z",
//!   "echo": { "body": "...", "kind": "..." }
//! }
//! ```

use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Path, State};
use axum::http::header::{AUTHORIZATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_anon::{AnonGate, ANON_COOKIE_HEADER};
use feedbackmonk_core::{FeedbackKind, ResourceKind, Tier};
use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, JwtError, VerifiedClaims};

use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Hard cap on the feedback body. Matches the schema CHECK constraint on
/// `feedback.body` (1..=16384). Exceeding -> 413.
pub const MAX_BODY_CHARS: usize = 16384;

/// Cookie attributes for the minted anon cookie.
///
/// `SameSite=None; Secure` — the widget embeds **cross-site** (customer origin →
/// feedbackmonk API) and the anonymous path fetches with `credentials:
/// "include"`, so the cookie must be `SameSite=None` (sent in a third-party
/// context) and therefore `Secure` (the browser requires `Secure` with
/// `SameSite=None`). A `SameSite=Lax` cookie would be silently dropped
/// cross-site, disabling per-cookie dedup (FR-FBR-06) and leaving only IP-based
/// dedup. `HttpOnly` keeps it unreadable to page JS (privacy; the widget never
/// reads it). Path scoped to `/api/v1`; `Max-Age=30d`. See DEC-FBR-IMPL-09.
///
/// `Secure` requires a secure context: production/self-host run behind TLS, and
/// browsers treat `http://localhost` as secure for dev. A self-host deployment
/// served over plain HTTP on a non-localhost host would have the cookie dropped
/// by the browser; anon dedup then degrades to IP-only (submission still
/// succeeds). Browsers increasingly partition/expire third-party cookies; if
/// that erodes dedup materially, the long-term path is a header-carried anon
/// token instead of a cookie (deferred — see DEC-FBR-IMPL-09 alternatives).
const ANON_COOKIE_MAX_AGE_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Deserialize)]
pub struct FeedbackRequest {
    pub body: String,
    /// `bug | feature | question | other`. Defaults to `other` when absent.
    #[serde(default)]
    pub kind: Option<String>,
    /// Anonymous-mode only -- ignored in auth mode (email is read from
    /// the verified JWT claims). Optional.
    #[serde(default)]
    pub email: Option<String>,
    /// External crash-event correlation key (parity Gap #2; e.g. GitCellar's
    /// Glitchtip event id). **Auth-mode only** — it comes from the signed-in
    /// Desktop context, so it is read from the request body ONLY when a
    /// verified JWT is present and ignored on the anonymous path. Persisted as
    /// a first-class `feedback.crash_event_id` column (NOT `external_metadata`).
    /// Correlation to crash detail is best-effort/off-path (see
    /// `crash_correlation`); storing the link never blocks or fails a submit.
    #[serde(default)]
    pub crash_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackResponse {
    pub feedback_id: String,
    pub accepted_at: chrono::DateTime<Utc>,
    pub echo: FeedbackEcho,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackEcho {
    pub body: String,
    pub kind: &'static str,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `POST /api/v1/projects/{project_id}/feedback`.
///
/// Returns `ApiError` for non-JWT failures (404, 413, 400, 500). JWT failure
/// produces an explicit 401 with `{"error": "<JwtError variant>"}` body so
/// integrations can disambiguate (`BadSignature`, `Expired`, `WrongAudience`, ...).
/// Anon-mode rate-limit produces 429 with `Retry-After` header.
pub async fn submit(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<FeedbackRequest>,
) -> Result<Response, ApiError> {
    // ----- 1. Body validation (Contract C3) --------------------------------
    let kind = parse_kind(req.kind.as_deref())?;
    validate_body(&req.body)?;

    // ----- 2. Project scope (DEC-PODS-001) ---------------------------------
    let project_scope = state.projects.open_for_submission(project_id).await?;

    // ----- 2b. Tier-cap predicate (FR-FBR-14, Contract C17) ----------------
    // ProjectScope embeds the tenant; consult the tier-cap predicate
    // before any write. Public submission endpoint -> 402 Payment
    // Required when the rolling-30d cap fires (Contract C18).
    let cap = state
        .tier_quotas
        .check_tier_quota(project_scope.tenant(), ResourceKind::FeedbackInRollingMonth)
        .await?;
    if !cap.allowed {
        return Err(ApiError::TierCapExceeded {
            tier: cap.tier,
            resource: cap.resource,
            current: cap.current,
            limit: cap.limit.unwrap_or(0),
            upgrade_hint: upgrade_hint_for_feedback(cap.tier),
        });
    }

    // ----- 3. Auth-mode dispatch -------------------------------------------
    if let Some(token) = extract_bearer(&headers) {
        submit_authenticated_path(
            &state,
            &project_scope,
            &token,
            project_id,
            req.crash_event_id.as_deref(),
            &req.body,
            kind,
        )
        .await
    } else {
        let client_ip = addr.ip().to_string();
        submit_anonymous_path(
            &state,
            &project_scope,
            project_id,
            &client_ip,
            &headers,
            req.email.as_deref(),
            &req.body,
            kind,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Auth-mode path
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn submit_authenticated_path(
    state: &AppState,
    project_scope: &feedbackmonk_repository::ProjectScope,
    token: &str,
    project_id: Uuid,
    crash_event_id: Option<&str>,
    body: &str,
    kind: FeedbackKind,
) -> Result<Response, ApiError> {
    let active_keys = state.signing_keys.list_active(project_scope).await?;
    let now_unix = current_unix_timestamp();

    let claims: VerifiedClaims = match jwt_verify_with_leeway(
        token,
        project_id,
        &active_keys,
        now_unix,
        state.jwt_iat_leeway_seconds,
    ) {
        Ok(c) => c,
        Err(e) => return Ok(jwt_error_response(&e)),
    };

    let feedback_id = state
        .feedback
        .submit_authenticated(
            project_scope,
            &claims.sub,
            claims.email.as_deref(),
            claims.name.as_deref(),
            claims.external_metadata.as_ref(),
            crash_event_id,
            body,
            kind,
        )
        .await?;

    Ok(success_response(feedback_id.as_str(), body, kind, None))
}

// ---------------------------------------------------------------------------
// Anonymous-mode path
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn submit_anonymous_path(
    state: &AppState,
    project_scope: &feedbackmonk_repository::ProjectScope,
    project_id: Uuid,
    client_ip: &str,
    headers: &HeaderMap,
    optional_email: Option<&str>,
    body: &str,
    kind: FeedbackKind,
) -> Result<Response, ApiError> {
    let (cookie_value, set_cookie_header) = resolve_anon_cookie(headers);
    let token_hash = AnonGate::token_hash(client_ip, &cookie_value, project_id);

    match state.anon_gate.check(&token_hash, project_id) {
        Ok(_) => {}
        Err(feedbackmonk_anon::RateLimitError::Exceeded {
            retry_after_seconds,
        }) => return Ok(rate_limited_response(retry_after_seconds)),
    }

    let feedback_id = state
        .feedback
        .submit_anonymous(project_scope, &token_hash, optional_email, body, kind)
        .await?;

    Ok(success_response(
        feedback_id.as_str(),
        body,
        kind,
        set_cookie_header,
    ))
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn parse_kind(s: Option<&str>) -> Result<FeedbackKind, ApiError> {
    Ok(match s {
        None | Some("" | "other") => FeedbackKind::Other,
        Some("bug") => FeedbackKind::Bug,
        Some("feature") => FeedbackKind::Feature,
        Some("question") => FeedbackKind::Question,
        Some(unknown) => {
            return Err(ApiError::BadRequest(format!(
                "kind must be one of bug|feature|question|other; got {unknown:?}"
            )));
        }
    })
}

fn validate_body(body: &str) -> Result<(), ApiError> {
    let len = body.chars().count();
    if len == 0 {
        return Err(ApiError::BadRequest("body must be non-empty".into()));
    }
    if len > MAX_BODY_CHARS {
        // 413 Payload Too Large per Contract C3.
        return Err(ApiError::PayloadTooLarge(format!(
            "body exceeds {MAX_BODY_CHARS} characters"
        )));
    }
    Ok(())
}

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let stripped = value.strip_prefix("Bearer ")?;
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

/// Read the anon cookie from `X-Feedbackmonk-Anon-Cookie` request header. If
/// absent, mint a fresh cookie value and return a `Set-Cookie` header for
/// the response.
fn resolve_anon_cookie(headers: &HeaderMap) -> (String, Option<HeaderValue>) {
    if let Some(existing) = headers.get(ANON_COOKIE_HEADER).and_then(|v| v.to_str().ok()) {
        if !existing.is_empty() {
            return (existing.to_string(), None);
        }
    }
    let minted = AnonGate::mint_cookie();
    let set_cookie = format!(
        "{ANON_COOKIE_HEADER}={minted}; Path=/api/v1; Max-Age={ANON_COOKIE_MAX_AGE_SECONDS}; HttpOnly; Secure; SameSite=None"
    );
    let header_value = HeaderValue::from_str(&set_cookie).ok();
    (minted, header_value)
}

/// User-facing upgrade hint when the feedback rolling-window cap fires.
/// Free/Starter/Pro carry distinct copy; `SelfHost` is unreachable
/// (None cap) but kept exhaustive.
fn upgrade_hint_for_feedback(tier: Tier) -> String {
    match tier {
        Tier::Free => "Upgrade to Starter for 500 feedback per month.".into(),
        Tier::Starter => "Upgrade to Pro for 10,000 feedback per month.".into(),
        Tier::Pro => "Contact support — you've hit the Pro monthly cap.".into(),
        Tier::SelfHost => {
            "Contact support — SelfHost should not have a monthly cap.".into()
        }
    }
}

fn current_unix_timestamp() -> i64 {
    #[allow(clippy::cast_possible_wrap)]
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Response builders
// ---------------------------------------------------------------------------

fn success_response(
    feedback_id: &str,
    body: &str,
    kind: FeedbackKind,
    set_cookie: Option<HeaderValue>,
) -> Response {
    let resp = FeedbackResponse {
        feedback_id: feedback_id.to_string(),
        accepted_at: Utc::now(),
        echo: FeedbackEcho {
            body: body.to_string(),
            kind: kind.as_str(),
        },
    };
    let mut response = (StatusCode::OK, Json(resp)).into_response();
    if let Some(cookie) = set_cookie {
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    response
}

fn jwt_error_response(err: &JwtError) -> Response {
    let body = Json(json!({ "error": err.variant_name() }));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

fn rate_limited_response(retry_after_seconds: u64) -> Response {
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

// ---------------------------------------------------------------------------
// Router subtree -- composed into main router by Worker A's router::router()
// ---------------------------------------------------------------------------

/// Worker B's submission subtree. Exposed for `router.rs` to `.merge()`.
pub fn submission_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route(
            "/api/v1/projects/:project_id/feedback",
            axum::routing::post(submit),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Unit tests (helpers only -- handler integration tests live in
// `tests/feedback_integration.rs` once a test harness is wired)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn hdr(name: &str, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
        h
    }

    #[test]
    fn parse_kind_known_values() {
        assert_eq!(parse_kind(Some("bug")).unwrap(), FeedbackKind::Bug);
        assert_eq!(parse_kind(Some("feature")).unwrap(), FeedbackKind::Feature);
        assert_eq!(
            parse_kind(Some("question")).unwrap(),
            FeedbackKind::Question
        );
        assert_eq!(parse_kind(Some("other")).unwrap(), FeedbackKind::Other);
        assert_eq!(parse_kind(None).unwrap(), FeedbackKind::Other);
        assert_eq!(parse_kind(Some("")).unwrap(), FeedbackKind::Other);
    }

    #[test]
    fn parse_kind_rejects_unknown() {
        let err = parse_kind(Some("rant")).unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[test]
    fn validate_body_rejects_empty() {
        let err = validate_body("").unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[test]
    fn validate_body_rejects_oversize() {
        let big = "x".repeat(MAX_BODY_CHARS + 1);
        let err = validate_body(&big).unwrap_err();
        assert!(matches!(err, ApiError::PayloadTooLarge(_)));
    }

    #[test]
    fn validate_body_accepts_at_cap() {
        let cap = "x".repeat(MAX_BODY_CHARS);
        validate_body(&cap).unwrap();
        validate_body("one char").unwrap();
    }

    #[test]
    fn extract_bearer_present() {
        let h = hdr("Authorization", "Bearer abc.def.ghi");
        assert_eq!(extract_bearer(&h).as_deref(), Some("abc.def.ghi"));
    }

    #[test]
    fn extract_bearer_missing_prefix() {
        let h = hdr("Authorization", "abc.def.ghi");
        assert_eq!(extract_bearer(&h), None);
    }

    #[test]
    fn extract_bearer_no_header() {
        let h = HeaderMap::new();
        assert_eq!(extract_bearer(&h), None);
    }

    #[test]
    fn extract_bearer_empty_token() {
        let h = hdr("Authorization", "Bearer ");
        assert_eq!(extract_bearer(&h), None);
    }

    #[test]
    fn resolve_anon_cookie_uses_existing_when_present() {
        let h = hdr(ANON_COOKIE_HEADER, "my-cookie-xyz");
        let (cookie, set) = resolve_anon_cookie(&h);
        assert_eq!(cookie, "my-cookie-xyz");
        assert!(set.is_none(), "no Set-Cookie when cookie already present");
    }

    #[test]
    fn resolve_anon_cookie_mints_when_absent() {
        let h = HeaderMap::new();
        let (cookie, set) = resolve_anon_cookie(&h);
        assert_eq!(cookie.len(), 22, "22-char base64url-no-pad");
        let set_value = set.expect("Set-Cookie must be emitted").to_str().unwrap().to_string();
        assert!(set_value.contains("HttpOnly"));
        // Cross-site embed: cookie must be SameSite=None; Secure (not Lax), else
        // the browser drops it on the credentialed cross-origin submit.
        assert!(set_value.contains("SameSite=None"));
        assert!(set_value.contains("Secure"));
        assert!(!set_value.contains("SameSite=Lax"));
        assert!(set_value.contains(&format!("Max-Age={ANON_COOKIE_MAX_AGE_SECONDS}")));
        assert!(set_value.contains(&cookie));
    }

    #[test]
    fn current_unix_timestamp_is_recent() {
        let t = current_unix_timestamp();
        // Sanity: not at epoch, not absurdly far in the future.
        assert!(t > 1_700_000_000);
        assert!(t < 4_000_000_000);
    }
}
