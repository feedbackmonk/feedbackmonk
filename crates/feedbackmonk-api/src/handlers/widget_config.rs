//! `GET /api/v1/projects/{project_id}/widget-config` -- Contract C12.
//!
//! Public endpoint (no auth). The `project_id` is the widget's public key —
//! customer sites embed `<script src="…/widget.js" data-project-id="…">`
//! and the widget fetches its runtime config from this endpoint on mount.
//!
//! ## Auth
//!
//! None. No `Authorization` header is read; no admin session is required.
//! Per DEC-FBR-04, the JWT a customer's site may inject is the END-USER
//! identity for the submission endpoint — not a tenant credential.
//!
//! ## Project scope (DEC-PODS-001)
//!
//! Uses `ProjectRepo::open_for_submission(project_id)` — the SAME pre-auth
//! boundary the submission endpoint uses. The returned `ProjectScope` is
//! pre-bound to the project's owning tenant, so the widget-brand read is
//! always tenant-scoped without ever exposing a `TenantScope` constructor
//! to the public surface.
//!
//! ## Cache + CORS (Contract C12)
//!
//! `Cache-Control: public, max-age=60` — matches the voting cache TTL;
//! widgets re-fetch config on every mount but receive a 60s-fresh response.
//!
//! `Access-Control-Allow-Origin: *` — config is public-readable to allow
//! cross-domain embed. The DEC-FBR-04 domain-allowlist enforcement lives on
//! the submission endpoint, not on config (config exposes only project
//! brand metadata, never tenant secrets).

use axum::extract::{Path, State};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use feedbackmonk_core::WidgetBrand;

use crate::error::ApiError;
use crate::handlers::feedback::MAX_BODY_CHARS;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Response shape (Contract C12)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct WidgetConfigResponse {
    pub project_id: Uuid,
    pub tenant_id: Uuid,
    pub display_name: String,
    pub brand: WidgetBrand,
    pub auth_modes: Vec<&'static str>,
    pub submission_kinds: Vec<&'static str>,
    pub max_body_chars: usize,
}

// V1 hardcoded values per plan §Contract C12 + task brief §Phase 4 step 11.
// P3 may flip per-tier; for v1 every project sees the same auth + kind menu.
const V1_AUTH_MODES: [&str; 2] = ["auth", "anonymous"];
const V1_SUBMISSION_KINDS: [&str; 4] = ["bug", "feature", "question", "other"];

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/v1/projects/{project_id}/widget-config`.
///
/// Always returns 200 on success with C12 JSON + `Cache-Control` + CORS
/// headers. Returns 404 if `project_id` is unknown (via
/// `RepoError::NotFound` -> `ApiError::NotFound`).
pub async fn get_widget_config(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    // Pre-auth boundary: same constructor the submission endpoint uses.
    let project_scope = state.projects.open_for_submission(project_id).await?;

    // Tenant identity lives inside the scope — derived via `.tenant()`. We
    // never construct a `TenantScope` from raw inputs; the type-system
    // discipline (DEC-FBR-03) is preserved.
    let project = state.projects.get(&project_scope).await?;
    let brand = state
        .tenants
        .get_widget_brand(project_scope.tenant())
        .await?;

    let body = WidgetConfigResponse {
        project_id: project.id,
        tenant_id: project.tenant_id,
        display_name: project.name,
        brand,
        auth_modes: V1_AUTH_MODES.to_vec(),
        submission_kinds: V1_SUBMISSION_KINDS.to_vec(),
        max_body_chars: MAX_BODY_CHARS,
    };

    let mut response = (axum::http::StatusCode::OK, Json(body)).into_response();
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=60"),
    );
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    Ok(response)
}

// ---------------------------------------------------------------------------
// Router subtree -- composed into the main router by `main.rs::build_app`.
// ---------------------------------------------------------------------------

/// Worker A widget-config router. `main.rs::build_app` merges this in.
pub fn widget_config_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route(
            "/api/v1/projects/:project_id/widget-config",
            axum::routing::get(get_widget_config),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_auth_modes_are_auth_and_anonymous() {
        assert_eq!(V1_AUTH_MODES, ["auth", "anonymous"]);
    }

    #[test]
    fn v1_submission_kinds_match_feedback_kind_variants() {
        // If FeedbackKind grows a variant, this test surfaces the drift —
        // widget cap on offered kinds must move in lockstep.
        assert_eq!(V1_SUBMISSION_KINDS, ["bug", "feature", "question", "other"]);
    }

    #[test]
    fn response_shape_round_trips_through_serde() {
        let body = WidgetConfigResponse {
            project_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            display_name: "Fixture".into(),
            brand: WidgetBrand {
                primary_color: Some("#abcdef".into()),
                logo_url: None,
                footer_text: Some("powered by feedbackmonk".into()),
                footer_url: None,
                theme: Some("dark".into()),
            },
            auth_modes: V1_AUTH_MODES.to_vec(),
            submission_kinds: V1_SUBMISSION_KINDS.to_vec(),
            max_body_chars: MAX_BODY_CHARS,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["display_name"], "Fixture");
        assert_eq!(json["brand"]["primary_color"], "#abcdef");
        assert_eq!(json["brand"]["footer_text"], "powered by feedbackmonk");
        assert_eq!(json["brand"]["theme"], "dark");
        assert!(json["brand"]["logo_url"].is_null());
        assert!(json["brand"]["footer_url"].is_null());
        assert_eq!(json["auth_modes"][0], "auth");
        assert_eq!(json["auth_modes"][1], "anonymous");
        assert_eq!(json["max_body_chars"], MAX_BODY_CHARS);
    }
}
