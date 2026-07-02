//! Admin account-recovery surface (security scrutiny finding P1-1).
//!
//! feedbackmonk is a SOLE, no-fallback backend for its admins: before this
//! module there was no password reset, no verify-email resend, no logout, and
//! no session revocation. Consequences P1-1 called out:
//!   - a forgotten password permanently locked an admin out of their entire
//!     feedback corpus;
//!   - a single SMTP failure at signup bricked the account forever (committed
//!     unverified + 409 on re-signup + no resend);
//!   - a leaked 7-day session cookie had no kill switch short of rotating the
//!     global `FEEDBACKMONK_SESSION_SECRET` (which logs out ALL tenants).
//!
//! Endpoints (all mint/verify against the per-tenant `session_epoch`,
//! migration 00023):
//!   - `POST /api/v1/logout`                  (`AdminSession`) — revoke + clear cookie. 200.
//!   - `POST /api/v1/password-reset/request`  — email a reset link. ALWAYS 202.
//!   - `POST /api/v1/password-reset/confirm`  — set new password + revoke sessions. 200 / 400 / 401.
//!   - `POST /api/v1/verify-email/resend`     — re-send verify link. ALWAYS 202.
//!
//! ## Enumeration resistance
//!
//! `request` and `resend` ALWAYS return 202 whether or not an account exists in
//! the required state (verified for reset, pending for resend). No timing
//! equalizer is added: unlike login, these paths do NO argon2 work on the hit
//! branch, so the observable timing difference is a cheap DB write + an async
//! (fire-and-forget-logged) email send — not an argon2 verify.
//!
//! ## Rate limiting (email-bomb guard)
//!
//! Both public, email-triggering endpoints reuse the existing `LoginGate`
//! (keyed IP+email, per-minute), mirroring `handlers/login.rs`. An attacker
//! cannot use them to flood a victim's inbox or our SMTP relay.
//!
//! ## Token-at-rest posture (defense-in-depth, P2-16 sibling)
//!
//! Password-reset tokens are stored as `sha256(token)` hex digests
//! (`password_resets.token_hash`), so a read of that table cannot be replayed
//! to seize an account. Verify-email tokens keep the existing raw-token storage
//! (they only prove email control, and the flow already exists that way).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use feedbackmonk_anon::{LoginGate, RateLimitError};
use feedbackmonk_repository::{EmailVerificationRepo, PasswordResetRepo, TenantRepo};

use crate::auth::password::hash_password;
use crate::auth::session::clear_session_cookie;
use crate::auth::AdminSession;
use crate::email::Mailer;
use crate::error::ApiError;
use crate::state::AppState;

/// Minimum/maximum admin password length. MUST match `handlers/signup.rs`
/// (`PASSWORD_MIN_LEN` / `PASSWORD_MAX_LEN`) so a reset cannot set a password
/// signup would have rejected (or vice-versa).
const PASSWORD_MIN_LEN: usize = 8;
const PASSWORD_MAX_LEN: usize = 256;

/// Dedicated state for the reset/resend endpoints. Its own type (NOT `AppState`)
/// so the new `password_resets` repo + reset TTL do not ripple through the many
/// `AppState { … }` construction sites — the same pattern as `AttachmentState`
/// and `MeFeedbackDataState`. Logout runs on `AppState` (it needs `AdminSession`)
/// and is wired into the Worker A router directly.
#[derive(Clone)]
pub struct AccountRecoveryState {
    pub tenants: Arc<dyn TenantRepo>,
    pub password_resets: Arc<dyn PasswordResetRepo>,
    pub email_verifications: Arc<dyn EmailVerificationRepo>,
    pub mailer: Arc<dyn Mailer>,
    pub login_gate: LoginGate,
    /// Customer-facing base URL used in reset/verify links (no trailing slash).
    pub public_url: Arc<str>,
    /// TTL for password-reset tokens (`FEEDBACKMONK_RESET_TOKEN_TTL_HOURS`).
    pub reset_token_ttl: Duration,
    /// TTL for re-sent verify-email tokens (`FEEDBACKMONK_VERIFY_TOKEN_TTL_HOURS`).
    pub verify_token_ttl: Duration,
}

/// The reset/resend subtree. Logout is wired separately (see `logout`).
pub fn account_recovery_router(state: AccountRecoveryState) -> Router {
    Router::new()
        .route("/api/v1/password-reset/request", post(password_reset_request))
        .route("/api/v1/password-reset/confirm", post(password_reset_confirm))
        .route("/api/v1/verify-email/resend", post(verify_email_resend))
        .with_state(state)
}

// ----- request bodies ---------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct EmailBody {
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct ResetConfirmBody {
    pub token: String,
    pub new_password: String,
}

// ----- helpers ----------------------------------------------------------------

