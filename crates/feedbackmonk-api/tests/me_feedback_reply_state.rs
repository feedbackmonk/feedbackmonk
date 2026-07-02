#![allow(clippy::doc_markdown)] // test-file doc comments name JWT claim fields verbatim

//! Phase A A3 — reply-state fields on the me_feedback list
//! (`updated_at` + `reply_count` + `severity`) and the `?since=` delta-poll
//! filter. Mirrors the `me_feedback_isolation.rs` harness.
//!
//! Invariants asserted here (each a named test):
//!   1. `public_reply_bumps_reply_count_and_updated_at` — a PUBLIC reply moves
//!      `reply_count` 0→1 and pushes `updated_at` forward on both the list and
//!      the thread; the new fields are additive (existing fields untouched).
//!   2. `internal_reply_does_not_change_reply_state` — an INTERNAL reply moves
//!      NEITHER `reply_count` NOR `updated_at` (triage activity must not leak
//!      through the derived fields).
//!   3. `since_filters_by_updated_at` — `?since=<rfc3339>` returns only rows
//!      with `updated_at > since` (strict), with `total` matching the filter.

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
use feedbackmonk_api::me_feedback_router;
use feedbackmonk_api::state::AppState;
use feedbackmonk_core::FeedbackKind;
use feedbackmonk_repository::{
    ProjectScope, ReplyVisibility, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo,
    SqlxFeedbackRepo, SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo,
    SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo,
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
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
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

async fn fetch_list(app: &axum::Router, project_id: Uuid, jwt: &str, query: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_id}/me/feedback{query}"),
            Some(jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    body_to_json(resp.into_body()).await
}

/// Postgres `now()` is per-transaction; a tiny sleep between writes keeps the
/// strict `>` timestamp assertions robust at microsecond resolution.
async fn tick() {
    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
}

// ----- Invariant 1: PUBLIC reply bumps reply_count + updated_at ---------------

#[sqlx::test(migrations = "../../migrations")]
async fn public_reply_bumps_reply_count_and_updated_at(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "reply-bump@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    let fb = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "needs a reply", None, FeedbackKind::Bug)
        .await
        .unwrap();

    let app = me_feedback_router(state.clone());
    let jwt = mint_jwt(&signing, project_id, "user-A");

    // Fresh row: reply_count 0, updated_at == submitted_at, severity null
    // (key present — additive shape).
    let body = fetch_list(&app, project_id, &jwt, "").await;
    let item = &body["items"][0];
    assert_eq!(item["reply_count"], 0);
    assert_eq!(item["updated_at"], item["submitted_at"]);
    assert!(item.get("severity").is_some(), "severity key must be present");
    assert!(item["severity"].is_null());
    let updated_at_0 = item["updated_at"].as_str().unwrap().to_string();

    // A PUBLIC reply lands…
    tick().await;
    state
        .feedback_replies
        .create(&pscope, &fb, "we're on it", ReplyVisibility::Public, Uuid::new_v4())
        .await
        .unwrap();

    // …reply_count → 1 and updated_at moves forward.
    let body = fetch_list(&app, project_id, &jwt, "").await;
    let item = &body["items"][0];
    assert_eq!(item["reply_count"], 1);
    let updated_at_1 = item["updated_at"].as_str().unwrap().to_string();
    let t0 = chrono::DateTime::parse_from_rfc3339(&updated_at_0).unwrap();
    let t1 = chrono::DateTime::parse_from_rfc3339(&updated_at_1).unwrap();
    assert!(
        t1 > t0,
        "updated_at must move forward on a public reply ({updated_at_1} !> {updated_at_0})"
    );
    // Existing fields untouched (additive contract).
    assert!(item["feedback_id"].as_str().unwrap().starts_with("FB-"));
    assert_eq!(item["body"], "needs a reply");

    // The thread carries the same derived fields.
    let resp = app
        .clone()
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_id}/me/feedback/{}/thread", fb.as_str()),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let thread = body_to_json(resp.into_body()).await;
    assert_eq!(thread["reply_count"], 1);
    assert_eq!(thread["updated_at"].as_str().unwrap(), updated_at_1);
    assert!(thread["severity"].is_null());
}

// ----- Invariant 2: INTERNAL replies are invisible to the derived state -------

#[sqlx::test(migrations = "../../migrations")]
async fn internal_reply_does_not_change_reply_state(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "internal-no-bump@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    let fb = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "quiet row", None, FeedbackKind::Bug)
        .await
        .unwrap();

    let app = me_feedback_router(state.clone());
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let body = fetch_list(&app, project_id, &jwt, "").await;
    let updated_at_0 = body["items"][0]["updated_at"].as_str().unwrap().to_string();

    // An INTERNAL triage note lands…
    tick().await;
    state
        .feedback_replies
        .create(&pscope, &fb, "internal: assign to backend", ReplyVisibility::Internal, Uuid::new_v4())
        .await
        .unwrap();

    // …and NOTHING in the derived state moves (privacy: triage activity must
    // not leak through reply_count or updated_at).
    let body = fetch_list(&app, project_id, &jwt, "").await;
    let item = &body["items"][0];
    assert_eq!(item["reply_count"], 0, "internal reply leaked into reply_count");
    assert_eq!(
        item["updated_at"].as_str().unwrap(),
        updated_at_0,
        "internal reply leaked into updated_at"
    );
}

// ----- Invariant 3: ?since= delta filter ---------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn since_filters_by_updated_at(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "since-filter@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    let fb_old = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "older row", None, FeedbackKind::Bug)
        .await
        .unwrap();
    tick().await;
    state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "newer row", None, FeedbackKind::Feature)
        .await
        .unwrap();

    let app = me_feedback_router(state.clone());
    let jwt = mint_jwt(&signing, project_id, "user-A");

    // Baseline: both rows; capture the NEWEST updated_at as the cursor.
    let body = fetch_list(&app, project_id, &jwt, "").await;
    assert_eq!(body["total"], 2);
    let cursor = body["items"][0]["updated_at"].as_str().unwrap().to_string();

    // Strict `>`: since == the max updated_at ⇒ nothing has changed.
    let body = fetch_list(&app, project_id, &jwt, &format!("?since={cursor}")).await;
    assert_eq!(body["total"], 0, "since == max updated_at must return nothing");
    assert!(body["items"].as_array().unwrap().is_empty());

    // A PUBLIC reply on the OLD row pushes its updated_at past the cursor…
    tick().await;
    state
        .feedback_replies
        .create(&pscope, &fb_old, "revived", ReplyVisibility::Public, Uuid::new_v4())
        .await
        .unwrap();

    // …so the delta poll returns exactly that row (the untouched newer row
    // stays filtered out).
    let body = fetch_list(&app, project_id, &jwt, &format!("?since={cursor}")).await;
    assert_eq!(body["total"], 1);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["feedback_id"], fb_old.as_str());
    assert_eq!(items[0]["reply_count"], 1);

    // Absent `since` ⇒ current (full-list) behavior unchanged.
    let body = fetch_list(&app, project_id, &jwt, "").await;
    assert_eq!(body["total"], 2);
}
