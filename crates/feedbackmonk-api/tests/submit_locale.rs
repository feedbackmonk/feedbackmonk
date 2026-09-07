//! Router-level integration tests for FR-FBR-37 / Contract C37 — capturing the
//! submitter's UI locale at submit time.
//!
//! Submit is the ONE moment the submitter's language is knowable: the request
//! carries it and nothing afterwards does. These tests pin the resolution ladder
//! and, more importantly, the property that makes it safe to ship a widget ahead
//! of the server's locale table:
//!
//!   **a locale can never fail a submit.**
//!
//! Unlike `sentiment` / `severity` / `rating` — semantic content the caller must
//! get right, where an unknown value is a 400 — a locale the deployment does not
//! ship is silently dropped to NULL. Anything else would let a widget shipping a
//! new language break submission on an older server.
//!
//! Invariants asserted (each a named test):
//!   1. `payload_locale_is_stored` — an explicit `locale` wins and is persisted.
//!   2. `accept_language_is_the_fallback` — no payload field ⇒ the header is
//!      resolved (`de-AT` → `de`), so a plain browser still records a language.
//!   3. `payload_locale_beats_accept_language` — the widget states what it
//!      actually rendered; the browser's list is only a fallback.
//!   4. `unshipped_locale_stores_null_and_never_4xx` — the load-bearing one.
//!   5. `absent_everything_stores_null` — NULL means *unknown*, never `en`.
//!   6. `english_is_stored_when_actually_requested` — `en` is a real answer, and
//!      must stay distinguishable from the NULL above.
//!   7. `auth_mode_captures_the_same_way` — the ladder is mode-independent.
//!   8. `submitter_locale_is_absent_from_the_public_echo` — it is PII-adjacent
//!      admin data and must not appear in the submit response.
//!
//! Real Postgres per test via `sqlx::test` (DEC-FBR-03 — the repository layer is
//! the sole query path). Harness shape ported from `tests/submit_idempotency.rs`.

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
use feedbackmonk_core::FeedbackId;
use feedbackmonk_repository::{
    ProjectScope, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxSigningKeyRepo,
    SqlxTenantRepo, SqlxTierQuotaRepo,
};

// ----- Fakes ------------------------------------------------------------------

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
        ip_gate: feedbackmonk_anon::IpGate::with_default_quota(),
        trusted_proxy_hops: 0,
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


// ----- helpers ----------------------------------------------------------------

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
        "name": "Locale Integration",
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig.to_bytes()))
}

