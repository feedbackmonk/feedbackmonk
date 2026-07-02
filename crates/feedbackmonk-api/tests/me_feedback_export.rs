#![allow(clippy::doc_markdown)] // test-file doc comments name JWT claim fields verbatim

//! Phase A A5 — `GET /api/v1/projects/{project_id}/me/feedback/export`
//! (GDPR data portability, companion to the A1 erasure route). Mirrors the
//! `me_feedback_isolation.rs` harness.
//!
//! Invariants asserted here (each a named test):
//!   1. `export_returns_full_shape_for_own_rows_only` — the export document
//!      carries every owned row with its public replies + attachment metadata;
//!      another sub's rows, anonymous rows and INTERNAL replies NEVER appear.
//!   2. `export_requires_auth` — missing bearer → 401; the static `/export`
//!      segment does not collide with the `{fb}` param routes.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
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
use feedbackmonk_api::storage::{LocalFsStorage, ObjectStore};
use feedbackmonk_api::{me_feedback_data_router, MeFeedbackDataState};
use feedbackmonk_core::FeedbackKind;
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
    async fn send_password_reset_email(&self, _to: &str, _link: &str) -> anyhow::Result<()> {
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

fn test_storage() -> Arc<dyn ObjectStore> {
    let tmp = std::env::temp_dir().join(format!("fbm-me-exp-{}", Uuid::new_v4()));
    Arc::new(LocalFsStorage::new(tmp, "http://test.local/attachments"))
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

fn get_request(path: &str, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ----- Invariant 1: full shape, own rows only ----------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn export_returns_full_shape_for_own_rows_only(pool: PgPool) {
    let state = build_test_state(&pool);
    let storage = test_storage();
    let data_state = build_data_state(&state, Arc::clone(&storage));
    let attachments = SqlxAttachmentRepo::new(pool.clone());
    let (pscope, project_id) = seed_project(&state, "export@example.com").await;
    let signing = seed_signing_key(&state, &pscope).await;

    // Caller's row 1: public + internal replies.
    let fb1 = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "row with replies", None, FeedbackKind::Bug)
        .await
        .unwrap();
    let admin = Uuid::new_v4();
    state
        .feedback_replies
        .create(&pscope, &fb1, "PUBLIC: fixed in next release", ReplyVisibility::Public, admin)
        .await
        .unwrap();
    state
        .feedback_replies
        .create(&pscope, &fb1, "INTERNAL: root cause is the cache", ReplyVisibility::Internal, admin)
        .await
        .unwrap();

    // Caller's row 2: one attachment.
    let fb2 = state
        .feedback
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, "row with attachment", None, FeedbackKind::Feature)
        .await
        .unwrap();
    let fb2_uuid = attachments.resolve_feedback_uuid(&pscope, fb2.as_str()).await.unwrap();
    let key = format!("attachments/test/{fb2_uuid}/shot.png");
    let url = storage.put(&key, "image/png", b"PNGBYTES").await.unwrap();
    attachments
        .insert(
            &pscope,
            fb2_uuid,
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

    // Noise that must NOT leak: another sub's row (with a public reply) and an
    // anonymous row.
    let b_fb = state
        .feedback
        .submit_authenticated(&pscope, "user-B", Some("b@x.com"), None, None, None, "B's secret export bait", None, FeedbackKind::Bug)
        .await
        .unwrap();
    state
        .feedback_replies
        .create(&pscope, &b_fb, "reply on B's row", ReplyVisibility::Public, admin)
        .await
        .unwrap();
    state
        .feedback
        .submit_anonymous(&pscope, &[9u8; 32], None, "anonymous export bait", None, FeedbackKind::Other)
        .await
        .unwrap();

    let app = me_feedback_data_router(data_state);
    let jwt = mint_jwt(&signing, project_id, "user-A");
    let resp = app
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_id}/me/feedback/export"),
            Some(&jwt),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    // Envelope.
    assert_eq!(body["project_id"], project_id.to_string());
    assert!(body["exported_at"].is_string());
    let feedback = body["feedback"].as_array().expect("feedback array");
    assert_eq!(feedback.len(), 2, "exactly the caller's two rows");

    // Row 1: full shape + PUBLIC reply only.
    let item1 = feedback
        .iter()
        .find(|f| f["feedback_id"] == fb1.as_str())
        .expect("fb1 in export");
    assert_eq!(item1["body"], "row with replies");
    assert!(item1["kind"].is_string());
    assert!(item1["status"].is_string());
    assert!(item1.get("sentiment").is_some());
    assert!(item1.get("severity").is_some());
    assert!(item1["submitted_at"].is_string());
    assert!(item1["updated_at"].is_string());
    assert_eq!(item1["reply_count"], 1, "only the PUBLIC reply is counted");
    let replies = item1["replies"].as_array().unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["body"], "PUBLIC: fixed in next release");
    assert!(replies[0]["reply_id"].is_string());
    assert!(replies[0]["created_at"].is_string());
    assert!(item1["attachments"].as_array().unwrap().is_empty());

    // Row 2: attachment metadata.
    let item2 = feedback
        .iter()
        .find(|f| f["feedback_id"] == fb2.as_str())
        .expect("fb2 in export");
    let atts = item2["attachments"].as_array().unwrap();
    assert_eq!(atts.len(), 1);
    assert!(atts[0]["attachment_id"].is_string());
    assert_eq!(atts[0]["kind"], "image");
    assert_eq!(atts[0]["url"], url);
    assert_eq!(atts[0]["content_type"], "image/png");
    assert_eq!(atts[0]["byte_size"], 8);
    assert!(atts[0]["created_at"].is_string());
    // Internal object-store addressing never leaves the server.
    assert!(atts[0].get("storage_key").is_none(), "storage_key leaked into export");

    // Hard isolation: nothing of B's / anonymous / internal appears ANYWHERE.
    let serialized = body.to_string();
    assert!(!serialized.contains("B's secret export bait"), "B's body leaked");
    assert!(!serialized.contains("user-B"), "B's sub leaked");
    assert!(!serialized.contains("reply on B's row"), "B's reply leaked");
    assert!(!serialized.contains("anonymous export bait"), "anon row leaked");
    assert!(
        !serialized.contains("INTERNAL: root cause is the cache"),
        "internal reply leaked into export"
    );
}

// ----- Invariant 2: auth required ----------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn export_requires_auth(pool: PgPool) {
    let state = build_test_state(&pool);
    let data_state = build_data_state(&state, test_storage());
    let (_pscope, project_id) = seed_project(&state, "export-auth@example.com").await;

    let app = me_feedback_data_router(data_state);
    let resp = app
        .oneshot(get_request(
            &format!("/api/v1/projects/{project_id}/me/feedback/export"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
