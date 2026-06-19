#![allow(clippy::doc_markdown)] // test-file doc comments name types/columns/paths verbatim

//! ⛔ Privacy fixture — Public Feedback Board NO-PII invariant (C29 inv. 3,
//! sibling to Q24 / DEC-FBR-02 no-trackers brand promise). The public board
//! exposes feedback BODIES (the whole point) but MUST NEVER expose submitter
//! identity — email, name, JWT `sub`, anon token, external metadata, or a crash
//! correlation id. A leak here is a privacy regression on a fully public,
//! CORS-exposed, unauthenticated surface. This is the behavioral leg of
//! `public-board-moderation-gate` Probe B; the static leg scans the board source
//! for these same field tokens.
//!
//! Modeled on `tests/me_feedback_isolation.rs`. Real Postgres per test
//! (`sqlx::test`, DEC-FBR-03). Both an authenticated submission (carrying every
//! PII column) and an anonymous one (carrying email + anon token) are approved
//! onto the board, then the response is asserted to leak none of it.
//!
//! Invariants asserted (each a named test):
//!   1. `authenticated_submitter_pii_absent_from_board` — an approved auth-mode
//!      row exposes its body but NONE of sub/email/name/external_metadata/crash.
//!   2. `anonymous_submitter_identity_absent_from_board` — an approved anon row
//!      exposes its body but neither the opt-in email nor any anon-token field.
//!   3. `board_item_shape_is_public_only` — the single-item endpoint returns
//!      exactly the public wire keys and no others.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::AnonGate;
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
        anon_gate: AnonGate::new(NonZeroU32::new(1000).unwrap()),
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
    state.projects.set_board_settings(&pscope, Some(true), None).await.unwrap();
    (pscope, p.id)
}

async fn approve(state: &AppState, pool: &PgPool, scope: &ProjectScope, fb: &FeedbackId) {
    let mut tx = pool.begin().await.unwrap();
    state
        .feedback
        .moderate_in_executor(scope, &mut tx, fb, ModerationStatus::Approved, None, scope.tenant_id())
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 256 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).unwrap()
}

/// The submitter-PII tokens (field names AND values) that must NEVER appear in a
/// board response. Asserted as raw-substring absence over the serialized JSON.
fn assert_no_pii(serialized: &str) {
    for needle in [
        // wire field names
        "end_user_email",
        "end_user_name",
        "end_user_sub",
        "anon_token_hash",
        "external_metadata",
        "crash_event_id",
        "submitter",
    ] {
        assert!(
            !serialized.contains(needle),
            "board response leaked PII field token `{needle}`"
        );
    }
}

// ----- Invariant 1: authenticated submitter PII absent ------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn authenticated_submitter_pii_absent_from_board(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "priv-auth@example.com").await;

    let meta = json!({"plan": "pro", "internal_user_id": "u-SECRET-123"});
    let fb = state
        .feedback
        .submit_authenticated(
            &pscope,
            "auth0|sub-SECRET",
            Some("victim@private.example"),
            Some("Victim Real Name"),
            Some(&meta),
            Some("crash-SECRET-abc123"),
            "PUBLIC BODY: the checkout button is broken",
            FeedbackKind::Bug,
        )
        .await
        .unwrap();
    approve(&state, &pool, &pscope, &fb).await;

    let app = board_router(state);
    let resp = app
        .oneshot(get(&format!("/api/v1/projects/{project_id}/board")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    // The body IS exposed (that's the feature).
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["short_code"], fb.as_str());
    assert!(body.to_string().contains("PUBLIC BODY: the checkout button is broken"));

    // None of the submitter identity — field names OR values — leaks.
    let serialized = body.to_string();
    assert_no_pii(&serialized);
    for secret in [
        "auth0|sub-SECRET",
        "victim@private.example",
        "Victim Real Name",
        "u-SECRET-123",
        "crash-SECRET-abc123",
    ] {
        assert!(!serialized.contains(secret), "board leaked submitter value `{secret}`");
    }
}

// ----- Invariant 2: anonymous submitter identity absent ----------------------

#[sqlx::test(migrations = "../../migrations")]
async fn anonymous_submitter_identity_absent_from_board(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "priv-anon@example.com").await;

    // Anonymous submission carrying an opt-in email.
    let fb = state
        .feedback
        .submit_anonymous(
            &pscope,
            &[42u8; 32],
            Some("anon-optin@private.example"),
            "PUBLIC BODY: please add dark mode",
            FeedbackKind::Feature,
        )
        .await
        .unwrap();
    approve(&state, &pool, &pscope, &fb).await;

    let app = board_router(state);
    let resp = app
        .oneshot(get(&format!("/api/v1/projects/{project_id}/board")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    assert!(body.to_string().contains("PUBLIC BODY: please add dark mode"));
    let serialized = body.to_string();
    assert_no_pii(&serialized);
    assert!(
        !serialized.contains("anon-optin@private.example"),
        "board leaked anonymous submitter's opt-in email"
    );
}

// ----- Invariant 3: single-item shape is public-only -------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn board_item_shape_is_public_only(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "priv-shape@example.com").await;

    let fb = state
        .feedback
        .submit_authenticated(
            &pscope,
            "auth0|shape",
            Some("shape@private.example"),
            Some("Shape Name"),
            None,
            None,
            "PUBLIC BODY shape check",
            FeedbackKind::Question,
        )
        .await
        .unwrap();
    approve(&state, &pool, &pscope, &fb).await;

    let app = board_router(state);
    let resp = app
        .oneshot(get(&format!(
            "/api/v1/projects/{project_id}/board/items/{}",
            fb.as_str()
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    // Exactly the public wire keys — no more.
    let obj = body.as_object().expect("object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["accepted_at", "body", "kind", "short_code", "status", "vote_count"],
        "board item must carry exactly the public wire keys"
    );
    assert_no_pii(&body.to_string());
    assert!(!body.to_string().contains("shape@private.example"));
}
