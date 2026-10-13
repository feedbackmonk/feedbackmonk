#![allow(clippy::doc_markdown)] // test docs name types, routes and columns verbatim
#![allow(dead_code)] // the shared harness block is the same in every board_* prioritization test
#![allow(clippy::similar_names, clippy::too_many_lines, clippy::many_single_char_names)] // paired fixtures (list_a/list_b) and end-to-end scenarios

//! Board vote retraction -- FR-FBR-47, Contract C43. A signed-in voter
//! (bearer token or board session) retracts at any time; an anonymous voter
//! keeps the C30 window and its 403.

// ----- Shared harness (each tests/*.rs is its own crate; this block is the ----
// ----- same in every board_* prioritization test file) -----------------------

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, Response, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
use rand_core::OsRng;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::{AnonGate, ANON_COOKIE_HEADER};
use feedbackmonk_api::board_router;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_core::{BoardAccess, FeedbackId, FeedbackKind, ModerationStatus};
use feedbackmonk_repository::{
    BoardVoteRepo, ProjectScope, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo,
    SqlxFeedbackRepo, SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo,
    SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo,
};

struct StubMailer;
#[async_trait::async_trait]
impl Mailer for StubMailer {
    async fn send_verify_email(&self, _to: &str, _link: &str, _locale: feedbackmonk_i18n::Locale) -> anyhow::Result<()> {
        Ok(())
    }
    async fn send_password_reset_email(&self, _to: &str, _link: &str, _locale: feedbackmonk_i18n::Locale) -> anyhow::Result<()> {
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

fn build_test_state(pool: &PgPool) -> AppState {
    build_test_state_with(pool, Arc::new(feedbackmonk_repository::SqlxBoardVoteRepo::new(pool.clone())))
}

fn build_test_state_with(pool: &PgPool, board_votes: Arc<dyn BoardVoteRepo>) -> AppState {
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
        verify_token_ttl: chrono::Duration::hours(24),
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
        board_votes,
        voting_cache: feedbackmonk_api::VotingCache::new(),
        started_at: chrono::Utc::now(),
        health: SqlxHealthCheck::new(pool.clone()),
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

/// A verified tenant + project with the board ENABLED and the given access.
async fn seed_board(state: &AppState, email: &str, access: BoardAccess) -> (ProjectScope, Uuid) {
    let tenant = state.tenants.create(email, "hash").await.unwrap();
    let tscope = state.tenants.scope_for(tenant.id).await.unwrap();
    state.tenants.mark_verified(&tscope).await.unwrap();
    let p = state
        .projects
        .create(&tscope, "Proj", &format!("p-{}", &tenant.id.to_string()[..8]))
        .await
        .unwrap();
    let pscope = state.projects.open(&tscope, p.id).await.unwrap();
    state
        .projects
        .set_board_settings(&pscope, Some(true), None)
        .await
        .unwrap();
    state.projects.set_board_access(&pscope, access).await.unwrap();
    (pscope, p.id)
}

/// Submit an anonymous item of `kind` and approve it. Returns its short code.
async fn submit_approved(
    state: &AppState,
    pool: &PgPool,
    scope: &ProjectScope,
    body: &str,
    kind: FeedbackKind,
) -> FeedbackId {
    let fb = state
        .feedback
        .submit_anonymous(scope, &[7u8; 32], None, body, None, kind)
        .await
        .unwrap();
    moderate(state, pool, scope, &fb, ModerationStatus::Approved).await;
    fb
}

async fn moderate(
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

/// A customer-signed end-user JWT (EdDSA) with audience = `project_id`.
fn mint_jwt(signing: &DalekSigningKey, project_id: Uuid, sub: &str) -> String {
    let header = json!({"alg": "EdDSA", "typ": "JWT"});
    let now = chrono::Utc::now().timestamp();
    let payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
        "email": "never-stored@example.com",
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

/// How a request identifies its viewer.
#[derive(Clone, Copy)]
enum Who<'a> {
    Anon(&'a str),
    Bearer(&'a str),
    /// A full `Cookie:` header value (e.g. `fbm_board_<id>=<token>`).
    Cookie(&'a str),
}

fn request(method: &str, uri: &str, who: Who<'_>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    match who {
        Who::Anon(cookie) => builder = builder.header(ANON_COOKIE_HEADER, cookie),
        Who::Bearer(token) => builder = builder.header("authorization", format!("Bearer {token}")),
        Who::Cookie(cookie) => builder = builder.header("cookie", cookie),
    }
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let mut req = builder.body(body).unwrap();
    // Stable peer address so an anonymous voter id is deterministic.
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:54321".parse().unwrap()));
    req
}

async fn send(state: &AppState, req: Request<Body>) -> Response<Body> {
    board_router(state.clone()).oneshot(req).await.unwrap()
}

/// Send and return `(status, json body)`; an empty body reads as `Null`.
async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    who: Who<'_>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let resp = send(state, request(method, uri, who, body)).await;
    let status = resp.status();
    (status, body_json(resp).await)
}

async fn body_json(resp: Response<Body>) -> Value {
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }
}

fn board_uri(project_id: Uuid) -> String {
    format!("/api/v1/projects/{project_id}/board")
}

fn item_uri(project_id: Uuid, fb: &FeedbackId) -> String {
    format!("/api/v1/projects/{project_id}/board/items/{}", fb.as_str())
}

fn vote_uri(project_id: Uuid, fb: &FeedbackId) -> String {
    format!("{}/vote", item_uri(project_id, fb))
}

fn not_needed_uri(project_id: Uuid, fb: &FeedbackId) -> String {
    format!("{}/not-needed", item_uri(project_id, fb))
}

fn priorities_uri(project_id: Uuid) -> String {
    format!("/api/v1/projects/{project_id}/board/priorities")
}

fn session_uri(project_id: Uuid) -> String {
    format!("/api/v1/projects/{project_id}/board/session")
}

/// Exchange `jwt` for a board session; returns the `Cookie:` header value a
/// browser would send back (`name=value`).
async fn sign_in(state: &AppState, project_id: Uuid, jwt: &str) -> String {
    let resp = send(
        state,
        request("POST", &session_uri(project_id), Who::Anon("x"), Some(json!({ "token": jwt }))),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "board-session exchange failed");
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .expect("exchange sets the session cookie")
        .to_str()
        .unwrap()
        .to_string();
    set_cookie.split(';').next().unwrap().trim().to_string()
}

fn codes(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|i| i["short_code"].as_str().unwrap().to_string())
        .collect()
}

use feedbackmonk_repository::{
    BoardNoteRepo, BoardSessionRepo, BoardSignalRepo, BoardVote, RetractOutcome, SqlxBoardVoteRepo,
};

// ----- A vote store seen from later ----------------------------------------
//
// Every retract is decided as if the request arrived `ahead` after the vote:
// the window the handler passes is shortened by `ahead`, which is the same
// test as `now - cast_at > window` with `now` moved forward. No sleep, no
// fixture row edit; the real repository decides.

struct LaterVotes {
    inner: SqlxBoardVoteRepo,
    ahead: std::time::Duration,
}

#[async_trait::async_trait]
impl BoardVoteRepo for LaterVotes {
    async fn cast(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
        voter_id: &str,
        voter_mode: feedbackmonk_core::RoadmapVoterMode,
    ) -> feedbackmonk_repository::Result<BoardVote> {
        self.inner.cast(scope, feedback_id, voter_id, voter_mode).await
    }
    async fn retract(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
        voter_id: &str,
        window: std::time::Duration,
    ) -> feedbackmonk_repository::Result<RetractOutcome> {
        self.inner
            .retract(scope, feedback_id, voter_id, window.saturating_sub(self.ahead))
            .await
    }
    async fn vote_count_for_feedback(&self, scope: &ProjectScope, feedback_id: Uuid) -> feedbackmonk_repository::Result<i64> {
        self.inner.vote_count_for_feedback(scope, feedback_id).await
    }
    async fn has_voted(&self, scope: &ProjectScope, feedback_id: Uuid, voter_id: &str) -> feedbackmonk_repository::Result<bool> {
        self.inner.has_voted(scope, feedback_id, voter_id).await
    }
    fn signals(&self) -> &dyn BoardSignalRepo {
        self.inner.signals()
    }
    fn sessions(&self) -> &dyn BoardSessionRepo {
        self.inner.sessions()
    }
    fn notes(&self) -> &dyn BoardNoteRepo {
        self.inner.notes()
    }
}

/// A state whose retracts are judged 61 s after each cast.
fn state_61s_later(pool: &PgPool) -> AppState {
    build_test_state_with(
        pool,
        Arc::new(LaterVotes {
            inner: SqlxBoardVoteRepo::new(pool.clone()),
            ahead: std::time::Duration::from_secs(61),
        }),
    )
}

async fn vote_count(state: &AppState, pid: Uuid, fb: &FeedbackId) -> i64 {
    let (_, d) = call(state, "GET", &item_uri(pid, fb), Who::Anon("reader"), None).await;
    d["vote_count"].as_i64().unwrap()
}

// ===========================================================================
// FR-FBR-47: a signed-in voter retracts at any time; anonymous keeps C30's
// window and its 403.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn signed_in_retract_after_60s_succeeds(pool: PgPool) {
    let state = state_61s_later(&pool);
    let (scope, pid) = seed_board(&state, "ret-jwt@example.com", BoardAccess::Public).await;
    let f1 = submit_approved(&state, &pool, &scope, "bearer voter", FeedbackKind::Feature).await;
    let f2 = submit_approved(&state, &pool, &scope, "session voter", FeedbackKind::Feature).await;
    let key = seed_signing_key(&state, &scope).await;
    let jwt = mint_jwt(&key, pid, "user-a");

    // Bearer token.
    let (s, _) = call(&state, "POST", &vote_uri(pid, &f1), Who::Bearer(&jwt), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, b) = call(&state, "DELETE", &vote_uri(pid, &f1), Who::Bearer(&jwt), None).await;
    assert_eq!(s, StatusCode::OK, "signed-in retract 61 s later: {b}");
    assert_eq!(b["short_code"], f1.as_str());
    assert_eq!(vote_count(&state, pid, &f1).await, 0);

    // Board session.
    let cookie = sign_in(&state, pid, &jwt).await;
    let (s, b) = call(&state, "POST", &vote_uri(pid, &f2), Who::Cookie(&cookie), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["voter_mode"], "jwt", "a session voter votes as their sub");
    let (s, b) = call(&state, "DELETE", &vote_uri(pid, &f2), Who::Cookie(&cookie), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(vote_count(&state, pid, &f2).await, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn anonymous_retract_after_60s_is_still_403(pool: PgPool) {
    let state = state_61s_later(&pool);
    let (scope, pid) = seed_board(&state, "ret-anon@example.com", BoardAccess::Public).await;
    let fb = submit_approved(&state, &pool, &scope, "anon voter", FeedbackKind::Feature).await;

    let (s, _) = call(&state, "POST", &vote_uri(pid, &fb), Who::Anon("anon-1"), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, b) = call(&state, "DELETE", &vote_uri(pid, &fb), Who::Anon("anon-1"), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{b}");
    assert_eq!(b["error"], "RetractionWindowExpired");
    assert_eq!(vote_count(&state, pid, &fb).await, 1, "the vote is kept");
}

#[sqlx::test(migrations = "../../migrations")]
async fn anonymous_retract_inside_the_window_still_works(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "ret-anon-ok@example.com", BoardAccess::Public).await;
    let fb = submit_approved(&state, &pool, &scope, "anon voter", FeedbackKind::Feature).await;

    let (s, _) = call(&state, "POST", &vote_uri(pid, &fb), Who::Anon("anon-1"), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&state, "DELETE", &vote_uri(pid, &fb), Who::Anon("anon-1"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(vote_count(&state, pid, &fb).await, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn a_session_voter_and_a_bearer_voter_with_one_sub_share_one_vote(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "ret-one@example.com", BoardAccess::Public).await;
    let fb = submit_approved(&state, &pool, &scope, "one key", FeedbackKind::Feature).await;
    let key = seed_signing_key(&state, &scope).await;
    let jwt = mint_jwt(&key, pid, "user-a");
    let cookie = sign_in(&state, pid, &jwt).await;

    let (s, _) = call(&state, "POST", &vote_uri(pid, &fb), Who::Cookie(&cookie), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&state, "POST", &vote_uri(pid, &fb), Who::Bearer(&jwt), None).await;
    assert_eq!(s, StatusCode::CONFLICT, "same sub, same vote");
    let (s, _) = call(&state, "DELETE", &vote_uri(pid, &fb), Who::Bearer(&jwt), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(vote_count(&state, pid, &fb).await, 0);
}
