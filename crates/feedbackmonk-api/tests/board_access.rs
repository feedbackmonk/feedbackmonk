#![allow(clippy::doc_markdown)] // test docs name types, routes and columns verbatim
#![allow(dead_code)] // the shared harness block is the same in every board_* prioritization test
#![allow(clippy::similar_names, clippy::too_many_lines, clippy::many_single_char_names)] // paired fixtures (list_a/list_b) and end-to-end scenarios

//! Board access -- Contract C43 (FR-FBR-42/43, DEC-FBR-20/21).
//!
//! A `signed_in` board refuses every anonymous read and write with
//! `401 {"error":"SignInRequired"}`, which is distinct from the C29
//! board-disabled 404; the three `/board/session` routes are the exception.
//! A `public` board serves anonymous reads and votes as before, but the writes
//! that need a person refuse an anonymous viewer. The list takes a kind filter.

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

// ===========================================================================
// FR-FBR-43: a `signed_in` board refuses every anonymous read and write with
// the sign-in response, which is NOT the board-disabled 404.
// ===========================================================================

fn assert_sign_in_required(status: StatusCode, body: &Value, what: &str) {
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{what}: expected 401, got {status} {body}");
    assert_eq!(body["error"], "SignInRequired", "{what}: body {body}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn signed_in_board_refuses_every_anonymous_read_and_write(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "acc-si@example.com", BoardAccess::SignedIn).await;
    let fb = submit_approved(&state, &pool, &scope, "a feature", FeedbackKind::Feature).await;
    let anon = Who::Anon("anon-1");

    let (s, b) = call(&state, "GET", &board_uri(pid), anon, None).await;
    assert_sign_in_required(s, &b, "GET board");
    let (s, b) = call(&state, "GET", &format!("{}?kind=feature", board_uri(pid)), anon, None).await;
    assert_sign_in_required(s, &b, "GET board?kind");
    let (s, b) = call(&state, "GET", &item_uri(pid, &fb), anon, None).await;
    assert_sign_in_required(s, &b, "GET item");
    let (s, b) = call(&state, "POST", &vote_uri(pid, &fb), anon, None).await;
    assert_sign_in_required(s, &b, "POST vote");
    let (s, b) = call(&state, "DELETE", &vote_uri(pid, &fb), anon, None).await;
    assert_sign_in_required(s, &b, "DELETE vote");
    let (s, b) = call(&state, "PUT", &not_needed_uri(pid, &fb), anon, None).await;
    assert_sign_in_required(s, &b, "PUT not-needed");
    let (s, b) = call(&state, "DELETE", &not_needed_uri(pid, &fb), anon, None).await;
    assert_sign_in_required(s, &b, "DELETE not-needed");
    let (s, b) = call(&state, "GET", &priorities_uri(pid), anon, None).await;
    assert_sign_in_required(s, &b, "GET priorities");
    let (s, b) = call(
        &state,
        "PUT",
        &priorities_uri(pid),
        anon,
        Some(json!({ "items": [fb.as_str()] })),
    )
    .await;
    assert_sign_in_required(s, &b, "PUT priorities");

    // No anonymous vote was recorded (FR-FBR-43: none is accepted).
    let id = state
        .feedback
        .resolve_approved_board_feedback_id(&scope, &fb)
        .await
        .unwrap();
    assert_eq!(state.board_votes.vote_count_for_feedback(&scope, id).await.unwrap(), 0);

    // A hidden (pending) item answers the anonymous viewer with the SAME
    // sign-in response, so the refusal confirms nothing about it.
    let pending = state
        .feedback
        .submit_anonymous(&scope, &[7u8; 32], None, "pending", None, FeedbackKind::Feature)
        .await
        .unwrap();
    let (s, b) = call(&state, "GET", &item_uri(pid, &pending), anon, None).await;
    assert_sign_in_required(s, &b, "GET pending item");

    // The session routes are how an anonymous viewer becomes signed in, so
    // they are NOT refused (critic C-001).
    let (s, b) = call(&state, "GET", &session_uri(pid), anon, None).await;
    assert_eq!(s, StatusCode::OK, "GET session must answer an anonymous viewer: {b}");
    assert_eq!(b["signed_in"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn sign_in_response_is_distinct_from_board_disabled_404(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "acc-404@example.com", BoardAccess::SignedIn).await;
    let fb = submit_approved(&state, &pool, &scope, "a feature", FeedbackKind::Feature).await;

    let (s, _) = call(&state, "GET", &board_uri(pid), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Disable the board: every route is the C29 404, whatever the access says,
    // and never the sign-in response.
    state
        .projects
        .set_board_settings(&scope, Some(false), None)
        .await
        .unwrap();
    for (m, uri) in [
        ("GET", board_uri(pid)),
        ("GET", item_uri(pid, &fb)),
        ("POST", vote_uri(pid, &fb)),
        ("PUT", not_needed_uri(pid, &fb)),
        ("GET", priorities_uri(pid)),
        ("GET", session_uri(pid)),
    ] {
        let (s, b) = call(&state, m, &uri, Who::Anon("a"), None).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {uri} on a disabled board: {b}");
        assert_ne!(b["error"], "SignInRequired");
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn signed_in_board_serves_a_signed_in_viewer(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "acc-ok@example.com", BoardAccess::SignedIn).await;
    let fb = submit_approved(&state, &pool, &scope, "a feature", FeedbackKind::Feature).await;
    let key = seed_signing_key(&state, &scope).await;
    let jwt = mint_jwt(&key, pid, "user-a");

    let (s, b) = call(&state, "GET", &board_uri(pid), Who::Bearer(&jwt), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["access"], "signed_in");
    assert_eq!(b["signed_in"], true);
    assert_eq!(codes(&b["items"]), vec![fb.as_str().to_string()]);

    let (s, b) = call(&state, "POST", &vote_uri(pid, &fb), Who::Bearer(&jwt), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["voter_mode"], "jwt");

    // The same identity through a board session.
    let cookie = sign_in(&state, pid, &jwt).await;
    let (s, b) = call(&state, "GET", &item_uri(pid, &fb), Who::Cookie(&cookie), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["viewer"]["upvoted"], true);
}

#[sqlx::test(migrations = "../../migrations")]
async fn public_board_still_serves_anonymous_reads_and_votes(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "acc-pub@example.com", BoardAccess::Public).await;
    let fb = submit_approved(&state, &pool, &scope, "a feature", FeedbackKind::Feature).await;

    let (s, b) = call(&state, "GET", &board_uri(pid), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["access"], "public");
    assert_eq!(b["signed_in"], false);
    assert!(b["items"][0].get("viewer").is_none(), "anonymous: no viewer state");

    let (s, b) = call(&state, "POST", &vote_uri(pid, &fb), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["voter_mode"], "anon");

    // Writes that need a person refuse an anonymous viewer on a public board
    // too (DEC-FBR-20).
    let (s, b) = call(&state, "PUT", &not_needed_uri(pid, &fb), Who::Anon("a"), None).await;
    assert_sign_in_required(s, &b, "public PUT not-needed");
    let (s, b) = call(&state, "GET", &priorities_uri(pid), Who::Anon("a"), None).await;
    assert_sign_in_required(s, &b, "public GET priorities");
}

#[sqlx::test(migrations = "../../migrations")]
async fn an_invalid_bearer_token_is_an_error_never_a_silent_downgrade(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "acc-bad@example.com", BoardAccess::Public).await;
    let _key = seed_signing_key(&state, &scope).await;
    let stranger = DalekSigningKey::generate(&mut OsRng);
    let forged = mint_jwt(&stranger, pid, "user-a");

    let (s, b) = call(&state, "GET", &board_uri(pid), Who::Bearer(&forged), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{b}");
    assert_ne!(b["error"], "SignInRequired", "a bad token is the JWT error, not a prompt");
}

// ===========================================================================
// FR-FBR-42: kind filter.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn kind_filter_returns_only_that_kind_and_counts_only_those(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "acc-kind@example.com", BoardAccess::Public).await;
    let f1 = submit_approved(&state, &pool, &scope, "feature one", FeedbackKind::Feature).await;
    let _b1 = submit_approved(&state, &pool, &scope, "bug one", FeedbackKind::Bug).await;
    let f2 = submit_approved(&state, &pool, &scope, "feature two", FeedbackKind::Feature).await;

    let (s, b) = call(&state, "GET", &format!("{}?kind=feature", board_uri(pid)), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["total"], 2);
    let mut got = codes(&b["items"]);
    got.sort();
    let mut want = vec![f1.as_str().to_string(), f2.as_str().to_string()];
    want.sort();
    assert_eq!(got, want);
    assert!(b["items"].as_array().unwrap().iter().all(|i| i["kind"] == "feature"));

    let (_, b) = call(&state, "GET", &board_uri(pid), Who::Anon("a"), None).await;
    assert_eq!(b["total"], 3, "no filter: every approved item");

    let (s, b) = call(&state, "GET", &format!("{}?kind=bogus", board_uri(pid)), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
}
