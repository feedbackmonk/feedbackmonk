#![allow(clippy::doc_markdown)] // test-file doc comments name JSON fields verbatim

//! Integration tests for the per-user solicitation-state API (FR-FBR-29):
//!   GET  /api/v1/projects/{project_id}/me/solicitation
//!   POST /api/v1/projects/{project_id}/me/solicitation   { "event": ... }
//!
//! Exercises the full HTTP surface end-to-end (router + EdDSA-JWT auth +
//! repository), complementing the `feedbackmonk-core::solicitation` unit tests
//! (state-machine legality) and the `SqlxSolicitationRepo` repo tests
//! (storage). The load-bearing invariant here is the **`opted_out` terminal
//! privacy promise** — once a user opts out, no further event is honored.
//!
//! Auth + harness pattern ported from `tests/me_feedback_isolation.rs` (same
//! JWT minting, signing-key registration, and `sqlx::test` per-test pool).

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
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::solicitation_router;
use feedbackmonk_api::state::AppState;
use feedbackmonk_repository::{
    ProjectScope, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxSigningKeyRepo,
    SqlxTenantRepo, SqlxTierQuotaRepo,
};

// ----- Fakes ------------------------------------------------------------------

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
        health: SqlxHealthCheck::new(pool.clone()),
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

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

