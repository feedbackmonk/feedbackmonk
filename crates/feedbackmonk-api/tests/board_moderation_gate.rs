#![allow(clippy::doc_markdown)] // test-file doc comments name types/columns/paths verbatim

//! ⛔ Trust-boundary fixture — Public Feedback Board MODERATION GATE (C29 inv. 1
//! & 2). The load-bearing failure mode of the public board is **exposure of an
//! unmoderated row**: a `pending` or `rejected` feedback row appearing on a
//! public surface is the exact spam/abuse/off-brand exposure this feature exists
//! to prevent (Testability Gate Flag 1, Q2=5). This file is the behavioral leg
//! of `public-board-moderation-gate` Probe C; the static legs (Probe A/B) prove
//! the same property from source.
//!
//! Invariants asserted (each a named test):
//!   1. `board_returns_only_approved_rows` — list endpoint returns approved rows
//!      ONLY; pending + rejected are never present.
//!   2. `board_item_404s_for_non_approved` — the single-item endpoint 404s for a
//!      pending/rejected short_code (structurally unreachable), 200s for approved.
//!   3. `board_disabled_project_404s` — a project with `public_board_enabled=FALSE`
//!      returns 404 from both board endpoints even with an approved row present.
//!   4. `unknown_project_404s` — an unknown project_id 404s.
//!   5. `re_moderation_pulls_row_from_board` — approving then rejecting a row
//!      removes it from the board (the gate tracks the live moderation_status).
//!
//! Pattern ported from `tests/me_feedback_isolation.rs`. Real Postgres per test
//! via `sqlx::test` (DEC-FBR-03 — the repository layer is the sole query path).
//! Rows are moved to approved/rejected via the repository `moderate_in_executor`
//! (the same same-txn path the admin handler drives), keeping the test off the
//! AdminSession cookie machinery while exercising the real gate.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use serde_json::Value;
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

/// Seed a verified tenant + one project. Returns `(ProjectScope, project_id)`.
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

async fn submit(state: &AppState, scope: &ProjectScope, body: &str) -> FeedbackId {
    state
        .feedback
        .submit_anonymous(scope, &[7u8; 32], None, body, None, FeedbackKind::Other)
        .await
        .unwrap()
}

/// Move a feedback row to a moderation status via the same-txn repo path the
/// admin handler uses (keeps the test off the AdminSession cookie).
async fn set_moderation(
    state: &AppState,
    pool: &PgPool,
    scope: &ProjectScope,
    fb: &FeedbackId,
    to: ModerationStatus,
) {
    let mut tx = pool.begin().await.unwrap();
    state
        .feedback
        .moderate_in_executor(scope, &mut tx, fb, to, None, scope.tenant_id())
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

// ----- Invariant 1: approved-only list ---------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn board_returns_only_approved_rows(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "gate-list@example.com").await;
    enable_board(&state, &pscope).await;

    let approved = submit(&state, &pscope, "APPROVED visible row").await;
    let pending = submit(&state, &pscope, "PENDING hidden row").await;
    let rejected = submit(&state, &pscope, "REJECTED hidden row").await;
    set_moderation(&state, &pool, &pscope, &approved, ModerationStatus::Approved).await;
    set_moderation(&state, &pool, &pscope, &rejected, ModerationStatus::Rejected).await;
    // `pending` stays pending (default).
    let _ = &pending;

    let app = board_router(state);
    let resp = app
        .oneshot(get(&format!("/api/v1/projects/{project_id}/board")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "only the approved row is visible");
    assert_eq!(items[0]["short_code"], approved.as_str());
    assert_eq!(body["total"], 1);

    // Hard gate: pending + rejected bodies must NOT appear anywhere.
    let serialized = body.to_string();
    assert!(!serialized.contains("PENDING hidden row"), "pending row leaked onto board");
    assert!(!serialized.contains("REJECTED hidden row"), "rejected row leaked onto board");
}

// ----- Invariant 2: single-item 404 for non-approved -------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn board_item_404s_for_non_approved(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "gate-item@example.com").await;
    enable_board(&state, &pscope).await;

    let approved = submit(&state, &pscope, "approved item").await;
    let pending = submit(&state, &pscope, "pending item").await;
    let rejected = submit(&state, &pscope, "rejected item").await;
    set_moderation(&state, &pool, &pscope, &approved, ModerationStatus::Approved).await;
    set_moderation(&state, &pool, &pscope, &rejected, ModerationStatus::Rejected).await;

    let app = board_router(state);

    // Approved → 200.
    let ok = app
        .clone()
        .oneshot(get(&format!(
            "/api/v1/projects/{project_id}/board/items/{}",
            approved.as_str()
        )))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    // Pending → 404.
    let p = app
        .clone()
        .oneshot(get(&format!(
            "/api/v1/projects/{project_id}/board/items/{}",
            pending.as_str()
        )))
        .await
        .unwrap();
    assert_eq!(p.status(), StatusCode::NOT_FOUND, "pending item must 404, not leak");

    // Rejected → 404.
    let r = app
        .oneshot(get(&format!(
            "/api/v1/projects/{project_id}/board/items/{}",
            rejected.as_str()
        )))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND, "rejected item must 404, not leak");
}

// ----- Invariant 3: board-disabled project 404s ------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn board_disabled_project_404s(pool: PgPool) {
    let state = build_test_state(&pool);
    // Board NOT enabled (default public_board_enabled = FALSE).
    let (pscope, project_id) = seed_project(&state, "gate-disabled@example.com").await;

    // Even with an approved row present, a disabled board exposes nothing.
    let approved = submit(&state, &pscope, "approved but board off").await;
    set_moderation(&state, &pool, &pscope, &approved, ModerationStatus::Approved).await;

    let app = board_router(state);

    let list = app
        .clone()
        .oneshot(get(&format!("/api/v1/projects/{project_id}/board")))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::NOT_FOUND, "disabled board list must 404");

    let item = app
        .oneshot(get(&format!(
            "/api/v1/projects/{project_id}/board/items/{}",
            approved.as_str()
        )))
        .await
        .unwrap();
    assert_eq!(item.status(), StatusCode::NOT_FOUND, "disabled board item must 404");
}

// ----- Invariant 4: unknown project 404 --------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_project_404s(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = board_router(state);
    let resp = app
        .oneshot(get(&format!("/api/v1/projects/{}/board", Uuid::new_v4())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ----- Invariant 5: re-moderation pulls a row from the board -----------------

#[sqlx::test(migrations = "../../migrations")]
async fn re_moderation_pulls_row_from_board(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "gate-pull@example.com").await;
    enable_board(&state, &pscope).await;

    let fb = submit(&state, &pscope, "row that will be pulled").await;
    set_moderation(&state, &pool, &pscope, &fb, ModerationStatus::Approved).await;

    // Approved → visible.
    let app = board_router(state.clone());
    let before = app
        .oneshot(get(&format!("/api/v1/projects/{project_id}/board")))
        .await
        .unwrap();
    let before_body = body_to_json(before.into_body()).await;
    assert_eq!(before_body["items"].as_array().unwrap().len(), 1);

    // Pull from board (approve → reject).
    set_moderation(&state, &pool, &pscope, &fb, ModerationStatus::Rejected).await;

    let app2 = board_router(state);
    let after = app2
        .oneshot(get(&format!("/api/v1/projects/{project_id}/board")))
        .await
        .unwrap();
    let after_body = body_to_json(after.into_body()).await;
    assert_eq!(
        after_body["items"].as_array().unwrap().len(),
        0,
        "a row rejected after approval must disappear from the board"
    );
}
