#![allow(clippy::doc_markdown)] // test-file doc comments name types/columns/paths verbatim

//! Public Feedback Board VOTING mechanics (Contract C30, PF-BOARD-VOTING-01).
//!
//! The board sibling of the roadmap voting tests. Exercises the cast/retract
//! HTTP surface end-to-end against a real Postgres (`sqlx::test`), in both anon
//! and JWT voter modes:
//!
//!   1. `anon_cast_increments_vote_count` — anon POST vote → 200; the board read
//!      `vote_count` reflects the real aggregate (D1).
//!   2. `anon_double_cast_returns_409` — a second vote by the same anon voter on
//!      the same item → 409 AlreadyVoted (UNIQUE (feedback_id, voter_id), inv 1).
//!   3. `anon_retract_within_window_removes_vote` — DELETE within the window →
//!      200; vote_count back to 0.
//!   4. `retract_with_no_vote_returns_404` — DELETE with no prior vote → 404.
//!   5. `anon_rate_limit_returns_429` — a second anon CAST (any item) once the
//!      per-voter quota is exhausted → 429 (cast pre-checks the rate limit).
//!   6. `jwt_cast_increments_then_double_409` — Bearer-JWT cast → 200, then a
//!      second cast by the same `sub` → 409.
//!
//! The retraction-window 403 path (`RetractOutcome::WindowExpired`) is proven
//! deterministically at the repository layer (`board_votes.rs::
//! retract_outside_window_keeps_vote`, 0s window); the handler maps that variant
//! to 403 (`retraction_window_expired_response`, shared with roadmap), so an
//! HTTP-level 60s wait is not re-tested here.
//!
//! Voter resolution reuses the shared `voting_common` chokepoint — the SAME
//! anon/JWT primitive the roadmap voting handlers use (migration 00007/00018
//! invariant #2). ConnectInfo is injected explicitly because `oneshot` does not
//! run the `into_make_service_with_connect_info` layer (mirrors
//! router_submission_integration.rs).

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Duration;
use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
use rand_core::OsRng;
use serde_json::{json, Value};
use std::net::SocketAddr;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::{AnonGate, ANON_COOKIE_HEADER};
use feedbackmonk_api::board_router;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_core::{FeedbackId, FeedbackKind, ModerationStatus};
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
        session_secret: Arc::new([0x42u8; 32]),
        public_url: Arc::from("http://test.local"),
        verify_token_ttl: Duration::hours(24),
        anon_gate: AnonGate::new(NonZeroU32::new(anon_quota).unwrap()),
        login_gate: feedbackmonk_anon::LoginGate::with_default_quota(),
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

async fn enable_board(state: &AppState, scope: &ProjectScope) {
    state
        .projects
        .set_board_settings(scope, Some(true), None)
        .await
        .unwrap();
}

