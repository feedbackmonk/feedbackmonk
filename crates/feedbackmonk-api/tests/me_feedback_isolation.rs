#![allow(clippy::doc_markdown)] // test-file doc comments name JWT claim fields verbatim

//! ⛔ Task Zero isolation fixture — Gap #4 end-user my-feedback read API
//! (PODS collab-20260602-123000, CLAUDE-DELTA). This file is the FROZEN
//! contract for the load-bearing failure mode of the JWT-scoped read surface:
//! **isolation leakage**. It is authored before any feature code and mirrors
//! the byte-for-byte discipline used for the Q24 promote invariant.
//!
//! The two new routes:
//!   GET /api/v1/projects/{project_id}/me/feedback            (Bearer JWT)
//!   GET /api/v1/projects/{project_id}/me/feedback/{fb}/thread (Bearer JWT)
//!
//! Invariants asserted here (each a named test):
//!   1. `my_feedback_returns_only_callers_own_sub` — a caller sees ONLY
//!      feedback whose `end_user_sub` == their JWT `sub`, never another
//!      user's.
//!   2. `thread_returns_public_replies_only` — the thread endpoint returns
//!      status + PUBLIC replies ONLY; `internal` replies NEVER appear.
//!   3. `thread_for_other_users_feedback_id_404` — requesting another user's
//!      feedback by FB-id (under the caller's own valid JWT) is NotFound (404),
//!      not a data leak.
//!   4. `wrong_audience_project_returns_401` — a JWT minted for project A used
//!      against project B's path fails `WrongAudience` → 401 (cross-project).
//!   5. `anon_feedback_never_returned` — anonymous (`anon_token_hash`)
//!      feedback is never returned by the JWT read surface.
//!   6. `bad_jwt_returns_401` — `alg=none` attack → 401 AlgorithmNotAllowed.
//!   7. `my_feedback_happy_path_shape` — response shape + pagination smoke.
//!
//! Pattern ported from `tests/router_submission_integration.rs`; uses
//! `sqlx::test` for a real Postgres pool per test (DEC-FBR-03 — the
//! repository layer is the sole query path). `multi-tenant-isolation-check`
//! is the second leg; this fixture is the first.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Duration;
use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
use rand_core::OsRng;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_core::FeedbackKind;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::me_feedback_router;
use feedbackmonk_repository::{
    ProjectScope, ReplyVisibility, SqlxEmailVerificationRepo,
    SqlxFeedbackReplyRepo, SqlxFeedbackRepo, SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck,
    SqlxProjectRepo, SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo,
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
        ops_token: None,
        jwt_iat_leeway_seconds: 5,
        roadmap_items: Arc::new(feedbackmonk_repository::SqlxRoadmapItemRepo::new(pool.clone())),
        roadmap_votes: Arc::new(feedbackmonk_repository::SqlxRoadmapVoteRepo::new(pool.clone())),
        voting_cache: feedbackmonk_api::VotingCache::new(),
        started_at: chrono::Utc::now(),
        health: SqlxHealthCheck::new(pool.clone()),
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

/// Seed a verified tenant + one project. Returns the `ProjectScope` (for
/// repository writes) and the bare `project_id` (for HTTP URL + JWT `aud`).
async fn seed_project(state: &AppState, email: &str) -> (ProjectScope, Uuid) {
    let tenant = state.tenants.create(email, "hash").await.unwrap();
    let tscope = state.tenants.scope_for(tenant.id).await.unwrap();
    state.tenants.mark_verified(&tscope).await.unwrap();
    let p = state
        .projects
        .create(&tscope, "Proj", &format!("p-{}", &tenant.id.to_string()[..8]))
        .await
        .unwrap();
    let pscope = state.projects.open(&tscope, p.id).await.unwrap();
    (pscope, p.id)
}

async fn seed_signing_key(state: &AppState, scope: &ProjectScope) -> DalekSigningKey {
    let signing = DalekSigningKey::generate(&mut OsRng);
    let pk_bytes: [u8; 32] = signing.verifying_key().to_bytes();
    state
        .signing_keys
        .register(scope, &pk_bytes, "test-key")
        .await
        .unwrap();
    signing
}

/// Mint an EdDSA JWT for `(project_id, sub)` valid for ~5 minutes.
fn mint_jwt(signing: &DalekSigningKey, project_id: Uuid, sub: &str) -> String {
    let header = json!({"alg": "EdDSA", "typ": "JWT"});
    let now = chrono::Utc::now().timestamp();
    let payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
        "email": format!("{sub}@example.com"),
        "name": "Read Surface Test",
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

/// `alg=none` JWT — drives AlgorithmNotAllowed → 401.
fn mint_alg_none_jwt(project_id: Uuid, sub: &str) -> String {
    let header = json!({"alg": "none", "typ": "JWT"});
    let now = chrono::Utc::now().timestamp();
    let payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    format!("{header_b64}.{payload_b64}.")
}

fn get_request(path: &str, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 256 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ----- Invariant 1: own-sub only ---------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn my_feedback_returns_only_callers_own_sub(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "own-sub@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    // Two distinct end-users submit to the same project.
    let a = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "A's bug", FeedbackKind::Bug)
        .await
        .unwrap();
    state
        .feedback
        .submit_authenticated(&pscope, "user-B", Some("b@x.com"), None, None, None, "B's secret bug", FeedbackKind::Bug)
        .await
        .unwrap();

    let app = me_feedback_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(get_request(&format!("/api/v1/projects/{project_id}/me/feedback"), Some(&jwt)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "caller A must see exactly their own 1 row");
    assert_eq!(items[0]["feedback_id"], a.as_str());
    // Hard isolation: B's feedback id + body must NOT appear anywhere.
    let serialized = body.to_string();
    assert!(!serialized.contains("B's secret bug"), "B's body leaked into A's list");
    assert!(!serialized.contains("user-B"), "B's sub leaked into A's list");
}

// ----- Invariant 2: public replies only --------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn thread_returns_public_replies_only(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "pub-replies@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    let fb = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "needs triage", FeedbackKind::Bug)
        .await
        .unwrap();
    // Admin posts one public + one internal reply.
    let admin = Uuid::new_v4();
    state
        .feedback_replies
        .create(&pscope, &fb, "PUBLIC: we're on it", ReplyVisibility::Public, admin)
        .await
        .unwrap();
    state
        .feedback_replies
        .create(&pscope, &fb, "INTERNAL: assign to backend team", ReplyVisibility::Internal, admin)
        .await
        .unwrap();

    let app = me_feedback_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_id}/me/feedback/{}/thread", fb.as_str()),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    let replies = body["replies"].as_array().expect("replies array");
    assert_eq!(replies.len(), 1, "exactly the one PUBLIC reply");
    assert_eq!(replies[0]["body"], "PUBLIC: we're on it");
    // The internal reply body must NEVER appear in the thread payload.
    assert!(
        !body.to_string().contains("INTERNAL: assign to backend team"),
        "internal reply leaked into end-user thread"
    );
    // Status is surfaced.
    assert_eq!(body["feedback_id"], fb.as_str());
    assert!(body["status"].is_string());
}

// ----- Invariant 3: another user's FB-id → 404 -------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn thread_for_other_users_feedback_id_404(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "thread-404@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    // B owns this feedback.
    let b_fb = state
        .feedback
        .submit_authenticated(&pscope, "user-B", Some("b@x.com"), None, None, None, "B's private item", FeedbackKind::Bug)
        .await
        .unwrap();

    // A, with a perfectly valid JWT, asks for B's thread by id → 404.
    let app = me_feedback_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_id}/me/feedback/{}/thread", b_fb.as_str()),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "reading another user's feedback by id must 404, not leak"
    );
}

