//! `GET /api/v1/admin/tier` — admin tier-status endpoint
//! (P3 Stage 1, FR-FBR-14, Contract C17).
//!
//! Returns the authenticated tenant's current tier, the static
//! Contract-C19 quotas for that tier, and live usage (projects count +
//! rolling-30d feedback count + `period_start` ISO-8601). Read-only;
//! consumed by Stage 2's admin UI tier-settings page.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::state::AppState;

/// Wire shape for `GET /api/v1/admin/tier` — Contract C17 mirror.
#[derive(Debug, Clone, Serialize)]
pub struct TierStatusResponse {
    pub tier: String,
    pub quotas: TierQuotasWire,
    pub usage: TierUsageWire,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierQuotasWire {
    pub projects_per_org: Option<i64>,
    pub monthly_feedback_volume: Option<i64>,
    pub custom_branding: bool,
    pub custom_domain: bool,
    pub eu_residency: bool,
    /// Free-tier footer copy or `None` for paid tiers.
    pub footer_text: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierUsageWire {
    pub projects: i64,
    pub feedback_monthly: i64,
    pub period_start: DateTime<Utc>,
}

pub async fn get_tier_status(
    State(state): State<AppState>,
    session: AdminSession,
) -> Result<Json<TierStatusResponse>, ApiError> {
    let status = state.tier_quotas.get_tier_status(&session.scope).await?;
    Ok(Json(TierStatusResponse {
        tier: status.tier.as_db_str().to_string(),
        quotas: TierQuotasWire {
            projects_per_org: status.quotas.projects_per_org,
            monthly_feedback_volume: status.quotas.monthly_feedback_volume,
            custom_branding: status.quotas.custom_branding,
            custom_domain: status.quotas.custom_domain,
            eu_residency: status.quotas.eu_residency,
            footer_text: status.quotas.footer_text,
        },
        usage: TierUsageWire {
            projects: status.usage.projects,
            feedback_monthly: status.usage.feedback_monthly,
            period_start: status.usage.period_start,
        },
    }))
}

pub fn admin_tier_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/admin/tier", get(get_tier_status))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use chrono::Duration;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use feedbackmonk_anon::{AnonGate, DEFAULT_RATE_LIMIT_PER_HOUR};
    use feedbackmonk_repository::{
        SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
        SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxRoadmapItemRepo,
        SqlxRoadmapVoteRepo, SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo,
    };

    use crate::auth::session::issue_session_cookie;
    use crate::email::Mailer;
    use crate::roadmap_voting_cache::VotingCache;
    use std::num::NonZeroU32;

    struct StubMailer;
    #[async_trait::async_trait]
    impl Mailer for StubMailer {
        async fn send_verify_email(&self, _to: &str, _link: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    struct NoopEmailNotifier;
    #[async_trait::async_trait]
    impl crate::email::EmailNotifier for NoopEmailNotifier {
        async fn send_email(
            &self,
            _scope: &feedbackmonk_repository::TenantScope,
            _kind: crate::email::EmailKind,
            _ctx: crate::email::EmailContext,
        ) -> Result<crate::email::SendOutcome, crate::email::EmailError> {
            Ok(crate::email::SendOutcome::Skipped)
        }
    }

    fn build_test_state(pool: &PgPool, secret: [u8; 32]) -> AppState {
        AppState {
            pool: pool.clone(),
            tenants: Arc::new(SqlxTenantRepo::new(pool.clone())),
            projects: Arc::new(SqlxProjectRepo::new(pool.clone())),
            signing_keys: Arc::new(SqlxSigningKeyRepo::new(pool.clone())),
            feedback: Arc::new(SqlxFeedbackRepo::new(pool.clone())),
            feedback_history: Arc::new(SqlxFeedbackStatusHistoryRepo::new(pool.clone())),
            feedback_replies: Arc::new(SqlxFeedbackReplyRepo::new(pool.clone())),
            email_verifications: Arc::new(SqlxEmailVerificationRepo::new(pool.clone())),
            mailer: Arc::new(StubMailer),
            email_notifier: Arc::new(NoopEmailNotifier),
            session_secret: Arc::new(secret),
            public_url: Arc::from("http://localhost:14304"),
            verify_token_ttl: Duration::hours(24),
            anon_gate: AnonGate::new(NonZeroU32::new(DEFAULT_RATE_LIMIT_PER_HOUR).unwrap()),
            jwt_iat_leeway_seconds: 5,
            roadmap_items: Arc::new(SqlxRoadmapItemRepo::new(pool.clone())),
            roadmap_votes: Arc::new(SqlxRoadmapVoteRepo::new(pool.clone())),
            voting_cache: VotingCache::new(),
            started_at: Utc::now(),
            health: SqlxHealthCheck::new(pool.clone()),
            tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
        }
    }

    /// Test seam: set tenant tier via the repository's allowlisted
    /// `set_tier_for_test` helper (multi-tenant-isolation oracle
    /// inherent-method allowlist). Production tier writes will land via
    /// Polar webhook receiver (DEC-FBR-DEFER-01 deferred).
    async fn set_tier_via_repo(pool: &PgPool, tenant_id: uuid::Uuid, tier_str: &str) {
        let repo = SqlxTenantRepo::new(pool.clone());
        repo.set_tier_for_test(tenant_id, tier_str).await.unwrap();
    }

    async fn seed_session_cookie(
        state: &AppState,
        email: &str,
        tier_str: &str,
    ) -> String {
        let t = state.tenants.create(email, "h").await.unwrap();
        set_tier_via_repo(&state.pool, t.id, tier_str).await;
        let scope = state.tenants.scope_for(t.id).await.unwrap();
        state.tenants.mark_verified(&scope).await.unwrap();
        let cookie = issue_session_cookie(t.id, state.session_secret.as_ref());
        // Cookie::to_string() renders the full Set-Cookie form with
        // attributes; the request-header Cookie field wants just
        // `name=value`. Strip at the first `;`.
        cookie
            .to_string()
            .split(';')
            .next()
            .expect("cookie name=value")
            .to_string()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn returns_401_without_admin_session(pool: PgPool) {
        let state = build_test_state(&pool, [0x11u8; 32]);
        let app = admin_tier_router(state);
        let req = Request::get("/api/v1/admin/tier").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn free_tenant_returns_free_tier_quotas(pool: PgPool) {
        let state = build_test_state(&pool, [0x22u8; 32]);
        let cookie = seed_session_cookie(&state, "free-tier@example.com", "free").await;
        let app = admin_tier_router(state);
        let req = Request::get("/api/v1/admin/tier")
            .header(axum::http::header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 4 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["tier"], "free");
        assert_eq!(body["quotas"]["projects_per_org"], 1);
        assert_eq!(body["quotas"]["monthly_feedback_volume"], 50);
        assert_eq!(body["quotas"]["footer_text"], "powered by feedbackmonk");
        assert_eq!(body["usage"]["projects"], 0);
        assert_eq!(body["usage"]["feedback_monthly"], 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn pro_tenant_returns_unlimited_projects_quota(pool: PgPool) {
        let state = build_test_state(&pool, [0x33u8; 32]);
        let cookie = seed_session_cookie(&state, "pro-tier@example.com", "pro").await;
        let app = admin_tier_router(state);
        let req = Request::get("/api/v1/admin/tier")
            .header(axum::http::header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 4 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["tier"], "pro");
        assert!(body["quotas"]["projects_per_org"].is_null());
        assert_eq!(body["quotas"]["monthly_feedback_volume"], 10000);
        assert!(body["quotas"]["footer_text"].is_null());
        assert!(body["quotas"]["custom_domain"].as_bool().unwrap());
    }
}
