//! P3 Stage 1 integration smoke (FR-FBR-14, Contract C18 / C19).
//!
//! Three end-to-end scenarios exercised through the actual HTTP path
//! (axum router, real Postgres pool via `sqlx::test`):
//!
//! 1. Free-tier tenant creates 2nd project → 409 with structured
//!    `tier_cap_exceeded` body.
//! 2. Free-tier tenant submits 51st feedback in rolling 30-day window
//!    → 402 with same body shape.
//! 3. `GET /api/v1/projects/{id}/widget-config` for Free tenant
//!    returns `footer_text: Some("powered by feedbackmonk")`; for Pro
//!    tenant returns `footer_text: null`.
//!
//! These tests are what `tier-enforcement-status` Verification Oracle's
//! Probe C invokes when run with `--full`. Pattern mirrors
//! `tests/router_submission_integration.rs` (P1 Stage 3) — real router
//! shape, real DB, no mocks.

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::header::COOKIE;
use axum::http::{Request, StatusCode};
use chrono::Duration;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::{AnonGate, ANON_COOKIE_HEADER};
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{
    admin_feedback_routes, admin_tier_router, submission_router, widget_config_router,
    worker_a_router, VotingCache,
};
use feedbackmonk_repository::{
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxRoadmapItemRepo,
    SqlxRoadmapVoteRepo, SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo,
    WidgetBrandOverride,
};

// ----- Fakes ------------------------------------------------------------------

