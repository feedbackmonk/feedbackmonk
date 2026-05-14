//! `POST /api/v1/verify-email` -- redeem a verify token.
//!
//! Idempotency model:
//!   - First redemption (within TTL, `used_at IS NULL`): mark used + set
//!     `tenants.verified_at = now()` + mint session cookie. Status 200.
//!   - Second redemption within `REPLAY_WINDOW_SECS` of the first: mint a
//!     fresh session cookie (same tenant) + return 200. (Handles
//!     double-click + email-link prefetch + retries from spotty mobile network.)
//!   - Second redemption AFTER replay window: 410 Gone.
//!   - Expired token (`expires_at < now`): 410 Gone.
//!   - Unknown token: 401 (the token is the credential; revealing whether a
//!     given token exists is a small information leak we'd rather avoid).
//!
//! Per FR-FBR-02 the verify-email link can be opened multiple times legitimately
//! (mail clients prefetch links, users double-click). The replay window
//! tolerates that without giving an indefinite re-issuance ability.

use std::time::Duration as StdDuration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::session::issue_session_cookie;
use crate::error::ApiError;
use crate::state::AppState;

const REPLAY_WINDOW_SECS: i64 = 5 * 60;

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyEmailResponse {
    pub tenant_id: Uuid,
    pub verified: bool,
}

/// Decide what to do with a redemption row.
enum Outcome {
    /// First use within TTL.
    First,
    /// Second use within replay window.
    Replay,
    /// Expired or out-of-window.
    Gone,
}

fn classify(redemption_expires_at: DateTime<Utc>, used_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Outcome {
    if now > redemption_expires_at {
        return Outcome::Gone;
    }
    match used_at {
        None => Outcome::First,
        Some(used) => {
            if (now - used).num_seconds() <= REPLAY_WINDOW_SECS {
                Outcome::Replay
            } else {
                Outcome::Gone
            }
        }
    }
}

pub async fn verify(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<VerifyEmailRequest>,
) -> Result<(CookieJar, Json<VerifyEmailResponse>), ApiError> {
    let token = req.token.trim();
    if token.is_empty() {
        return Err(ApiError::BadRequest("token must not be empty".into()));
    }

    let redemption = state
        .email_verifications
        .redeem(token)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    let now = Utc::now();
    match classify(redemption.expires_at, redemption.used_at, now) {
        Outcome::Gone => Err(ApiError::Gone),
        Outcome::First => {
            let scope = state.tenants.scope_for(redemption.tenant_id).await?;
            state.tenants.mark_verified(&scope).await?;
            state.email_verifications.mark_used(&scope, token).await?;

            // Cookie-jar add returns a new jar with the cookie attached.
            let cookie = issue_session_cookie(redemption.tenant_id, state.session_secret.as_ref());
            // Yield to keep argon2-bound work fair on tokio's blocking pool.
            tokio::time::sleep(StdDuration::ZERO).await;
            let jar = jar.add(cookie);
            Ok((
                jar,
                Json(VerifyEmailResponse {
                    tenant_id: redemption.tenant_id,
                    verified: true,
                }),
            ))
        }
        Outcome::Replay => {
            // Already-verified tenant, but within the replay window -- re-mint the
            // session cookie so a slow client can still complete the flow.
            let cookie = issue_session_cookie(redemption.tenant_id, state.session_secret.as_ref());
            let jar = jar.add(cookie);
            Ok((
                jar,
                Json(VerifyEmailResponse {
                    tenant_id: redemption.tenant_id,
                    verified: true,
                }),
            ))
        }
    }
}

// Helper used by integration tests + monitoring. Not strictly part of the
// HTTP surface; lives here for proximity to the request handler.
#[must_use]
pub const fn status_for_outcome(outcome_is_gone: bool) -> StatusCode {
    if outcome_is_gone {
        StatusCode::GONE
    } else {
        StatusCode::OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn now_fixed() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-13T22:00:00Z").unwrap().with_timezone(&Utc)
    }

    #[test]
    fn unused_unexpired_classifies_first() {
        let now = now_fixed();
        let expires = now + ChronoDuration::hours(1);
        matches!(classify(expires, None, now), Outcome::First);
    }

    #[test]
    fn expired_classifies_gone() {
        let now = now_fixed();
        let expires = now - ChronoDuration::seconds(1);
        assert!(matches!(classify(expires, None, now), Outcome::Gone));
    }

    #[test]
    fn used_within_replay_window_classifies_replay() {
        let now = now_fixed();
        let expires = now + ChronoDuration::hours(1);
        let used = now - ChronoDuration::seconds(REPLAY_WINDOW_SECS - 10);
        assert!(matches!(classify(expires, Some(used), now), Outcome::Replay));
    }

    #[test]
    fn used_outside_replay_window_classifies_gone() {
        let now = now_fixed();
        let expires = now + ChronoDuration::hours(1);
        let used = now - ChronoDuration::seconds(REPLAY_WINDOW_SECS + 1);
        assert!(matches!(classify(expires, Some(used), now), Outcome::Gone));
    }
}