// ----- Invariant 4: cross-project audience → 401 -----------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn wrong_audience_project_returns_401(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope_a, project_a) = seed_project(&state, "aud-a@example.com").await;
    let (_scope_b, project_b) = seed_project(&state, "aud-b@example.com").await;
    let signing_a = seed_signing_key(&state, &scope_a).await;

    // JWT minted for project A (aud = A) used against project B's path.
    let app = me_feedback_router(state);
    let jwt_for_a = mint_jwt(&signing_a, project_a, "user-A");
    let resp = app
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_b}/me/feedback"),
            Some(&jwt_for_a),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "cross-project aud must 401");
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["error"], "WrongAudience");
}

// ----- Invariant 5: anonymous feedback never returned ------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn anon_feedback_never_returned(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "no-anon@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    // An anonymous submission (no end_user_sub) exists in the project.
    state
        .feedback
        .submit_anonymous(&pscope, &[7u8; 32], Some("anon@x.com"), "anonymous gripe", FeedbackKind::Other)
        .await
        .unwrap();
    // And one authenticated row for the caller.
    let mine = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "my own row", FeedbackKind::Bug)
        .await
        .unwrap();

    let app = me_feedback_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(get_request(&format!("/api/v1/projects/{project_id}/me/feedback"), Some(&jwt)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "only the caller's authenticated row; anon excluded");
    assert_eq!(items[0]["feedback_id"], mine.as_str());
    assert!(!body.to_string().contains("anonymous gripe"), "anon feedback leaked into JWT surface");
}

// ----- Invariant 6: bad JWT → 401 --------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn bad_jwt_returns_401(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "bad-jwt@example.com").await;
    let _ = seed_signing_key(&state, &pscope).await;

    let app = me_feedback_router(state);
    let tampered = mint_alg_none_jwt(project_id, "attacker");
    let resp = app
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_id}/me/feedback"),
            Some(&tampered),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["error"], "AlgorithmNotAllowed");
}

// ----- Invariant 7: happy-path shape + pagination ----------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn my_feedback_happy_path_shape(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "happy@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    for i in 0..3 {
        state
            .feedback
            .submit_authenticated(
                &pscope,
                "user-A",
                Some("a@x.com"),
                None,
                None,
                None,
                &format!("row {i}"),
                FeedbackKind::Feature,
            )
            .await
            .unwrap();
    }

    let app = me_feedback_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    // Page size 2 → first page has 2, total reports 3.
    let resp = app
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_id}/me/feedback?limit=2&offset=0"),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert_eq!(body["total"], 3);
    assert_eq!(body["limit"], 2);
    assert_eq!(body["offset"], 0);
    // Each item carries the documented shape.
    let item = &body["items"][0];
    assert!(item["feedback_id"].as_str().unwrap().starts_with("FB-"));
    assert!(item["kind"].is_string());
    assert!(item["status"].is_string());
    assert!(item["body"].is_string());
    assert!(item["submitted_at"].is_string());
}
