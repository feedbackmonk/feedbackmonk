#![allow(clippy::doc_markdown)] // test docs name types, routes and columns verbatim
#![allow(dead_code)] // the shared harness block is the same in both board_admin* test files
#![allow(clippy::similar_names, clippy::too_many_lines, clippy::many_single_char_names)] // paired fixtures and end-to-end scenarios

//! Owner surfaces for end-user prioritization -- Contract C44
//! (`docs/planning/handoffs/prioritization-contracts.md`):
//!
//! - the board-access setting on `GET|PATCH /admin/projects/{id}/board-settings`
//!   is the owner's alone (FR-FBR-43);
//! - `GET|PUT|DELETE /admin/feedback/{code}/board-note` sets, edits and clears
//!   the one public note per board item (FR-FBR-58);
//! - `GET /admin/projects/{id}/priority` returns the fitted order, per-item
//!   ranker count and uncertainty, the unranked set and when it was computed,
//!   with votes and not-needed beside the rank and no merged score
//!   (FR-FBR-55).
//!
//! The privacy half of C44 (FR-FBR-56) is `board_admin_privacy.rs`.

// ----- Shared harness (each tests/*.rs is its own crate; this block is the ----
// ----- same in both board_admin* test files) ----------------------------------

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
use rand_core::OsRng;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{board_router, moderation_router};
use feedbackmonk_core::{BoardAccess, FeedbackId, FeedbackKind, ModerationStatus};
use feedbackmonk_repository::{
    ProjectScope, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxSigningKeyRepo,
    SqlxTenantRepo, SqlxTierQuotaRepo,
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
        board_votes: Arc::new(feedbackmonk_repository::SqlxBoardVoteRepo::new(pool.clone())),
        voting_cache: feedbackmonk_api::VotingCache::new(),
        started_at: chrono::Utc::now(),
        health: SqlxHealthCheck::new(pool.clone()),
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

/// The public board and the owner moderation surface, as `build_app` merges
/// them (minus host binding, CORS and rate limiting, which other tests own).
fn app(state: &AppState) -> Router {
    board_router(state.clone()).merge(moderation_router(state.clone()))
}

/// An owner: a verified tenant with one project whose board is ENABLED with
/// `access`, plus that tenant's admin session cookie (`name=value`).
struct Owner {
    scope: ProjectScope,
    project_id: Uuid,
    cookie: String,
}

async fn seed_owner(state: &AppState, email: &str, access: BoardAccess) -> Owner {
    let tenant = state.tenants.create(email, "hash").await.unwrap();
    let tscope = state.tenants.scope_for(tenant.id).await.unwrap();
    state.tenants.mark_verified(&tscope).await.unwrap();
    let p = state
        .projects
        .create(&tscope, "Proj", &format!("p-{}", &tenant.id.to_string()[..8]))
        .await
        .unwrap();
    let scope = state.projects.open(&tscope, p.id).await.unwrap();
    state
        .projects
        .set_board_settings(&scope, Some(true), None)
        .await
        .unwrap();
    state.projects.set_board_access(&scope, access).await.unwrap();
    let cookie = feedbackmonk_api::auth::issue_session_cookie(
        tenant.id,
        i64::from(tenant.session_epoch),
        state.session_secret.as_ref(),
    )
    .to_string()
    .split(';')
    .next()
    .unwrap()
    .to_string();
    Owner {
        scope,
        project_id: p.id,
        cookie,
    }
}

/// Submit an anonymous item of `kind` and moderate it to `to`. Returns its
/// short code.
async fn submit_moderated(
    state: &AppState,
    pool: &PgPool,
    scope: &ProjectScope,
    body: &str,
    kind: FeedbackKind,
    to: ModerationStatus,
) -> FeedbackId {
    let fb = state
        .feedback
        .submit_anonymous(scope, &[7u8; 32], None, body, None, kind)
        .await
        .unwrap();
    if to != ModerationStatus::Pending {
        let mut tx = pool.begin().await.unwrap();
        state
            .feedback
            .moderate_in_executor(scope, &mut tx, &fb, to, None, scope.tenant_id())
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    fb
}

async fn submit_approved(
    state: &AppState,
    pool: &PgPool,
    scope: &ProjectScope,
    body: &str,
    kind: FeedbackKind,
) -> FeedbackId {
    submit_moderated(state, pool, scope, body, kind, ModerationStatus::Approved).await
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

/// How a request identifies its caller.
#[derive(Clone, Copy)]
enum Who<'a> {
    Nobody,
    /// An admin session cookie (`name=value`).
    Admin(&'a str),
    /// An end-user bearer JWT.
    Bearer(&'a str),
}

async fn call(app: &Router, method: &str, uri: &str, who: Who<'_>, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    match who {
        Who::Nobody => {}
        Who::Admin(cookie) => builder = builder.header("cookie", cookie),
        Who::Bearer(token) => builder = builder.header("authorization", format!("Bearer {token}")),
    }
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let mut req = builder.body(body).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo::<SocketAddr>("127.0.0.1:54321".parse().unwrap()));
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, json)
}

fn settings_uri(project_id: Uuid) -> String {
    format!("/api/v1/admin/projects/{project_id}/board-settings")
}

fn priority_uri(project_id: Uuid) -> String {
    format!("/api/v1/admin/projects/{project_id}/priority")
}

fn note_uri(fb: &FeedbackId) -> String {
    format!("/api/v1/admin/feedback/{}/board-note", fb.as_str())
}

fn board_item_uri(project_id: Uuid, fb: &FeedbackId) -> String {
    format!("/api/v1/projects/{project_id}/board/items/{}", fb.as_str())
}

fn keys_of(v: &Value) -> BTreeSet<String> {
    v.as_object().expect("object").keys().cloned().collect()
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| (*k).to_string()).collect()
}