/// Submit an anonymous feedback row and APPROVE it (the only state votable on
/// the board). Returns its public `short_code`.
async fn submit_approved(
    state: &AppState,
    pool: &PgPool,
    scope: &ProjectScope,
    body: &str,
) -> FeedbackId {
    let fb = state
        .feedback
        .submit_anonymous(scope, &[7u8; 32], None, body, None, FeedbackKind::Other)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    state
        .feedback
        .moderate_in_executor(scope, &mut tx, &fb, ModerationStatus::Approved, None, scope.tenant_id())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    fb
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
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

fn vote_path(project_id: Uuid, short_code: &str) -> String {
    format!("/api/v1/projects/{project_id}/board/items/{short_code}/vote")
}

/// Build a vote request with ConnectInfo (so the anon `token_hash` resolves) and
/// optional anon-cookie / Bearer headers. `method` is "POST" (cast) or "DELETE".
fn vote_request(
    method: &str,
    project_id: Uuid,
    short_code: &str,
    anon_cookie: Option<&str>,
    bearer: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(vote_path(project_id, short_code));
    if let Some(cookie) = anon_cookie {
        builder = builder.header(ANON_COOKIE_HEADER, cookie);
    }
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let mut req = builder.body(Body::empty()).unwrap();
    // Stable peer addr so the anon token_hash is deterministic across requests.
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:54321".parse().unwrap()));
    req
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 256 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Read the board item's current `vote_count` via the public GET item endpoint.
async fn read_vote_count(state: &AppState, project_id: Uuid, short_code: &str) -> i64 {
    let app = board_router(state.clone());
    let resp = app
        .oneshot(
            Request::get(format!(
                "/api/v1/projects/{project_id}/board/items/{short_code}"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "board item read failed");
    let body = body_to_json(resp.into_body()).await;
    body["vote_count"].as_i64().expect("vote_count i64")
}

// ----- 1: anon cast increments the real vote_count ---------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn anon_cast_increments_vote_count(pool: PgPool) {
    let state = build_test_state(&pool, 1000);
    let (pscope, project_id) = seed_project(&state, "bv-cast@example.com").await;
    enable_board(&state, &pscope).await;
    let fb = submit_approved(&state, &pool, &pscope, "dark mode please").await;

    assert_eq!(read_vote_count(&state, project_id, fb.as_str()).await, 0);

    let app = board_router(state.clone());
    let resp = app
        .oneshot(vote_request("POST", project_id, fb.as_str(), Some("anon-1"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["short_code"], fb.as_str());
    assert_eq!(body["voter_mode"], "anon");

    assert_eq!(read_vote_count(&state, project_id, fb.as_str()).await, 1);
}

// ----- 2: anon double cast → 409 ---------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn anon_double_cast_returns_409(pool: PgPool) {
    let state = build_test_state(&pool, 1000);
    let (pscope, project_id) = seed_project(&state, "bv-dup@example.com").await;
    enable_board(&state, &pscope).await;
    let fb = submit_approved(&state, &pool, &pscope, "feature x").await;

    let app = board_router(state.clone());
    let first = app
        .oneshot(vote_request("POST", project_id, fb.as_str(), Some("anon-1"), None))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let app = board_router(state.clone());
    let second = app
        .oneshot(vote_request("POST", project_id, fb.as_str(), Some("anon-1"), None))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::CONFLICT, "double vote → 409");

    // No silent upsert — count stays at 1.
    assert_eq!(read_vote_count(&state, project_id, fb.as_str()).await, 1);
}

// ----- 3: anon retract within window removes the vote ------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn anon_retract_within_window_removes_vote(pool: PgPool) {
    let state = build_test_state(&pool, 1000);
    let (pscope, project_id) = seed_project(&state, "bv-ret@example.com").await;
    enable_board(&state, &pscope).await;
    let fb = submit_approved(&state, &pool, &pscope, "retract me").await;

    let app = board_router(state.clone());
    let cast = app
        .oneshot(vote_request("POST", project_id, fb.as_str(), Some("anon-1"), None))
        .await
        .unwrap();
    assert_eq!(cast.status(), StatusCode::OK);
    assert_eq!(read_vote_count(&state, project_id, fb.as_str()).await, 1);

    let app = board_router(state.clone());
    let retract = app
        .oneshot(vote_request("DELETE", project_id, fb.as_str(), Some("anon-1"), None))
        .await
        .unwrap();
    assert_eq!(retract.status(), StatusCode::OK);
    let body = body_to_json(retract.into_body()).await;
    assert_eq!(body["short_code"], fb.as_str());

    assert_eq!(read_vote_count(&state, project_id, fb.as_str()).await, 0);
}

// ----- 4: retract with no prior vote → 404 -----------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn retract_with_no_vote_returns_404(pool: PgPool) {
    let state = build_test_state(&pool, 1000);
    let (pscope, project_id) = seed_project(&state, "bv-ret-none@example.com").await;
    enable_board(&state, &pscope).await;
    let fb = submit_approved(&state, &pool, &pscope, "never voted").await;

    let app = board_router(state.clone());
    let retract = app
        .oneshot(vote_request("DELETE", project_id, fb.as_str(), Some("anon-1"), None))
        .await
        .unwrap();
    assert_eq!(retract.status(), StatusCode::NOT_FOUND, "no vote → 404 VoteNotFound");
}

// ----- 5: anon rate limit on cast → 429 --------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn anon_rate_limit_returns_429(pool: PgPool) {
    // Quota of 1: the first anon cast consumes the token; a second cast by the
    // same voter (even on a different item) trips the rate limit.
    let state = build_test_state(&pool, 1);
    let (pscope, project_id) = seed_project(&state, "bv-rl@example.com").await;
    enable_board(&state, &pscope).await;
    let fb1 = submit_approved(&state, &pool, &pscope, "item one").await;
    let fb2 = submit_approved(&state, &pool, &pscope, "item two").await;

    let app = board_router(state.clone());
    let first = app
        .oneshot(vote_request("POST", project_id, fb1.as_str(), Some("anon-1"), None))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let app = board_router(state.clone());
    let second = app
        .oneshot(vote_request("POST", project_id, fb2.as_str(), Some("anon-1"), None))
        .await
        .unwrap();
    assert_eq!(
        second.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "rate-limited anon cast → 429"
    );
}

// ----- 6: JWT cast → 200, then double cast same sub → 409 --------------------

#[sqlx::test(migrations = "../../migrations")]
async fn jwt_cast_increments_then_double_409(pool: PgPool) {
    let state = build_test_state(&pool, 1000);
    let (pscope, project_id) = seed_project(&state, "bv-jwt@example.com").await;
    enable_board(&state, &pscope).await;
    let fb = submit_approved(&state, &pool, &pscope, "jwt votable").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let token = mint_jwt(&signing, project_id, "user-sub-1");

    let app = board_router(state.clone());
    let cast = app
        .oneshot(vote_request("POST", project_id, fb.as_str(), None, Some(&token)))
        .await
        .unwrap();
    assert_eq!(cast.status(), StatusCode::OK);
    let body = body_to_json(cast.into_body()).await;
    assert_eq!(body["voter_mode"], "jwt");
    assert_eq!(read_vote_count(&state, project_id, fb.as_str()).await, 1);

    // Same sub votes again → 409.
    let app = board_router(state.clone());
    let again = app
        .oneshot(vote_request("POST", project_id, fb.as_str(), None, Some(&token)))
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::CONFLICT, "same sub double vote → 409");
    assert_eq!(read_vote_count(&state, project_id, fb.as_str()).await, 1);
}
