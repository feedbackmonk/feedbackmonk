//! Runner-token lifecycle admin surface (Contract C25, FR-FBR-24 — P5b).
//!
//! `GET    /api/v1/projects/{project_id}/runner-tokens`        -- list issued tokens
//! `POST   /api/v1/projects/{project_id}/runner-tokens`        -- register an issued token (visibility)
//! `DELETE /api/v1/projects/{project_id}/runner-tokens/{jti}`  -- revoke a token by jti
//!
//! ## Why these endpoints exist (and what they do NOT do)
//!
//! feedbackmonk holds NO private key (DEC-FBR-04): the customer mints the runner
//! write-token client-side with the private half of a registered `runner`-class
//! signing key (`feedbackmonk-runner mint-token`, Worker B). The token is
//! self-verifying. So **issuance is not a server endpoint** — these endpoints
//! are *lifecycle*:
//!   - `POST` is OPTIONAL bookkeeping so the admin UI can list issued tokens.
//!   - `DELETE` is the load-bearing one: it writes the token's `jti` to the
//!     append-only revocation denylist, after which `verify_runner_token`
//!     rejects it (even before its short `exp`). A jti can be revoked WITHOUT
//!     prior registration (revoke-before-register).
//!
//! All routes are behind `AdminSession` and merged **WITHOUT** `.layer(cors)` —
//! this is an admin surface, never a browser embed (Ripple Analysis: do not
//! CORS-expose admin endpoints). The structural security bound is C22 inv. 2: a
//! runner token can author runner-only transitions but can **never** author
//! `approve`, so even full runner-token compromise cannot bypass the approval
//! gate — which is why automating runner-token lifecycle is safe.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_repository::{NewRunnerToken, ProjectScope};

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::state::AppState;

const LABEL_MAX_LEN: usize = 100;
const JTI_MAX_LEN: usize = 200;

#[derive(Debug, Deserialize)]
pub struct RegisterRunnerTokenRequest {
    /// The token's `jti` claim (a client-minted UUID). Single-sourced from the
    /// runner that minted it.
    pub jti: String,
    /// Human label for the admin UI (e.g. "ci-runner").
    pub label: String,
    /// The token's `exp` as an RFC3339 timestamp (optional; visibility only).
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// One runner-token row in the admin list: registry fields + the joined
/// revocation state.
#[derive(Debug, Serialize)]
pub struct RunnerTokenView {
    pub jti: String,
    pub label: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// `Some` when this jti has been revoked (token is dead regardless of exp).
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct RunnerTokenListResponse {
    pub items: Vec<RunnerTokenView>,
}

fn validate_label(label: &str) -> Result<(), ApiError> {
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed.len() > LABEL_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "label must be 1..={LABEL_MAX_LEN} chars after trim"
        )));
    }
    Ok(())
}

fn validate_jti(jti: &str) -> Result<(), ApiError> {
    let trimmed = jti.trim();
    if trimmed.is_empty() || trimmed.len() > JTI_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "jti must be 1..={JTI_MAX_LEN} chars after trim"
        )));
    }
    Ok(())
}

async fn admin_scope(
    state: &AppState,
    session: &AdminSession,
    project_id: Uuid,
) -> Result<ProjectScope, ApiError> {
    Ok(state.projects.open(&session.scope, project_id).await?)
}

/// `GET /api/v1/projects/:project_id/runner-tokens` — list issued runner tokens
/// with their revocation state (admin).
pub async fn list(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<RunnerTokenListResponse>, ApiError> {
    let scope = admin_scope(&state, &session, project_id).await?;

    // Registry rows + the revocation denylist, joined by jti at the handler
    // layer (the two repos are independent — revoke-before-register is allowed).
    let registry = state.runner_tokens.list(&scope).await?;
    let revoked: HashMap<String, DateTime<Utc>> = state
        .runner_token_revocations
        .list(&scope)
        .await?
        .into_iter()
        .map(|r| (r.jti, r.revoked_at))
        .collect();

    let items = registry
        .into_iter()
        .map(|r| RunnerTokenView {
            revoked_at: revoked.get(&r.jti).copied(),
            jti: r.jti,
            label: r.label,
            expires_at: r.expires_at,
            created_at: r.created_at,
        })
        .collect();
    Ok(Json(RunnerTokenListResponse { items }))
}

/// `POST /api/v1/projects/:project_id/runner-tokens` — register an issued token
/// for visibility (admin). Idempotent upsert on `(project, jti)`.
pub async fn register(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
    Json(req): Json<RegisterRunnerTokenRequest>,
) -> Result<StatusCode, ApiError> {
    validate_jti(&req.jti)?;
    validate_label(&req.label)?;
    let scope = admin_scope(&state, &session, project_id).await?;
    state
        .runner_tokens
        .register(
            &scope,
            NewRunnerToken {
                jti: req.jti.trim(),
                label: req.label.trim(),
                expires_at: req.expires_at,
            },
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/v1/projects/:project_id/runner-tokens/:jti` — revoke a token by
/// jti (admin). Writes to the append-only denylist; `verify_runner_token`
/// rejects the token thereafter. Idempotent (a second revoke is a no-op).
pub async fn revoke(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, jti)): Path<(Uuid, String)>,
) -> Result<StatusCode, ApiError> {
    validate_jti(&jti)?;
    let scope = admin_scope(&state, &session, project_id).await?;

    // Copy the label from the registry for audit when the token was registered;
    // None when revoking an unregistered jti (revoke-before-register).
    let label = state
        .runner_tokens
        .list(&scope)
        .await?
        .into_iter()
        .find(|r| r.jti == jti)
        .map(|r| r.label);
    state
        .runner_token_revocations
        .revoke(&scope, jti.trim(), label.as_deref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Runner-token lifecycle router — behind `AdminSession`, merged WITHOUT CORS.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/projects/:project_id/runner-tokens",
            get(list).post(register),
        )
        .route(
            "/api/v1/projects/:project_id/runner-tokens/:jti",
            delete(revoke),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_validation() {
        validate_label("ci-runner").unwrap();
        assert!(validate_label("").is_err());
        assert!(validate_label("   ").is_err());
        assert!(validate_label(&"x".repeat(LABEL_MAX_LEN + 1)).is_err());
    }

    #[test]
    fn jti_validation() {
        validate_jti(&Uuid::new_v4().to_string()).unwrap();
        assert!(validate_jti("").is_err());
        assert!(validate_jti(&"x".repeat(JTI_MAX_LEN + 1)).is_err());
    }
}
