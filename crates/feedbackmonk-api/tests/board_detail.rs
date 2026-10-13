#![allow(clippy::doc_markdown)] // test docs name types, routes and columns verbatim
#![allow(dead_code)] // the shared harness block is the same in every board_* prioritization test
#![allow(clippy::similar_names, clippy::too_many_lines, clippy::many_single_char_names)] // paired fixtures (list_a/list_b) and end-to-end scenarios

//! Board detail -- Contract C43 `GET /board/items/{short_code}`
//! (FR-FBR-57/58/59): the public record, the owner's board note, and none of
//! the submitter's or the team's private data.

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
use feedbackmonk_repository::ReplyVisibility;

const DETAIL_KEYS: [&str; 9] = [
    "accepted_at",
    "board_note",
    "body",
    "in_ranking_pool",
    "kind",
    "short_code",
    "status",
    "timeline",
    "vote_count",
];

fn sorted_keys(v: &Value) -> Vec<String> {
    let mut keys: Vec<String> = v.as_object().expect("object").keys().cloned().collect();
    keys.sort_unstable();
    keys
}

/// An approved, authenticated submission carrying every private field the
/// board must never show: identity, metadata, a crash link, a public and an
/// internal reply, a status change with a note, and a translated body.
async fn seed_private_item(state: &AppState, pool: &PgPool, scope: &ProjectScope) -> FeedbackId {
    let meta = json!({ "plan": "SECRET-META-PLAN" });
    let fb = state
        .feedback
        .submit_authenticated(
            scope,
            "auth0|SECRET-SUB",
            Some("secret-submitter@private.example"),
            Some("Secret Submitter Name"),
            Some(&meta),
            Some("SECRET-CRASH-ID"),
            "Bitte Dunkelmodus hinzufuegen",
            None,
            FeedbackKind::Feature,
        )
        .await
        .unwrap();
    moderate(state, pool, scope, &fb, ModerationStatus::Approved).await;

    for (text, vis) in [
        ("SECRET-PUBLIC-REPLY", ReplyVisibility::Public),
        ("SECRET-INTERNAL-REPLY", ReplyVisibility::Internal),
    ] {
        state
            .feedback_replies
            .create(scope, &fb, text, vis, scope.tenant_id())
            .await
            .unwrap();
    }

    let mut tx = pool.begin().await.unwrap();
    let from = state
        .feedback
        .update_status_in_executor(scope, &mut tx, &fb, FeedbackStatus::Triaged)
        .await
        .unwrap();
    state
        .feedback_history
        .append_in_executor(
            scope,
            &mut tx,
            &fb,
            from,
            FeedbackStatus::Triaged,
            Some("SECRET-STATUS-NOTE"),
            None,
            scope.tenant_id(),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let id = state
        .feedback
        .resolve_approved_board_feedback_id(scope, &fb)
        .await
        .unwrap();
    state
        .feedback
        .set_translation(id, "SECRET-TRANSLATED please add dark mode", "de")
        .await
        .unwrap();
    fb
}

fn assert_nothing_private(v: &Value) {
    let text = v.to_string();
    for needle in [
        "SECRET",
        "secret-submitter",
        "Secret Submitter",
        "end_user",
        "submitter",
        "anon_token",
        "external_metadata",
        "crash_event_id",
        "reply",
        "replies",
        "reason_note",
        "transitioned_by",
        "attachment",
        "translated",
        "translation",
        "source_lang",
        "locale",
    ] {
        assert!(!text.contains(needle), "board detail leaks `{needle}`: {text}");
    }
}

// ===========================================================================
// FR-FBR-57/59: the detail is the item's public record and nothing else.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn detail_carries_the_public_record_and_none_of_the_private_fields(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "det-priv@example.com", BoardAccess::Public).await;
    let fb = seed_private_item(&state, &pool, &scope).await;

    let (s, d) = call(&state, "GET", &item_uri(pid, &fb), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(sorted_keys(&d), DETAIL_KEYS, "anonymous detail: exactly the public keys");
    assert_eq!(d["short_code"], fb.as_str());
    assert_eq!(d["body"], "Bitte Dunkelmodus hinzufuegen", "verbatim, submitted language");
    assert_eq!(d["kind"], "feature");
    assert_eq!(d["status"], "triaged");
    assert_eq!(d["vote_count"], 0);
    assert!(d["accepted_at"].is_string());
    assert_eq!(d["board_note"], Value::Null);
    assert_eq!(d["in_ranking_pool"], true);
    let timeline = d["timeline"].as_array().unwrap();
    assert!(!timeline.is_empty(), "the status change is on the timeline");
    for entry in timeline {
        assert_eq!(sorted_keys(entry), ["at", "status"], "status and date only: {entry}");
    }
    assert_eq!(timeline.last().unwrap()["status"], "triaged");
    assert_nothing_private(&d);

    // Signed in: the same record plus the caller's own state, nothing more.
    let key = seed_signing_key(&state, &scope).await;
    let jwt = mint_jwt(&key, pid, "viewer-1");
    let (s, d) = call(&state, "GET", &item_uri(pid, &fb), Who::Bearer(&jwt), None).await;
    assert_eq!(s, StatusCode::OK);
    let mut want: Vec<&str> = DETAIL_KEYS.to_vec();
    want.push("viewer");
    want.sort_unstable();
    assert_eq!(sorted_keys(&d), want);
    assert_eq!(sorted_keys(&d["viewer"]), ["not_needed", "priority_position", "upvoted"]);
    assert_nothing_private(&d);

    // The list never carries them either.
    for who in [Who::Anon("a"), Who::Bearer(&jwt)] {
        let (_, list) = call(&state, "GET", &board_uri(pid), who, None).await;
        assert_nothing_private(&list);
    }
}

// ===========================================================================
// FR-FBR-58: the owner's board note shows on the detail.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn detail_shows_the_board_note_when_there_is_one(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "det-note@example.com", BoardAccess::Public).await;
    let fb = submit_approved(&state, &pool, &scope, "dark mode", FeedbackKind::Feature).await;

    state
        .board_votes
        .notes()
        .set_board_note(&scope, &fb, "We're starting this in November.")
        .await
        .unwrap();
    let (s, d) = call(&state, "GET", &item_uri(pid, &fb), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(d["board_note"]["body"], "We're starting this in November.");
    assert!(d["board_note"]["updated_at"].is_string());
    assert_eq!(sorted_keys(&d["board_note"]), ["body", "updated_at"]);

    state.board_votes.notes().clear_board_note(&scope, &fb).await.unwrap();
    let (_, d) = call(&state, "GET", &item_uri(pid, &fb), Who::Anon("a"), None).await;
    assert_eq!(d["board_note"], Value::Null);
}

#[sqlx::test(migrations = "../../migrations")]
async fn detail_of_a_hidden_or_out_of_pool_item(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "det-hidden@example.com", BoardAccess::Public).await;
    let pending = state
        .feedback
        .submit_anonymous(&scope, &[7u8; 32], None, "pending", None, FeedbackKind::Feature)
        .await
        .unwrap();
    let (s, _) = call(&state, "GET", &item_uri(pid, &pending), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "a pending item is not on the board");

    let bug = submit_approved(&state, &pool, &scope, "a bug", FeedbackKind::Bug).await;
    let (s, d) = call(&state, "GET", &item_uri(pid, &bug), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(d["in_ranking_pool"], false, "a bug is on the board but not in the pool");
}
