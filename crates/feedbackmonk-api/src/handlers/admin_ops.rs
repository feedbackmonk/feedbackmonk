//! `PATCH /api/v1/ops/tenants/{tenant_id}` — operator tier + brand-override
//! mutation (DEC-FBR-IMPL-11).
//!
//! OPERATOR surface, guarded by the `OpsAuth` bearer-token extractor — NOT the
//! per-tenant `AdminSession`. This is the privilege separation that keeps
//! FR-FBR-14 intact: a Free tenant's own admin session cannot flip its tier or
//! suppress its "powered by feedbackmonk" badge. Only the operator holding
//! `FEEDBACKMONK_OPS_TOKEN` can. With the env var unset the route is invisible
//! (404 via `OpsAuth`).
//!
//! Body (both top-level keys optional; absent ⇒ that facet is left unchanged):
//! ```json
//! {
//!   "tier": "self_host",                     // optional: set the pricing tier
//!   "branding": {                            // optional: REPLACE all override
//!     "footer_text_override": "",            //   columns (PUT semantics within
//!     "footer_url": null,                    //   the object — an absent field
//!     "theme": "dark",                       //   means null/clear). "" suppresses
//!     "primary_color": "#7c3aed",            //   the footer; non-empty = custom.
//!     "logo_url": null
//!   }
//! }
//! ```
//! Returns 200 with the resulting tier + raw stored override + the resolved
//! `WidgetBrand` (what the widget will actually see) so the operator can confirm
//! the flip in one round-trip. This is the endpoint the GitCellar self-host flip
//! is driven through (set `tier=self_host` + `footer_text_override=""` now; clear
//! the override when feedbackmonk.com is live) instead of raw SQL.

use axum::extract::{Path, State};
use axum::routing::patch;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{Tier, WidgetBrand};
use feedbackmonk_repository::WidgetBrandOverride;

use crate::auth::OpsAuth;
use crate::error::ApiError;
use crate::state::AppState;

const MAX_FOOTER_TEXT_LEN: usize = 120;
const MAX_URL_LEN: usize = 2048;

#[derive(Debug, Deserialize)]
pub struct OpsTenantPatch {
    /// Pricing tier wire value (`free|starter|pro|self_host`). Absent ⇒ tier
    /// unchanged.
    #[serde(default)]
    pub tier: Option<String>,
    /// Full-replace widget brand override. Absent ⇒ overrides unchanged;
    /// present ⇒ every override column is set to the supplied value (absent
    /// sub-field ⇒ null/clear).
    #[serde(default)]
    pub branding: Option<BrandingOverrideBody>,
}

