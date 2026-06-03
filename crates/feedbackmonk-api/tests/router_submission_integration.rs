#![allow(clippy::doc_markdown)] // test-file doc comments name JwtError variants verbatim

//! Router-level integration tests for the public submission handler
//! `POST /api/v1/projects/{project_id}/feedback` (FR-FBR-03 + FR-FBR-05 +
//! FR-FBR-06; Contract C3). Carry-forward critic C-002 — Stage 1 deferred
//! these because `tests/handlers.rs` was scoped to the signup/projects
//! lane and adding submission coverage during P0 would have widened the
//! handler-test surface mid-arc. Stage 3 (P1 closes-the-loop) is the
//! quiescent boundary where the deferred coverage lands.
//!
//! Coverage (5 named tests):
//!   1. `jwt_submit_happy_path` — auth-mode, valid EdDSA signature, 200 + FB-id
//!   2. `anon_submit_happy_path` — no Authorization header, 200 + FB-id + Set-Cookie
//!   3. `bad_jwt_returns_401_alg_none` — `alg=none` attack → 401 AlgorithmNotAllowed
//!   4. `anon_rate_limit_returns_429_on_excess` — gate quota exceeded → 429 + Retry-After
//!   5. `empty_body_returns_400` — body validation, Contract C3
//!
//! Pattern ported from `tests/handlers.rs`; uses `sqlx::test` for a real
//! Postgres pool per test (DEC-FBR-03 — the repository layer is the sole
//! query path, so router-level tests exercise the same code the binary runs).

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

use feedbackmonk_anon::{AnonGate, ANON_COOKIE_HEADER};
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{admin_feedback_routes, submission_router, worker_a_router};
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

