//! `POST /api/v1/login` -- admin (tenant) session login by email + password.
//!
//! Signup already collects a password and stores an argon2id hash
//! (`tenants.password_hash`); verify-email then mints a one-time 7-day session
//! cookie. Without this endpoint the password is dead weight and an admin is
//! locked out once that session lapses. Login verifies the stored hash and
//! mints the *same* signed session cookie `verify-email` issues, so the admin
//! UI can re-authenticate.
//!
//! Security properties (DEC-FBR-IMPL-10):
//!   1. **Pre-argon2 rate-limit** (`LoginGate`, keyed by client-IP + email).
//!      The throttle runs BEFORE any password hashing, so it caps both
//!      password brute-force AND the argon2-CPU-DoS vector (every attempt that
//!      reaches the verify step burns a full argon2id computation on our CPU).
//!   2. **Account-enumeration resistance**. Unknown email and wrong password
//!      both return a generic `401 unauthorized`. On the unknown-email path a
//!      dummy argon2 verify runs so response timing does not distinguish "no
//!      such account" from "bad password".
//!   3. **Verified-gate**. A correct password for a not-yet-verified tenant
//!      returns `403 forbidden` (mirrors the `AdminSession` extractor). Only a
//!      caller who already proved the password reaches this branch, so it
//!      leaks nothing to an anonymous prober.
//!   4. **Constant-time compare**. argon2 PHC verification is constant-time.

use std::net::SocketAddr;
use std::sync::OnceLock;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_anon::{LoginGate, RateLimitError};

use crate::auth::password::{hash_password, verify_password};
use crate::auth::session::issue_session_cookie;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub tenant_id: Uuid,
    pub verified: bool,
}

/// A valid argon2id PHC string used ONLY to equalize timing on the
/// unknown-email path. Computed once (with the same parameters as a real
/// signup hash, so the verify cost matches) and never matches any password.
fn timing_equalizer_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        // Hashing a fixed throwaway value; the result is discarded as a
        // credential -- its only job is to make `verify_password` do a real
        // argon2id computation on the no-such-account branch.
        hash_password("feedbackmonk\0login-timing-equalizer\0value")
            .expect("dummy hash generation cannot fail with valid input")
    })
    .as_str()
}

/// 429 response with a `Retry-After` header (mirrors the anon-submission path).
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

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    // Normalize the email the same way signup does (trim + lowercase) so the
    // rate-limit bucket and the `find_by_email` lookup agree on the key.
    let email = req.email.trim().to_ascii_lowercase();

    // (1) Throttle BEFORE any argon2 work -- brute-force + CPU-DoS guard.
    let key = LoginGate::key_hash(&addr.ip().to_string(), &email);
    if let Err(RateLimitError::Exceeded { retry_after_seconds }) = state.login_gate.check(&key) {
        return Ok(rate_limited_response(retry_after_seconds));
    }

    // (2) Pre-auth tenant lookup (allowlisted unscoped query -- DEC-FBR-03).
    let Some(tenant) = state.tenants.find_by_email(&email).await? else {
        // (3a) Unknown account: burn an equivalent argon2 verify so timing
        //      does not reveal account non-existence, then fail generically.
        let _ = verify_password(&req.password, timing_equalizer_hash());
        return Err(ApiError::Unauthorized);
    };

    // (3b) Known account: constant-time password check.
    if !verify_password(&req.password, &tenant.password_hash)? {
        return Err(ApiError::Unauthorized);
    }

    // (4) Verified-gate: correct password but pending verification -> 403.
    if tenant.verified_at.is_none() {
        return Err(ApiError::Forbidden);
    }

    // (5) Mint the same signed session cookie verify-email issues.
    let jar = jar.add(issue_session_cookie(tenant.id, state.session_secret.as_ref()));
    Ok((
        jar,
        Json(LoginResponse {
            tenant_id: tenant.id,
            verified: true,
        }),
    )
        .into_response())
}
