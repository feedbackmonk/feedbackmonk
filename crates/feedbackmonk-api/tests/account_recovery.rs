//! Integration tests for the admin account-recovery surface (scrutiny P1-1 +
//! P2-1). Drives the real `worker_a_router` (logout/login/signup/verify) merged
//! with `account_recovery_router` (password reset + verify resend) against a
//! `sqlx::test` Postgres pool and a recording mailer that exposes the minted
//! tokens from the emailed links.
//!
//! Invariants asserted (one per flow):
//!   1. `logout_bumps_epoch_and_revokes_session` — a cookie valid before logout
//!      is rejected after (session revocation via `session_epoch`).
//!   2. `password_reset_request_unknown_email_202_no_token` — request for a
//!      nonexistent email returns 202 and mints NO token (no enumeration).
//!   3. `password_reset_full_flow_rotates_password_and_revokes` — request →
//!      confirm sets the new password (old fails, new logs in), the reset token
//!      is single-use, and a pre-reset cookie is revoked.
//!   4. `verify_email_resend_pending_tenant_mints_redeemable_token` — resend for
//!      a pending tenant mints a fresh, redeemable verify token.
//!   5. `signup_duplicate_email_returns_202_not_409` — duplicate signup does not
//!      enumerate (P2-1).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{Request, StatusCode};
use axum::Router;
use chrono::Duration;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::router::router as worker_a_router;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{account_recovery_router, AccountRecoveryState};
use feedbackmonk_repository::{
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxPasswordResetRepo, SqlxProjectRepo, SqlxSigningKeyRepo,
    SqlxTenantRepo, SqlxTierQuotaRepo,
};

// ----- Recording mailer -------------------------------------------------------

#[derive(Default)]
struct RecordingMailer {
    verify: Mutex<Vec<(String, String)>>,
    reset: Mutex<Vec<(String, String)>>,
}

#[async_trait::async_trait]
impl Mailer for RecordingMailer {
    async fn send_verify_email(&self, to: &str, link: &str) -> anyhow::Result<()> {
        self.verify.lock().unwrap().push((to.to_string(), link.to_string()));
        Ok(())
    }

    async fn send_password_reset_email(&self, to: &str, link: &str) -> anyhow::Result<()> {
        self.reset.lock().unwrap().push((to.to_string(), link.to_string()));
        Ok(())
    }
}

impl RecordingMailer {
    fn latest_verify_token(&self) -> Option<String> {
        let v = self.verify.lock().unwrap();
        let (_, link) = v.last()?;
        Some(link.split("token=").nth(1)?.to_string())
    }

    fn latest_reset_token(&self) -> Option<String> {
        let r = self.reset.lock().unwrap();
        let (_, link) = r.last()?;
        Some(link.split("token=").nth(1)?.to_string())
    }

    fn reset_count(&self) -> usize {
        self.reset.lock().unwrap().len()
    }
}

// ----- EmailNotifier no-op ----------------------------------------------------

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

// ----- Wiring -----------------------------------------------------------------