struct StubMailer;
#[async_trait::async_trait]
impl Mailer for StubMailer {
    async fn send_verify_email(&self, _to: &str, _link: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct NoopEmailNotifier;
#[async_trait::async_trait]
impl feedbackmonk_api::email::EmailNotifier for NoopEmailNotifier {
    async fn send_email(
        &self,
        _scope: &feedbackmonk_repository::TenantScope,
        _kind: feedbackmonk_api::email::EmailKind,
        _ctx: feedbackmonk_api::email::EmailContext,
    ) -> Result<feedbackmonk_api::email::SendOutcome, feedbackmonk_api::email::EmailError> {
        Ok(feedbackmonk_api::email::SendOutcome::Skipped)
    }
}

// ----- Test wiring ------------------------------------------------------------

fn build_test_state(pool: &PgPool, anon_quota: u32) -> AppState {
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
        session_secret: Arc::new([0x33u8; 32]),
        public_url: Arc::from("http://test.local"),
        verify_token_ttl: Duration::hours(24),
        // High anon quota so the rate-limit gate doesn't fire before the
        // tier-cap predicate does (scenario 2 needs ~51 submissions).
        anon_gate: AnonGate::new(NonZeroU32::new(anon_quota).unwrap()),
        login_gate: feedbackmonk_anon::LoginGate::with_default_quota(),
        ops_token: None,
        jwt_iat_leeway_seconds: 5,
        roadmap_items: Arc::new(SqlxRoadmapItemRepo::new(pool.clone())),
        roadmap_votes: Arc::new(SqlxRoadmapVoteRepo::new(pool.clone())),
        voting_cache: VotingCache::new(),
        started_at: chrono::Utc::now(),
        health: SqlxHealthCheck::new(pool.clone()),
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

fn build_router(state: AppState) -> axum::Router {
    worker_a_router(state.clone())
        .merge(submission_router(state.clone()))
        .merge(admin_feedback_routes(state.clone()))
        .merge(widget_config_router(state.clone()))
        .merge(admin_tier_router(state))
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 64 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Signup + verify-email + return the session cookie. Mirrors the P0
/// `signup_and_verify` helper in tests/handlers.rs (which uses a
/// `RecordingMailer` to extract the token); here we bypass the email
/// path entirely by calling the repo directly to mark verified, then
/// minting a session cookie via the auth helper.
async fn seed_verified_session(state: &AppState, email: &str, tier: &str) -> (Uuid, String) {
    let tenant = state.tenants.create(email, "h").await.unwrap();
    let scope = state.tenants.scope_for(tenant.id).await.unwrap();
    state.tenants.mark_verified(&scope).await.unwrap();
    SqlxTenantRepo::new(state.pool.clone())
        .set_tier_for_test(tenant.id, tier)
        .await
        .unwrap();
    let cookie = feedbackmonk_api::auth::issue_session_cookie(
        tenant.id,
        state.session_secret.as_ref(),
    );
    let cookie_str = cookie
        .to_string()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    (tenant.id, cookie_str)
}

fn create_project_request(cookie: &str, slug: &str) -> Request<Body> {
    Request::post("/api/v1/projects")
        .header("content-type", "application/json")
        .header(COOKIE, cookie)
        .body(Body::from(
            json!({"name": format!("Project {slug}"), "slug": slug}).to_string(),
        ))
        .unwrap()
}

fn submission_request(project_id: Uuid, body_text: &str, cookie: &str) -> Request<Body> {
    let mut req = Request::post(format!("/api/v1/projects/{project_id}/feedback"))
        .header("content-type", "application/json")
        .header(ANON_COOKIE_HEADER, cookie)
        .body(Body::from(
            json!({"body": body_text, "kind": "other"}).to_string(),
        ))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo::<SocketAddr>(
        "127.0.0.1:55555".parse().unwrap(),
    ));
    req
}

// ----- Scenario 1: 2nd project on Free → 409 ----------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn smoke_free_tenant_second_project_yields_409_tier_cap_exceeded(pool: PgPool) {
    let state = build_test_state(&pool, 1000);
    let (_tenant_id, cookie) = seed_verified_session(&state, "smoke-p1@example.com", "free").await;
    let app = build_router(state);

    // First project succeeds (cap = 1).
    let r1 = app
        .clone()
        .oneshot(create_project_request(&cookie, "first"))
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::OK, "first project must succeed");

    // Second project fires the cap → 409.
    let r2 = app
        .oneshot(create_project_request(&cookie, "second"))
        .await
        .unwrap();
    assert_eq!(
        r2.status(),
        StatusCode::CONFLICT,
        "second project on Free tier must yield 409"
    );
    let body = body_to_json(r2.into_body()).await;
    assert_eq!(body["error"], "tier_cap_exceeded");
    assert_eq!(body["tier"], "free");
    assert_eq!(body["resource"], "project");
    assert_eq!(body["current"], 1);
    assert_eq!(body["limit"], 1);
    assert!(
        body["upgrade_hint"].as_str().unwrap().contains("Starter"),
        "upgrade_hint must reference next tier; got {}",
        body["upgrade_hint"]
    );
}

// ----- Scenario 2: 51st feedback on Free → 402 --------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn smoke_free_tenant_51st_feedback_yields_402_tier_cap_exceeded(pool: PgPool) {
    // anon_quota set high so the rate-limit gate never fires before the
    // tier cap does. Per-(ip, cookie, project) bucket = anon_quota; we
    // rotate the cookie per-submission to dodge that gate.
    let state = build_test_state(&pool, 5000);
    let (_tenant_id, cookie) = seed_verified_session(&state, "smoke-p2@example.com", "free").await;

    // One project (within cap).
    let app = build_router(state);
    let r = app
        .clone()
        .oneshot(create_project_request(&cookie, "proj"))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = body_to_json(r.into_body()).await;
    let project_id_str = body["project_id"].as_str().unwrap().to_string();
    let project_id: Uuid = project_id_str.parse().unwrap();

    // Submit 50 feedback rows (anonymous mode; rotating cookies to
    // bypass per-bucket anon rate-limit). Each must succeed.
    for i in 0..50 {
        let req = submission_request(
            project_id,
            &format!("submission {i}"),
            &format!("rotating-cookie-{i}"),
        );
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "submission {i} (within cap) must succeed; got {}",
            resp.status()
        );
    }

    // 51st submission fires the cap → 402.
    let req = submission_request(project_id, "submission 50 (over cap)", "rotating-cookie-50");
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::PAYMENT_REQUIRED,
        "51st submission on Free tier must yield 402; got {}",
        resp.status()
    );
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["error"], "tier_cap_exceeded");
    assert_eq!(body["tier"], "free");
    assert_eq!(body["resource"], "feedback_in_rolling_month");
    assert_eq!(body["current"], 50);
    assert_eq!(body["limit"], 50);
    assert!(
        body["upgrade_hint"].as_str().unwrap().contains("Starter"),
        "upgrade_hint must reference next tier; got {}",
        body["upgrade_hint"]
    );
}