fn mint_jwt(signing: &DalekSigningKey, project_id: Uuid, sub: &str) -> String {
    let header = json!({"alg": "EdDSA", "typ": "JWT"});
    let now = chrono::Utc::now().timestamp();
    let payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
        "email": format!("{sub}@example.com"),
        "name": "Solicitation Test",
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

fn solicitation_path(project_id: Uuid) -> String {
    format!("/api/v1/projects/{project_id}/me/solicitation")
}

fn get_request(path: &str, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

fn post_event(path: &str, bearer: &str, event: &str) -> Request<Body> {
    Request::post(path)
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "event": event }).to_string()))
        .unwrap()
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 256 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ----- never-prompted default ------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn never_prompted_returns_eligible_default(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-default@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    let app = solicitation_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(get_request(&solicitation_path(project_id), Some(&jwt)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    // A sub with no record: default eligible, never prompted, no 404.
    assert_eq!(body["status"], "eligible");
    assert_eq!(body["eligible"], true);
    assert_eq!(body["prompt_count"], 0);
    assert!(body["prompted_at"].is_null());
    assert!(body["last_event_at"].is_null());
    assert!(body["next_eligible_at"].is_null());
    assert!(body["policy"]["cooldown_days"].as_i64().unwrap() >= 1);
}

// ----- prompted → dismissed --------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn prompted_then_dismissed_flow(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-dismiss@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = solicitation_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let path = solicitation_path(project_id);

    // prompted: status flips, count bumps, cooldown starts → not eligible.
    let resp = app
        .clone()
        .oneshot(post_event(&path, &jwt, "prompted"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["status"], "prompted");
    assert_eq!(body["prompt_count"], 1);
    assert_eq!(body["eligible"], false);
    assert!(body["prompted_at"].is_string());
    assert!(body["next_eligible_at"].is_string(), "cooldown sets a next-eligible time");

    // dismissed: legal from prompted.
    let resp = app
        .oneshot(post_event(&path, &jwt, "dismissed"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["status"], "dismissed");
    assert_eq!(body["prompt_count"], 1);
    assert_eq!(body["eligible"], false, "still within cooldown after dismissal");
}

// ----- prompted → gave_feedback ----------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn prompted_then_gave_feedback_flow(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-gave@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = solicitation_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let path = solicitation_path(project_id);

    app.clone().oneshot(post_event(&path, &jwt, "prompted")).await.unwrap();
    let resp = app
        .oneshot(post_event(&path, &jwt, "gave_feedback"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["status"], "gave_feedback");
}

// ----- illegal transition (dismiss without prompt) → 409 ---------------------

#[sqlx::test(migrations = "../../migrations")]
async fn dismiss_without_prompt_is_409(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-illegal@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = solicitation_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");

    // No prompt outstanding (eligible) → dismissed is illegal.
    let resp = app
        .oneshot(post_event(&solicitation_path(project_id), &jwt, "dismissed"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["error"], "IllegalTransition");
}

// ----- opt-out is terminal (THE privacy invariant) ---------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn opt_out_is_terminal(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-optout@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = solicitation_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let path = solicitation_path(project_id);

    // Opt out directly from eligible (allowed from any non-terminal state).
    let resp = app.clone().oneshot(post_event(&path, &jwt, "opted_out")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["status"], "opted_out");
    assert_eq!(body["eligible"], false);
    assert!(body["next_eligible_at"].is_null(), "opted-out is permanently ineligible");

    // Any further event (prompted) is rejected — the terminal guarantee.
    let resp = app.clone().oneshot(post_event(&path, &jwt, "prompted")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["error"], "OptedOut");

    // dismissed / gave_feedback are likewise rejected after opt-out.
    for ev in ["dismissed", "gave_feedback"] {
        let resp = app.clone().oneshot(post_event(&path, &jwt, ev)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT, "{ev} after opt-out must 409");
        let body = body_to_json(resp.into_body()).await;
        assert_eq!(body["error"], "OptedOut");
    }

    // And a fresh GET still reflects the terminal, ineligible state.
    let resp = app.oneshot(get_request(&path, Some(&jwt))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["status"], "opted_out");
    assert_eq!(body["eligible"], false);
}

// ----- repeated opt-out is idempotent ----------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn opt_out_is_idempotent(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-idem@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = solicitation_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let path = solicitation_path(project_id);

    app.clone().oneshot(post_event(&path, &jwt, "opted_out")).await.unwrap();
    // opted_out → opted_out is a no-op success, not a 409.
    let resp = app.oneshot(post_event(&path, &jwt, "opted_out")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["status"], "opted_out");
}

// ----- auth: missing bearer → 401 --------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn missing_jwt_returns_401(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-noauth@example.com").await;
    let _ = seed_signing_key(&state, &pscope).await;

    let app = solicitation_router(state);
    let resp = app
        .oneshot(get_request(&solicitation_path(project_id), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["error"], "unauthorized");
}

// ----- auth: cross-project audience → 401 ------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn wrong_audience_returns_401(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope_a, project_a) = seed_project(&state, "solicit-aud-a@example.com").await;
    let (_scope_b, project_b) = seed_project(&state, "solicit-aud-b@example.com").await;
    let signing_a = seed_signing_key(&state, &scope_a).await;

    let app = solicitation_router(state);
    let jwt_for_a = mint_jwt(&signing_a, project_a, "user-A");
    // JWT minted for A used against B's path.
    let resp = app
        .oneshot(get_request(&solicitation_path(project_b), Some(&jwt_for_a)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["error"], "WrongAudience");
}

// ----- malformed event → 400 -------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn malformed_event_returns_400(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-badevent@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = solicitation_router(state);
    let jwt = mint_jwt(&signing, project_id, "user-A");

    let resp = app
        .oneshot(post_event(&solicitation_path(project_id), &jwt, "bogus_event"))
        .await
        .unwrap();
    // Axum rejects an unknown enum value at JSON deserialization → 400/422.
    assert!(
        resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "unknown event value must be a client error, got {}",
        resp.status()
    );
}

// ----- per-sub isolation -----------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn opt_out_is_per_sub(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "solicit-persub@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = solicitation_router(state);

    // user-A opts out.
    let jwt_a = mint_jwt(&signing, project_id, "user-A");
    app.clone().oneshot(post_event(&solicitation_path(project_id), &jwt_a, "opted_out")).await.unwrap();

    // user-B (same project) is unaffected: still eligible by default.
    let jwt_b = mint_jwt(&signing, project_id, "user-B");
    let resp = app
        .oneshot(get_request(&solicitation_path(project_id), Some(&jwt_b)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["status"], "eligible");
    assert_eq!(body["eligible"], true);
}
