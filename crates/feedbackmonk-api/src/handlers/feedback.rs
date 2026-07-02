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
//!   "echo": { "body": "...", "sentiment": null, "severity": null, "kind": "..." }
//! }
//! ```
//!
//! ## Phase A A4 — severity + Idempotency-Key
//!
//! - `severity` (optional body field, BOTH modes): `low|medium|high|blocker`;
//!   unrecognized ⇒ 400; echoed back (Phase A A4a, D-A4).
//! - `Idempotency-Key` (optional request header, BOTH modes): a retry carrying
//!   the same key returns `200` with the ORIGINAL `feedback_id` and creates NO
//!   new row (first-write-wins; dedupe is transactional in the repository —
//!   Phase A A4b, migration 00021). The clustering-on-submit hook is skipped
//!   on a dedupe hit (no new row to cluster).

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
use feedbackmonk_core::{FeedbackKind, KeyClass, ResourceKind, Sentiment, Severity};
use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, JwtError, VerifiedClaims};

use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Hard cap on the feedback body. Matches the schema CHECK constraint on
/// `feedback.body` (1..=16384). Exceeding -> 413.
pub const MAX_BODY_CHARS: usize = 16384;

/// Max length of a client-supplied anon-mode email (scrutiny P2-6). RFC 5321
/// caps an address at 254 chars; anything longer (or malformed) is rejected
/// with a 400 rather than stored as attacker-chosen, unvalidated text.
pub const MAX_ANON_EMAIL_CHARS: usize = 254;

/// Max length of the external crash-event correlation key (scrutiny P2-6,
/// Contract §5.6). A correlation id is a short opaque token; an unbounded value
/// is rejected with a 400.
pub const MAX_CRASH_EVENT_ID_CHARS: usize = 128;

#[derive(Debug, Clone, Deserialize)]
pub struct FeedbackRequest {
    /// The free-text feedback body. OPTIONAL since FR-FBR-28: a sentiment-only
    /// submission (no body) is valid. Absent/null/empty ⇒ no body. At least one
    /// of `body` / `sentiment` must be present, else `400`.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional 3-point satisfaction signal: `negative | neutral | positive`
    /// (FR-FBR-28). Absent ⇒ no sentiment. An unrecognized value ⇒ `400`.
    #[serde(default)]
    pub sentiment: Option<String>,
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
    /// Optional 4-point impact signal: `low | medium | high | blocker`
    /// (Phase A A4a, D-A4). Tenant-generic and valid in BOTH auth and anon
    /// modes (unlike `crash_event_id`, which is auth-only). Absent ⇒ no
    /// severity. An unrecognized value ⇒ `400`.
    #[serde(default)]
    pub severity: Option<String>,
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
    /// Echoes the stored sentiment, or `null` when none was given (FR-FBR-28).
    pub sentiment: Option<Sentiment>,
    /// Echoes the stored severity, or `null` when none was given (Phase A A4a).
    pub severity: Option<Severity>,
    pub kind: &'static str,
}

