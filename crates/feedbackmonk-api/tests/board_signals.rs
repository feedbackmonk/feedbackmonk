#![allow(clippy::doc_markdown)] // test docs name types, routes and columns verbatim
#![allow(dead_code)] // the shared harness block is the same in every board_* prioritization test
#![allow(clippy::similar_names, clippy::too_many_lines, clippy::many_single_char_names)] // paired fixtures (list_a/list_b) and end-to-end scenarios

//! Board signals -- a signed-in viewer's own state, "not needed", and the
//! public surface's silence about them (FR-FBR-46/48/49, Contract C43).

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

fn item_by_code<'a>(list: &'a Value, fb: &FeedbackId) -> &'a Value {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["short_code"] == fb.as_str())
        .unwrap_or_else(|| panic!("{} not on the board page: {list}", fb.as_str()))
}

/// Every key anywhere in a JSON value, recursively.
fn all_keys(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, child) in m {
                out.push(k.clone());
                all_keys(child, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|c| all_keys(c, out)),
        _ => {}
    }
}

// ===========================================================================
// FR-FBR-46: a signed-in viewer sees their own state, and only their own.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn per_viewer_state_is_the_callers_only(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "sig-own@example.com", BoardAccess::Public).await;
    let up = submit_approved(&state, &pool, &scope, "upvoted by A", FeedbackKind::Feature).await;
    let nn = submit_approved(&state, &pool, &scope, "not needed by A", FeedbackKind::Feature).await;
    let ranked = submit_approved(&state, &pool, &scope, "ranked by A", FeedbackKind::Feature).await;
    let key = seed_signing_key(&state, &scope).await;
    let a = mint_jwt(&key, pid, "user-a");
    let b_jwt = mint_jwt(&key, pid, "user-b");

    let (s, _) = call(&state, "POST", &vote_uri(pid, &up), Who::Bearer(&a), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&state, "PUT", &not_needed_uri(pid, &nn), Who::Bearer(&a), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, b) = call(
        &state,
        "PUT",
        &priorities_uri(pid),
        Who::Bearer(&a),
        Some(json!({ "items": [ranked.as_str()] })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");

    // A sees exactly A's own state — on the list, and on the detail.
    let (_, list_a) = call(&state, "GET", &board_uri(pid), Who::Bearer(&a), None).await;
    assert_eq!(list_a["signed_in"], true);
    assert_eq!(item_by_code(&list_a, &up)["viewer"], json!({"upvoted": true, "not_needed": false, "priority_position": null}));
    assert_eq!(item_by_code(&list_a, &nn)["viewer"], json!({"upvoted": false, "not_needed": true, "priority_position": null}));
    assert_eq!(item_by_code(&list_a, &ranked)["viewer"], json!({"upvoted": false, "not_needed": false, "priority_position": 0}));
    let (_, detail_a) = call(&state, "GET", &item_uri(pid, &nn), Who::Bearer(&a), None).await;
    assert_eq!(detail_a["viewer"]["not_needed"], true);

    // B, signed in through a board session, sees none of A's state.
    let cookie_b = sign_in(&state, pid, &b_jwt).await;
    let (_, list_b) = call(&state, "GET", &board_uri(pid), Who::Cookie(&cookie_b), None).await;
    for fb in [&up, &nn, &ranked] {
        assert_eq!(
            item_by_code(&list_b, fb)["viewer"],
            json!({"upvoted": false, "not_needed": false, "priority_position": null}),
            "B must not see A's state on {}",
            fb.as_str()
        );
    }
    let (_, detail_b) = call(&state, "GET", &item_uri(pid, &nn), Who::Cookie(&cookie_b), None).await;
    assert_eq!(detail_b["viewer"]["not_needed"], false);
    // The public aggregate (vote_count) is the same for everyone.
    assert_eq!(item_by_code(&list_b, &up)["vote_count"], 1);

    // An anonymous viewer receives no per-viewer state at all.
    let (_, list_anon) = call(&state, "GET", &board_uri(pid), Who::Anon("z"), None).await;
    assert_eq!(list_anon["signed_in"], false);
    for item in list_anon["items"].as_array().unwrap() {
        assert!(item.get("viewer").is_none(), "anonymous item carries viewer: {item}");
    }
    let (_, detail_anon) = call(&state, "GET", &item_uri(pid, &up), Who::Anon("z"), None).await;
    assert!(detail_anon.get("viewer").is_none(), "{detail_anon}");

    // No response names the other user.
    for v in [&list_b, &detail_b, &list_anon] {
        assert!(!v.to_string().contains("user-a"), "{v}");
    }
}

// ===========================================================================
// FR-FBR-48: not needed — signed-in, pool-only, exclusive with the upvote.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn not_needed_and_upvote_are_exclusive(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "sig-excl@example.com", BoardAccess::Public).await;
    let fb = submit_approved(&state, &pool, &scope, "a feature", FeedbackKind::Feature).await;
    let key = seed_signing_key(&state, &scope).await;
    let a = mint_jwt(&key, pid, "user-a");

    let (s, _) = call(&state, "POST", &vote_uri(pid, &fb), Who::Bearer(&a), None).await;
    assert_eq!(s, StatusCode::OK);

    let (s, b) = call(&state, "PUT", &not_needed_uri(pid, &fb), Who::Bearer(&a), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b, json!({"short_code": fb.as_str(), "not_needed": true, "upvote_cleared": true}));
    let (_, d) = call(&state, "GET", &item_uri(pid, &fb), Who::Bearer(&a), None).await;
    assert_eq!(d["vote_count"], 0, "the upvote was cleared");
    assert_eq!(d["viewer"]["upvoted"], false);
    assert_eq!(d["viewer"]["not_needed"], true);

    // Marking again is idempotent.
    let (s, b) = call(&state, "PUT", &not_needed_uri(pid, &fb), Who::Bearer(&a), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["upvote_cleared"], false);

    // Upvoting clears the mark.
    let (s, _) = call(&state, "POST", &vote_uri(pid, &fb), Who::Bearer(&a), None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, d) = call(&state, "GET", &item_uri(pid, &fb), Who::Bearer(&a), None).await;
    assert_eq!(d["viewer"]["upvoted"], true);
    assert_eq!(d["viewer"]["not_needed"], false);

    // Clear works and reports the state.
    call(&state, "PUT", &not_needed_uri(pid, &fb), Who::Bearer(&a), None).await;
    let (s, b) = call(&state, "DELETE", &not_needed_uri(pid, &fb), Who::Bearer(&a), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b, json!({"short_code": fb.as_str(), "not_needed": false}));
    let (_, d) = call(&state, "GET", &item_uri(pid, &fb), Who::Bearer(&a), None).await;
    assert_eq!(d["viewer"]["not_needed"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn not_needed_outside_the_ranking_pool_is_404(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "sig-pool@example.com", BoardAccess::Public).await;
    let bug = submit_approved(&state, &pool, &scope, "a bug", FeedbackKind::Bug).await;
    let pending = state
        .feedback
        .submit_anonymous(&scope, &[7u8; 32], None, "pending", None, FeedbackKind::Feature)
        .await
        .unwrap();
    let key = seed_signing_key(&state, &scope).await;
    let a = mint_jwt(&key, pid, "user-a");

    for fb in [&bug, &pending] {
        let (s, b) = call(&state, "PUT", &not_needed_uri(pid, fb), Who::Bearer(&a), None).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{}: {b}", fb.as_str());
    }
    let (s, _) = call(&state, "PUT", &not_needed_uri(pid, &FeedbackId::from("FB-NOPE00".to_string())), Who::Bearer(&a), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ===========================================================================
// FR-FBR-49: a not-needed count appears in no public response, and the
// board's order does not depend on it.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn no_public_response_carries_a_not_needed_count(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "sig-count@example.com", BoardAccess::Public).await;
    let f1 = submit_approved(&state, &pool, &scope, "one", FeedbackKind::Feature).await;
    let f2 = submit_approved(&state, &pool, &scope, "two", FeedbackKind::Feature).await;
    let f3 = submit_approved(&state, &pool, &scope, "three", FeedbackKind::Feature).await;
    let key = seed_signing_key(&state, &scope).await;
    let observer = mint_jwt(&key, pid, "observer");

    let (_, before) = call(&state, "GET", &board_uri(pid), Who::Anon("z"), None).await;
    let order_before = codes(&before["items"]);

    // Five users mark f2 not needed; nobody marks the others.
    for i in 0..5 {
        let jwt = mint_jwt(&key, pid, &format!("marker-{i}"));
        let (s, _) = call(&state, "PUT", &not_needed_uri(pid, &f2), Who::Bearer(&jwt), None).await;
        assert_eq!(s, StatusCode::OK);
    }

    let (_, after) = call(&state, "GET", &board_uri(pid), Who::Anon("z"), None).await;
    assert_eq!(codes(&after["items"]), order_before, "order must not depend on marks");

    let mut responses = vec![after];
    for who in [Who::Anon("z"), Who::Bearer(&observer)] {
        responses.push(call(&state, "GET", &board_uri(pid), who, None).await.1);
        responses.push(call(&state, "GET", &format!("{}?kind=feature", board_uri(pid)), who, None).await.1);
        for fb in [&f1, &f2, &f3] {
            responses.push(call(&state, "GET", &item_uri(pid, fb), who, None).await.1);
        }
    }
    responses.push(call(&state, "GET", &priorities_uri(pid), Who::Bearer(&observer), None).await.1);
    responses.push(
        call(&state, "PUT", &priorities_uri(pid), Who::Bearer(&observer), Some(json!({"items": [f2.as_str(), f1.as_str()]})))
            .await
            .1,
    );
    responses.push(call(&state, "POST", &vote_uri(pid, &f2), Who::Bearer(&observer), None).await.1);
    responses.push(call(&state, "GET", &session_uri(pid), Who::Anon("z"), None).await.1);

    for r in &responses {
        assert!(!r.is_null(), "every probed route answers JSON");
        let mut keys = Vec::new();
        all_keys(r, &mut keys);
        for k in &keys {
            let k = k.to_ascii_lowercase();
            assert!(
                !(k.contains("not_needed") && k != "not_needed") && !k.contains("priority_score") && k != "rank",
                "public response carries `{k}`: {r}"
            );
        }
        // `not_needed` appears only as the caller's own flag inside `viewer`,
        // and the observer never marked anything.
        for item in r["items"].as_array().into_iter().flatten() {
            assert!(item.get("not_needed").is_none(), "{item}");
            if let Some(v) = item.get("viewer") {
                assert_eq!(v["not_needed"], false);
            }
        }
        assert!(!r.to_string().contains("marker-"), "{r}");
    }
}
