#![allow(clippy::doc_markdown)] // test-file doc comments name JWT claim fields verbatim

//! Phase A A1 — `DELETE /api/v1/projects/{project_id}/me/feedback/{fb}`
//! (P0 right-to-erasure) integration fixture. Mirrors the
//! `me_feedback_isolation.rs` harness (real Postgres via `sqlx::test`,
//! EdDSA JWT minting, router `oneshot`).
//!
//! Invariants asserted here (each a named test):
//!   1. `delete_own_feedback_erases_rows_and_bytes` — the owner's DELETE
//!      returns 204; attachment object BYTES are purged from the store;
//!      attachment rows + replies vanish via FK cascade; the row disappears
//!      from the list and the thread 404s; a second DELETE → 404.
//!   2. `delete_other_users_feedback_404_and_row_survives` — deleting another
//!      sub's feedback (under the caller's own valid JWT) is 404, and the
//!      target row SURVIVES untouched (never a leak / never a cross-sub
//!      erasure).
//!   3. `delete_anonymous_feedback_404_and_row_survives` — anonymous rows are
//!      structurally unreachable through the JWT erasure surface.
//!   4. `delete_unknown_id_404` — an unknown FB id is 404.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
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
use feedbackmonk_api::storage::{LocalFsStorage, ObjectStore};
use feedbackmonk_api::{me_feedback_data_router, me_feedback_router, MeFeedbackDataState};
use feedbackmonk_core::{FeedbackId, FeedbackKind};
use feedbackmonk_repository::{
    AttachmentKind, AttachmentRepo, NewAttachment, ProjectScope, ReplyVisibility,
    SqlxAttachmentRepo, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
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

/// The A1/A5 sub-router state, sharing the AppState repo handles + a
/// test-local `LocalFsStorage` whose byte-level effects the tests inspect.
fn build_data_state(state: &AppState, storage: Arc<dyn ObjectStore>) -> MeFeedbackDataState {
    MeFeedbackDataState {
        projects: Arc::clone(&state.projects),
        signing_keys: Arc::clone(&state.signing_keys),
        feedback: Arc::clone(&state.feedback),
        feedback_replies: Arc::clone(&state.feedback_replies),
        attachments: Arc::new(SqlxAttachmentRepo::new(state.pool.clone())),
        storage,
        jwt_iat_leeway_seconds: 5,
    }
}

/// Fresh per-test LocalFs store rooted in a unique temp dir.
fn test_storage() -> Arc<dyn ObjectStore> {
    let tmp = std::env::temp_dir().join(format!("fbm-me-del-{}", Uuid::new_v4()));
    Arc::new(LocalFsStorage::new(tmp, "http://test.local/attachments"))
}

/// Read + thread + delete + export composed like `main::build_app` does.
fn build_app(state: AppState, data_state: MeFeedbackDataState) -> Router {
    me_feedback_router(state).merge(me_feedback_data_router(data_state))
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
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

fn request(method: &str, path: &str, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 256 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Store an attachment object + its metadata row for `fb_uuid`; returns the
/// storage key.
async fn seed_attachment(
    storage: &Arc<dyn ObjectStore>,
    attachments: &SqlxAttachmentRepo,
    scope: &ProjectScope,
    fb_uuid: Uuid,
    name: &str,
) -> String {
    let key = format!("attachments/test/{fb_uuid}/{name}.png");
    let url = storage.put(&key, "image/png", b"PNGBYTES").await.unwrap();
    attachments
        .insert(
            scope,
            fb_uuid,
            &NewAttachment {
                kind: AttachmentKind::Image,
                storage_key: &key,
                url: &url,
                content_type: "image/png",
                byte_size: 8,
            },
        )
        .await
        .unwrap();
    key
}

// ----- Invariant 1: full erasure (bytes + rows + cascade) ---------------------

#[sqlx::test(migrations = "../../migrations")]
async fn delete_own_feedback_erases_rows_and_bytes(pool: PgPool) {
    let state = build_test_state(&pool);
    let storage = test_storage();
    let data_state = build_data_state(&state, Arc::clone(&storage));
    let attachments = SqlxAttachmentRepo::new(pool.clone());
    let (pscope, project_id) = seed_project(&state, "erase@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    // The caller's feedback with replies (public + internal) and 2 attachments.
    let fb = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "erase me", None, FeedbackKind::Bug)
        .await
        .unwrap();
    let admin = Uuid::new_v4();
    state
        .feedback_replies
        .create(&pscope, &fb, "public reply", ReplyVisibility::Public, admin)
        .await
        .unwrap();
    state
        .feedback_replies
        .create(&pscope, &fb, "internal note", ReplyVisibility::Internal, admin)
        .await
        .unwrap();
    let fb_uuid = attachments.resolve_feedback_uuid(&pscope, fb.as_str()).await.unwrap();
    let key1 = seed_attachment(&storage, &attachments, &pscope, fb_uuid, "one").await;
    let key2 = seed_attachment(&storage, &attachments, &pscope, fb_uuid, "two").await;
    // Sanity: bytes are retrievable before the erasure.
    assert_eq!(storage.get(&key1).await.unwrap(), b"PNGBYTES");

    let app = build_app(state.clone(), data_state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let del_path = format!("/api/v1/projects/{project_id}/me/feedback/{}", fb.as_str());

    // DELETE → 204.
    let resp = app
        .clone()
        .oneshot(request("DELETE", &del_path, Some(&jwt)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Object BYTES purged (not just metadata rows).
    assert!(storage.get(&key1).await.is_err(), "attachment 1 bytes survived erasure");
    assert!(storage.get(&key2).await.is_err(), "attachment 2 bytes survived erasure");

    // Cascade: attachment rows + replies gone.
    assert!(
        attachments.list_for_feedback(&pscope, fb_uuid).await.unwrap().is_empty(),
        "attachment rows survived the FK cascade"
    );
    assert!(
        state
            .feedback_replies
            .list_for_feedback(&pscope, &fb)
            .await
            .unwrap()
            .is_empty(),
        "reply rows survived the FK cascade"
    );

    // Gone from the list…
    let resp = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/api/v1/projects/{project_id}/me/feedback"),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["total"], 0);
    assert!(body["items"].as_array().unwrap().is_empty());

    // …and the thread 404s.
    let resp = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/api/v1/projects/{project_id}/me/feedback/{}/thread", fb.as_str()),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Second delete of the already-erased id → 404.
    let resp = app
        .oneshot(request("DELETE", &del_path, Some(&jwt)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ----- Invariant 2: another sub's row is untouchable --------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn delete_other_users_feedback_404_and_row_survives(pool: PgPool) {
    let state = build_test_state(&pool);
    let data_state = build_data_state(&state, test_storage());
    let (pscope, project_id) = seed_project(&state, "cross-sub@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    let b_fb = state
        .feedback
        .submit_authenticated(&pscope, "user-B", Some("b@x.com"), None, None, None, "B's row", None, FeedbackKind::Bug)
        .await
        .unwrap();

    let app = build_app(state.clone(), data_state);
    let jwt_a = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(request(
            "DELETE",
            &format!("/api/v1/projects/{project_id}/me/feedback/{}", b_fb.as_str()),
            Some(&jwt_a),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "deleting another sub's feedback must 404, not erase"
    );

    // B's row SURVIVES.
    let survivor = state
        .feedback
        .get_for_end_user(&pscope, "user-B", &b_fb)
        .await
        .unwrap();
    assert_eq!(survivor.body, "B's row");
}

// ----- Invariant 3: anonymous rows unreachable --------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn delete_anonymous_feedback_404_and_row_survives(pool: PgPool) {
    let state = build_test_state(&pool);
    let data_state = build_data_state(&state, test_storage());
    let attachments = SqlxAttachmentRepo::new(pool.clone());
    let (pscope, project_id) = seed_project(&state, "anon-del@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    let anon_fb = state
        .feedback
        .submit_anonymous(&pscope, &[7u8; 32], None, "anon gripe", None, FeedbackKind::Other)
        .await
        .unwrap();

    let app = build_app(state, data_state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(request(
            "DELETE",
            &format!("/api/v1/projects/{project_id}/me/feedback/{}", anon_fb.as_str()),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // The anonymous row survives (still resolvable within scope).
    assert!(attachments
        .resolve_feedback_uuid(&pscope, anon_fb.as_str())
        .await
        .is_ok());
}

// ----- Invariant 4: unknown id → 404 ------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn delete_unknown_id_404(pool: PgPool) {
    let state = build_test_state(&pool);
    let data_state = build_data_state(&state, test_storage());
    let (pscope, project_id) = seed_project(&state, "unknown-del@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    let app = build_app(state.clone(), data_state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(request(
            "DELETE",
            &format!("/api/v1/projects/{project_id}/me/feedback/FB-NOPE99"),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Belt & braces: the repo-level delete is a no-op for unknown ids too.
    let deleted = state
        .feedback
        .delete_for_end_user(&pscope, "user-A", &FeedbackId::from("FB-NOPE99".to_string()))
        .await
        .unwrap();
    assert!(!deleted);
}
// NOTE: the behavioral proof that erasure SCRUBS the P5-derived text (scrutiny
// P0-1) lives in the repository crate — `crates/feedbackmonk-repository/tests/
// erasure_derived_scrub.rs` — where raw-SQL seeding of the analysis corpus is
// permitted (the multi-tenant-isolation-check oracle forbids raw SQL outside the
// repository crate; the derived tables have no scope-layer create-with-text API).