#[derive(Debug, Default, Deserialize)]
pub struct BrandingOverrideBody {
    #[serde(default)]
    pub footer_text_override: Option<String>,
    #[serde(default)]
    pub footer_url: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub primary_color: Option<String>,
    #[serde(default)]
    pub logo_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpsTenantResponse {
    pub tenant_id: Uuid,
    pub tier: String,
    /// Raw stored override columns (post-write).
    pub brand_override: WidgetBrandOverride,
    /// The resolved brand the widget-config endpoint will return — override
    /// layered over the tier default. Lets the operator confirm the effect.
    pub resolved_widget_brand: WidgetBrand,
}

fn validate_theme(theme: Option<&str>) -> Result<(), ApiError> {
    if let Some(t) = theme {
        if !matches!(t, "auto" | "light" | "dark") {
            return Err(ApiError::BadRequest(
                "theme must be one of: auto, light, dark".into(),
            ));
        }
    }
    Ok(())
}

/// Light hex-color validation: `#` + exactly 3 or 6 hex digits. Keeps obviously
/// broken values out; the widget treats it as a CSS custom-prop value.
fn validate_primary_color(color: Option<&str>) -> Result<(), ApiError> {
    if let Some(c) = color {
        let ok = (c.len() == 4 || c.len() == 7)
            && c.starts_with('#')
            && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
        if !ok {
            return Err(ApiError::BadRequest(
                "primary_color must be a hex color like #2563eb or #abc".into(),
            ));
        }
    }
    Ok(())
}

fn validate_url(field: &str, url: Option<&str>) -> Result<(), ApiError> {
    if let Some(u) = url {
        if u.len() > MAX_URL_LEN {
            return Err(ApiError::BadRequest(format!("{field} too long")));
        }
        if !(u.starts_with("https://") || u.starts_with("http://")) {
            return Err(ApiError::BadRequest(format!(
                "{field} must be an http(s) URL"
            )));
        }
    }
    Ok(())
}

fn validate_branding(b: &BrandingOverrideBody) -> Result<(), ApiError> {
    validate_theme(b.theme.as_deref())?;
    validate_primary_color(b.primary_color.as_deref())?;
    validate_url("footer_url", b.footer_url.as_deref())?;
    validate_url("logo_url", b.logo_url.as_deref())?;
    if let Some(t) = &b.footer_text_override {
        if t.len() > MAX_FOOTER_TEXT_LEN {
            return Err(ApiError::BadRequest(format!(
                "footer_text_override must be <= {MAX_FOOTER_TEXT_LEN} chars (use \"\" to suppress)"
            )));
        }
    }
    Ok(())
}

pub async fn patch_tenant(
    State(state): State<AppState>,
    _ops: OpsAuth,
    Path(tenant_id): Path<Uuid>,
    Json(req): Json<OpsTenantPatch>,
) -> Result<Json<OpsTenantResponse>, ApiError> {
    // Validate before any write.
    let tier = match &req.tier {
        Some(s) => Some(
            Tier::from_db_str(s)
                .map_err(|_| ApiError::BadRequest(format!("unknown tier {s:?}")))?,
        ),
        None => None,
    };
    if let Some(b) = &req.branding {
        validate_branding(b)?;
    }

    // Resolve a scope from the path tenant_id (404 if no such tenant). This is
    // the allowlisted `scope_for` pre-auth bridge — the ops token has already
    // authorized the caller.
    let scope = state.tenants.scope_for(tenant_id).await?;

    if let Some(tier) = tier {
        state.tenants.set_tier(&scope, tier).await?;
    }
    if let Some(b) = req.branding {
        let over = WidgetBrandOverride {
            footer_text_override: b.footer_text_override,
            footer_url: b.footer_url,
            theme: b.theme,
            primary_color: b.primary_color,
            logo_url: b.logo_url,
        };
        state.tenants.set_widget_brand_override(&scope, &over).await?;
    }

    let current_tier = state.tenants.get_tier(&scope).await?;
    let brand_override = state.tenants.get_widget_brand_override(&scope).await?;
    let resolved_widget_brand = state.tenants.get_widget_brand(&scope).await?;

    Ok(Json(OpsTenantResponse {
        tenant_id,
        tier: current_tier.as_db_str().to_string(),
        brand_override,
        resolved_widget_brand,
    }))
}

/// Ops router. Merged WITHOUT the public CORS layer (operator-only, called
/// server-side / via curl — never from a browser embed).
pub fn ops_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/ops/tenants/:tenant_id", patch(patch_tenant))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{header::AUTHORIZATION, Request, StatusCode};
    use chrono::{Duration, Utc};
    use sqlx::PgPool;
    use tower::ServiceExt;

    use feedbackmonk_anon::{AnonGate, DEFAULT_RATE_LIMIT_PER_HOUR};
    use feedbackmonk_repository::{
        SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
        SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxRoadmapItemRepo,
        SqlxRoadmapVoteRepo, SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo, TenantRepo,
    };

    use crate::email::Mailer;
    use crate::roadmap_voting_cache::VotingCache;
    use std::num::NonZeroU32;

    const OPS_TOKEN: &str = "test-ops-secret-token";

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

