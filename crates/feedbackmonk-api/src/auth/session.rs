//! Signed-cookie admin session.
//!
//! Cookie value format (URL-safe base64, no padding; `.` separators):
//!
//! ```text
//! <b64(tenant_uuid_bytes_16)>.<b64(issued_unix_be_8)>.<b64(session_epoch_be_8)>.<b64(hmac_sha256_32)>
//! ```
//!
//! The HMAC input is the concatenation of the raw 16-byte tenant UUID, the
//! 8-byte big-endian Unix timestamp, and the 8-byte big-endian session epoch.
//! The HMAC key is the 32-byte session secret loaded from
//! `FEEDBACKMONK_SESSION_SECRET` (hex-encoded in env).
//!
//! Lifetime: 7 days from issuance. Tampered or expired cookies yield 401.
//! Forged cookies for unknown tenants yield 401 (the extractor confirms the
//! tenant exists via `TenantRepo::scope_for`). Pending-verification tenants
//! (`verified_at IS NULL`) yield 403 -- they may not call admin endpoints.
//!
//! **Session revocation (scrutiny P1-1)**: the cookie carries the tenant's
//! `session_epoch` at issuance. On every request the extractor compares it
//! against the tenant's CURRENT `session_epoch` (migration 00023) and yields
//! 401 on mismatch. Bumping a tenant's epoch (logout, password-reset confirm)
//! therefore invalidates every outstanding cookie for that tenant without
//! rotating the global session secret (which would log out all tenants).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use feedbackmonk_repository::TenantScope;

use crate::error::ApiError;
use crate::state::AppState;

pub const SESSION_COOKIE_NAME: &str = "feedbackmonk_session";
const SESSION_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

type HmacSha256 = Hmac<Sha256>;

/// Authenticated admin session extracted from the signed cookie.
#[derive(Debug, Clone, Copy)]
pub struct AdminSession {
    pub scope: TenantScope,
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AdminSession {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let jar = CookieJar::from_headers(&parts.headers);
        let raw = jar
            .get(SESSION_COOKIE_NAME)
            .map(|c| c.value().to_string())
            .ok_or(ApiError::Unauthorized)?;

        let (tenant_id, _issued_at, cookie_epoch) =
            verify_cookie_value(&raw, state.session_secret.as_ref())
                .ok_or(ApiError::Unauthorized)?;

        // Resolve to a TenantScope (validates the tenant row still exists).
        let scope = state
            .tenants
            .scope_for(tenant_id)
            .await
            .map_err(|_| ApiError::Unauthorized)?;

        let tenant = state.tenants.get(&scope).await?;

        // Session-revocation check (scrutiny P1-1): reject any cookie whose
        // epoch != the tenant's current epoch. A logout / password-reset
        // confirm bumps the epoch, invalidating every outstanding cookie.
        if i64::from(tenant.session_epoch) != cookie_epoch {
            return Err(ApiError::Unauthorized);
        }

        // Reject pending-verification tenants (defense-in-depth; sessions are
        // only minted post-verification, but a cookie outliving a manual
        // unverify would otherwise be accepted).
        if tenant.verified_at.is_none() {
            return Err(ApiError::Forbidden);
        }

        Ok(Self { scope })
    }
}

/// Build the signed cookie *value* for `tenant_id` issued at `now` carrying
/// `session_epoch`.
fn build_cookie_value(
    tenant_id: Uuid,
    issued_unix: i64,
    session_epoch: i64,
    secret: &[u8; 32],
) -> String {
    let mut tenant_bytes = [0u8; 16];
    tenant_bytes.copy_from_slice(tenant_id.as_bytes());
    let issued_bytes = issued_unix.to_be_bytes();
    let epoch_bytes = session_epoch.to_be_bytes();

    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&tenant_bytes);
    mac.update(&issued_bytes);
    mac.update(&epoch_bytes);
    let tag = mac.finalize().into_bytes();

    format!(
        "{}.{}.{}.{}",
        URL_SAFE_NO_PAD.encode(tenant_bytes),
        URL_SAFE_NO_PAD.encode(issued_bytes),
        URL_SAFE_NO_PAD.encode(epoch_bytes),
        URL_SAFE_NO_PAD.encode(tag),
    )
}

