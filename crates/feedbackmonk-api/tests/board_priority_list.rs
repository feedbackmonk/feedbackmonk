#![allow(clippy::doc_markdown)] // test docs name types, routes and columns verbatim
#![allow(dead_code)] // the shared harness block is the same in every board_* prioritization test
#![allow(clippy::similar_names, clippy::too_many_lines, clippy::many_single_char_names)] // paired fixtures (list_a/list_b) and end-to-end scenarios

//! Personal priority lists -- Contract C43 `GET|PUT /board/priorities`
//! (FR-FBR-50/51/61), plus critic C-004: concurrent saves and reads of one
//! user's list surface `Ok` or `Conflict`, never a raw unique violation.

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

use feedbackmonk_core::FeedbackStatus;
use feedbackmonk_repository::RepoError;

async fn set_status(state: &AppState, pool: &PgPool, scope: &ProjectScope, fb: &FeedbackId, to: FeedbackStatus) {
    let mut tx = pool.begin().await.unwrap();
    state
        .feedback
        .update_status_in_executor(scope, &mut tx, fb, to)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn put_list(state: &AppState, pid: Uuid, who: Who<'_>, items: &[&FeedbackId]) -> (StatusCode, Value) {
    let items: Vec<&str> = items.iter().map(|f| f.as_str()).collect();
    call(state, "PUT", &priorities_uri(pid), who, Some(json!({ "items": items }))).await
}

fn s(fb: &FeedbackId) -> String {
    fb.as_str().to_string()
}

// ===========================================================================
// FR-FBR-50: one ordered list per viewer, saved whole.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn a_list_is_saved_whole_and_read_back(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "pl-save@example.com", BoardAccess::SignedIn).await;
    let f1 = submit_approved(&state, &pool, &scope, "one", FeedbackKind::Feature).await;
    let f2 = submit_approved(&state, &pool, &scope, "two", FeedbackKind::Feature).await;
    let f3 = submit_approved(&state, &pool, &scope, "three", FeedbackKind::Feature).await;
    let _bug = submit_approved(&state, &pool, &scope, "a bug", FeedbackKind::Bug).await;
    let key = seed_signing_key(&state, &scope).await;
    let cookie = sign_in(&state, pid, &mint_jwt(&key, pid, "user-a")).await;
    let me = Who::Cookie(&cookie);

    // Empty to start: every pool item is "not yet ranked"; the bug is not in
    // the pool at all.
    let (st, b) = call(&state, "GET", &priorities_uri(pid), me, None).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(codes(&b["entries"]), Vec::<String>::new());
    let mut unranked = codes(&b["unranked"]);
    unranked.sort();
    let mut pool_codes = vec![s(&f1), s(&f2), s(&f3)];
    pool_codes.sort();
    assert_eq!(unranked, pool_codes);
    assert_eq!(b["influences_priority"], false);
    assert_eq!(b["removed_hidden"], 0);

    // One item: accepted, does not yet influence priority.
    let (st, b) = put_list(&state, pid, me, &[&f2]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(codes(&b["entries"]), vec![s(&f2)]);
    assert_eq!(b["influences_priority"], false);

    // The whole order, top first.
    let (st, b) = put_list(&state, pid, me, &[&f3, &f1, &f2]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(codes(&b["entries"]), vec![s(&f3), s(&f1), s(&f2)]);
    assert_eq!(codes(&b["unranked"]), Vec::<String>::new());
    assert_eq!(b["influences_priority"], true);
    let entry = &b["entries"][0];
    let mut keys: Vec<&str> = entry.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["accepted_at", "body", "kind", "short_code", "status", "vote_count"]);

    // Reorder and shrink; the read-back is the saved order.
    let (st, _) = put_list(&state, pid, me, &[&f1, &f3]).await;
    assert_eq!(st, StatusCode::OK);
    let (_, b) = call(&state, "GET", &priorities_uri(pid), me, None).await;
    assert_eq!(codes(&b["entries"]), vec![s(&f1), s(&f3)]);
    assert_eq!(codes(&b["unranked"]), vec![s(&f2)]);
    let (_, board) = call(&state, "GET", &board_uri(pid), me, None).await;
    let pos = |fb: &FeedbackId| {
        board["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["short_code"] == fb.as_str())
            .unwrap()["viewer"]["priority_position"]
            .clone()
    };
    assert_eq!(pos(&f1), json!(0));
    assert_eq!(pos(&f3), json!(1));
    assert_eq!(pos(&f2), Value::Null);

    // [] clears.
    let (st, b) = put_list(&state, pid, me, &[]).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(codes(&b["entries"]), Vec::<String>::new());
}

#[sqlx::test(migrations = "../../migrations")]
async fn a_bad_save_leaves_the_stored_list_unchanged(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "pl-bad@example.com", BoardAccess::Public).await;
    let f1 = submit_approved(&state, &pool, &scope, "one", FeedbackKind::Feature).await;
    let f2 = submit_approved(&state, &pool, &scope, "two", FeedbackKind::Feature).await;
    let bug = submit_approved(&state, &pool, &scope, "a bug", FeedbackKind::Bug).await;
    let pending = state
        .feedback
        .submit_anonymous(&scope, &[7u8; 32], None, "pending", None, FeedbackKind::Feature)
        .await
        .unwrap();
    let key = seed_signing_key(&state, &scope).await;
    let a = mint_jwt(&key, pid, "user-a");
    let me = Who::Bearer(&a);

    let (st, _) = put_list(&state, pid, me, &[&f1, &f2]).await;
    assert_eq!(st, StatusCode::OK);

    for bad in [&bug, &pending] {
        let (st, b) = put_list(&state, pid, me, &[&f2, bad]).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{b}");
        assert_eq!(b["error"], "NotInRankingPool");
    }
    let (st, b) = put_list(&state, pid, me, &[&f2, &f1, &f2]).await;
    assert_eq!(st, StatusCode::CONFLICT, "{b}");
    assert_eq!(b["error"], "DuplicateItem");

    let (_, b) = call(&state, "GET", &priorities_uri(pid), me, None).await;
    assert_eq!(codes(&b["entries"]), vec![s(&f1), s(&f2)], "unchanged after refused saves");

    // Another user's list is untouched by mine, and mine is not theirs.
    let other = mint_jwt(&key, pid, "user-b");
    let (_, b) = call(&state, "GET", &priorities_uri(pid), Who::Bearer(&other), None).await;
    assert_eq!(codes(&b["entries"]), Vec::<String>::new());
}

// ===========================================================================
// FR-FBR-51: an item leaving the pool is reported once, then dropped.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn items_leaving_the_pool_are_reported_once_then_dropped(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "pl-drop@example.com", BoardAccess::Public).await;
    let keep = submit_approved(&state, &pool, &scope, "keep", FeedbackKind::Feature).await;
    let ship = submit_approved(&state, &pool, &scope, "ship", FeedbackKind::Feature).await;
    let hide = submit_approved(&state, &pool, &scope, "hide", FeedbackKind::Feature).await;
    let key = seed_signing_key(&state, &scope).await;
    let a = mint_jwt(&key, pid, "user-a");
    let me = Who::Bearer(&a);

    let (st, _) = put_list(&state, pid, me, &[&ship, &keep, &hide]).await;
    assert_eq!(st, StatusCode::OK);

    set_status(&state, &pool, &scope, &ship, FeedbackStatus::Shipped).await;
    moderate(&state, &pool, &scope, &hide, ModerationStatus::Pending).await;

    let (_, b) = call(&state, "GET", &priorities_uri(pid), me, None).await;
    assert_eq!(codes(&b["entries"]), vec![s(&keep)]);
    assert_eq!(codes(&b["removed"]), vec![s(&ship)]);
    assert_eq!(b["removed_hidden"], 1);
    assert!(!b.to_string().contains("\"hide\""), "a hidden item's content is not shown");

    let (_, b) = call(&state, "GET", &priorities_uri(pid), me, None).await;
    assert_eq!(codes(&b["entries"]), vec![s(&keep)]);
    assert_eq!(codes(&b["removed"]), Vec::<String>::new());
    assert_eq!(b["removed_hidden"], 0);
}