    fn build_state(pool: &PgPool, ops_token: Option<&str>) -> AppState {
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
            session_secret: Arc::new([0x55u8; 32]),
            public_url: Arc::from("http://localhost:14304"),
            verify_token_ttl: Duration::hours(24),
            anon_gate: AnonGate::new(NonZeroU32::new(DEFAULT_RATE_LIMIT_PER_HOUR).unwrap()),
            login_gate: feedbackmonk_anon::LoginGate::with_default_quota(),
            jwt_iat_leeway_seconds: 5,
            roadmap_items: Arc::new(SqlxRoadmapItemRepo::new(pool.clone())),
            roadmap_votes: Arc::new(SqlxRoadmapVoteRepo::new(pool.clone())),
            voting_cache: VotingCache::new(),
            started_at: Utc::now(),
            health: SqlxHealthCheck::new(pool.clone()),
            tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
            ops_token: ops_token.map(Arc::from),
        }
    }

    async fn seed_tenant(pool: &PgPool, email: &str) -> Uuid {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create(email, "h").await.unwrap();
        t.id
    }

    fn patch_req(tenant_id: Uuid, token: Option<&str>, body: &serde_json::Value) -> Request<Body> {
        let mut b = Request::patch(format!("/api/v1/ops/tenants/{tenant_id}"))
            .header("content-type", "application/json");
        if let Some(tok) = token {
            b = b.header(AUTHORIZATION, format!("Bearer {tok}"));
        }
        b.body(Body::from(body.to_string())).unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn missing_token_yields_401(pool: PgPool) {
        let state = build_state(&pool, Some(OPS_TOKEN));
        let tid = seed_tenant(&pool, "a@example.com").await;
        let app = ops_router(state);
        let resp = app
            .oneshot(patch_req(tid, None, &serde_json::json!({"tier": "pro"})))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn wrong_token_yields_401(pool: PgPool) {
        let state = build_state(&pool, Some(OPS_TOKEN));
        let tid = seed_tenant(&pool, "b@example.com").await;
        let app = ops_router(state);
        let resp = app
            .oneshot(patch_req(tid, Some("not-the-token"), &serde_json::json!({"tier": "pro"})))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn token_unset_disables_endpoint_with_404(pool: PgPool) {
        // FEEDBACKMONK_OPS_TOKEN unset ⇒ ops surface invisible.
        let state = build_state(&pool, None);
        let tid = seed_tenant(&pool, "c@example.com").await;
        let app = ops_router(state);
        let resp = app
            .oneshot(patch_req(tid, Some(OPS_TOKEN), &serde_json::json!({"tier": "pro"})))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn sets_tier_and_suppresses_footer_for_owner_tenant(pool: PgPool) {
        // The GitCellar flip: tier=self_host (generous quotas) + footer
        // suppressed ("") — branding decoupled from tier.
        let state = build_state(&pool, Some(OPS_TOKEN));
        let tid = seed_tenant(&pool, "owner@example.com").await;
        let app = ops_router(state.clone());
        let resp = app
            .oneshot(patch_req(
                tid,
                Some(OPS_TOKEN),
                &serde_json::json!({
                    "tier": "self_host",
                    "branding": {
                        "footer_text_override": "",
                        "theme": "auto"
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 8 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["tier"], "self_host");
        // Footer resolved to null (suppressed) even though SelfHost already
        // has no tier footer — and theme override stuck.
        assert!(body["resolved_widget_brand"]["footer_text"].is_null());
        assert_eq!(body["resolved_widget_brand"]["theme"], "auto");
        assert_eq!(body["brand_override"]["footer_text_override"], "");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn rejects_bad_theme_and_color(pool: PgPool) {
        let state = build_state(&pool, Some(OPS_TOKEN));
        let tid = seed_tenant(&pool, "bad@example.com").await;
        let app = ops_router(state);
        let resp = app
            .clone()
            .oneshot(patch_req(
                tid,
                Some(OPS_TOKEN),
                &serde_json::json!({"branding": {"theme": "neon"}}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = app
            .oneshot(patch_req(
                tid,
                Some(OPS_TOKEN),
                &serde_json::json!({"branding": {"primary_color": "blue"}}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn unknown_tenant_yields_404(pool: PgPool) {
        let state = build_state(&pool, Some(OPS_TOKEN));
        let app = ops_router(state);
        let resp = app
            .oneshot(patch_req(
                Uuid::new_v4(),
                Some(OPS_TOKEN),
                &serde_json::json!({"tier": "pro"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