/// Build an `AppState` with a configurable anon-gate quota so the rate-limit
/// test can drive exceedance in <10 calls (default `DEFAULT_RATE_LIMIT_PER_HOUR`
/// = 10 would slow the test without changing coverage).
fn build_test_state(pool: &PgPool, anon_quota_per_hour: u32) -> AppState {
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
        anon_gate: AnonGate::new(NonZeroU32::new(anon_quota_per_hour).unwrap()),
        login_gate: feedbackmonk_anon::LoginGate::with_default_quota(),
        jwt_iat_leeway_seconds: 5,
        // P2 fields — mechanical AppState extension per
        // docs/test-modifications/20260514-p2-appstate-roadmap-fields.md.
        roadmap_items: Arc::new(feedbackmonk_repository::SqlxRoadmapItemRepo::new(
            pool.clone(),
        )),
        roadmap_votes: Arc::new(feedbackmonk_repository::SqlxRoadmapVoteRepo::new(
            pool.clone(),
        )),
        voting_cache: feedbackmonk_api::VotingCache::new(),
        started_at: chrono::Utc::now(),
        health: SqlxHealthCheck::new(pool.clone()),
        // P3 Stage 1 fixture extension — see
        // docs/test-modifications/20260514-p3-appstate-tier-quotas.md.
        // Seeded tenants default to Free. Each router-submission test
        // exercises ≤ 4 submissions per tenant — well within Free's
        // 50/month cap, so the new tier-check at the submission path
        // (Phase 4 wiring) does not alter test outcomes.
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

/// Compose the same router shape as `main::build_app` (minus middleware).
/// The submission route lives on `submission_router`; the admin-session +
/// signup routes live on `worker_a_router`; admin feedback on
/// `admin_feedback_routes`. All three share the same `AppState`.
fn build_router(state: AppState) -> axum::Router {
    worker_a_router(state.clone())
        .merge(submission_router(state.clone()))
        .merge(admin_feedback_routes(state))
}

/// Seed a verified tenant + one project. Returns the `ProjectScope` (for
/// repository writes) and the bare `project_id` (for HTTP URL building).
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

/// Register a fresh Ed25519 signing key against the project. Returns the
/// dalek signing key so the caller can mint JWTs verifiable by the project.
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

/// Mint an EdDSA JWT for `(project_id, sub)` valid for ~5 minutes.
fn mint_jwt(signing: &DalekSigningKey, project_id: Uuid, sub: &str) -> String {
    let header = json!({"alg": "EdDSA", "typ": "JWT"});
    let now = chrono::Utc::now().timestamp();
    let payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
        "email": format!("{sub}@example.com"),
        "name": "Router Integration",
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

/// Mint an `alg=none` JWT — no signature, used to drive the
/// AlgorithmNotAllowed → 401 path (Contract C2 invariant 1).
fn mint_alg_none_jwt(project_id: Uuid, sub: &str) -> String {
    let header = json!({"alg": "none", "typ": "JWT"});
    let now = chrono::Utc::now().timestamp();
    let payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    format!("{header_b64}.{payload_b64}.")
}

/// Build a submission request, populating `ConnectInfo` so the handler's
/// `ConnectInfo<SocketAddr>` extractor resolves (the actual binary uses
/// `into_make_service_with_connect_info`; `oneshot` needs an explicit
/// extension).
#[allow(clippy::needless_pass_by_value)] // owned `Value` keeps call sites cleaner
fn submission_request(
    project_id: Uuid,
    body_json: Value,
    bearer: Option<&str>,
    anon_cookie: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::post(format!("/api/v1/projects/{project_id}/feedback"))
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(cookie) = anon_cookie {
        builder = builder.header(ANON_COOKIE_HEADER, cookie);
    }
    let mut req = builder
        .body(Body::from(serde_json::to_vec(&body_json).unwrap()))
        .unwrap();
    // axum's ConnectInfo extractor reads this extension. Stable peer for
    // the rate-limit test so all 11 calls hash to the same bucket.
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:54321".parse().unwrap()));
    req
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 64 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ----- Tests ------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn jwt_submit_happy_path(pool: PgPool) {
    let state = build_test_state(&pool, 10);
    let (pscope, project_id) = seed_project(&state, "jwt-ok@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;
    let app = build_router(state);

    let jwt = mint_jwt(&signing, project_id, "end-user-1");
    let req = submission_request(
        project_id,
        json!({"body": "router integration: auth-mode happy path", "kind": "bug"}),
        Some(&jwt),
        None,
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "expected 200 OK on JWT submit");
    let body = body_to_json(resp.into_body()).await;
    let fb_id = body["feedback_id"].as_str().expect("feedback_id present");
    assert!(
        fb_id.starts_with("FB-"),
        "feedback_id must be FB-XXXXXX form (got {fb_id})"
    );
    assert_eq!(body["echo"]["kind"], "bug");
}

#[sqlx::test(migrations = "../../migrations")]
async fn anon_submit_happy_path(pool: PgPool) {
    let state = build_test_state(&pool, 10);
    let (_pscope, project_id) = seed_project(&state, "anon-ok@example.com").await;
    let app = build_router(state);

    // No Authorization header AND no anon-cookie header → handler mints a
    // cookie and emits Set-Cookie on the response.
    let req = submission_request(
        project_id,
        json!({"body": "router integration: anon-mode happy path", "kind": "feature"}),
        None,
        None,
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "expected 200 OK on anon submit");
    let set_cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .expect("Set-Cookie issued for fresh anon submission")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        set_cookie.contains(ANON_COOKIE_HEADER),
        "Set-Cookie must carry the anon cookie name (got {set_cookie})"
    );
    let body = body_to_json(resp.into_body()).await;
    let fb_id = body["feedback_id"].as_str().expect("feedback_id present");
    assert!(fb_id.starts_with("FB-"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn bad_jwt_returns_401_alg_none(pool: PgPool) {
    let state = build_test_state(&pool, 10);
    let (pscope, project_id) = seed_project(&state, "bad-jwt@example.com").await;
    // Seed a real signing key so we exercise the active-keys path; the
    // alg=none token must fail in the header allow-list check BEFORE any
    // signature work (Contract C2 invariant 1).
    let _ = seed_signing_key(&state, &pscope).await;
    let app = build_router(state);

    let tampered = mint_alg_none_jwt(project_id, "attacker");
    let req = submission_request(
        project_id,
        json!({"body": "router integration: alg=none must 401", "kind": "other"}),
        Some(&tampered),
        None,
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "alg=none JWT must yield 401"
    );
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(
        body["error"], "AlgorithmNotAllowed",
        "401 body must carry JwtError variant name per Contract C2"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn anon_rate_limit_returns_429_on_excess(pool: PgPool) {
    // Quota of 2/hr keeps the test fast; the gate's `(token_hash, project_id)`
    // key derives from `(client_ip, anon_cookie, project_id)`. Reusing the
    // same cookie across N+1 calls drives the same bucket.
    let state = build_test_state(&pool, 2);
    let (_pscope, project_id) = seed_project(&state, "ratelimit@example.com").await;
    let app = build_router(state);
    let stable_cookie = "test-cookie-stable";

    for i in 0..2 {
        let req = submission_request(
            project_id,
            json!({"body": format!("burst {i}"), "kind": "other"}),
            None,
            Some(stable_cookie),
        );
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "submission {i} (within quota) must succeed"
        );
    }

    // 3rd submission against the same (ip, cookie, project) bucket → 429.
    let req = submission_request(
        project_id,
        json!({"body": "burst overflow", "kind": "other"}),
        None,
        Some(stable_cookie),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "submission past quota must yield 429"
    );
    let retry_after = resp
        .headers()
        .get("Retry-After")
        .expect("Retry-After header present on 429")
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!(retry_after >= 1, "Retry-After must be >=1s (rounded up)");
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["error"], "RateLimitExceeded");
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_body_returns_400(pool: PgPool) {
    let state = build_test_state(&pool, 10);
    let (_pscope, project_id) = seed_project(&state, "empty-body@example.com").await;
    let app = build_router(state);

    // Empty body → Contract C3 body validation; never reaches the
    // anon/JWT dispatch. Returns 400 with the BadRequest error type.
    let req = submission_request(
        project_id,
        json!({"body": "", "kind": "bug"}),
        None,
        None,
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty body must yield 400"
    );
}
