//! `POST /api/v1/signup` -- tenant signup.
//!
//! Flow:
//! 1. Validate `email` and `password` shape.
//! 2. Hash the password with argon2id.
//! 3. `TenantRepo::create(email, hash)` -- inserts pending-verification tenant.
//!    On unique-violation (`RepoError::Conflict`), return 409.
//! 4. Mint a 32-byte random token; store via `EmailVerificationRepo::create`.
//! 5. Send the verify email via `Mailer`.
//! 6. Return 202 Accepted with the tenant id (informational only; client cannot
//!    do anything with it until verify-email completes).
//!
//! The email send failure path is logged but does NOT roll back the tenant.
//! Customers can request a re-send later (P1 work); for P0 we surface a generic
//! 202 so we never reveal whether an email already exists in our system.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::password::hash_password;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct SignupResponse {
    pub tenant_id: Uuid,
    pub message: &'static str,
}

const PASSWORD_MIN_LEN: usize = 8;
const PASSWORD_MAX_LEN: usize = 256;
const EMAIL_MAX_LEN: usize = 320; // RFC 5321 max local + @ + max domain.

fn validate_email(email: &str) -> Result<String, ApiError> {
    let trimmed = email.trim().to_ascii_lowercase();
    if trimmed.is_empty() || trimmed.len() > EMAIL_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "email must be 1..={EMAIL_MAX_LEN} chars"
        )));
    }
    // Minimal RFC-tolerant shape check: one '@', non-empty on both sides, a '.'
    // somewhere in the domain. P1 admin UI can do client-side validation; the
    // server-side check exists to catch obvious garbage, not to police RFC 5322.
    let (local, domain) = trimmed
        .split_once('@')
        .ok_or_else(|| ApiError::BadRequest("email must contain '@'".into()))?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err(ApiError::BadRequest("email shape is invalid".into()));
    }
    Ok(trimmed)
}

fn validate_password(password: &str) -> Result<(), ApiError> {
    if password.len() < PASSWORD_MIN_LEN || password.len() > PASSWORD_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "password must be {PASSWORD_MIN_LEN}..={PASSWORD_MAX_LEN} chars"
        )));
    }
    Ok(())
}

/// Generate a 32-byte random verify token, base64url-encoded (43 chars).
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn signup(
    State(state): State<AppState>,
    Json(req): Json<SignupRequest>,
) -> Result<(StatusCode, Json<SignupResponse>), ApiError> {
    let email = validate_email(&req.email)?;
    validate_password(&req.password)?;

    let hash = hash_password(&req.password)?;
    let tenant = state.tenants.create(&email, &hash).await?;

    // Mint scope for the freshly-created tenant -- needed by the verification
    // repo's scope-disciplined `create`.
    let scope = state.tenants.scope_for(tenant.id).await?;

    let token = generate_token();
    let expires_at = Utc::now() + state.verify_token_ttl;
    state
        .email_verifications
        .create(&scope, &token, expires_at)
        .await?;

    let link = format!("{}/verify-email?token={token}", state.public_url);
    if let Err(e) = state.mailer.send_verify_email(&email, &link).await {
        // Do NOT fail the request -- the tenant row is committed. Log loudly.
        tracing::error!(error = %e, tenant_id = %tenant.id, "verify email send failed");
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(SignupResponse {
            tenant_id: tenant.id,
            message: "check your email to verify",
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_validation_round_trip() {
        assert_eq!(validate_email("Alice@Example.COM").unwrap(), "alice@example.com");
        assert_eq!(validate_email("  user@host.io ").unwrap(), "user@host.io");
    }

    #[test]
    fn email_validation_rejects_garbage() {
        assert!(validate_email("").is_err());
        assert!(validate_email("noatsign").is_err());
        assert!(validate_email("@nodomain").is_err());
        assert!(validate_email("user@").is_err());
        assert!(validate_email("user@nodot").is_err());
        assert!(validate_email(&format!("u@{}", "x".repeat(EMAIL_MAX_LEN))).is_err());
    }

    #[test]
    fn password_validation() {
        validate_password("hunter22").unwrap();
        validate_password(&"x".repeat(PASSWORD_MIN_LEN)).unwrap();
        assert!(validate_password("short").is_err());
        assert!(validate_password(&"x".repeat(PASSWORD_MAX_LEN + 1)).is_err());
    }

    #[test]
    fn generated_token_is_base64url_43_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 43); // ceil(32 * 4 / 3) without padding = 43
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        // Two consecutive calls are overwhelmingly distinct.
        assert_ne!(generate_token(), generate_token());
    }
}
