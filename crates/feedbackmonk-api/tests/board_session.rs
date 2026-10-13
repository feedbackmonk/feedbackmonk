#![allow(clippy::doc_markdown)] // test docs name types, routes and columns verbatim
#![allow(dead_code)] // the shared harness block is the same in every board_* prioritization test
#![allow(clippy::similar_names, clippy::too_many_lines, clippy::many_single_char_names)] // paired fixtures (list_a/list_b) and end-to-end scenarios

//! Board session -- the one-time token handoff (FR-FBR-44, DEC-FBR-21,
//! Contract C43).
//!
//! The end-user JWT is accepted only by `POST /board/session`. The exchange
//! sets a host-only, HttpOnly, Secure, SameSite=Lax cookie named for the
//! project, lasting at most 8 h, and stores only the hash of a fresh random
//! token and the `sub`. A token in a query string or as a cookie is never a
//! session; a session ends at sign-out and at `expires_at`.

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

use feedbackmonk_api::handlers::board_session::{board_session_cookie_name, hash_board_session_token};
use feedbackmonk_core::BOARD_SESSION_MAX_AGE_SECS;
use feedbackmonk_repository::{
    BoardNoteRepo, BoardSession, BoardSessionRepo, BoardSignalRepo, BoardVote, RetractOutcome,
    SqlxBoardVoteRepo,
};

// ----- A store seen from later --------------------------------------------
//
// The board store as a request `ahead` in the future sees it: every session's
// timestamps read `ahead` earlier. It moves the clock without sleeping and
// without touching a row (critic C-005): the API must refuse a session whose
// `expires_at` has passed by its own clock, whatever the store returned.

struct LaterSessions {
    inner: SqlxBoardVoteRepo,
    ahead: chrono::Duration,
}

#[async_trait::async_trait]
impl BoardSessionRepo for LaterSessions {
    async fn create_board_session(
        &self,
        scope: &ProjectScope,
        token_hash: &str,
        user_sub: &str,
        ttl_secs: i64,
    ) -> feedbackmonk_repository::Result<BoardSession> {
        self.inner.sessions().create_board_session(scope, token_hash, user_sub, ttl_secs).await
    }
    async fn resolve_board_session(
        &self,
        scope: &ProjectScope,
        token_hash: &str,
    ) -> feedbackmonk_repository::Result<Option<BoardSession>> {
        Ok(self
            .inner
            .sessions()
            .resolve_board_session(scope, token_hash)
            .await?
            .map(|s| BoardSession {
                user_sub: s.user_sub,
                created_at: s.created_at - self.ahead,
                expires_at: s.expires_at - self.ahead,
            }))
    }
    async fn end_board_session(&self, scope: &ProjectScope, token_hash: &str) -> feedbackmonk_repository::Result<bool> {
        self.inner.sessions().end_board_session(scope, token_hash).await
    }
    async fn purge_expired_board_sessions(&self, scope: &ProjectScope) -> feedbackmonk_repository::Result<u64> {
        self.inner.sessions().purge_expired_board_sessions(scope).await
    }
}

struct LaterBoard {
    inner: SqlxBoardVoteRepo,
    sessions: LaterSessions,
}

#[async_trait::async_trait]
impl BoardVoteRepo for LaterBoard {
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
        self.inner.retract(scope, feedback_id, voter_id, window).await
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
        &self.sessions
    }
    fn notes(&self) -> &dyn BoardNoteRepo {
        self.inner.notes()
    }
}

fn set_cookie_of(resp: &Response<Body>) -> String {
    resp.headers()
        .get("set-cookie")
        .expect("Set-Cookie present")
        .to_str()
        .unwrap()
        .to_string()
}