/// Request header carrying the client's submit-dedupe key (Phase A A4b).
/// Any non-empty value is honored verbatim; absent/empty ⇒ no dedupe. A retry
/// with the same key returns the ORIGINAL `feedback_id` with no new row.
pub const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";

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
    // ----- 1. Body + sentiment + severity validation (Contract C3 /
    //          FR-FBR-28 / Phase A A4a) --------------------------------------
    let kind = parse_kind(req.kind.as_deref())?;
    let sentiment = parse_sentiment(req.sentiment.as_deref())?;
    let severity = parse_severity(req.severity.as_deref())?;
    let body = req.body.as_deref().unwrap_or("");
    validate_submission(body, sentiment)?;
    // Scrutiny P2-6: cap the external crash-event correlation key (auth-mode
    // field, but validated here regardless — anon ignores it downstream).
    validate_crash_event_id(req.crash_event_id.as_deref())?;

    // Phase A A4b: optional client dedupe key (exactly-once on retry).
    let idempotency_key = extract_idempotency_key(&headers);

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
        // Scrutiny P2-5: this is the PUBLIC, unauthenticated submit endpoint.
        // Return a BARE 402 that discloses nothing about the tenant — no
        // `current` / `limit` / `tier` / upgrade copy (all of which leak the
        // tenant's volume and plan to an anonymous submitter). The detailed,
        // structured tier body (Contract C18) is reserved for the
        // ADMIN-authenticated `GET /api/v1/admin/tier` endpoint.
        let body = Json(json!({ "error": "tier_cap_exceeded" }));
        return Ok((StatusCode::PAYMENT_REQUIRED, body).into_response());
    }

    // ----- 3. Auth-mode dispatch -------------------------------------------
    if let Some(token) = extract_bearer(&headers) {
        submit_authenticated_path(
            &state,
            &project_scope,
            &token,
            project_id,
            req.crash_event_id.as_deref(),
            body,
            sentiment,
            severity,
            kind,
            idempotency_key.as_deref(),
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
            body,
            sentiment,
            severity,
            kind,
            idempotency_key.as_deref(),
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
    sentiment: Option<Sentiment>,
    severity: Option<Severity>,
    kind: FeedbackKind,
    idempotency_key: Option<&str>,
) -> Result<Response, ApiError> {
    // P5b (C25): end-user JWT verification selects ONLY identity-class keys — a
    // runner-class key can never authenticate a submitter (privilege separation).
    let active_keys = state
        .signing_keys
        .list_active_for_class(project_scope, KeyClass::Identity)
        .await?;
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

    let outcome = state
        .feedback
        .submit_authenticated_full(
            project_scope,
            &claims.sub,
            claims.email.as_deref(),
            claims.name.as_deref(),
            claims.external_metadata.as_ref(),
            crash_event_id,
            body,
            sentiment,
            severity,
            kind,
            idempotency_key,
        )
        .await?;

    // A4b: a dedupe hit created NO new row — the clustering-on-submit hook
    // must not run for it (there is nothing new to cluster).
    if !outcome.deduped {
        cluster_on_submit_best_effort(state, project_scope, &outcome.feedback_id, body, kind)
            .await;
    }

    Ok(success_response(
        outcome.feedback_id.as_str(),
        body,
        sentiment,
        severity,
        kind,
        None,
    ))
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
    sentiment: Option<Sentiment>,
    severity: Option<Severity>,
    kind: FeedbackKind,
    idempotency_key: Option<&str>,
) -> Result<Response, ApiError> {
    // Scrutiny P2-6: validate the client-supplied email shape before it is
    // stored (attacker-chosen, unvalidated email = impersonation vector). Only
    // a non-empty value is validated; an absent/empty email means "no email".
    if let Some(email) = optional_email {
        if !email.is_empty() {
            validate_anon_email(email)?;
        }
    }

    let (cookie_value, set_cookie_header) = resolve_anon_cookie(headers);
    let token_hash = AnonGate::token_hash(client_ip, &cookie_value, project_id);

    match state.anon_gate.check(&token_hash, project_id) {
        Ok(_) => {}
        Err(feedbackmonk_anon::RateLimitError::Exceeded {
            retry_after_seconds,
        }) => return Ok(rate_limited_response(retry_after_seconds)),
    }

    let outcome = state
        .feedback
        .submit_anonymous_full(
            project_scope,
            &token_hash,
            optional_email,
            body,
            sentiment,
            severity,
            kind,
            idempotency_key,
        )
        .await?;

    // A4b: a dedupe hit created NO new row — skip the clustering hook.
    if !outcome.deduped {
        cluster_on_submit_best_effort(state, project_scope, &outcome.feedback_id, body, kind)
            .await;
    }

    Ok(success_response(
        outcome.feedback_id.as_str(),
        body,
        sentiment,
        severity,
        kind,
        set_cookie_header,
    ))
}

// ---------------------------------------------------------------------------
// FR-FBR-19 clustering-on-submit hook (best-effort, post-insert)
// ---------------------------------------------------------------------------

/// Assign the just-accepted feedback to a cluster (FR-FBR-19). **Best-effort**:
/// a clustering failure is logged and NEVER fails an already-accepted submit
/// (`feedback.cluster_id` is nullable and a later sweep re-clusters — MSG-003
/// Q1). Adds no response-shape change and only the deterministic heuristic's
/// cost (the public submit path stays CORS-exposed; clustering adds NO new
/// external surface). The assignment itself is atomic (see
/// `clusters::assign_cluster_on_submit`).
async fn cluster_on_submit_best_effort(
    state: &AppState,
    project_scope: &feedbackmonk_repository::ProjectScope,
    feedback_id: &feedbackmonk_core::FeedbackId,
    body: &str,
    kind: FeedbackKind,
) {
    if let Err(e) =
        crate::handlers::clusters::assign_cluster_on_submit(state, project_scope, feedback_id, body, kind)
            .await
    {
        tracing::warn!(
            target: "clustering",
            feedback_id = %feedback_id,
            error = %e,
            "clustering-on-submit failed (submit still accepted; cluster_id left NULL for a sweep to assign)"
        );
    }
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

/// Parse the optional `sentiment` field. Absent / empty ⇒ `None`. An
/// unrecognized value ⇒ `400` (FR-FBR-28).
fn parse_sentiment(s: Option<&str>) -> Result<Option<Sentiment>, ApiError> {
    match s {
        None | Some("") => Ok(None),
        Some(v) => Sentiment::parse(v).map(Some).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "sentiment must be one of negative|neutral|positive; got {v:?}"
            ))
        }),
    }
}

