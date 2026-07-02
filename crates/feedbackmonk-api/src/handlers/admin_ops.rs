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

use axum::extract::{Path, Query, State};
use axum::routing::{patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{Tier, WidgetBrand};
use feedbackmonk_repository::{TranslationFlag, WidgetBrandOverride};

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

// ---------- FR-FBR-30 (#5): manual translation backfill ----------

/// Default backfill batch size when the operator omits `?limit=`.
const DEFAULT_BACKFILL_LIMIT: i64 = 1000;
/// Hard cap on a single backfill call so one request can't lock a huge table.
const MAX_BACKFILL_LIMIT: i64 = 100_000;

#[derive(Debug, Deserialize)]
pub struct BackfillParams {
    /// Max rows to stamp `pending` in this call. Operator calls repeatedly until
    /// `stamped` is 0. Defaults to `DEFAULT_BACKFILL_LIMIT`.
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct BackfillResponse {
    /// Rows stamped `pending` by this call (0 ⇒ nothing left to backfill).
    pub stamped: u64,
    /// Whether a translation provider is currently enabled. When `false`, the
    /// stamped rows wait until a provider is configured (no worker drains them
    /// yet) — surfaced so the operator notices a misconfiguration.
    pub translation_enabled: bool,
}

/// `POST /api/v1/ops/translation/backfill?limit=N` — stamp never-considered,
/// body-bearing feedback rows as `pending` so the translate-after-accept worker
/// picks them up (FR-FBR-30 lazy-backfill escape hatch, #5). Operator surface,
/// guarded by `OpsAuth` (404 when `FEEDBACKMONK_OPS_TOKEN` unset). Idempotent in
/// effect: re-running only stamps rows that are still `translation_status IS
/// NULL`. Bounded by `limit`; call until `stamped` is 0.
pub async fn backfill_translations(
    State(state): State<AppState>,
    _ops: OpsAuth,
    Query(params): Query<BackfillParams>,
) -> Result<Json<BackfillResponse>, ApiError> {
    let limit = params.limit.unwrap_or(DEFAULT_BACKFILL_LIMIT);
    if limit <= 0 || limit > MAX_BACKFILL_LIMIT {
        return Err(ApiError::BadRequest(format!(
            "limit must be between 1 and {MAX_BACKFILL_LIMIT}"
        )));
    }
    let stamped = state.feedback.backfill_pending_translations(limit).await?;
    Ok(Json(BackfillResponse {
        stamped,
        translation_enabled: TranslationFlag::enabled(),
    }))
}

/// Ops router. Merged WITHOUT the public CORS layer (operator-only, called
/// server-side / via curl — never from a browser embed).
pub fn ops_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/ops/tenants/:tenant_id", patch(patch_tenant))
        .route(
            "/api/v1/ops/translation/backfill",
            post(backfill_translations),
        )
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
    use feedbackmonk_core::FeedbackKind;
    use feedbackmonk_repository::{
        FeedbackRepo, ProjectRepo, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo,
        SqlxFeedbackRepo, SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo,
        SqlxRoadmapItemRepo, SqlxRoadmapVoteRepo, SqlxSigningKeyRepo, SqlxTenantRepo,
        SqlxTierQuotaRepo, TenantRepo,
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
        async fn send_password_reset_email(&self, _to: &str, _link: &str) -> anyhow::Result<()> {
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
            ip_gate: feedbackmonk_anon::IpGate::with_default_quota(),
            trusted_proxy_hops: 0,
            jwt_iat_leeway_seconds: 5,
            roadmap_items: Arc::new(SqlxRoadmapItemRepo::new(pool.clone())),
            roadmap_votes: Arc::new(SqlxRoadmapVoteRepo::new(pool.clone())),
            board_votes: Arc::new(feedbackmonk_repository::SqlxBoardVoteRepo::new(pool.clone())),
            voting_cache: VotingCache::new(),
            started_at: Utc::now(),
            health: SqlxHealthCheck::new(pool.clone()),
            tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
            ops_token: ops_token.map(Arc::from),
            clusters: Arc::new(feedbackmonk_repository::SqlxClusterRepo::new(pool.clone())),
            recommendations: Arc::new(feedbackmonk_repository::SqlxRecommendationRepo::new(pool.clone())),
            analysis_sweeps: Arc::new(feedbackmonk_repository::SqlxAnalysisSweepRepo::new(pool.clone())),
            work_orders: Arc::new(feedbackmonk_repository::SqlxWorkOrderRepo::new(pool.clone())),
            work_order_events: Arc::new(feedbackmonk_repository::SqlxWorkOrderEventRepo::new(pool.clone())),
            runner_tokens: Arc::new(feedbackmonk_repository::SqlxRunnerTokenRepo::new(pool.clone())),
            runner_token_revocations: Arc::new(feedbackmonk_repository::SqlxRunnerTokenRevocationRepo::new(pool.clone())),
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

    // ---------- FR-FBR-30 (#5): backfill endpoint ----------

    fn backfill_req(token: Option<&str>, query: &str) -> Request<Body> {
        let mut b = Request::post(format!("/api/v1/ops/translation/backfill{query}"));
        if let Some(tok) = token {
            b = b.header(AUTHORIZATION, format!("Bearer {tok}"));
        }
        b.body(Body::empty()).unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn backfill_requires_ops_token(pool: PgPool) {
        let state = build_state(&pool, Some(OPS_TOKEN));
        let app = ops_router(state);
        let resp = app.oneshot(backfill_req(None, "")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn backfill_rejects_bad_limit(pool: PgPool) {
        let state = build_state(&pool, Some(OPS_TOKEN));
        let app = ops_router(state);
        for q in ["?limit=0", "?limit=-3", "?limit=999999"] {
            let resp = app
                .clone()
                .oneshot(backfill_req(Some(OPS_TOKEN), q))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "limit {q}");
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn backfill_stamps_null_body_bearing_rows(pool: PgPool) {
        // Seed a project + two body-bearing feedback rows. Pin the flag OFF so
        // submit leaves translation_status NULL — exactly the backfill target.
        TranslationFlag::set(false);
        let state = build_state(&pool, Some(OPS_TOKEN));
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let frepo = SqlxFeedbackRepo::new(pool.clone());
        let t = trepo.create("backfill@example.com", "h").await.unwrap();
        let tscope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&tscope, "Proj", "bf-proj").await.unwrap();
        let pscope = prepo.open(&tscope, p.id).await.unwrap();
        for i in 0..2u8 {
            frepo
                .submit_anonymous(&pscope, &[i; 32], None, "Hallo", None, FeedbackKind::Bug)
                .await
                .unwrap();
        }

        let app = ops_router(state);
        let resp = app
            .oneshot(backfill_req(Some(OPS_TOKEN), "?limit=10"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 8 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["stamped"], 2);
        // No provider configured in tests ⇒ enabled flag false (operator hint).
        assert_eq!(body["translation_enabled"], false);
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