/// Verify a cookie value. Returns `Some((tenant_id, issued_unix, session_epoch))`
/// on success. The caller compares `session_epoch` against the tenant's current
/// epoch to enforce revocation.
fn verify_cookie_value(value: &str, secret: &[u8; 32]) -> Option<(Uuid, i64, i64)> {
    let mut parts = value.splitn(4, '.');
    let tenant_b64 = parts.next()?;
    let issued_b64 = parts.next()?;
    let epoch_b64 = parts.next()?;
    let tag_b64 = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let tenant_bytes = URL_SAFE_NO_PAD.decode(tenant_b64).ok()?;
    let issued_bytes = URL_SAFE_NO_PAD.decode(issued_b64).ok()?;
    let epoch_bytes = URL_SAFE_NO_PAD.decode(epoch_b64).ok()?;
    let supplied_tag = URL_SAFE_NO_PAD.decode(tag_b64).ok()?;
    if tenant_bytes.len() != 16
        || issued_bytes.len() != 8
        || epoch_bytes.len() != 8
        || supplied_tag.len() != 32
    {
        return None;
    }

    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&tenant_bytes);
    mac.update(&issued_bytes);
    mac.update(&epoch_bytes);
    let expected_tag = mac.finalize().into_bytes();

    if expected_tag.as_slice().ct_eq(&supplied_tag).unwrap_u8() != 1 {
        return None;
    }

    let mut tid = [0u8; 16];
    tid.copy_from_slice(&tenant_bytes);
    let mut isb = [0u8; 8];
    isb.copy_from_slice(&issued_bytes);
    let issued_unix = i64::from_be_bytes(isb);
    let mut epb = [0u8; 8];
    epb.copy_from_slice(&epoch_bytes);
    let session_epoch = i64::from_be_bytes(epb);

    let now = Utc::now().timestamp();
    if now - issued_unix > SESSION_MAX_AGE_SECS || now < issued_unix - 60 {
        // Expired (or clock-skewed >1min into the future).
        return None;
    }

    Some((Uuid::from_bytes(tid), issued_unix, session_epoch))
}

/// Build a `Set-Cookie` for a freshly-minted admin session. `session_epoch` is
/// the tenant's current epoch at issuance; a later bump revokes this cookie.
pub fn issue_session_cookie(
    tenant_id: Uuid,
    session_epoch: i64,
    secret: &[u8; 32],
) -> Cookie<'static> {
    let issued = Utc::now().timestamp();
    let value = build_cookie_value(tenant_id, issued, session_epoch, secret);
    Cookie::build((SESSION_COOKIE_NAME, value))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(SESSION_MAX_AGE_SECS))
        .build()
}

/// Build a `Set-Cookie` that CLEARS the admin session cookie (logout). Same
/// name/path/flags as [`issue_session_cookie`] with an empty value and an
/// immediate expiry so the browser drops it. The server-side revocation
/// (epoch bump) is the authoritative kill; this just tidies the client.
#[must_use]
pub fn clear_session_cookie() -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE_NAME, ""))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(0))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn cookie_value_round_trip() {
        let tenant = Uuid::new_v4();
        let now = Utc::now().timestamp();
        let value = build_cookie_value(tenant, now, 7, &secret());
        let (t, i, e) = verify_cookie_value(&value, &secret()).unwrap();
        assert_eq!(t, tenant);
        assert_eq!(i, now);
        assert_eq!(e, 7);
    }

    #[test]
    fn epoch_is_hmac_covered_and_round_trips() {
        // A cookie minted at epoch 3 must verify carrying epoch 3, and a cookie
        // at a different epoch produces a distinct value (revocation substrate).
        let tenant = Uuid::new_v4();
        let now = Utc::now().timestamp();
        let v3 = build_cookie_value(tenant, now, 3, &secret());
        let v4 = build_cookie_value(tenant, now, 4, &secret());
        assert_ne!(v3, v4, "different epoch => different signed cookie");
        assert_eq!(verify_cookie_value(&v3, &secret()).unwrap().2, 3);
        assert_eq!(verify_cookie_value(&v4, &secret()).unwrap().2, 4);
    }

    #[test]
    fn tampered_cookie_rejected_byte_by_byte() {
        let tenant = Uuid::new_v4();
        let now = Utc::now().timestamp();
        let good = build_cookie_value(tenant, now, 0, &secret());
        // Flip a single byte anywhere in the cookie -- must reject.
        let mut bytes = good.into_bytes();
        let original = bytes[5];
        bytes[5] = if original == b'a' { b'b' } else { b'a' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(verify_cookie_value(&tampered, &secret()).is_none());
    }

    #[test]
    fn wrong_secret_rejected() {
        let value = build_cookie_value(Uuid::new_v4(), Utc::now().timestamp(), 0, &secret());
        let other = [0x99u8; 32];
        assert!(verify_cookie_value(&value, &other).is_none());
    }

    #[test]
    fn expired_cookie_rejected() {
        let tenant = Uuid::new_v4();
        let old = Utc::now().timestamp() - SESSION_MAX_AGE_SECS - 1;
        let value = build_cookie_value(tenant, old, 0, &secret());
        assert!(verify_cookie_value(&value, &secret()).is_none());
    }

    #[test]
    fn future_dated_cookie_rejected() {
        let tenant = Uuid::new_v4();
        let future = Utc::now().timestamp() + 3600;
        let value = build_cookie_value(tenant, future, 0, &secret());
        assert!(verify_cookie_value(&value, &secret()).is_none());
    }

    #[test]
    fn malformed_cookie_rejected() {
        assert!(verify_cookie_value("not-a-cookie", &secret()).is_none());
        assert!(verify_cookie_value("a.b", &secret()).is_none());
        assert!(verify_cookie_value("a.b.c", &secret()).is_none());
        assert!(verify_cookie_value("a.b.c.d.e", &secret()).is_none());
    }

    #[test]
    fn clear_cookie_is_empty_and_expires_immediately() {
        let c = clear_session_cookie();
        assert_eq!(c.name(), SESSION_COOKIE_NAME);
        assert_eq!(c.value(), "");
        assert_eq!(c.max_age(), Some(time::Duration::seconds(0)));
    }
}