/// Parse the optional `severity` field (Phase A A4a). Absent / empty ⇒
/// `None`. An unrecognized value ⇒ `400`. Mirrors [`parse_sentiment`].
fn parse_severity(s: Option<&str>) -> Result<Option<Severity>, ApiError> {
    match s {
        None | Some("") => Ok(None),
        Some(v) => Severity::parse(v).map(Some).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "severity must be one of low|medium|high|blocker; got {v:?}"
            ))
        }),
    }
}

/// Read the optional `Idempotency-Key` request header (Phase A A4b). The value
/// is honored VERBATIM (no trimming — the key is an opaque client token);
/// absent, non-UTF-8, or empty ⇒ `None` (no dedupe, today's behavior).
fn extract_idempotency_key(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(IDEMPOTENCY_KEY_HEADER)?.to_str().ok()?;
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

/// Validate the (body, sentiment) pair (Contract C3 / FR-FBR-28):
/// - body, when present, must be ≤ `MAX_BODY_CHARS` (413 otherwise);
/// - at least one of a non-empty body or a sentiment must be present (400
///   otherwise) — a fully-empty submission is rejected. The DB
///   `feedback_body_or_sentiment_check` is the backstop.
fn validate_submission(body: &str, sentiment: Option<Sentiment>) -> Result<(), ApiError> {
    let len = body.chars().count();
    if len > MAX_BODY_CHARS {
        // 413 Payload Too Large per Contract C3.
        return Err(ApiError::PayloadTooLarge(format!(
            "body exceeds {MAX_BODY_CHARS} characters"
        )));
    }
    if len == 0 && sentiment.is_none() {
        return Err(ApiError::BadRequest(
            "a submission must include a body or a sentiment".into(),
        ));
    }
    Ok(())
}

/// Validate a client-supplied anon-mode email (scrutiny P2-6). Basic RFC-ish
/// shape check — NOT deliverability: bounded length, exactly one `@`, non-empty
/// local + domain parts, a dotted domain, and no embedded whitespace. Invalid ⇒
/// 400 (we reject rather than silently drop, so the submitter learns their
/// address was malformed instead of a bad address being stored as a
/// impersonation-grade attacker-chosen string). Only called for a non-empty,
/// anon-mode email; auth-mode email comes from the verified JWT and is untouched.
fn validate_anon_email(email: &str) -> Result<(), ApiError> {
    let invalid = || ApiError::BadRequest("email is not a valid address".into());
    if email.chars().count() > MAX_ANON_EMAIL_CHARS || email.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let mut parts = email.split('@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    // Exactly one '@' (a third `parts.next()` means two or more).
    if parts.next().is_some() || local.is_empty() || domain.is_empty() {
        return Err(invalid());
    }
    // Domain must have a dot with non-empty first/last labels.
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return Err(invalid());
    }
    Ok(())
}

/// Cap the external crash-event correlation key length (scrutiny P2-6). Absent
/// ⇒ ok; over [`MAX_CRASH_EVENT_ID_CHARS`] ⇒ 400. Legitimate correlation ids
/// (e.g. a Glitchtip event id) are well under the cap, so this is zero friction.
fn validate_crash_event_id(crash_event_id: Option<&str>) -> Result<(), ApiError> {
    if let Some(id) = crash_event_id {
        if id.chars().count() > MAX_CRASH_EVENT_ID_CHARS {
            return Err(ApiError::BadRequest(format!(
                "crash_event_id exceeds {MAX_CRASH_EVENT_ID_CHARS} characters"
            )));
        }
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
/// the response. The `Set-Cookie` attributes come from the SHARED
/// [`crate::handlers::anon_cookie`] helper so the submit + vote paths cannot
/// drift apart (scrutiny P2-3): `SameSite=None; Secure; HttpOnly` — the widget
/// embeds cross-site and fetches credentialed, so a `SameSite=Lax` cookie would
/// be dropped, degrading per-cookie dedup (FR-FBR-06). See DEC-FBR-IMPL-09.
fn resolve_anon_cookie(headers: &HeaderMap) -> (String, Option<HeaderValue>) {
    if let Some(existing) = headers.get(ANON_COOKIE_HEADER).and_then(|v| v.to_str().ok()) {
        if !existing.is_empty() {
            return (existing.to_string(), None);
        }
    }
    let minted = AnonGate::mint_cookie();
    let header_value = crate::handlers::anon_cookie::set_cookie_header(&minted);
    (minted, header_value)
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
    sentiment: Option<Sentiment>,
    severity: Option<Severity>,
    kind: FeedbackKind,
    set_cookie: Option<HeaderValue>,
) -> Response {
    let resp = FeedbackResponse {
        feedback_id: feedback_id.to_string(),
        accepted_at: Utc::now(),
        echo: FeedbackEcho {
            body: body.to_string(),
            sentiment,
            severity,
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
    fn validate_submission_rejects_fully_empty() {
        // Empty body AND no sentiment → 400 (FR-FBR-28).
        let err = validate_submission("", None).unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[test]
    fn validate_submission_allows_sentiment_only() {
        // Sentiment-only submission (no body) is valid (FR-FBR-28).
        validate_submission("", Some(Sentiment::Positive)).unwrap();
    }

    #[test]
    fn validate_submission_rejects_oversize() {
        let big = "x".repeat(MAX_BODY_CHARS + 1);
        let err = validate_submission(&big, None).unwrap_err();
        assert!(matches!(err, ApiError::PayloadTooLarge(_)));
        // Oversize body is 413 even when a sentiment is present.
        let err2 = validate_submission(&big, Some(Sentiment::Neutral)).unwrap_err();
        assert!(matches!(err2, ApiError::PayloadTooLarge(_)));
    }

    #[test]
    fn validate_submission_accepts_body_at_cap() {
        let cap = "x".repeat(MAX_BODY_CHARS);
        validate_submission(&cap, None).unwrap();
        validate_submission("one char", None).unwrap();
    }

    #[test]
    fn parse_sentiment_known_and_unknown() {
        assert_eq!(parse_sentiment(None).unwrap(), None);
        assert_eq!(parse_sentiment(Some("")).unwrap(), None);
        assert_eq!(parse_sentiment(Some("positive")).unwrap(), Some(Sentiment::Positive));
        assert_eq!(parse_sentiment(Some("negative")).unwrap(), Some(Sentiment::Negative));
        assert_eq!(parse_sentiment(Some("neutral")).unwrap(), Some(Sentiment::Neutral));
        assert!(matches!(parse_sentiment(Some("happy")), Err(ApiError::BadRequest(_))));
    }

    #[test]
    fn parse_severity_known_and_unknown() {
        assert_eq!(parse_severity(None).unwrap(), None);
        assert_eq!(parse_severity(Some("")).unwrap(), None);
        assert_eq!(parse_severity(Some("low")).unwrap(), Some(Severity::Low));
        assert_eq!(parse_severity(Some("medium")).unwrap(), Some(Severity::Medium));
        assert_eq!(parse_severity(Some("high")).unwrap(), Some(Severity::High));
        assert_eq!(parse_severity(Some("blocker")).unwrap(), Some(Severity::Blocker));
        // D-A4 chose `blocker`, not `critical`; case-sensitive like sentiment.
        assert!(matches!(parse_severity(Some("critical")), Err(ApiError::BadRequest(_))));
        assert!(matches!(parse_severity(Some("HIGH")), Err(ApiError::BadRequest(_))));
    }

    #[test]
    fn extract_idempotency_key_present_verbatim() {
        let h = hdr("Idempotency-Key", "retry-abc-123");
        assert_eq!(extract_idempotency_key(&h).as_deref(), Some("retry-abc-123"));
        // Header-name lookup is case-insensitive.
        let h2 = hdr("idempotency-key", " padded ");
        assert_eq!(
            extract_idempotency_key(&h2).as_deref(),
            Some(" padded "),
            "value is honored verbatim, never trimmed"
        );
    }

    #[test]
    fn extract_idempotency_key_absent_or_empty_is_none() {
        assert_eq!(extract_idempotency_key(&HeaderMap::new()), None);
        let h = hdr("Idempotency-Key", "");
        assert_eq!(extract_idempotency_key(&h), None);
    }

    #[test]
    fn validate_anon_email_accepts_reasonable_addresses() {
        validate_anon_email("user@example.com").unwrap();
        validate_anon_email("first.last+tag@sub.example.co.uk").unwrap();
    }

    #[test]
    fn validate_anon_email_rejects_malformed() {
        assert!(validate_anon_email("no-at-sign").is_err());
        assert!(validate_anon_email("two@@example.com").is_err());
        assert!(validate_anon_email("a@b@example.com").is_err());
        assert!(validate_anon_email("@example.com").is_err());
        assert!(validate_anon_email("user@").is_err());
        assert!(validate_anon_email("user@nodot").is_err());
        assert!(validate_anon_email("user@.com").is_err());
        assert!(validate_anon_email("user@example.").is_err());
        assert!(validate_anon_email("user name@example.com").is_err());
    }

    #[test]
    fn validate_anon_email_rejects_oversize() {
        let long = format!("{}@example.com", "x".repeat(MAX_ANON_EMAIL_CHARS));
        assert!(validate_anon_email(&long).is_err());
    }

    #[test]
    fn validate_crash_event_id_caps_length() {
        validate_crash_event_id(None).unwrap();
        validate_crash_event_id(Some("evt-abc-123")).unwrap();
        validate_crash_event_id(Some(&"x".repeat(MAX_CRASH_EVENT_ID_CHARS))).unwrap();
        assert!(matches!(
            validate_crash_event_id(Some(&"x".repeat(MAX_CRASH_EVENT_ID_CHARS + 1))),
            Err(ApiError::BadRequest(_))
        ));
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
        assert!(set_value.contains(&format!(
            "Max-Age={}",
            crate::handlers::anon_cookie::ANON_COOKIE_MAX_AGE_SECONDS
        )));
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
