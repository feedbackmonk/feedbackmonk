//! Integration tests for Worker A handlers.
//!
//! Drives the actual `Router` produced by `feedbackmonk_api::router::router` with
//! a real `sqlx::test` Postgres pool + a `Mailer` fake that records sent
//! verify-emails. The fake exposes the token via `latest_token()` so tests
//! can complete the signup -> verify flow end-to-end.

use std::sync::{Arc, Mutex};

use axum::body::{to_bytes, Body};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::Duration;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::router::router;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_repository::{
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxProjectRepo, SqlxSigningKeyRepo, SqlxTenantRepo,
    SqlxTierQuotaRepo,
};

// ----- Mailer fake ------------------------------------------------------------

#[derive(Default)]
struct RecordingMailer {
    sent: Mutex<Vec<(String, String)>>,
}

#[async_trait::async_trait]
impl Mailer for RecordingMailer {
    async fn send_verify_email(&self, to: &str, link: &str) -> anyhow::Result<()> {
        self.sent.lock().unwrap().push((to.to_string(), link.to_string()));
        Ok(())
    }
}

impl RecordingMailer {
    fn latest_token(&self) -> Option<String> {
        let sent = self.sent.lock().unwrap();
        let (_, link) = sent.last()?;
        let token = link.split("token=").nth(1)?.to_string();
        Some(token)
    }

