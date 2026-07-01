//! Router-level integration tests for the Phase A A4 submit-surface additions
//! (plan: 20260701T161200-feedbackmonk-phase-a-gitcellar-contract.md, Stream 2):
//!
//! - **A4a first-class `severity`** — optional `low|medium|high|blocker` body
//!   field, valid in BOTH auth and anon modes, echoed back, persisted to
//!   `feedback.severity` (migration 00020). Includes Stream 1's deferred
//!   non-null-severity read coverage (the `me_feedback` surface).
//! - **A4b `Idempotency-Key` dedupe** — a retry with the same header returns
//!   the ORIGINAL `feedback_id` with 200 and creates NO second row
//!   (transactional insert-then-conflict-rollback in the repository;
//!   migration 00021). First-write-wins on a same-key different-body retry.
//!
//! Harness mirrors `tests/router_submission_integration.rs` (real Postgres
//! pool per test via `sqlx::test`; DEC-FBR-03 — the repository layer is the
//! sole query path, so router-level tests exercise the same code the binary
//! runs).

use std::net::SocketAddr;
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
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{admin_feedback_routes, submission_router, worker_a_router};
use feedbackmonk_core::Severity;
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
        email_notifier: Arc::new(NoopEmailNotifier),
        session_secret: Arc::new([0x42u8; 32]),
        public_url: Arc::from("http://test.local"),
        verify_token_ttl: Duration::hours(24),
        anon_gate: AnonGate::new(NonZeroU32::new(10).unwrap()),
        login_gate: feedbackmonk_anon::LoginGate::with_default_quota(),
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
        "name": "Idempotency Integration",
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

/// Build a submission request. `idempotency_key`, when `Some`, is sent as the
/// `Idempotency-Key` header (A4b). `ConnectInfo` is a stable peer so all
/// requests in a test share one anon rate-limit bucket.
#[allow(clippy::needless_pass_by_value)] // owned `Value` keeps call sites cleaner
fn submission_request(
    project_id: Uuid,
    body_json: Value,
    bearer: Option<&str>,
    idempotency_key: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::post(format!("/api/v1/projects/{project_id}/feedback"))
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(key) = idempotency_key {
        builder = builder.header("Idempotency-Key", key);
    }
    let mut req = builder
        .body(Body::from(serde_json::to_vec(&body_json).unwrap()))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:54321".parse().unwrap()));
    req
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 64 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// POST a submission and return `(status, response_json)`.
async fn submit(
    app: &axum::Router,
    project_id: Uuid,
    body_json: Value,
    bearer: Option<&str>,
    idempotency_key: Option<&str>,
) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(submission_request(project_id, body_json, bearer, idempotency_key))
        .await
        .unwrap();
    let status = resp.status();
    (status, body_to_json(resp.into_body()).await)
}

/// Count the project's committed feedback rows via the repository read path.
async fn row_count(state: &AppState, pscope: &ProjectScope) -> usize {
    state.feedback.list_recent(pscope, 100).await.unwrap().len()
}

// ----- A4a: severity ------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn severity_round_trips_and_persists_on_auth_submit(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "sev-auth@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = build_router(state.clone());

    let jwt = mint_jwt(&signing, project_id, "sev-user");
    let (status, body) = submit(
        &app,
        project_id,
        json!({"body": "crashes on save", "kind": "bug", "severity": "high"}),
        Some(&jwt),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["echo"]["severity"], "high", "severity must be echoed");

    // Persistence through the me_feedback read surface (EndUserFeedback) —
    // closes Stream 1's deferred non-null severity read coverage.
    let (items, total) = state
        .feedback
        .list_for_end_user(&pscope, "sev-user", 10, 0, None)
        .await
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(items[0].severity, Some(Severity::High));
    assert_eq!(items[0].feedback_id.as_str(), body["feedback_id"].as_str().unwrap());
}

