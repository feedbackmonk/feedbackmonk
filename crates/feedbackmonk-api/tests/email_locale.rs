//! Integration tests for FR-FBR-37's END of the pipe: the language captured at
//! submit reaching the email a submitter actually receives.
//!
//! The unit tests in `email::send` pin the ladder itself
//! (`resolve_recipient_locale`); the unit tests in `email::templates` pin
//! English byte-identity. What neither can reach is the PLUMBING between them —
//! that `feedback.submitter_locale`, written by the submit handler, survives the
//! repository read and arrives in the `EmailContext` the notifier renders from.
//! A break there is silent: every email still sends, in English, forever.
//!
//! Invariants asserted (each a named test):
//!   1. `status_change_email_carries_the_submitters_locale` — submit in German,
//!      transition, and the notification context says `de`.
//!   2. `public_reply_email_carries_the_submitters_locale` — the second of the
//!      two paths that email a submitter; both read the same row.
//!   3. `unknown_submitter_locale_reaches_the_notifier_as_none` — the pre-
//!      FR-FBR-37 shape, which must keep rendering exactly as it does today.
//!   4. `tenant_locale_is_the_fallback_and_the_submitter_overrides_it` — the
//!      ladder over REAL stored values, both rungs, through the repository.
//!   5. `rendering_in_the_resolved_locale_never_emits_a_raw_key` — the C35
//!      rule-6 guarantee at the point of use, across every shipped locale.
//!
//! Real Postgres per test via `sqlx::test`. Harness shape ported from
//! `tests/submit_idempotency.rs`; the notifier is a recorder so the assertions
//! are about what was HANDED to the mailer, not about SMTP.

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use chrono::Duration;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::{render_status_change, resolve_recipient_locale, EmailContext, Mailer, StatusChangeContext};
use feedbackmonk_i18n::Locale;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{admin_feedback_routes, submission_router, worker_a_router};
use feedbackmonk_core::{FeedbackId, FeedbackStatus};
use feedbackmonk_repository::{
    ProjectScope, TenantScope, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
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
        email_notifier: Arc::new(RecordingNotifier::default()),
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

fn build_router(state: AppState) -> axum::Router {
    worker_a_router(state.clone())
        .merge(submission_router(state.clone()))
        .merge(admin_feedback_routes(state))
}


// ----- recording notifier -----------------------------------------------------

/// Captures every `EmailContext` handed to the send chokepoint. Unlike the
/// `NoopEmailNotifier` the other integration tests use, this one keeps the
/// context — which is the entire subject of this file.
#[derive(Default)]
struct RecordingNotifier {
    sent: std::sync::Mutex<Vec<(feedbackmonk_api::email::EmailKind, EmailContext)>>,
}

impl RecordingNotifier {
    fn last(&self) -> EmailContext {
        self.sent
            .lock()
            .unwrap()
            .last()
            .expect("no email was dispatched")
            .1
            .clone()
    }
}

#[async_trait::async_trait]
impl feedbackmonk_api::email::EmailNotifier for RecordingNotifier {
    async fn send_email(
        &self,
        _scope: &feedbackmonk_repository::TenantScope,
        kind: feedbackmonk_api::email::EmailKind,
        ctx: EmailContext,
    ) -> Result<feedbackmonk_api::email::SendOutcome, feedbackmonk_api::email::EmailError> {
        if ctx.submitter_email.is_none() {
            return Ok(feedbackmonk_api::email::SendOutcome::Skipped);
        }
        self.sent.lock().unwrap().push((kind, ctx));
        Ok(feedbackmonk_api::email::SendOutcome::Sent)
    }
}

// ----- helpers ----------------------------------------------------------------

/// Build the app state with a recording notifier, returning both so a test can
/// inspect what the chokepoint received.
fn state_with_recorder(pool: &PgPool) -> (AppState, Arc<RecordingNotifier>) {
    let recorder = Arc::new(RecordingNotifier::default());
    let mut state = build_test_state(pool);
    state.email_notifier = recorder.clone();
    (state, recorder)
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

async fn seed_project_for(state: &AppState, tscope: &TenantScope) -> (ProjectScope, Uuid) {
    let p = state
        .projects
        .create(
            tscope,
            "Proj",
            &format!("p-{}", &tscope.tenant_id().to_string()[..8]),
        )
        .await
        .unwrap();
    let pscope = state.projects.open(tscope, p.id).await.unwrap();
    (pscope, p.id)
}

/// Submit anonymously WITH an email address (so the notification is addressable)
/// and an optional `locale`, returning the new feedback's short code.
async fn submit_with_locale(
    app: &axum::Router,
    project_id: Uuid,
    locale: Option<&str>,
) -> String {
    let mut body = json!({"body": "something is broken", "email": "submitter@example.com"});
    if let Some(l) = locale {
        body["locale"] = json!(l);
    }
    let mut req = Request::post(format!("/api/v1/projects/{project_id}/feedback"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:54321".parse().unwrap()));
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "submit failed");
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    json["feedback_id"].as_str().unwrap().to_string()
}

async fn transition(app: &axum::Router, cookie: &str, fb: &str, to: &str) -> StatusCode {
    let req = Request::post(format!("/api/v1/admin/feedback/{fb}/transition"))
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .body(Body::from(json!({"to_status": to}).to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

async fn public_reply(app: &axum::Router, cookie: &str, fb: &str) -> StatusCode {
    let req = Request::post(format!("/api/v1/admin/feedback/{fb}/reply"))
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .body(Body::from(
            json!({"body": "we are on it", "visibility": "public"}).to_string(),
        ))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

// ----- tests ------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn status_change_email_carries_the_submitters_locale(pool: PgPool) {
    let (state, recorder) = state_with_recorder(&pool);
    let app = build_router(state.clone());
    let (tscope, cookie) = seed_admin(&state, "status@example.com").await;
    let (_pscope, project_id) = seed_project_for(&state, &tscope).await;

    let fb = submit_with_locale(&app, project_id, Some("de")).await;
    assert_eq!(
        transition(&app, &cookie, &fb, "triaged").await,
        StatusCode::OK
    );

    let ctx = recorder.last();
    assert_eq!(
        ctx.submitter_locale.as_deref(),
        Some("de"),
        "the language captured at submit did not survive to the notification"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn public_reply_email_carries_the_submitters_locale(pool: PgPool) {
    let (state, recorder) = state_with_recorder(&pool);
    let app = build_router(state.clone());
    let (tscope, cookie) = seed_admin(&state, "reply@example.com").await;
    let (_pscope, project_id) = seed_project_for(&state, &tscope).await;

    let fb = submit_with_locale(&app, project_id, Some("ja")).await;
    assert_eq!(public_reply(&app, &cookie, &fb).await, StatusCode::OK);

    assert_eq!(recorder.last().submitter_locale.as_deref(), Some("ja"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_submitter_locale_reaches_the_notifier_as_none(pool: PgPool) {
    let (state, recorder) = state_with_recorder(&pool);
    let app = build_router(state.clone());
    let (tscope, cookie) = seed_admin(&state, "none@example.com").await;
    let (_pscope, project_id) = seed_project_for(&state, &tscope).await;

    // The pre-FR-FBR-37 shape: no locale anywhere. This is what every existing
    // production row looks like, and it must keep rendering English.
    let fb = submit_with_locale(&app, project_id, None).await;
    assert_eq!(
        transition(&app, &cookie, &fb, "triaged").await,
        StatusCode::OK
    );

    let ctx = recorder.last();
    assert_eq!(ctx.submitter_locale, None);
    assert_eq!(
        resolve_recipient_locale(ctx.submitter_locale.as_deref(), None),
        Locale::EN
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn tenant_locale_is_the_fallback_and_the_submitter_overrides_it(pool: PgPool) {
    let (state, recorder) = state_with_recorder(&pool);
    let app = build_router(state.clone());
    let (tscope, cookie) = seed_admin(&state, "ladder@example.com").await;
    let (_pscope, project_id) = seed_project_for(&state, &tscope).await;

    // The admin's console is in French…
    state.tenants.set_locale(&tscope, Some("fr")).await.unwrap();
    let tenant_locale = state.tenants.get_locale(&tscope).await.unwrap();
    assert_eq!(tenant_locale.as_deref(), Some("fr"));

    // …a submitter with NO captured language gets the tenant's, through the
    // same values the chokepoint reads.
    let anon = submit_with_locale(&app, project_id, None).await;
    assert_eq!(
        transition(&app, &cookie, &anon, "triaged").await,
        StatusCode::OK
    );
    let ctx = recorder.last();
    assert_eq!(
        resolve_recipient_locale(ctx.submitter_locale.as_deref(), tenant_locale.as_deref()),
        Locale::parse("fr").unwrap()
    );

    // …and a submitter WITH one overrides it: this mail goes to the submitter,
    // not the admin, so the admin's language must not win.
    let german = submit_with_locale(&app, project_id, Some("de")).await;
    assert_eq!(
        transition(&app, &cookie, &german, "triaged").await,
        StatusCode::OK
    );
    let ctx = recorder.last();
    assert_eq!(
        resolve_recipient_locale(ctx.submitter_locale.as_deref(), tenant_locale.as_deref()),
        Locale::parse("de").unwrap()
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn rendering_in_the_resolved_locale_never_emits_a_raw_key(pool: PgPool) {
    let (state, recorder) = state_with_recorder(&pool);
    let app = build_router(state.clone());
    let (tscope, cookie) = seed_admin(&state, "render@example.com").await;
    let (_pscope, project_id) = seed_project_for(&state, &tscope).await;

    let fb = submit_with_locale(&app, project_id, Some("de")).await;
    assert_eq!(
        transition(&app, &cookie, &fb, "triaged").await,
        StatusCode::OK
    );
    let ctx = recorder.last();
    let brand = state.tenants.get_brand(&tscope).await.unwrap();
    let fb_id = FeedbackId::from(fb);

    // Every shipped locale, on the real brand and the real captured context.
    // A catalog in ANY state must produce a sendable email (C35 rule 6): text,
    // no raw `email.*` keys, no unfilled `{{placeholders}}`.
    for locale in Locale::all() {
        let rendered = render_status_change(
            &brand,
            &StatusChangeContext {
                feedback_id: &fb_id,
                from_status: FeedbackStatus::Submitted,
                to_status: FeedbackStatus::Triaged,
                reason_note: None,
                // FR-FBR-40 (additive): no outbound machine translation here —
                // this test is about the catalog, and `None` renders exactly the
                // pre-FR-FBR-40 body.
                translated_reason_note: None,
            },
            locale,
        );
        for part in [&rendered.subject, &rendered.body] {
            assert!(!part.is_empty(), "{locale}: empty part");
            assert!(!part.contains("email."), "{locale}: raw key leaked: {part}");
            assert!(!part.contains("{{"), "{locale}: unfilled placeholder: {part}");
        }
        assert!(
            rendered.subject.contains(fb_id.as_str()),
            "{locale}: Contract C10 subject lost the feedback id"
        );
    }

    // …and the resolved locale for THIS submitter is German, so the assertion
    // above is exercising the path the recipient would really take.
    assert_eq!(
        resolve_recipient_locale(ctx.submitter_locale.as_deref(), None),
        Locale::parse("de").unwrap()
    );
}