// ===========================================================================
// FR-FBR-43: the board-access setting is the owner's alone.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn board_access_defaults_public_and_round_trips_for_the_owner(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "access-owner@example.com", BoardAccess::Public).await;

    let (status, body) = call(&app, "GET", &settings_uri(owner.project_id), Who::Admin(&owner.cookie), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "public_board_enabled": true, "board_requires_moderation": true, "board_access": "public" })
    );

    let (status, body) = call(
        &app,
        "PATCH",
        &settings_uri(owner.project_id),
        Who::Admin(&owner.cookie),
        Some(json!({ "board_access": "signed_in" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["board_access"], "signed_in");
    assert_eq!(body["public_board_enabled"], true, "the other legs are untouched");
    assert_eq!(body["board_requires_moderation"], true);

    // Persisted, and visible to the repository the public board reads.
    let stored = state.projects.get_board_settings(&owner.scope).await.unwrap();
    assert_eq!(stored.board_access, BoardAccess::SignedIn);
    let (_, body) = call(&app, "GET", &settings_uri(owner.project_id), Who::Admin(&owner.cookie), None).await;
    assert_eq!(body["board_access"], "signed_in");

    // A patch of another leg keeps the access setting.
    let (status, body) = call(
        &app,
        "PATCH",
        &settings_uri(owner.project_id),
        Who::Admin(&owner.cookie),
        Some(json!({ "board_requires_moderation": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["board_access"], "signed_in");
    assert_eq!(body["board_requires_moderation"], false);

    // And back.
    let (status, body) = call(
        &app,
        "PATCH",
        &settings_uri(owner.project_id),
        Who::Admin(&owner.cookie),
        Some(json!({ "board_access": "public" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["board_access"], "public");
}

#[sqlx::test(migrations = "../../migrations")]
async fn board_access_refuses_an_unknown_value_with_400(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "access-bad@example.com", BoardAccess::Public).await;

    for bad in [json!("everyone"), json!("SIGNED_IN"), json!(""), json!(1)] {
        let (status, body) = call(
            &app,
            "PATCH",
            &settings_uri(owner.project_id),
            Who::Admin(&owner.cookie),
            Some(json!({ "board_access": bad })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "board_access = {bad}: {body}");
    }
    let stored = state.projects.get_board_settings(&owner.scope).await.unwrap();
    assert_eq!(stored.board_access, BoardAccess::Public, "a refused patch changes nothing");
}

#[sqlx::test(migrations = "../../migrations")]
async fn board_access_cannot_be_changed_by_anyone_but_the_owner(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "access-only-owner@example.com", BoardAccess::Public).await;
    let other = seed_owner(&state, "access-other-tenant@example.com", BoardAccess::Public).await;
    let signing = seed_signing_key(&state, &owner.scope).await;
    let end_user = mint_jwt(&signing, owner.project_id, "end-user-1");
    let patch = json!({ "board_access": "signed_in" });

    // No session at all.
    let (status, _) = call(&app, "PATCH", &settings_uri(owner.project_id), Who::Nobody, Some(patch.clone())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, "GET", &settings_uri(owner.project_id), Who::Nobody, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A valid end-user identity for this very project is not an owner.
    let (status, _) = call(&app, "PATCH", &settings_uri(owner.project_id), Who::Bearer(&end_user), Some(patch.clone())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Another tenant's owner cannot reach this project.
    let (status, _) = call(&app, "PATCH", &settings_uri(owner.project_id), Who::Admin(&other.cookie), Some(patch.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a foreign project is refused as it is on every admin project route");

    // The public board has no route that changes it.
    for method in ["PATCH", "PUT", "POST"] {
        let (status, _) = call(
            &app,
            method,
            &format!("/api/v1/projects/{}/board/settings", owner.project_id),
            Who::Bearer(&end_user),
            Some(patch.clone()),
        )
        .await;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
            "{method} board/settings answered {status}"
        );
    }

    let stored = state.projects.get_board_settings(&owner.scope).await.unwrap();
    assert_eq!(stored.board_access, BoardAccess::Public, "nobody but the owner moved it");
}

// ===========================================================================
// FR-FBR-58: one board note per item -- set, edit, clear.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn board_note_set_edit_and_clear(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "note-owner@example.com", BoardAccess::Public).await;
    let fb = submit_approved(&state, &pool, &owner.scope, "Dark mode please", FeedbackKind::Feature).await;
    let me = Who::Admin(&owner.cookie);

    // None yet.
    let (status, body) = call(&app, "GET", &note_uri(&fb), me, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "short_code": fb.as_str(), "note": null }));

    // Set (trimmed).
    let (status, body) = call(&app, "PUT", &note_uri(&fb), me, Some(json!({ "body": "  We're starting this in November.  " }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["short_code"], fb.as_str());
    assert_eq!(body["note"]["body"], "We're starting this in November.");
    assert_eq!(keys_of(&body["note"]), set(&["body", "updated_at"]));
    let first_at = body["note"]["updated_at"].as_str().unwrap().to_string();

    let (_, body) = call(&app, "GET", &note_uri(&fb), me, None).await;
    assert_eq!(body["note"]["body"], "We're starting this in November.");

    // The public detail view shows it (FR-FBR-57/58).
    let (status, detail) = call(&app, "GET", &board_item_uri(owner.project_id, &fb), Who::Nobody, None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["board_note"]["body"], "We're starting this in November.");

    // Edit replaces it: still one note.
    let (status, body) = call(&app, "PUT", &note_uri(&fb), me, Some(json!({ "body": "Shipped to beta." }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["note"]["body"], "Shipped to beta.");
    let edited_at = body["note"]["updated_at"].as_str().unwrap().to_string();
    let parse = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap();
    assert!(parse(&edited_at) >= parse(&first_at), "edit moves updated_at forward");
    let (_, detail) = call(&app, "GET", &board_item_uri(owner.project_id, &fb), Who::Nobody, None).await;
    assert_eq!(detail["board_note"]["body"], "Shipped to beta.");

    // Clear.
    let (status, body) = call(&app, "DELETE", &note_uri(&fb), me, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "short_code": fb.as_str(), "note": null }));
    let (_, body) = call(&app, "GET", &note_uri(&fb), me, None).await;
    assert_eq!(body["note"], Value::Null);
    let (_, detail) = call(&app, "GET", &board_item_uri(owner.project_id, &fb), Who::Nobody, None).await;
    assert_eq!(detail["board_note"], Value::Null);

    // Clearing an absent note is still a clean answer.
    let (status, body) = call(&app, "DELETE", &note_uri(&fb), me, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["note"], Value::Null);
}

#[sqlx::test(migrations = "../../migrations")]
async fn board_note_body_must_be_1_to_4000_chars_after_trim(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "note-bounds@example.com", BoardAccess::Public).await;
    let fb = submit_approved(&state, &pool, &owner.scope, "Export to CSV", FeedbackKind::Feature).await;
    let me = Who::Admin(&owner.cookie);

    for bad in [String::new(), "   \n\t ".to_string(), "x".repeat(4001)] {
        let (status, body) = call(&app, "PUT", &note_uri(&fb), me, Some(json!({ "body": bad }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "len {}: {body}", bad.len());
    }
    let (_, body) = call(&app, "GET", &note_uri(&fb), me, None).await;
    assert_eq!(body["note"], Value::Null, "a refused note stores nothing");

    // Exactly 4000 (multi-byte chars count as one each) is accepted.
    let max = "é".repeat(4000);
    let (status, body) = call(&app, "PUT", &note_uri(&fb), me, Some(json!({ "body": max }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn board_note_exists_only_on_board_items_and_only_for_the_owner(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "note-scope@example.com", BoardAccess::Public).await;
    let other = seed_owner(&state, "note-scope-other@example.com", BoardAccess::Public).await;
    let pending = submit_moderated(&state, &pool, &owner.scope, "Pending one", FeedbackKind::Feature, ModerationStatus::Pending).await;
    let rejected = submit_moderated(&state, &pool, &owner.scope, "Rejected one", FeedbackKind::Feature, ModerationStatus::Rejected).await;
    let approved = submit_approved(&state, &pool, &owner.scope, "Approved one", FeedbackKind::Feature).await;
    let me = Who::Admin(&owner.cookie);
    let note = json!({ "body": "hello" });

    for fb in [&pending, &rejected] {
        for (method, body) in [("GET", None), ("PUT", Some(note.clone())), ("DELETE", None)] {
            let (status, _) = call(&app, method, &note_uri(fb), me, body).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} note on a non-board item");
        }
    }
    let (status, _) = call(&app, "GET", &note_uri(&FeedbackId::from("FB-NOPE00".to_string())), me, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // No session; another tenant's owner.
    for (method, body) in [("GET", None), ("PUT", Some(note.clone())), ("DELETE", None)] {
        let (status, _) = call(&app, method, &note_uri(&approved), Who::Nobody, body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} without a session");
        let (status, _) = call(&app, method, &note_uri(&approved), Who::Admin(&other.cookie), body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} from another tenant");
    }
    let (_, body) = call(&app, "GET", &note_uri(&approved), me, None).await;
    assert_eq!(body["note"], Value::Null, "no refused write landed");
}

// ===========================================================================
// FR-FBR-55: the priority view.
// ===========================================================================

/// The exact key sets of a ranked and an unranked item. Nothing combines
/// votes, not-needed and priority (no `score`, no `strength`).
fn ranked_keys() -> BTreeSet<String> {
    set(&[
        "short_code",
        "body_excerpt",
        "status",
        "rank",
        "rankers",
        "uncertainty",
        "vote_count",
        "not_needed_count",
    ])
}

fn unranked_keys() -> BTreeSet<String> {
    set(&["short_code", "body_excerpt", "status", "rankers", "vote_count", "not_needed_count"])
}

#[sqlx::test(migrations = "../../migrations")]
async fn priority_view_returns_order_rankers_uncertainty_unranked_and_computed_at(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "prio-owner@example.com", BoardAccess::Public).await;
    let signing = seed_signing_key(&state, &owner.scope).await;
    let scope = &owner.scope;

    let a = submit_approved(&state, &pool, scope, "Alpha feature", FeedbackKind::Feature).await;
    let b = submit_approved(&state, &pool, scope, "Bravo feature", FeedbackKind::Feature).await;
    let c = submit_approved(&state, &pool, scope, "Charlie feature", FeedbackKind::Feature).await;
    let d = submit_approved(&state, &pool, scope, "Delta feature", FeedbackKind::Feature).await;
    let e = submit_approved(&state, &pool, scope, "Echo feature", FeedbackKind::Feature).await;
    // Outside the ranking pool: a bug, and a pending feature.
    let bug = submit_approved(&state, &pool, scope, "A bug", FeedbackKind::Bug).await;
    let pending = submit_moderated(&state, &pool, scope, "Pending feature", FeedbackKind::Feature, ModerationStatus::Pending).await;

    let signals = state.board_votes.signals();
    signals.save_priority_list(scope, "u1", &[a.clone(), b.clone(), c.clone()]).await.unwrap();
    signals.save_priority_list(scope, "u2", &[a.clone(), b.clone()]).await.unwrap();
    signals.save_priority_list(scope, "u3", &[a.clone(), c.clone()]).await.unwrap();
    // A one-item list states no pair: e is placed, but unranked.
    signals.save_priority_list(scope, "u4", &[e.clone()]).await.unwrap();
    signals.set_not_needed(scope, &d, "u5").await.unwrap();
    signals.set_not_needed(scope, &c, "u6").await.unwrap();

    // Two signed-in upvotes on b, one on d.
    for (sub, fb) in [("v1", &b), ("v2", &b), ("v3", &d)] {
        let jwt = mint_jwt(&signing, owner.project_id, sub);
        let (status, body) = call(
            &app,
            "POST",
            &format!("{}/vote", board_item_uri(owner.project_id, fb)),
            Who::Bearer(&jwt),
            None,
        )
        .await;
        assert!(status.is_success(), "vote by {sub}: {status} {body}");
    }

    let before = chrono::Utc::now();
    let (status, view) = call(&app, "GET", &priority_uri(owner.project_id), Who::Admin(&owner.cookie), None).await;
    let after = chrono::Utc::now();
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        keys_of(&view),
        set(&["computed_at", "contributing_rankers", "ranked", "unranked"])
    );

    // computed-at: when this answer was fitted.
    let computed_at = chrono::DateTime::parse_from_rfc3339(view["computed_at"].as_str().unwrap()).unwrap();
    assert!(computed_at >= before - chrono::Duration::seconds(1) && computed_at <= after + chrono::Duration::seconds(1));

    // u1, u2, u3 state pairs; u4's one-item list does not count.
    assert_eq!(view["contributing_rankers"], 3);

    // Order: a is above everything in every list.
    let ranked = view["ranked"].as_array().unwrap();
    let ranked_codes: Vec<&str> = ranked.iter().map(|i| i["short_code"].as_str().unwrap()).collect();
    assert_eq!(ranked_codes.len(), 3, "{view}");
    assert_eq!(ranked_codes[0], a.as_str());
    let ranked_set: BTreeSet<&str> = ranked_codes.iter().copied().collect();
    assert_eq!(ranked_set, [a.as_str(), b.as_str(), c.as_str()].into_iter().collect());

    for (i, item) in ranked.iter().enumerate() {
        assert_eq!(keys_of(item), ranked_keys(), "{item}");
        assert_eq!(item["rank"], i + 1, "rank is the 1-based position");
        let u = item["uncertainty"].as_f64().unwrap();
        assert!(u.is_finite() && u > 0.0, "uncertainty is a positive standard error: {item}");
    }
    let by_code = |code: &str| ranked.iter().find(|i| i["short_code"] == code).unwrap().clone();
    assert_eq!(by_code(a.as_str())["rankers"], 3);
    assert_eq!(by_code(b.as_str())["rankers"], 2);
    assert_eq!(by_code(c.as_str())["rankers"], 2);
    // Votes and not-needed sit beside the rank, as raw counts.
    assert_eq!(by_code(b.as_str())["vote_count"], 2);
    assert_eq!(by_code(a.as_str())["vote_count"], 0);
    assert_eq!(by_code(c.as_str())["not_needed_count"], 1);
    assert_eq!(by_code(a.as_str())["status"], "submitted");
    assert_eq!(by_code(a.as_str())["body_excerpt"], "Alpha feature");

    // Unranked: d (no list) and e (one-item list), with no rank, no uncertainty.
    let unranked = view["unranked"].as_array().unwrap();
    let unranked_codes: BTreeSet<&str> = unranked.iter().map(|i| i["short_code"].as_str().unwrap()).collect();
    assert_eq!(unranked_codes, [d.as_str(), e.as_str()].into_iter().collect());
    for item in unranked {
        assert_eq!(keys_of(item), unranked_keys(), "{item}");
    }
    let un = |code: &str| unranked.iter().find(|i| i["short_code"] == code).unwrap().clone();
    assert_eq!(un(d.as_str())["rankers"], 0);
    assert_eq!(un(d.as_str())["vote_count"], 1);
    assert_eq!(un(d.as_str())["not_needed_count"], 1);
    assert_eq!(un(e.as_str())["rankers"], 1);

    // Outside the pool: absent from both.
    let all = view.to_string();
    assert!(!all.contains(bug.as_str()), "a bug is not in the ranking pool");
    assert!(!all.contains(pending.as_str()), "a pending item is not on the board");
}

/// The uncertainty shown is the engine's own Wald standard error for the same
/// input -- not a re-scaled "confidence". That error is NOT monotone in the
/// evidence: a unanimous, lopsided item can carry a larger one than an item
/// with a single comparison. The test pins the shown value to the engine's,
/// so the console's label must read as uncertainty of the estimate.
#[sqlx::test(migrations = "../../migrations")]
async fn priority_view_uncertainty_is_the_engines_standard_error(pool: PgPool) {
    use feedbackmonk_core::{fit_priority, PriorityFitConfig};

    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "prio-se@example.com", BoardAccess::Public).await;
    let scope = &owner.scope;
    let a = submit_approved(&state, &pool, scope, "One", FeedbackKind::Feature).await;
    let b = submit_approved(&state, &pool, scope, "Two", FeedbackKind::Feature).await;
    let c = submit_approved(&state, &pool, scope, "Three", FeedbackKind::Feature).await;
    let d = submit_approved(&state, &pool, scope, "Four", FeedbackKind::Feature).await;
    let signals = state.board_votes.signals();
    // a beats b unanimously, five times; c over d once.
    let mut lists: Vec<Vec<String>> = Vec::new();
    for i in 0..5 {
        signals.save_priority_list(scope, &format!("s{i}"), &[a.clone(), b.clone()]).await.unwrap();
        lists.push(vec![a.as_str().to_string(), b.as_str().to_string()]);
    }
    signals.save_priority_list(scope, "t0", &[c.clone(), d.clone()]).await.unwrap();
    lists.push(vec![c.as_str().to_string(), d.as_str().to_string()]);

    let pool_keys: Vec<String> = [&a, &b, &c, &d].iter().map(|f| f.as_str().to_string()).collect();
    let fit = fit_priority(&pool_keys, &lists, &PriorityFitConfig::default());

    let (status, view) = call(&app, "GET", &priority_uri(owner.project_id), Who::Admin(&owner.cookie), None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let ranked = view["ranked"].as_array().unwrap();
    assert_eq!(ranked.len(), fit.ranked.len());
    for (shown, fitted) in ranked.iter().zip(&fit.ranked) {
        assert_eq!(shown["short_code"], fitted.key.as_str(), "the shown order is the fit's order");
        let u = shown["uncertainty"].as_f64().unwrap();
        assert!(
            (u - fitted.std_error).abs() < 1e-9,
            "uncertainty {u} is the engine's standard error {}",
            fitted.std_error
        );
        assert_eq!(shown["rankers"], fitted.rankers);
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn priority_view_of_an_empty_pool_is_empty_not_an_error(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "prio-empty@example.com", BoardAccess::Public).await;
    let (status, view) = call(&app, "GET", &priority_uri(owner.project_id), Who::Admin(&owner.cookie), None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["contributing_rankers"], 0);
    assert_eq!(view["ranked"], json!([]));
    assert_eq!(view["unranked"], json!([]));
    assert!(view["computed_at"].is_string());
}

#[sqlx::test(migrations = "../../migrations")]
async fn priority_view_is_the_owners_alone(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let owner = seed_owner(&state, "prio-auth@example.com", BoardAccess::Public).await;
    let other = seed_owner(&state, "prio-auth-other@example.com", BoardAccess::Public).await;
    let signing = seed_signing_key(&state, &owner.scope).await;
    let end_user = mint_jwt(&signing, owner.project_id, "end-user-1");

    let (status, _) = call(&app, "GET", &priority_uri(owner.project_id), Who::Nobody, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, "GET", &priority_uri(owner.project_id), Who::Bearer(&end_user), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, "GET", &priority_uri(owner.project_id), Who::Admin(&other.cookie), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a foreign project is refused as it is on every admin project route");
}
