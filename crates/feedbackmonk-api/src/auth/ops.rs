//! Operator bearer-token guard for the ops mutation surface (DEC-FBR-IMPL-11).
//!
//! The ops endpoint (`PATCH /api/v1/ops/tenants/{id}` — set tier + widget brand
//! override) is an OPERATOR surface, deliberately separate from the per-tenant
//! `AdminSession`: every tenant holds its own `AdminSession` (their triage-
//! dashboard cookie), so gating tier/footer mutation behind it would let a Free
//! tenant upgrade itself and strip the FR-FBR-14 badge. There is no superadmin
//! system in v1; this shared-secret bearer token is the minimal honest operator
//! surface (mirrors the deploy-time-env posture of `FEEDBACKMONK_CORS_ORIGINS`).
//!
//! Behavior:
//!   - `FEEDBACKMONK_OPS_TOKEN` unset (`state.ops_token == None`) ⇒ the endpoint
//!     is DISABLED: extractor rejects with **404 Not Found** so an attacker
//!     cannot even distinguish "endpoint exists but I lack the token" from
//!     "no such route" on a deployment that didn't opt in.
//!   - Missing / malformed `Authorization` header, or wrong token ⇒ **401**.
//!   - Token matches (constant-time) ⇒ extraction succeeds.

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use subtle::ConstantTimeEq;

use crate::error::ApiError;
use crate::state::AppState;

/// Marker proving the caller presented a valid ops bearer token. Carries no
/// data — its existence in a handler signature is the authorization.
#[derive(Debug, Clone, Copy)]
pub struct OpsAuth;

#[axum::async_trait]
impl FromRequestParts<AppState> for OpsAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        // Feature-off when no token is configured: present as 404 so the ops
        // surface is invisible on deployments that didn't opt in.
        let Some(expected) = state.ops_token.as_deref() else {
            return Err(ApiError::NotFound);
        };

        let presented = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .ok_or(ApiError::Unauthorized)?;

        // Constant-time compare (length-independent: compare equal-length only
        // after a length check, which itself leaks nothing useful here).
        let matches = presented.len() == expected.len()
            && presented
                .as_bytes()
                .ct_eq(expected.as_bytes())
                .unwrap_u8()
                == 1;
        if !matches {
            return Err(ApiError::Unauthorized);
        }
        Ok(OpsAuth)
    }
}