/// Mint a 32-byte random token, base64url-encoded (43 chars). Mirrors
/// `signup::generate_token`.
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// sha256 hex digest of the wire token — the at-rest form for reset tokens.
fn sha256_hex(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut s = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// 429 with a `Retry-After` header (mirrors `handlers/login.rs`).
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

/// The generic 202 both email-triggering endpoints return regardless of whether
/// an account exists (enumeration resistance).
fn accepted() -> Response {
    (
        StatusCode::ACCEPTED,
        Json(json!({ "message": "if an account matches, an email has been sent" })),
    )
        .into_response()
}

// ----- logout (AppState) ------------------------------------------------------

/// `POST /api/v1/logout` — bump the tenant's `session_epoch` (revokes every
/// outstanding session for the tenant — correct for the single-admin model)
/// AND clear the client cookie. AdminSession-gated: only a currently-valid
/// session can log out. 200.
pub async fn logout(
    session: AdminSession,
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    state.tenants.bump_session_epoch(&session.scope).await?;
    let jar = jar.add(clear_session_cookie());
    Ok((jar, Json(json!({ "message": "logged out" }))).into_response())
}

// ----- password reset request -------------------------------------------------

pub async fn password_reset_request(
    State(state): State<AccountRecoveryState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<EmailBody>,
) -> Result<Response, ApiError> {
    let email = req.email.trim().to_ascii_lowercase();

    // Email-bomb guard (IP+email, per-minute). Mirror login's pre-work throttle.
    let key = LoginGate::key_hash(&addr.ip().to_string(), &email);
    if let Err(RateLimitError::Exceeded { retry_after_seconds }) = state.login_gate.check(&key) {
        return Ok(rate_limited_response(retry_after_seconds));
    }

    // Only mint+send for an EXISTING, VERIFIED tenant. Everything else silently
    // no-ops to the same 202 (no enumeration).
    if let Some(tenant) = state.tenants.find_by_email(&email).await? {
        if tenant.verified_at.is_some() {
            let scope = state.tenants.scope_for(tenant.id).await?;
            let token = generate_token();
            let digest = sha256_hex(&token);
            let expires_at = Utc::now() + state.reset_token_ttl;
            state.password_resets.create(&scope, &digest, expires_at).await?;

            let link = format!("{}/reset-password?token={token}", state.public_url);
            if let Err(e) = state.mailer.send_password_reset_email(&email, &link).await {
                // Do NOT surface send failure to the caller (that would leak
                // account existence + is an email-independent transient).
                tracing::error!(error = %e, tenant_id = %tenant.id, "password reset email send failed");
            }
        }
    }

    Ok(accepted())
}

// ----- password reset confirm -------------------------------------------------

pub async fn password_reset_confirm(
    State(state): State<AccountRecoveryState>,
    Json(req): Json<ResetConfirmBody>,
) -> Result<Response, ApiError> {
    let token = req.token.trim();
    if token.is_empty() {
        return Err(ApiError::BadRequest("token must not be empty".into()));
    }
    if req.new_password.len() < PASSWORD_MIN_LEN || req.new_password.len() > PASSWORD_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "password must be {PASSWORD_MIN_LEN}..={PASSWORD_MAX_LEN} chars"
        )));
    }

    let digest = sha256_hex(token);
    // Unknown / expired / already-used token all fail identically as 401 — the
    // token IS the credential.
    let reset = state
        .password_resets
        .redeem(&digest)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    let now = Utc::now();
    if reset.used_at.is_some() || now > reset.expires_at {
        return Err(ApiError::Unauthorized);
    }

    let scope = state.tenants.scope_for(reset.tenant_id).await?;
    let new_hash = hash_password(&req.new_password)?;

    // Order: set the new hash, burn the token (single-use), then revoke every
    // outstanding session (epoch bump) so a leaked pre-reset cookie is dead.
    state.tenants.set_password(&scope, &new_hash).await?;
    state.password_resets.mark_used(&scope, &digest).await?;
    state.tenants.bump_session_epoch(&scope).await?;

    Ok((StatusCode::OK, Json(json!({ "message": "password updated" }))).into_response())
}

// ----- verify-email resend ----------------------------------------------------

pub async fn verify_email_resend(
    State(state): State<AccountRecoveryState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<EmailBody>,
) -> Result<Response, ApiError> {
    let email = req.email.trim().to_ascii_lowercase();

    let key = LoginGate::key_hash(&addr.ip().to_string(), &email);
    if let Err(RateLimitError::Exceeded { retry_after_seconds }) = state.login_gate.check(&key) {
        return Ok(rate_limited_response(retry_after_seconds));
    }

    // Only re-send for an EXISTING, PENDING (unverified) tenant. Verified or
    // unknown → silent 202 (no enumeration; no point re-verifying).
    if let Some(tenant) = state.tenants.find_by_email(&email).await? {
        if tenant.verified_at.is_none() {
            let scope = state.tenants.scope_for(tenant.id).await?;
            let token = generate_token();
            let expires_at = Utc::now() + state.verify_token_ttl;
            state.email_verifications.create(&scope, &token, expires_at).await?;

            let link = format!("{}/verify-email?token={token}", state.public_url);
            if let Err(e) = state.mailer.send_verify_email(&email, &link).await {
                tracing::error!(error = %e, tenant_id = %tenant.id, "verify email resend failed");
            }
        }
    }

    Ok(accepted())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_is_stable_and_64_hex_chars() {
        let a = sha256_hex("token-abc");
        let b = sha256_hex("token-abc");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(sha256_hex("token-abc"), sha256_hex("token-abd"));
    }

    #[test]
    fn generated_token_is_43_char_base64url() {
        let t = generate_token();
        assert_eq!(t.len(), 43);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(generate_token(), generate_token());
    }
}