// ----- Scenario 3: widget-config tier-aware footer flip -----------------------

#[sqlx::test(migrations = "../../migrations")]
async fn smoke_widget_config_footer_flips_per_tier(pool: PgPool) {
    let state = build_test_state(&pool, 100);
    let app = build_router(state.clone());

    // ---- Free tenant: expect Some("powered by feedbackmonk") ----
    let (_t_free, cookie_free) =
        seed_verified_session(&state, "smoke-p3-free@example.com", "free").await;
    let r = app
        .clone()
        .oneshot(create_project_request(&cookie_free, "free-proj"))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = body_to_json(r.into_body()).await;
    let free_proj_id = body["project_id"].as_str().unwrap();

    let req = Request::get(format!("/api/v1/projects/{free_proj_id}/widget-config"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(
        body["brand"]["footer_text"], "powered by feedbackmonk",
        "Free tier widget-config MUST carry the powered-by footer"
    );

    // ---- Pro tenant: expect null ----
    let (_t_pro, cookie_pro) =
        seed_verified_session(&state, "smoke-p3-pro@example.com", "pro").await;
    let r = app
        .clone()
        .oneshot(create_project_request(&cookie_pro, "pro-proj"))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = body_to_json(r.into_body()).await;
    let pro_proj_id = body["project_id"].as_str().unwrap();

    let req = Request::get(format!("/api/v1/projects/{pro_proj_id}/widget-config"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert!(
        body["brand"]["footer_text"].is_null(),
        "Pro tier widget-config MUST have footer_text = null; got {}",
        body["brand"]["footer_text"]
    );
}

// ----- Scenario 4: per-tenant footer override supersedes tier default ---------
// DEC-FBR-IMPL-11: badge visibility decoupled from tier. This proves BOTH legs:
//   (a) a Free tenant with NO override still shows the footer (FR-FBR-14 default
//       holds — already asserted in Scenario 3), and
//   (b) a Free tenant whose admin set footer_text_override = "" suppresses the
//       footer while staying on Free (quotas unchanged).
// The override write goes through the repository (the ops endpoint's writer),
// then the public widget-config read is verified to reflect it.

#[sqlx::test(migrations = "../../migrations")]
async fn smoke_footer_override_supersedes_tier_on_free(pool: PgPool) {
    let state = build_test_state(&pool, 100);
    let app = build_router(state.clone());

    let (tenant_id, cookie) =
        seed_verified_session(&state, "smoke-p4-override@example.com", "free").await;
    let r = app
        .clone()
        .oneshot(create_project_request(&cookie, "ovr-proj"))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = body_to_json(r.into_body()).await;
    let proj_id = body["project_id"].as_str().unwrap().to_string();

    // Baseline (no override): Free tenant shows the badge — FR-FBR-14 default.
    let req = Request::get(format!("/api/v1/projects/{proj_id}/widget-config"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(
        body["brand"]["footer_text"], "powered by feedbackmonk",
        "Free tenant with no override MUST show the badge (FR-FBR-14)"
    );

    // Admin (ops) suppresses the footer via the override — tier stays Free.
    let scope = state.tenants.scope_for(tenant_id).await.unwrap();
    state
        .tenants
        .set_widget_brand_override(
            &scope,
            &WidgetBrandOverride {
                footer_text_override: Some(String::new()),
                theme: Some("dark".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let req = Request::get(format!("/api/v1/projects/{proj_id}/widget-config"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert!(
        body["brand"]["footer_text"].is_null(),
        "footer_text_override = \"\" MUST suppress the badge; got {}",
        body["brand"]["footer_text"]
    );
    assert_eq!(
        body["brand"]["theme"], "dark",
        "theme override must surface in widget-config"
    );
    // Tier untouched — still Free (caps unchanged).
    assert_eq!(state.tenants.get_tier(&scope).await.unwrap(), feedbackmonk_core::Tier::Free);
}