// ===========================================================================
// FR-FBR-44: the exchange, the cookie, and nothing else accepts the token.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn exchange_sets_a_host_only_httponly_secure_project_cookie(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "sess-x@example.com", BoardAccess::SignedIn).await;
    let key = seed_signing_key(&state, &scope).await;
    let jwt = mint_jwt(&key, pid, "user-a");

    let resp = send(
        &state,
        request("POST", &session_uri(pid), Who::Anon("x"), Some(json!({ "token": jwt }))),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = set_cookie_of(&resp);
    let body = body_json(resp).await;

    let name = board_session_cookie_name(pid);
    assert!(cookie.starts_with(&format!("{name}=")), "cookie name is per project: {cookie}");
    let lower = cookie.to_ascii_lowercase();
    let attrs: Vec<&str> = lower.split(';').map(str::trim).collect();
    assert!(attrs.contains(&"httponly"), "HttpOnly: {cookie}");
    assert!(attrs.contains(&"secure"), "Secure: {cookie}");
    assert!(attrs.contains(&"samesite=lax"), "SameSite=Lax: {cookie}");
    assert!(attrs.contains(&"path=/"), "Path=/: {cookie}");
    assert!(
        attrs.contains(&format!("max-age={BOARD_SESSION_MAX_AGE_SECS}").as_str()),
        "Max-Age is the 8 h ceiling: {cookie}"
    );
    assert!(!lower.contains("domain="), "host-only: no Domain attribute: {cookie}");

    assert_eq!(body["signed_in"], true);
    let expires = chrono::DateTime::parse_from_rfc3339(body["expires_at"].as_str().unwrap()).unwrap();
    let left = expires.with_timezone(&chrono::Utc) - chrono::Utc::now();
    assert!(left <= chrono::Duration::seconds(BOARD_SESSION_MAX_AGE_SECS), "at most 8 h");
    assert!(left > chrono::Duration::seconds(BOARD_SESSION_MAX_AGE_SECS - 60));
    // The session carries the sub and nothing else; the response echoes neither
    // the sub nor any other claim.
    let text = body.to_string();
    assert!(!text.contains("user-a") && !text.contains("never-stored"), "{text}");

    // The cookie value is a fresh random token, not the JWT; only its hash is
    // stored, so the raw value does not resolve as a key.
    let pair = cookie.split(';').next().unwrap();
    let token = pair.split_once('=').unwrap().1;
    assert!(!token.is_empty() && token != jwt);
    let stored = state
        .board_votes
        .sessions()
        .resolve_board_session(&scope, &hash_board_session_token(token))
        .await
        .unwrap()
        .expect("stored under the token's hash");
    assert_eq!(stored.user_sub, "user-a");
    assert!(state
        .board_votes
        .sessions()
        .resolve_board_session(&scope, &hash_board_session_token(&jwt))
        .await
        .unwrap()
        .is_none());

    // The cookie signs the browser in: a signed_in board now serves it.
    let (s, b) = call(&state, "GET", &board_uri(pid), Who::Cookie(pair), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["signed_in"], true);
    let (s, b) = call(&state, "GET", &session_uri(pid), Who::Cookie(pair), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["signed_in"], true);
    assert!(b["expires_at"].is_string());
}

