//! `POST /api/v1/projects` -- create a project under the authenticated tenant.
//! `GET  /api/v1/projects` -- list projects owned by the authenticated tenant.
//!
//! Both are admin-session-gated (`AdminSession` extractor). The session
//! resolves to a `TenantScope`; the repository methods enforce the tenant
//! boundary at the type-system level.

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectResponse {
    pub project_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
    /// HTML+JS snippet the customer pastes into their site. The widget itself
    /// (`/widget.js`) is P2 work; this snippet is forward-looking documentation.
    pub embed_snippet: String,
}

#[derive(Debug, Serialize)]
pub struct ListProjectsResponse {
    pub projects: Vec<ProjectListItem>,
}

#[derive(Debug, Serialize)]
pub struct ProjectListItem {
    pub project_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

const SLUG_MAX_LEN: usize = 64;
const NAME_MAX_LEN: usize = 200;

fn validate_slug(slug: &str) -> Result<(), ApiError> {
    if slug.is_empty() || slug.len() > SLUG_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "slug must be 1..={SLUG_MAX_LEN} chars"
        )));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::BadRequest(
            "slug must be lowercase letters, digits, or '-'".into(),
        ));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(ApiError::BadRequest(
            "slug must not start or end with '-'".into(),
        ));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > NAME_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "name must be 1..={NAME_MAX_LEN} chars after trim"
        )));
    }
    Ok(())
}

fn build_embed_snippet(public_url: &str, slug: &str) -> String {
    format!(
        "<script src=\"{public_url}/widget.js\" data-project=\"{slug}\"></script>"
    )
}

pub async fn create(
    State(state): State<AppState>,
    session: AdminSession,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ProjectResponse>, ApiError> {
    validate_name(&req.name)?;
    validate_slug(&req.slug)?;

    let project = state
        .projects
        .create(&session.scope, req.name.trim(), &req.slug)
        .await?;

    Ok(Json(ProjectResponse {
        project_id: project.id,
        name: project.name,
        slug: project.slug.clone(),
        created_at: project.created_at,
        embed_snippet: build_embed_snippet(&state.public_url, &project.slug),
    }))
}

pub async fn list(
    State(state): State<AppState>,
    session: AdminSession,
) -> Result<Json<ListProjectsResponse>, ApiError> {
    let projects = state.projects.list_for_tenant(&session.scope).await?;
    Ok(Json(ListProjectsResponse {
        projects: projects
            .into_iter()
            .map(|p| ProjectListItem {
                project_id: p.id,
                name: p.name,
                slug: p.slug,
                created_at: p.created_at,
            })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_validation_accepts_valid() {
        validate_slug("my-project").unwrap();
        validate_slug("p1").unwrap();
        validate_slug("abc-123-def").unwrap();
    }

    #[test]
    fn slug_validation_rejects_invalid() {
        assert!(validate_slug("").is_err());
        assert!(validate_slug("Has-Caps").is_err());
        assert!(validate_slug("under_score").is_err());
        assert!(validate_slug("-leading").is_err());
        assert!(validate_slug("trailing-").is_err());
        assert!(validate_slug(&"x".repeat(SLUG_MAX_LEN + 1)).is_err());
    }

    #[test]
    fn name_validation() {
        validate_name("My Project").unwrap();
        validate_name("  Trimmed  ").unwrap();
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(NAME_MAX_LEN + 1)).is_err());
    }

    #[test]
    fn embed_snippet_includes_public_url_and_slug() {
        let s = build_embed_snippet("https://feedbackmonk.example", "my-app");
        assert!(s.contains("https://feedbackmonk.example/widget.js"));
        assert!(s.contains("data-project=\"my-app\""));
    }
}
