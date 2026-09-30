//! Integration tests for `GET /api/v1/admin/feedback/:id/crash` (parity gap #2,
//! adoption contract §5.6): the stored `crash_event_id` resolves to the banner
//! shape through the correlator, best-effort, admin-only.
//!
//! Invariants asserted (each a named test):
//!   1. `linked_event_resolves_to_the_banner_shape` — a known id comes back
//!      `linked` with the contract's fields, and the detail read carries the id.
//!   2. `unconfigured_tracker_answers_unavailable_with_the_id` — no correlator
//!      (the four GLITCHTIP settings unset) is a 200, never an error.
//!   3. `tracker_down_or_unknown_id_degrades` — `unavailable` / `not_found`.
//!   4. `row_without_a_crash_id_answers_none` — and never calls the tracker.
//!   5. `requires_an_admin_session_and_the_owning_tenant` — 401 without a
//!      session; another tenant's admin gets 404, and the tracker is not asked.
//!   6. `the_tracker_serves_only_the_tenant_that_owns_it` — its token is one
//!      tenant's credential: another tenant's own crash-linked row answers
//!      `unavailable` and the tracker is never asked.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{
    admin_feedback_routes, crash_admin_router, CorrelationOutcome, CrashCorrelator, CrashEvent,
    CrashState,
};
use feedbackmonk_core::FeedbackKind;
use feedbackmonk_repository::{
    ProjectScope, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxSigningKeyRepo,
    SqlxTenantRepo, SqlxTierQuotaRepo, TenantScope,
};

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

/// Canned tracker: known events, a down switch, and a call counter.
#[derive(Default)]
struct FakeTracker {
    events: HashMap<String, CrashEvent>,
    down: bool,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl CrashCorrelator for FakeTracker {
    async fn correlate(&self, id: &str) -> CorrelationOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.down {
            return CorrelationOutcome::Unavailable;
        }
        self.events.get(id).cloned().map_or(CorrelationOutcome::NotFound, CorrelationOutcome::Linked)
    }
}

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
        recommendations: Arc::new(feedbackmonk_repository::SqlxRecommendationRepo::new(pool.clone())),
        analysis_sweeps: Arc::new(feedbackmonk_repository::SqlxAnalysisSweepRepo::new(pool.clone())),
        work_orders: Arc::new(feedbackmonk_repository::SqlxWorkOrderRepo::new(pool.clone())),
        work_order_events: Arc::new(feedbackmonk_repository::SqlxWorkOrderEventRepo::new(pool.clone())),
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

fn app(
    state: &AppState,
    correlator: Option<Arc<dyn CrashCorrelator>>,
    tenant: &TenantScope,
) -> axum::Router {
    let tenant_id = Some(tenant.tenant_id());
    admin_feedback_routes(state.clone())
        .merge(crash_admin_router(CrashState { app: state.clone(), correlator, tenant_id }))
}

async fn seed_admin(state: &AppState, email: &str) -> (TenantScope, String) {
    let tenant = state.tenants.create(email, "hash").await.unwrap();
    let scope = state.tenants.scope_for(tenant.id).await.unwrap();
    state.tenants.mark_verified(&scope).await.unwrap();
    let cookie = feedbackmonk_api::auth::issue_session_cookie(
        tenant.id,
        i64::from(tenant.session_epoch),
        state.session_secret.as_ref(),
    )
    .to_string()
    .split(';')
    .next()
    .unwrap()
    .to_string();
    (scope, cookie)
}

async fn seed_project(state: &AppState, tscope: &TenantScope) -> ProjectScope {
    let p = state
        .projects
        .create(tscope, "Proj", &format!("p-{}", &tscope.tenant_id().to_string()[..8]))
        .await
        .unwrap();
    state.projects.open(tscope, p.id).await.unwrap()
}

async fn seed_feedback(state: &AppState, scope: &ProjectScope, crash_event_id: Option<&str>) -> String {
    state
        .feedback
        .submit_authenticated_full(
            scope, "user|1", None, None, None, crash_event_id, "app panicked on save", None, None,
            None, FeedbackKind::Bug, None, None,
        )
        .await
        .unwrap()
        .feedback_id
        .as_str()
        .to_string()
}