/// POST a submission; `accept_language`, when `Some`, is sent as the header.
async fn submit(
    app: &axum::Router,
    project_id: Uuid,
    body_json: Value,
    accept_language: Option<&str>,
    bearer: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::post(format!("/api/v1/projects/{project_id}/feedback"))
        .header("content-type", "application/json");
    if let Some(al) = accept_language {
        builder = builder.header("accept-language", al);
    }
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let mut req = builder
        .body(Body::from(serde_json::to_vec(&body_json).unwrap()))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:54321".parse().unwrap()));
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

/// Read back what LANDED, not what a response echoed — through the repository,
/// because DEC-FBR-03 makes the tenant-scoped repository the sole query path and
/// the `multi-tenant-isolation-check` oracle enforces it in test code too. Using
/// the same read the admin detail endpoint uses also means these tests would
/// catch the column being dropped from that projection.
async fn stored_locale(
    state: &AppState,
    scope: &ProjectScope,
    short_code: &str,
) -> Option<String> {
    let (feedback, _) = state
        .feedback
        .get_with_history(scope, &FeedbackId::from(short_code.to_string()))
        .await
        .unwrap();
    feedback.submitter_locale
}

/// Submit anonymously and return the stored `submitter_locale`, asserting the
/// submission itself succeeded.
async fn submit_and_read_locale(
    app: &axum::Router,
    state: &AppState,
    scope: &ProjectScope,
    project_id: Uuid,
    body_json: Value,
    accept_language: Option<&str>,
) -> Option<String> {
    let (status, json) = submit(app, project_id, body_json, accept_language, None).await;
    assert_eq!(status, StatusCode::OK, "submit rejected: {json}");
    stored_locale(state, scope, json["feedback_id"].as_str().unwrap()).await
}

// ----- tests ------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn payload_locale_is_stored(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = build_router(state.clone());
    let (scope, project_id) = seed_project(&state, "payload@example.com").await;

    let got = submit_and_read_locale(
        &app,
        &state,
        &scope,
        project_id,
        json!({"body": "hallo", "locale": "de"}),
        None,
    )
    .await;
    assert_eq!(got.as_deref(), Some("de"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn accept_language_is_the_fallback(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = build_router(state.clone());
    let (scope, project_id) = seed_project(&state, "header@example.com").await;

    // `de-AT` is not a shipped bundle; the C34 resolver walks it to its base.
    let got = submit_and_read_locale(
        &app,
        &state,
        &scope,
        project_id,
        json!({"body": "servus"}),
        Some("de-AT,de;q=0.9,en;q=0.5"),
    )
    .await;
    assert_eq!(got.as_deref(), Some("de"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn payload_locale_beats_accept_language(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = build_router(state.clone());
    let (scope, project_id) = seed_project(&state, "both@example.com").await;

    // The widget rendered in French; the browser prefers German. What the human
    // actually READ is the widget's language.
    let got = submit_and_read_locale(
        &app,
        &state,
        &scope,
        project_id,
        json!({"body": "bonjour", "locale": "fr"}),
        Some("de,en;q=0.5"),
    )
    .await;
    assert_eq!(got.as_deref(), Some("fr"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn unshipped_locale_stores_null_and_never_4xx(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = build_router(state.clone());
    let (scope, project_id) = seed_project(&state, "unshipped@example.com").await;

    // Every one of these is a value the server cannot honour. NONE of them may
    // fail the submit — the feedback is what matters, the locale is metadata.
    let hostile = "x".repeat(5000);
    for locale in [
        "da",             // a real language we do not ship
        "de-AT",          // not canonical: `locale` is exact-match by contract
        "xx-YY-ZZ",       // structurally plausible nonsense
        "",               // empty string
        "' OR 1=1 --",    // hostile
        hostile.as_str(), // unbounded
    ] {
        let got = submit_and_read_locale(
            &app,
            &state,
            &scope,
            project_id,
            json!({"body": "still accepted", "locale": locale}),
            None,
        )
        .await;
        assert_eq!(got, None, "locale {locale:?} should have stored NULL");
    }

    // A garbage Accept-Language header is equally harmless.
    let got = submit_and_read_locale(
        &app,
        &state,
        &scope,
        project_id,
        json!({"body": "header garbage"}),
        Some("*;q=nonsense, ;;;, ----"),
    )
    .await;
    assert_eq!(got, None);
}

#[sqlx::test(migrations = "../../migrations")]
async fn absent_everything_stores_null(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = build_router(state.clone());
    let (scope, project_id) = seed_project(&state, "absent@example.com").await;

    let got = submit_and_read_locale(
        &app,
        &state,
        &scope,
        project_id,
        json!({"body": "no locale anywhere"}),
        None,
    )
    .await;
    assert_eq!(
        got, None,
        "NULL means *we do not know*; storing `en` here would be an invented fact"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn english_is_stored_when_actually_requested(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = build_router(state.clone());
    let (scope, project_id) = seed_project(&state, "english@example.com").await;

    // The counterpart to the test above: an explicit English preference is a
    // stated fact and IS recorded, so the two cases stay distinguishable.
    let from_payload = submit_and_read_locale(
        &app,
        &state,
        &scope,
        project_id,
        json!({"body": "hello", "locale": "en"}),
        None,
    )
    .await;
    assert_eq!(from_payload.as_deref(), Some("en"));

    let from_header = submit_and_read_locale(
        &app,
        &state,
        &scope,
        project_id,
        json!({"body": "hello again"}),
        Some("en-GB,en;q=0.9"),
    )
    .await;
    assert_eq!(from_header.as_deref(), Some("en"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn auth_mode_captures_the_same_way(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = build_router(state.clone());
    let (scope, project_id) = seed_project(&state, "authmode@example.com").await;
    let signing = seed_signing_key(&state, &scope).await;
    let jwt = mint_jwt(&signing, project_id, "auth0|locale-user");

    let (status, json) = submit(
        &app,
        project_id,
        json!({"body": "signed in", "locale": "ja"}),
        Some("de"),
        Some(&jwt),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        stored_locale(&state, &scope, json["feedback_id"].as_str().unwrap())
            .await
            .as_deref(),
        Some("ja"),
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn submitter_locale_is_absent_from_the_public_echo(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = build_router(state.clone());
    let (_scope, project_id) = seed_project(&state, "echo@example.com").await;

    let (status, json) = submit(
        &app,
        project_id,
        json!({"body": "check the echo", "locale": "de"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // C37: the column is admin-read-only. The submit response is the most public
    // surface there is, and it must not reflect the value back.
    let serialised = json.to_string();
    assert!(
        !serialised.contains("submitter_locale") && !serialised.contains("\"locale\""),
        "submit echo leaked the submitter locale: {serialised}"
    );
}