fn build_state(pool: &PgPool, mailer: Arc<RecordingMailer>) -> AppState {
    AppState {
        pool: pool.clone(),
        tenants: Arc::new(SqlxTenantRepo::new(pool.clone())),
        projects: Arc::new(SqlxProjectRepo::new(pool.clone())),
        signing_keys: Arc::new(SqlxSigningKeyRepo::new(pool.clone())),
        feedback: Arc::new(SqlxFeedbackRepo::new(pool.clone())),
        feedback_history: Arc::new(SqlxFeedbackStatusHistoryRepo::new(pool.clone())),
        feedback_replies: Arc::new(SqlxFeedbackReplyRepo::new(pool.clone())),
        email_verifications: Arc::new(SqlxEmailVerificationRepo::new(pool.clone())),
        mailer,
        email_notifier: Arc::new(NoopEmailNotifier),
        session_secret: Arc::new([0x42u8; 32]),
        public_url: Arc::from("http://test.local"),
        verify_token_ttl: Duration::hours(24),
        anon_gate: AnonGate::new(std::num::NonZeroU32::new(10).unwrap()),
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
        runner_token_revocations: Arc::new(feedbackmonk_repository::SqlxRunnerTokenRevocationRepo::new(pool.clone())),
        jwt_iat_leeway_seconds: 5,
        roadmap_items: Arc::new(feedbackmonk_repository::SqlxRoadmapItemRepo::new(pool.clone())),
        roadmap_votes: Arc::new(feedbackmonk_repository::SqlxRoadmapVoteRepo::new(pool.clone())),
        board_votes: Arc::new(feedbackmonk_repository::SqlxBoardVoteRepo::new(pool.clone())),
        voting_cache: feedbackmonk_api::VotingCache::new(),
        started_at: chrono::Utc::now(),
        health: feedbackmonk_repository::SqlxHealthCheck::new(pool.clone()),
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

fn build_ar_state(state: &AppState) -> AccountRecoveryState {
    AccountRecoveryState {
        tenants: Arc::clone(&state.tenants),
        password_resets: Arc::new(SqlxPasswordResetRepo::new(state.pool.clone())),
        email_verifications: Arc::clone(&state.email_verifications),
        mailer: Arc::clone(&state.mailer),
        login_gate: state.login_gate.clone(),
        public_url: Arc::clone(&state.public_url),
        reset_token_ttl: Duration::hours(1),
        verify_token_ttl: state.verify_token_ttl,
    }
}

/// The merged app: logout/login/signup/verify + reset/resend.
fn build_app(state: AppState) -> Router {
    let ar = build_ar_state(&state);
    worker_a_router(state).merge(account_recovery_router(ar))
}

/// Build a JSON POST with a `ConnectInfo` extension (login + reset/resend need
/// it for the IP-keyed rate-limit bucket).
#[allow(clippy::needless_pass_by_value)] // ergonomic: callers pass `json!(…)` temporaries
fn post_json(path: &str, body: serde_json::Value) -> Request<Body> {
    let mut req = Request::post(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:40000".parse().unwrap()));
    req
}

fn extract_cookie(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap().to_string()
}

/// Signup + verify; returns the session cookie (`name=value`).
async fn signup_and_verify(app: &Router, mailer: &RecordingMailer, email: &str) -> String {
    let resp = app
        .clone()
        .oneshot(post_json("/api/v1/signup", json!({"email": email, "password": "hunter22"})))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let token = mailer.latest_verify_token().expect("verify token captured");
    let resp = app
        .clone()
        .oneshot(post_json("/api/v1/verify-email", json!({"token": token})))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    extract_cookie(
        resp.headers().get(SET_COOKIE).expect("Set-Cookie").to_str().unwrap(),
    )
}

async fn get_projects_status(app: &Router, cookie: &str) -> StatusCode {
    let req = Request::get("/api/v1/projects")
        .header(COOKIE, cookie)
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

async fn login_status(app: &Router, email: &str, password: &str) -> StatusCode {
    app.clone()
        .oneshot(post_json("/api/v1/login", json!({"email": email, "password": password})))
        .await
        .unwrap()
        .status()
}

// ----- 1: logout revokes the session -----------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn logout_bumps_epoch_and_revokes_session(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let state = build_state(&pool, mailer.clone());
    let app = build_app(state);

    let cookie = signup_and_verify(&app, &mailer, "logout@example.com").await;
    // The fresh cookie authenticates an admin route.
    assert_eq!(get_projects_status(&app, &cookie).await, StatusCode::OK);

    // Logout succeeds with the valid cookie.
    let resp = app
        .clone()
        .oneshot(
            Request::post("/api/v1/logout")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The SAME cookie is now revoked (epoch bumped) — both a fresh logout and a
    // protected route reject it with 401.
    assert_eq!(get_projects_status(&app, &cookie).await, StatusCode::UNAUTHORIZED);
    let resp = app
        .oneshot(
            Request::post("/api/v1/logout")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ----- 2: reset request for unknown email → 202, no token --------------------

#[sqlx::test(migrations = "../../migrations")]
async fn password_reset_request_unknown_email_202_no_token(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let state = build_state(&pool, mailer.clone());
    let app = build_app(state);

    let resp = app
        .oneshot(post_json(
            "/api/v1/password-reset/request",
            json!({"email": "nobody@example.com"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED, "no enumeration: always 202");
    assert_eq!(mailer.reset_count(), 0, "no reset token minted for unknown email");
}

// ----- 3: full reset flow -----------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn password_reset_full_flow_rotates_password_and_revokes(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let state = build_state(&pool, mailer.clone());
    let app = build_app(state);

    // Existing verified tenant + a live pre-reset session cookie.
    let pre_reset_cookie = signup_and_verify(&app, &mailer, "reset@example.com").await;
    assert_eq!(get_projects_status(&app, &pre_reset_cookie).await, StatusCode::OK);

    // Request the reset.
    let resp = app
        .clone()
        .oneshot(post_json(
            "/api/v1/password-reset/request",
            json!({"email": "reset@example.com"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let token = mailer.latest_reset_token().expect("reset token captured");

    // Confirm with the token + a new password.
    let resp = app
        .clone()
        .oneshot(post_json(
            "/api/v1/password-reset/confirm",
            json!({"token": token, "new_password": "newpassw0rd"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Old password no longer logs in; the new one does.
    assert_eq!(
        login_status(&app, "reset@example.com", "hunter22").await,
        StatusCode::UNAUTHORIZED,
        "old password must fail after reset"
    );
    assert_eq!(
        login_status(&app, "reset@example.com", "newpassw0rd").await,
        StatusCode::OK,
        "new password must log in after reset"
    );

    // The reset token is single-use.
    let resp = app
        .clone()
        .oneshot(post_json(
            "/api/v1/password-reset/confirm",
            json!({"token": token, "new_password": "anotherpw1"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "reset token is single-use");

    // The pre-reset session cookie is revoked (epoch bumped by confirm).
    assert_eq!(
        get_projects_status(&app, &pre_reset_cookie).await,
        StatusCode::UNAUTHORIZED,
        "pre-reset session must be revoked"
    );
}

// ----- 4: verify-email resend for a pending tenant ---------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn verify_email_resend_pending_tenant_mints_redeemable_token(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let state = build_state(&pool, mailer.clone());
    let app = build_app(state);

    // Signup only — tenant stays pending (simulates the SMTP-brick scenario).
    let resp = app
        .clone()
        .oneshot(post_json(
            "/api/v1/signup",
            json!({"email": "pending@example.com", "password": "hunter22"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let verify_count_before = mailer.verify.lock().unwrap().len();

    // Resend → 202 + a fresh verify email.
    let resp = app
        .clone()
        .oneshot(post_json(
            "/api/v1/verify-email/resend",
            json!({"email": "pending@example.com"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    assert_eq!(
        mailer.verify.lock().unwrap().len(),
        verify_count_before + 1,
        "resend must mint a fresh verify email"
    );

    // The re-sent token redeems (account is no longer bricked).
    let token = mailer.latest_verify_token().expect("resent verify token");
    let resp = app
        .oneshot(post_json("/api/v1/verify-email", json!({"token": token})))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "resent token must be redeemable");
}

// ----- 5: signup duplicate → 202 (P2-1, no enumeration) ----------------------

#[sqlx::test(migrations = "../../migrations")]
async fn signup_duplicate_email_returns_202_not_409(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let state = build_state(&pool, mailer);
    let app = build_app(state);

    let first = app
        .clone()
        .oneshot(post_json("/api/v1/signup", json!({"email": "dupe@example.com", "password": "hunter22"})))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::ACCEPTED);

    let second = app
        .oneshot(post_json("/api/v1/signup", json!({"email": "dupe@example.com", "password": "hunter22"})))
        .await
        .unwrap();
    assert_eq!(
        second.status(),
        StatusCode::ACCEPTED,
        "duplicate signup must not reveal existence via 409"
    );
}