async fn get(app: &axum::Router, path: &str, cookie: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::get(path);
    if let Some(c) = cookie {
        req = req.header("cookie", c);
    }
    let resp = app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn event(id: &str) -> CrashEvent {
    CrashEvent {
        crash_event_id: id.to_string(),
        title: "TypeError: cannot read 'id' of undefined".to_string(),
        culprit: Some("renderBanner (app/banner.tsx)".to_string()),
        level: Some("fatal".to_string()),
        permalink: Some("https://glitchtip.example/events/abc/".to_string()),
        last_seen: Some("2026-06-02T11:59:00Z".to_string()),
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn linked_event_resolves_to_the_banner_shape(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope, cookie) = seed_admin(&state, "crash-linked@example.com").await;
    let pscope = seed_project(&state, &tscope).await;
    let fb = seed_feedback(&state, &pscope, Some("evt-abc")).await;
    let mut tracker = FakeTracker::default();
    tracker.events.insert("evt-abc".into(), event("evt-abc"));
    let app = app(&state, Some(Arc::new(tracker)), &tscope);

    let (s, body) = get(&app, &format!("/api/v1/admin/feedback/{fb}/crash"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["status"], "linked");
    assert_eq!(body["crash_event_id"], "evt-abc");
    assert_eq!(body["crash"]["title"], "TypeError: cannot read 'id' of undefined");
    assert_eq!(body["crash"]["level"], "fatal");
    assert_eq!(body["crash"]["permalink"], "https://glitchtip.example/events/abc/");

    let (s, detail) = get(&app, &format!("/api/v1/admin/feedback/{fb}"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(detail["crash_event_id"], "evt-abc", "the detail read carries the stored id");
}

#[sqlx::test(migrations = "../../migrations")]
async fn unconfigured_tracker_answers_unavailable_with_the_id(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope, cookie) = seed_admin(&state, "crash-unconf@example.com").await;
    let pscope = seed_project(&state, &tscope).await;
    let fb = seed_feedback(&state, &pscope, Some("evt-1")).await;
    let app = app(&state, None, &tscope);

    let (s, body) = get(&app, &format!("/api/v1/admin/feedback/{fb}/crash"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["status"], "unavailable");
    assert_eq!(body["crash_event_id"], "evt-1");
    assert!(body.get("crash").is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn tracker_down_or_unknown_id_degrades(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope, cookie) = seed_admin(&state, "crash-degrade@example.com").await;
    let pscope = seed_project(&state, &tscope).await;
    let fb = seed_feedback(&state, &pscope, Some("evt-gone")).await;

    let down = FakeTracker { down: true, ..FakeTracker::default() };
    let (_, body) = get(&app(&state, Some(Arc::new(down)), &tscope), &format!("/api/v1/admin/feedback/{fb}/crash"), Some(&cookie)).await;
    assert_eq!(body["status"], "unavailable");

    let (s, body) = get(&app(&state, Some(Arc::new(FakeTracker::default())), &tscope), &format!("/api/v1/admin/feedback/{fb}/crash"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["status"], "not_found");
}

#[sqlx::test(migrations = "../../migrations")]
async fn row_without_a_crash_id_answers_none(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope, cookie) = seed_admin(&state, "crash-none@example.com").await;
    let pscope = seed_project(&state, &tscope).await;
    let fb = seed_feedback(&state, &pscope, None).await;
    let tracker = Arc::new(FakeTracker::default());
    let app = app(&state, Some(tracker.clone()), &tscope);

    let (s, body) = get(&app, &format!("/api/v1/admin/feedback/{fb}/crash"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["status"], "none");
    assert!(body["crash_event_id"].is_null());
    assert_eq!(tracker.calls.load(Ordering::SeqCst), 0, "no crash id, no tracker call");
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_an_admin_session_and_the_owning_tenant(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope, _cookie) = seed_admin(&state, "crash-owner@example.com").await;
    let pscope = seed_project(&state, &tscope).await;
    let fb = seed_feedback(&state, &pscope, Some("evt-private")).await;
    let (other_scope, other_cookie) = seed_admin(&state, "crash-other@example.com").await;
    seed_project(&state, &other_scope).await;
    let mut tracker = FakeTracker::default();
    tracker.events.insert("evt-private".into(), event("evt-private"));
    let tracker = Arc::new(tracker);
    let app = app(&state, Some(tracker.clone()), &tscope);

    let (s, _) = get(&app, &format!("/api/v1/admin/feedback/{fb}/crash"), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = get(&app, &format!("/api/v1/admin/feedback/{fb}/crash"), Some(&other_cookie)).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "another tenant's admin cannot resolve this row");
    assert_eq!(tracker.calls.load(Ordering::SeqCst), 0, "the tracker is never asked on a refused read");
}

#[sqlx::test(migrations = "../../migrations")]
async fn the_tracker_serves_only_the_tenant_that_owns_it(pool: PgPool) {
    let state = build_test_state(&pool);
    let (owner_scope, _owner_cookie) = seed_admin(&state, "crash-tracker-owner@example.com").await;
    seed_project(&state, &owner_scope).await;
    let (tscope, cookie) = seed_admin(&state, "crash-other-tenant@example.com").await;
    let pscope = seed_project(&state, &tscope).await;
    let fb = seed_feedback(&state, &pscope, Some("evt-owned")).await;
    let mut tracker = FakeTracker::default();
    tracker.events.insert("evt-owned".into(), event("evt-owned"));
    let tracker = Arc::new(tracker);
    // The tracker is bound to owner_scope's tenant; tscope's admin reads its own row.
    let app = app(&state, Some(tracker.clone()), &owner_scope);

    let (s, body) = get(&app, &format!("/api/v1/admin/feedback/{fb}/crash"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["status"], "unavailable");
    assert_eq!(body["crash_event_id"], "evt-owned");
    assert_eq!(tracker.calls.load(Ordering::SeqCst), 0, "another tenant never reaches the tracker");
}