#[sqlx::test(migrations = "../../migrations")]
async fn the_token_is_accepted_only_by_the_post_exchange(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "sess-only@example.com", BoardAccess::SignedIn).await;
    let key = seed_signing_key(&state, &scope).await;
    let jwt = mint_jwt(&key, pid, "user-a");

    // In a query string: the board page and the session read ignore it.
    for uri in [
        format!("{}?token={jwt}", board_uri(pid)),
        format!("{}?jwt={jwt}", board_uri(pid)),
        format!("{}?access_token={jwt}", board_uri(pid)),
    ] {
        let (s, b) = call(&state, "GET", &uri, Who::Anon("a"), None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{uri}: {b}");
        assert_eq!(b["error"], "SignInRequired");
    }
    let (s, b) = call(&state, "GET", &format!("{}?token={jwt}", session_uri(pid)), Who::Anon("a"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["signed_in"], false, "GET never exchanges a token: {b}");
    assert!(b["expires_at"].is_null());

    // As a Cookie: the JWT itself is not a session.
    let jar = format!("{}={jwt}", board_session_cookie_name(pid));
    let (s, _) = call(&state, "GET", &board_uri(pid), Who::Cookie(&jar), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // A POST with a body that is not the token shape is refused.
    let resp = send(&state, request("POST", &session_uri(pid), Who::Anon("a"), Some(json!({ "jwt": jwt })))).await;
    assert!(resp.status().is_client_error(), "missing `token` field: {}", resp.status());
    assert!(resp.headers().get("set-cookie").is_none());

    // A token that fails verification: the JWT error, and no cookie.
    let stranger = DalekSigningKey::generate(&mut OsRng);
    let forged = mint_jwt(&stranger, pid, "user-a");
    let resp = send(&state, request("POST", &session_uri(pid), Who::Anon("a"), Some(json!({ "token": forged })))).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(resp.headers().get("set-cookie").is_none());
    assert_ne!(body_json(resp).await["error"], "SignInRequired");

    // Audience = project: a token minted for another project is refused.
    let wrong_aud = mint_jwt(&key, Uuid::new_v4(), "user-a");
    let resp = send(&state, request("POST", &session_uri(pid), Who::Anon("a"), Some(json!({ "token": wrong_aud })))).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(resp.headers().get("set-cookie").is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn a_session_is_scoped_to_its_project(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope_a, pid_a) = seed_board(&state, "sess-a@example.com", BoardAccess::SignedIn).await;
    let (_scope_b, pid_b) = seed_board(&state, "sess-b@example.com", BoardAccess::SignedIn).await;
    let key = seed_signing_key(&state, &scope_a).await;
    let cookie_a = sign_in(&state, pid_a, &mint_jwt(&key, pid_a, "user-a")).await;

    // Same token under project B's cookie name: not a session there.
    let token = cookie_a.split_once('=').unwrap().1;
    let cookie_b = format!("{}={token}", board_session_cookie_name(pid_b));
    let (s, _) = call(&state, "GET", &board_uri(pid_b), Who::Cookie(&cookie_b), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&state, "GET", &board_uri(pid_b), Who::Cookie(&cookie_a), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn sign_out_ends_the_session_and_expires_the_cookie(pool: PgPool) {
    let state = build_test_state(&pool);
    let (scope, pid) = seed_board(&state, "sess-out@example.com", BoardAccess::SignedIn).await;
    let key = seed_signing_key(&state, &scope).await;
    let cookie = sign_in(&state, pid, &mint_jwt(&key, pid, "user-a")).await;

    let resp = send(&state, request("DELETE", &session_uri(pid), Who::Cookie(&cookie), None)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cleared = set_cookie_of(&resp).to_ascii_lowercase();
    assert!(cleared.starts_with(&format!("{}=", board_session_cookie_name(pid))));
    assert!(cleared.contains("max-age=0"), "the cookie is expired: {cleared}");
    let body = body_json(resp).await;
    assert_eq!(body["signed_in"], false);
    assert!(body["expires_at"].is_null());

    // The old cookie no longer signs in, even if the browser kept it.
    let (s, b) = call(&state, "GET", &board_uri(pid), Who::Cookie(&cookie), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{b}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn an_expired_session_is_refused(pool: PgPool) {
    // The store as seen 8 h + 1 s after the exchange (no sleep, critic C-005).
    let later = Arc::new(LaterBoard {
        inner: SqlxBoardVoteRepo::new(pool.clone()),
        sessions: LaterSessions {
            inner: SqlxBoardVoteRepo::new(pool.clone()),
            ahead: chrono::Duration::seconds(BOARD_SESSION_MAX_AGE_SECS + 1),
        },
    });
    let state = build_test_state_with(&pool, later);
    let (scope, pid) = seed_board(&state, "sess-exp@example.com", BoardAccess::SignedIn).await;
    let key = seed_signing_key(&state, &scope).await;
    let cookie = sign_in(&state, pid, &mint_jwt(&key, pid, "user-a")).await;

    let (s, b) = call(&state, "GET", &board_uri(pid), Who::Cookie(&cookie), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "an expired session is no session: {b}");
    assert_eq!(b["error"], "SignInRequired");
    let (s, b) = call(&state, "GET", &session_uri(pid), Who::Cookie(&cookie), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["signed_in"], false, "{b}");

    // Control: the same cookie one hour in is still live.
    let soon = Arc::new(LaterBoard {
        inner: SqlxBoardVoteRepo::new(pool.clone()),
        sessions: LaterSessions {
            inner: SqlxBoardVoteRepo::new(pool.clone()),
            ahead: chrono::Duration::hours(1),
        },
    });
    let state_soon = build_test_state_with(&pool, soon);
    let (s, b) = call(&state_soon, "GET", &board_uri(pid), Who::Cookie(&cookie), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
}
