//! Router-level integration tests for FR-FBR-38 / Contract C38 — the tenant
//! Language settings endpoint (`GET`/`PUT /api/v1/admin/settings/locale`).
//!
//! Invariants asserted (each a named test):
//!   1. `unset_tenant_reads_null_and_false` — an untouched tenant's baseline.
//!      `null` is *never chosen* (follow the browser / fall back to English),
//!      deliberately not `"en"`.
//!   2. `put_sets_and_get_reads_back` — the round trip, including the PUT echo
//!      reflecting STORED state rather than the request.
//!   3. `explicit_null_clears_the_locale` — and `absent_field_leaves_it_alone`:
//!      the absent-vs-null distinction that lets the SPA drive two independent
//!      controls without either resetting the other.
//!   4. `unknown_code_is_400_invalid_locale` — the machine-readable body C38
//!      specifies, with the stored value left untouched by the failed write.
//!   5. `translate_outbound_round_trips_independently` — the FR-FBR-40 reserve
//!      persists now even though nothing consults it yet.
//!   6. `requires_an_admin_session` — both verbs are 401 without a session.
//!   7. `settings_are_tenant_scoped` — tenant A's PUT is invisible to tenant B.
//!      The multi-tenant invariant (DEC-FBR-03) at the newest write surface.
//!
//! Real Postgres per test via `sqlx::test`. Harness shape ported from
//! `tests/submit_idempotency.rs`.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_repository::{
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxSigningKeyRepo,
    SqlxTenantRepo, SqlxTierQuotaRepo,
};

// ----- Fakes ------------------------------------------------------------------