// ===========================================================================
// Critic C-004: two concurrent saves or reads of one user's list never surface
// a raw unique-violation; each succeeds or is `Conflict`.
// ===========================================================================

fn assert_ok_or_conflict<T: std::fmt::Debug>(r: &Result<T, RepoError>, what: &str) {
    match r {
        Ok(_) | Err(RepoError::Conflict) => {}
        Err(e) => panic!("{what}: concurrent list write surfaced {e:?}"),
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn concurrent_saves_and_reads_of_one_list_never_raise_a_raw_unique_violation(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, _pid) = seed_board(&state, "pl-race@example.com", BoardAccess::Public).await;
    let mut items = Vec::new();
    for i in 0..6 {
        items.push(submit_approved(&state, &pool, &scope, &format!("item {i}"), FeedbackKind::Feature).await);
    }
    let signals = state.board_votes.signals();
    let forward: Vec<FeedbackId> = items.clone();
    let backward: Vec<FeedbackId> = items.iter().rev().cloned().collect();

    for round in 0..25 {
        let (r1, r2) = tokio::join!(
            signals.save_priority_list(&scope, "racer", &forward),
            signals.save_priority_list(&scope, "racer", &backward),
        );
        assert_ok_or_conflict(&r1, &format!("round {round} save A"));
        assert_ok_or_conflict(&r2, &format!("round {round} save B"));

        let stored = signals.load_priority_list(&scope, "racer").await.unwrap();
        let got: Vec<FeedbackId> = stored.entries.iter().map(|i| i.feedback_id.clone()).collect();
        assert!(got == forward || got == backward, "round {round}: list is one whole save, got {got:?}");
    }

    // Reads that drop removed entries race each other and a save.
    for round in 0..10 {
        signals.save_priority_list(&scope, "racer", &forward).await.unwrap();
        set_status(&state, &pool, &scope, &items[round % 6], FeedbackStatus::WontFix).await;
        let (r1, r2, r3) = tokio::join!(
            signals.load_priority_list(&scope, "racer"),
            signals.load_priority_list(&scope, "racer"),
            signals.save_priority_list(&scope, "racer", &[]),
        );
        assert_ok_or_conflict(&r1, &format!("round {round} load A"));
        assert_ok_or_conflict(&r2, &format!("round {round} load B"));
        assert_ok_or_conflict(&r3, &format!("round {round} clear"));
        set_status(&state, &pool, &scope, &items[round % 6], FeedbackStatus::Triaged).await;
    }
}
