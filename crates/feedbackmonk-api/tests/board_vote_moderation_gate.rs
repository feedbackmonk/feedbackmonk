#![allow(clippy::doc_markdown)] // test-file doc comments name types/columns/paths verbatim

//! ⛔ Trust-boundary fixture — Public Feedback Board VOTE-PATH moderation gate
//! (Contract C30 / plan D2, PF-BOARD-VOTING-01). The load-bearing failure mode
//! of board voting is the vote endpoint becoming an **existence oracle** for
//! hidden feedback: if a vote/retract on a `pending`/`rejected`/board-disabled
//! item returned anything other than 404, an attacker could confirm the
//! existence (and, via timing/status, the moderation state) of feedback the
//! board deliberately hides. This is the privacy sibling of C29 / FR-FBR-27,
//! applied to the WRITE surface.
//!
//! This file is the behavioral leg of `public-board-moderation-gate` Probe C for
//! the vote path; the static leg (Probe B, v1.1.0) proves the same property from
//! source (the vote handlers route through `ensure_board_enabled` + an
//! approved-only resolution before any write).
//!
//! Invariants asserted (each a named test):
//!   1. `vote_on_pending_item_404s` — POST vote on a pending item → 404, no row.
//!   2. `vote_on_rejected_item_404s` — POST vote on a rejected item → 404.
//!   3. `vote_on_board_disabled_404s` — POST vote on an approved item whose
//!      project has `public_board_enabled = FALSE` → 404 (C29 inv. 2 extended).
//!   4. `vote_on_unknown_short_code_404s` — POST vote on an unknown code → 404
//!      (indistinguishable from the hidden-row 404 — no existence signal).
//!   5. `retract_on_pending_item_404s` — DELETE on a pending item → 404 (the
//!      gate runs before the vote lookup, so it can't leak via retract either).
//!   6. `vote_on_approved_item_succeeds` — control: approved + board-enabled →
//!      200 (the gate is not over-broad).
//!
//! Pattern ported from `tests/board_moderation_gate.rs` (the READ-path gate).
//! Rows are moved to approved/rejected via the repository `moderate_in_executor`
//! (the same same-txn path the admin handler drives). ConnectInfo is injected
//! explicitly (oneshot does not run the connect-info layer).

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use chrono::Duration;
use sqlx::PgPool;
use std::net::SocketAddr;
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

fn vote_request(method: &str, project_id: Uuid, short_code: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(format!(
            "/api/v1/projects/{project_id}/board/items/{short_code}/vote"
        ))
        .header(ANON_COOKIE_HEADER, "anon-1")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:54321".parse().unwrap()));
    req
}

// ----- 1: pending item not votable -------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn vote_on_pending_item_404s(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "bvg-pending@example.com").await;
    enable_board(&state, &pscope).await;
    let pending = submit(&state, &pscope, "pending row").await; // stays pending

    let app = board_router(state.clone());
    let resp = app
        .oneshot(vote_request("POST", project_id, pending.as_str()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "pending item must 404, not leak");
}

// ----- 2: rejected item not votable ------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn vote_on_rejected_item_404s(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "bvg-rejected@example.com").await;
    enable_board(&state, &pscope).await;
    let rejected = submit(&state, &pscope, "rejected row").await;
    set_moderation(&state, &pool, &pscope, &rejected, ModerationStatus::Rejected).await;

    let app = board_router(state.clone());
    let resp = app
        .oneshot(vote_request("POST", project_id, rejected.as_str()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "rejected item must 404");
}

// ----- 3: board-disabled project not votable (even with an approved row) ------

#[sqlx::test(migrations = "../../migrations")]
async fn vote_on_board_disabled_404s(pool: PgPool) {
    let state = build_test_state(&pool);
    // Board NOT enabled (default public_board_enabled = FALSE).
    let (pscope, project_id) = seed_project(&state, "bvg-disabled@example.com").await;
    let approved = submit(&state, &pscope, "approved but board off").await;
    set_moderation(&state, &pool, &pscope, &approved, ModerationStatus::Approved).await;

    let app = board_router(state.clone());
    let resp = app
        .oneshot(vote_request("POST", project_id, approved.as_str()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "board-disabled vote must 404");
}

// ----- 4: unknown short_code 404 (no existence signal) -----------------------

#[sqlx::test(migrations = "../../migrations")]
async fn vote_on_unknown_short_code_404s(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "bvg-unknown@example.com").await;
    enable_board(&state, &pscope).await;

    let app = board_router(state.clone());
    let resp = app
        .oneshot(vote_request("POST", project_id, "FB-NOPE00"))
        .await
        .unwrap();
    // Indistinguishable from the hidden-row 404 — no existence oracle.
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "unknown code must 404");
}

// ----- 5: retract on a pending item 404s (gate before vote lookup) -----------

#[sqlx::test(migrations = "../../migrations")]
async fn retract_on_pending_item_404s(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "bvg-ret-pending@example.com").await;
    enable_board(&state, &pscope).await;
    let pending = submit(&state, &pscope, "pending row").await;

    let app = board_router(state.clone());
    let resp = app
        .oneshot(vote_request("DELETE", project_id, pending.as_str()))
        .await
        .unwrap();
    // Gate runs first → 404 (NOT a VoteNotFound 404 that would confirm the item
    // exists-but-unvoted; either way the wire status is the same 404).
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "retract on pending must 404");
}

// ----- 6: control — approved + board-enabled vote succeeds --------------------

#[sqlx::test(migrations = "../../migrations")]
async fn vote_on_approved_item_succeeds(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "bvg-ok@example.com").await;
    enable_board(&state, &pscope).await;
    let approved = submit(&state, &pscope, "approved + votable").await;
    set_moderation(&state, &pool, &pscope, &approved, ModerationStatus::Approved).await;

    let app = board_router(state.clone());
    let resp = app
        .oneshot(vote_request("POST", project_id, approved.as_str()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "the gate must not block a legit vote");
}