#[sqlx::test(migrations = "../../migrations")]
async fn severity_accepted_on_anonymous_submit(pool: PgPool) {
    // Severity is tenant-generic: valid WITHOUT a JWT (unlike crash_event_id).
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "sev-anon@example.com").await;
    let app = build_router(state.clone());

    let (status, body) = submit(
        &app,
        project_id,
        json!({"body": "anon blocker report", "kind": "bug", "severity": "blocker"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["echo"]["severity"], "blocker");
    assert_eq!(row_count(&state, &pscope).await, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn invalid_severity_returns_400(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "sev-bad@example.com").await;
    let app = build_router(state.clone());

    // D-A4 chose `blocker`, not `critical` — the GitCellar-internal value must
    // be rejected, not silently mapped.
    let (status, body) = submit(
        &app,
        project_id,
        json!({"body": "x", "kind": "bug", "severity": "critical"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err = body["error"].as_str().unwrap_or_default();
    assert!(
        err.contains("severity must be one of low|medium|high|blocker"),
        "400 body must name the valid values (got {err:?})"
    );
    // The rejected submission must not have written a row.
    assert_eq!(row_count(&state, &pscope).await, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn absent_severity_is_null_and_behavior_unchanged(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "sev-none@example.com").await;
    let app = build_router(state.clone());

    let (status, body) = submit(
        &app,
        project_id,
        json!({"body": "no severity here", "kind": "other"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["echo"]["severity"].is_null(), "echo.severity must be null");
    assert_eq!(row_count(&state, &pscope).await, 1);
}

// ----- A4b: Idempotency-Key dedupe ----------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn same_idempotency_key_returns_same_id_and_one_row(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "idem-same@example.com").await;
    let app = build_router(state.clone());

    let payload = json!({"body": "flaky network retry", "kind": "bug"});
    let (s1, b1) = submit(&app, project_id, payload.clone(), None, Some("retry-key-1")).await;
    let (s2, b2) = submit(&app, project_id, payload, None, Some("retry-key-1")).await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK, "a dedupe hit is a SUCCESS (200), not an error");
    assert_eq!(
        b1["feedback_id"], b2["feedback_id"],
        "the retry must return the ORIGINAL feedback_id"
    );
    assert_eq!(
        row_count(&state, &pscope).await,
        1,
        "exactly ONE feedback row per idempotency key"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn different_idempotency_keys_create_distinct_rows(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "idem-diff@example.com").await;
    let app = build_router(state.clone());

    let payload = json!({"body": "two distinct submissions", "kind": "other"});
    let (_, b1) = submit(&app, project_id, payload.clone(), None, Some("key-A")).await;
    let (_, b2) = submit(&app, project_id, payload, None, Some("key-B")).await;
    assert_ne!(b1["feedback_id"], b2["feedback_id"]);
    assert_eq!(row_count(&state, &pscope).await, 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn absent_idempotency_header_keeps_current_behavior(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "idem-absent@example.com").await;
    let app = build_router(state.clone());

    let payload = json!({"body": "no header, new row each time", "kind": "other"});
    let (_, b1) = submit(&app, project_id, payload.clone(), None, None).await;
    let (_, b2) = submit(&app, project_id, payload, None, None).await;
    assert_ne!(b1["feedback_id"], b2["feedback_id"]);
    assert_eq!(row_count(&state, &pscope).await, 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn same_key_different_body_is_first_write_wins(pool: PgPool) {
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "idem-fww@example.com").await;
    let app = build_router(state.clone());

    let (_, b1) = submit(
        &app,
        project_id,
        json!({"body": "the FIRST body", "kind": "bug"}),
        None,
        Some("fww-key"),
    )
    .await;
    let (s2, b2) = submit(
        &app,
        project_id,
        json!({"body": "a DIFFERENT retry body", "kind": "bug"}),
        None,
        Some("fww-key"),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b1["feedback_id"], b2["feedback_id"]);

    let rows = state.feedback.list_recent(&pscope, 10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].body, "the FIRST body", "the stored body is the first write");
}

#[sqlx::test(migrations = "../../migrations")]
async fn auth_mode_dedupes_with_severity(pool: PgPool) {
    // Both A4 legs together on the JWT path: the retry returns the original
    // id AND the severity written by the first submit persists.
    let state = build_test_state(&pool);
    let (pscope, project_id) = seed_project(&state, "idem-auth@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = build_router(state.clone());

    let jwt = mint_jwt(&signing, project_id, "retry-user");
    let payload = json!({"body": "auth retry", "kind": "bug", "severity": "medium"});
    let (_, b1) = submit(&app, project_id, payload.clone(), Some(&jwt), Some("auth-key")).await;
    let (s2, b2) = submit(&app, project_id, payload, Some(&jwt), Some("auth-key")).await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b1["feedback_id"], b2["feedback_id"]);
    assert_eq!(b2["echo"]["severity"], "medium");

    let (items, total) = state
        .feedback
        .list_for_end_user(&pscope, "retry-user", 10, 0, None)
        .await
        .unwrap();
    assert_eq!(total, 1, "one committed row despite two submits");
    assert_eq!(items[0].severity, Some(Severity::Medium));
}