    fn send_count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

// ----- EmailNotifier no-op (P0 handler tests don't exercise it) ---------------

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

fn build_test_state(pool: &PgPool, mailer: Arc<RecordingMailer>) -> (AppState, Arc<RecordingMailer>) {
    let state = AppState {
        pool: pool.clone(),
        tenants: Arc::new(SqlxTenantRepo::new(pool.clone())),
        projects: Arc::new(SqlxProjectRepo::new(pool.clone())),
        signing_keys: Arc::new(SqlxSigningKeyRepo::new(pool.clone())),
        feedback: Arc::new(SqlxFeedbackRepo::new(pool.clone())),
        feedback_history: Arc::new(SqlxFeedbackStatusHistoryRepo::new(pool.clone())),
        feedback_replies: Arc::new(SqlxFeedbackReplyRepo::new(pool.clone())),
        email_verifications: Arc::new(SqlxEmailVerificationRepo::new(pool.clone())),
        mailer: mailer.clone(),
        email_notifier: Arc::new(NoopEmailNotifier),
        session_secret: Arc::new([0x42u8; 32]),
        public_url: Arc::from("http://test.local"),
        verify_token_ttl: Duration::hours(24),
        anon_gate: AnonGate::new(std::num::NonZeroU32::new(10).unwrap()),
        jwt_iat_leeway_seconds: 5,
        // P2 fields — mechanical AppState extension per
        // docs/test-modifications/20260514-p2-appstate-roadmap-fields.md.
        roadmap_items: Arc::new(feedbackmonk_repository::SqlxRoadmapItemRepo::new(
            pool.clone(),
        )),
        roadmap_votes: Arc::new(feedbackmonk_repository::SqlxRoadmapVoteRepo::new(
            pool.clone(),
        )),
        voting_cache: feedbackmonk_api::VotingCache::new(),
        started_at: chrono::Utc::now(),
        health: feedbackmonk_repository::SqlxHealthCheck::new(pool.clone()),
        // P3 Stage 1 fixture extension — see
        // docs/test-modifications/20260514-p3-appstate-tier-quotas.md.
        // Tier defaults to Free for newly-created tenants; the
        // signup/projects/signing-keys tests in this file create at most
        // one project and zero feedback per tenant, well within the
        // Free-tier caps (1 project / 50 feedback rolling).
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    };
    (state, mailer)
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 64 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn extract_session_cookie(set_cookie: &str) -> String {
    // "feedbackmonk_session=...; HttpOnly; ..."
    set_cookie
        .split(';')
        .next()
        .expect("Set-Cookie value")
        .to_string()
}

async fn signup_and_verify(state: AppState, mailer: &RecordingMailer, email: &str) -> String {
    let app = router(state.clone());

    // signup
    let req = Request::post("/api/v1/signup")
        .header("content-type", "application/json")
        .body(Body::from(json!({"email": email, "password": "hunter22"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // verify-email
    let token = mailer.latest_token().expect("mailer captured token");
    let req = Request::post("/api/v1/verify-email")
        .header("content-type", "application/json")
        .body(Body::from(json!({"token": token}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    extract_session_cookie(
        resp.headers()
            .get(SET_COOKIE)
            .expect("Set-Cookie issued")
            .to_str()
            .unwrap(),
    )
}

// ----- Signup -----------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn signup_happy_path_creates_tenant_and_sends_email(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, mailer) = build_test_state(&pool, mailer);
    let app = router(state);

    let req = Request::post("/api/v1/signup")
        .header("content-type", "application/json")
        .body(Body::from(json!({"email": "a@example.com", "password": "hunter22"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body = body_to_json(resp.into_body()).await;
    assert!(body["tenant_id"].is_string());
    assert_eq!(mailer.send_count(), 1);
    assert!(mailer.latest_token().is_some());
}

#[sqlx::test(migrations = "../../migrations")]
async fn signup_duplicate_email_yields_409(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, _) = build_test_state(&pool, mailer);
    let app = router(state);

    let make_req = || {
        Request::post("/api/v1/signup")
            .header("content-type", "application/json")
            .body(Body::from(json!({"email": "dup@example.com", "password": "hunter22"}).to_string()))
            .unwrap()
    };

    let resp = app.clone().oneshot(make_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let resp = app.oneshot(make_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn signup_rejects_bad_email_and_short_password(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, _) = build_test_state(&pool, mailer);
    let app = router(state);

    let bad_email = Request::post("/api/v1/signup")
        .header("content-type", "application/json")
        .body(Body::from(json!({"email": "garbage", "password": "hunter22"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(bad_email).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let short_pw = Request::post("/api/v1/signup")
        .header("content-type", "application/json")
        .body(Body::from(json!({"email": "ok@example.com", "password": "x"}).to_string()))
        .unwrap();
    let resp = app.oneshot(short_pw).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ----- Verify-email -----------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn verify_email_happy_path_marks_verified_and_issues_session(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, mailer) = build_test_state(&pool, mailer);
    let _cookie = signup_and_verify(state.clone(), &mailer, "ver@example.com").await;

    // Use the repository (single query path per DEC-FBR-03) to confirm the
    // tenant transitioned to verified state.
    let row = state
        .tenants
        .find_by_email("ver@example.com")
        .await
        .unwrap()
        .expect("signup created the tenant");
    assert!(row.verified_at.is_some(), "tenant verified_at should be set");
}

#[sqlx::test(migrations = "../../migrations")]
async fn verify_email_idempotent_within_replay_window(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, mailer) = build_test_state(&pool, mailer);
    let app = router(state);

    // signup
    let req = Request::post("/api/v1/signup")
        .header("content-type", "application/json")
        .body(Body::from(json!({"email": "rep@example.com", "password": "hunter22"}).to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap();
    let token = mailer.latest_token().unwrap();

    let make_verify = || {
        Request::post("/api/v1/verify-email")
            .header("content-type", "application/json")
            .body(Body::from(json!({"token": token.clone()}).to_string()))
            .unwrap()
    };

    let r1 = app.clone().oneshot(make_verify()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let r2 = app.oneshot(make_verify()).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK, "second redemption within replay window must succeed");
}

#[sqlx::test(migrations = "../../migrations")]
async fn verify_email_unknown_token_yields_401(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, _) = build_test_state(&pool, mailer);
    let app = router(state);

    let req = Request::post("/api/v1/verify-email")
        .header("content-type", "application/json")
        .body(Body::from(json!({"token": "never-issued-token"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn verify_email_expired_token_yields_410(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    // Override TTL to a negative duration -- signup will mint a token that's
    // already past its expiry. No raw SQL needed: the existing handler path
    // creates the row via the repository, the oracle stays happy.
    let (mut state, mailer) = build_test_state(&pool, mailer);
    state.verify_token_ttl = Duration::seconds(-3600);
    let app = router(state);

    let req = Request::post("/api/v1/signup")
        .header("content-type", "application/json")
        .body(Body::from(json!({"email": "exp@example.com", "password": "hunter22"}).to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap();
    let token = mailer.latest_token().expect("mailer captured token");

    let req = Request::post("/api/v1/verify-email")
        .header("content-type", "application/json")
        .body(Body::from(json!({"token": token}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::GONE);
}

// ----- Projects (admin-session-gated) -----------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn projects_endpoints_require_session(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, _) = build_test_state(&pool, mailer);
    let app = router(state);

    let post = Request::post("/api/v1/projects")
        .header("content-type", "application/json")
        .body(Body::from(json!({"name": "X", "slug": "x"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(post).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let get = Request::get("/api/v1/projects").body(Body::empty()).unwrap();
    let resp = app.oneshot(get).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn project_create_and_list_returns_only_own_projects(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, mailer) = build_test_state(&pool, mailer);

    let cookie_t1 = signup_and_verify(state.clone(), &mailer, "t1@example.com").await;
    let cookie_t2 = signup_and_verify(state.clone(), &mailer, "t2@example.com").await;
    let app = router(state);

    let create_for = |cookie: &str, slug: &str| {
        Request::post("/api/v1/projects")
            .header("content-type", "application/json")
            .header(COOKIE, cookie)
            .body(Body::from(json!({"name": format!("Project {slug}"), "slug": slug}).to_string()))
            .unwrap()
    };

    let r = app.clone().oneshot(create_for(&cookie_t1, "alpha")).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = body_to_json(r.into_body()).await;
    assert_eq!(body["slug"], "alpha");
    assert!(body["embed_snippet"].as_str().unwrap().contains("data-project=\"alpha\""));

    // P3 Stage 1: Free tier cap is 1 project per tenant (FR-FBR-14
    // Contract C19). t2 creates one project; the second would 409 on
    // the tier-cap check. Multi-tenant isolation assertion is preserved
    // — t1's list does NOT contain t2's project, and vice versa.
    app.clone().oneshot(create_for(&cookie_t2, "beta")).await.unwrap();

    let list_for = |cookie: &str| {
        Request::get("/api/v1/projects")
            .header(COOKIE, cookie)
            .body(Body::empty())
            .unwrap()
    };

    let r = app.clone().oneshot(list_for(&cookie_t1)).await.unwrap();
    let body = body_to_json(r.into_body()).await;
    let slugs: Vec<String> = body["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["slug"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(slugs, vec!["alpha".to_string()]);

    let r = app.oneshot(list_for(&cookie_t2)).await.unwrap();
    let body = body_to_json(r.into_body()).await;
    let slugs: Vec<String> = body["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["slug"].as_str().unwrap().to_string())
        .collect();
    // Multi-tenant invariant: t2's list contains only their own project.
    assert!(slugs.contains(&"beta".into()));
    assert!(!slugs.contains(&"alpha".into()));
}

#[sqlx::test(migrations = "../../migrations")]
async fn session_cookie_tamper_rejected(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, mailer) = build_test_state(&pool, mailer);

    let good_cookie = signup_and_verify(state.clone(), &mailer, "tamp@example.com").await;
    let app = router(state);

    // Mutate the last char of the cookie value (very last char is HMAC tail).
    let mut bytes = good_cookie.into_bytes();
    let last = bytes.len() - 1;
    bytes[last] = if bytes[last] == b'a' { b'b' } else { b'a' };
    let bad_cookie = String::from_utf8(bytes).unwrap();

    let req = Request::get("/api/v1/projects")
        .header(COOKIE, bad_cookie)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ----- Signing keys -----------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn signing_key_register_then_deactivate(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, mailer) = build_test_state(&pool, mailer);

    let cookie = signup_and_verify(state.clone(), &mailer, "sk@example.com").await;
    let app = router(state);

    // Create a project.
    let req = Request::post("/api/v1/projects")
        .header("content-type", "application/json")
        .header(COOKIE, &cookie)
        .body(Body::from(json!({"name": "P", "slug": "p"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_to_json(resp.into_body()).await;
    let project_id = body["project_id"].as_str().unwrap().to_string();

    // Register a real-looking Ed25519 public key.
    let pk = STANDARD.encode([7u8; 32]);
    let req = Request::post(format!("/api/v1/projects/{project_id}/signing-keys"))
        .header("content-type", "application/json")
        .header(COOKIE, &cookie)
        .body(Body::from(json!({"public_key_b64": pk, "label": "primary"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_to_json(resp.into_body()).await;
    let key_id = body["key_id"].as_str().unwrap().to_string();

    // Deactivate it.
    let req = Request::delete(format!("/api/v1/projects/{project_id}/signing-keys/{key_id}"))
        .header(COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn signing_key_rejects_bad_input(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, mailer) = build_test_state(&pool, mailer);

    let cookie = signup_and_verify(state.clone(), &mailer, "skb@example.com").await;
    let app = router(state);

    let req = Request::post("/api/v1/projects")
        .header("content-type", "application/json")
        .header(COOKIE, &cookie)
        .body(Body::from(json!({"name": "P", "slug": "p"}).to_string()))
        .unwrap();
    let body = body_to_json(app.clone().oneshot(req).await.unwrap().into_body()).await;
    let project_id = body["project_id"].as_str().unwrap().to_string();

    // Short key.
    let bad = Request::post(format!("/api/v1/projects/{project_id}/signing-keys"))
        .header("content-type", "application/json")
        .header(COOKIE, &cookie)
        .body(Body::from(json!({"public_key_b64": STANDARD.encode([1u8; 16]), "label": "x"}).to_string()))
        .unwrap();
    let r = app.clone().oneshot(bad).await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);

    // All-zero key.
    let zero = Request::post(format!("/api/v1/projects/{project_id}/signing-keys"))
        .header("content-type", "application/json")
        .header(COOKIE, &cookie)
        .body(Body::from(json!({"public_key_b64": STANDARD.encode([0u8; 32]), "label": "x"}).to_string()))
        .unwrap();
    let r = app.clone().oneshot(zero).await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);

    // Non-base64 garbage.
    let bad_b64 = Request::post(format!("/api/v1/projects/{project_id}/signing-keys"))
        .header("content-type", "application/json")
        .header(COOKIE, &cookie)
        .body(Body::from(json!({"public_key_b64": "$$$ not b64 $$$", "label": "x"}).to_string()))
        .unwrap();
    let r = app.oneshot(bad_b64).await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn signing_key_cross_tenant_forbidden(pool: PgPool) {
    let mailer = Arc::new(RecordingMailer::default());
    let (state, mailer) = build_test_state(&pool, mailer);

    let cookie_t1 = signup_and_verify(state.clone(), &mailer, "ct1@example.com").await;
    let cookie_t2 = signup_and_verify(state.clone(), &mailer, "ct2@example.com").await;
    let app = router(state);

    // t1 creates a project.
    let req = Request::post("/api/v1/projects")
        .header("content-type", "application/json")
        .header(COOKIE, &cookie_t1)
        .body(Body::from(json!({"name": "T1", "slug": "t1"}).to_string()))
        .unwrap();
    let body = body_to_json(app.clone().oneshot(req).await.unwrap().into_body()).await;
    let t1_project_id = body["project_id"].as_str().unwrap().to_string();

    // t2 attempts to register a key against t1's project: must be forbidden.
    let pk = STANDARD.encode([9u8; 32]);
    let req = Request::post(format!("/api/v1/projects/{t1_project_id}/signing-keys"))
        .header("content-type", "application/json")
        .header(COOKIE, &cookie_t2)
        .body(Body::from(json!({"public_key_b64": pk, "label": "evil"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