struct StubMailer;
#[async_trait::async_trait]
impl Mailer for StubMailer {
    async fn send_verify_email(&self, _to: &str, _link: &str, _locale: feedbackmonk_i18n::Locale) -> anyhow::Result<()> {
        Ok(())
    }
    async fn send_password_reset_email(&self, _to: &str, _link: &str, _locale: feedbackmonk_i18n::Locale) -> anyhow::Result<()> {
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

// ----- Test wiring (mirrors router_submission_integration.rs) ------------------

fn build_test_state(pool: &PgPool) -> AppState {
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
        session_secret: Arc::new([0x42u8; 32]),
        public_url: Arc::from("http://test.local"),
        verify_token_ttl: Duration::hours(24),
        anon_gate: AnonGate::new(NonZeroU32::new(10).unwrap()),
        login_gate: feedbackmonk_anon::LoginGate::with_default_quota(),
        ip_gate: feedbackmonk_anon::IpGate::with_default_quota(),
        trusted_proxy_hops: 0,
        ops_token: None,
        clusters: Arc::new(feedbackmonk_repository::SqlxClusterRepo::new(pool.clone())),
        recommendations: Arc::new(feedbackmonk_repository::SqlxRecommendationRepo::new(
            pool.clone(),
        )),
        analysis_sweeps: Arc::new(feedbackmonk_repository::SqlxAnalysisSweepRepo::new(
            pool.clone(),
        )),
        work_orders: Arc::new(feedbackmonk_repository::SqlxWorkOrderRepo::new(pool.clone())),
        work_order_events: Arc::new(feedbackmonk_repository::SqlxWorkOrderEventRepo::new(
            pool.clone(),
        )),
        runner_tokens: Arc::new(feedbackmonk_repository::SqlxRunnerTokenRepo::new(pool.clone())),
        runner_token_revocations: Arc::new(
            feedbackmonk_repository::SqlxRunnerTokenRevocationRepo::new(pool.clone()),
        ),
        jwt_iat_leeway_seconds: 5,
        roadmap_items: Arc::new(feedbackmonk_repository::SqlxRoadmapItemRepo::new(pool.clone())),
        roadmap_votes: Arc::new(feedbackmonk_repository::SqlxRoadmapVoteRepo::new(pool.clone())),
        board_votes: Arc::new(feedbackmonk_repository::SqlxBoardVoteRepo::new(pool.clone())),
        voting_cache: feedbackmonk_api::VotingCache::new(),
        started_at: chrono::Utc::now(),
        health: SqlxHealthCheck::new(pool.clone()),
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

// ----- helpers ----------------------------------------------------------------

const PATH: &str = "/api/v1/admin/settings/locale";

fn settings_app(state: AppState) -> axum::Router {
    feedbackmonk_api::tenant_settings_router(state)
}

/// A verified tenant plus its admin session cookie.
async fn seed_admin(state: &AppState, email: &str) -> String {
    let tenant = state.tenants.create(email, "hash").await.unwrap();
    let scope = state.tenants.scope_for(tenant.id).await.unwrap();
    state.tenants.mark_verified(&scope).await.unwrap();
    feedbackmonk_api::auth::issue_session_cookie(
        tenant.id,
        i64::from(tenant.session_epoch),
        state.session_secret.as_ref(),
    )
    .to_string()
    .split(';')
    .next()
    .unwrap()
    .to_string()
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

async fn get_settings(app: &axum::Router, cookie: Option<&str>) -> (StatusCode, Value) {
    let mut b = Request::get(PATH);
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    send(app, b.body(Body::empty()).unwrap()).await
}

#[allow(clippy::needless_pass_by_value)] // owned `Value` keeps call sites cleaner
async fn put_settings(
    app: &axum::Router,
    cookie: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut b = Request::put(PATH).header("content-type", "application/json");
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    send(
        app,
        b.body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap(),
    )
    .await
}

// ----- tests ------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn unset_tenant_reads_null_and_false(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = settings_app(state.clone());
    let cookie = seed_admin(&state, "baseline@example.com").await;

    let (status, json) = get_settings(&app, Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        json["locale"].is_null(),
        "an untouched tenant has NEVER CHOSEN a language; that is not the same as choosing English"
    );
    assert_eq!(json["translate_outbound"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn put_sets_and_get_reads_back(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = settings_app(state.clone());
    let cookie = seed_admin(&state, "roundtrip@example.com").await;

    let (status, echoed) = put_settings(&app, Some(&cookie), json!({"locale": "pt-BR"})).await;
    assert_eq!(status, StatusCode::OK, "{echoed}");
    // The PUT echoes STORED state, so a partial update still shows both fields.
    assert_eq!(echoed["locale"], "pt-BR");
    assert_eq!(echoed["translate_outbound"], false);

    let (_, read) = get_settings(&app, Some(&cookie)).await;
    assert_eq!(read, echoed, "GET disagreed with the PUT echo");
}

#[sqlx::test(migrations = "../../migrations")]
async fn explicit_null_clears_the_locale(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = settings_app(state.clone());
    let cookie = seed_admin(&state, "clear@example.com").await;

    put_settings(&app, Some(&cookie), json!({"locale": "de"})).await;
    let (status, json) = put_settings(&app, Some(&cookie), json!({"locale": null})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["locale"].is_null(), "explicit null must CLEAR the setting");
}

#[sqlx::test(migrations = "../../migrations")]
async fn absent_field_leaves_it_alone(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = settings_app(state.clone());
    let cookie = seed_admin(&state, "absent@example.com").await;

    put_settings(&app, Some(&cookie), json!({"locale": "ja"})).await;
    // A translate_outbound-only PUT — the shape the SPA's second toggle sends.
    // If absent-vs-null were collapsed, this would silently wipe the language.
    let (status, json) =
        put_settings(&app, Some(&cookie), json!({"translate_outbound": true})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["locale"], "ja", "an absent field must not clear the stored value");
    assert_eq!(json["translate_outbound"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_code_is_400_invalid_locale(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = settings_app(state.clone());
    let cookie = seed_admin(&state, "invalid@example.com").await;

    put_settings(&app, Some(&cookie), json!({"locale": "de"})).await;

    for bad in ["da", "de-AT", "xx", "not a locale", "EN"] {
        let (status, json) = put_settings(&app, Some(&cookie), json!({"locale": bad})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} was accepted");
        // C38 specifies a machine-readable `code`, not `ApiError`'s `{"error": …}`.
        assert_eq!(json["code"], "invalid_locale", "{bad}: wrong error body");
    }

    // A rejected write leaves the previous setting intact — a failed validation
    // must not be a destructive operation.
    let (_, json) = get_settings(&app, Some(&cookie)).await;
    assert_eq!(json["locale"], "de");
}

#[sqlx::test(migrations = "../../migrations")]
async fn translate_outbound_round_trips_independently(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = settings_app(state.clone());
    let cookie = seed_admin(&state, "reserve@example.com").await;

    let (status, json) =
        put_settings(&app, Some(&cookie), json!({"translate_outbound": true})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["translate_outbound"], true);
    assert!(json["locale"].is_null());

    let (_, json) = put_settings(&app, Some(&cookie), json!({"translate_outbound": false})).await;
    assert_eq!(json["translate_outbound"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_an_admin_session(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = settings_app(state.clone());

    let (status, _) = get_settings(&app, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = put_settings(&app, None, json!({"locale": "de"})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn settings_are_tenant_scoped(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = settings_app(state.clone());
    let a = seed_admin(&state, "tenant-a@example.com").await;
    let b = seed_admin(&state, "tenant-b@example.com").await;

    put_settings(&app, Some(&a), json!({"locale": "de", "translate_outbound": true})).await;

    // DEC-FBR-03: A's write is invisible to B. Every accessor on this path is
    // `&TenantScope`-first, and this is the behavioural proof of it.
    let (status, json) = get_settings(&app, Some(&b)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["locale"].is_null(), "tenant B saw tenant A's language");
    assert_eq!(json["translate_outbound"], false);

    // …and A still sees its own.
    let (_, json) = get_settings(&app, Some(&a)).await;
    assert_eq!(json["locale"], "de");
    assert_eq!(json["translate_outbound"], true);
}
