#![allow(clippy::doc_markdown)] // test docs name types, routes and columns verbatim
#![allow(dead_code)] // the shared harness block is the same in both board_admin* test files
#![allow(clippy::similar_names, clippy::too_many_lines, clippy::many_single_char_names)] // paired fixtures and end-to-end scenarios

//! Owner-surface privacy for end-user prioritization -- Contract C44,
//! FR-FBR-56: the owner sees aggregates only. No admin response names a
//! user, shows any one user's priority list, or says who marked an item not
//! needed. Two probes:
//!
//! 1. every C44 admin response (priority view, board settings, board note)
//!    and the moderation queue carries none of the end users' identifiers and
//!    no key that would hold one;
//! 2. the priority view is a function of the multiset of lists alone: swap
//!    which user holds which list, or who marked what, and the answer is the
//!    same (apart from `computed_at`).

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
// FR-FBR-56: no admin response names a user, a user's list, or who marked
// an item not needed.
// ===========================================================================

/// Keys that would carry a person or a per-person list, anywhere in a body.
const IDENTITY_KEYS: &[&str] = &[
    "sub",
    "user",
    "users",
    "user_sub",
    "user_id",
    "voter",
    "voter_id",
    "voters",
    "list",
    "lists",
    "rankers_list",
    "marked_by",
    "not_needed_by",
    "email",
];

fn assert_no_identity_keys(v: &Value, path: &str) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                assert!(
                    !IDENTITY_KEYS.contains(&k.as_str()),
                    "{path}.{k}: an admin response carries a per-person key"
                );
                assert_no_identity_keys(child, &format!("{path}.{k}"));
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                assert_no_identity_keys(child, &format!("{path}[{i}]"));
            }
        }
        _ => {}
    }
}

/// The end users' identifiers: distinctive, so a substring search of a body
/// cannot miss them and cannot match anything else.
const RANKERS: [&str; 3] = [
    "sub-ranker-alice-7f3a9c",
    "sub-ranker-bob-2be81d",
    "sub-ranker-carol-c04e55",
];
const MARKERS: [&str; 2] = ["sub-marker-dave-91d0aa", "sub-marker-erin-5e7f13"];
const VOTER: &str = "sub-voter-frank-a1b2c3";

struct Seeded {
    owner: Owner,
    items: Vec<FeedbackId>,
}

/// Five pool features, three rankers with lists, two not-needed markers and
/// one signed-in voter. `swap` permutes who holds which list and who marked
/// which item, leaving the multiset of lists and the per-item counts alone.
async fn seed_signals(state: &AppState, pool: &PgPool, email: &str, swap: bool) -> Seeded {
    let owner = seed_owner(state, email, BoardAccess::Public).await;
    let signing = seed_signing_key(state, &owner.scope).await;
    let scope = &owner.scope;
    let mut items = Vec::new();
    for body in ["Feature one", "Feature two", "Feature three", "Feature four", "Feature five"] {
        items.push(submit_approved(state, pool, scope, body, FeedbackKind::Feature).await);
    }
    let lists: [Vec<FeedbackId>; 3] = [
        vec![items[0].clone(), items[1].clone(), items[2].clone()],
        vec![items[1].clone(), items[0].clone()],
        vec![items[2].clone(), items[0].clone()],
    ];
    let order: [usize; 3] = if swap { [2, 0, 1] } else { [0, 1, 2] };
    let signals = state.board_votes.signals();
    for (who, list) in order.iter().zip(&lists) {
        signals.save_priority_list(scope, RANKERS[*who], list).await.unwrap();
    }
    let marks = if swap { [MARKERS[1], MARKERS[0]] } else { MARKERS };
    signals.set_not_needed(scope, &items[3], marks[0]).await.unwrap();
    signals.set_not_needed(scope, &items[4], marks[1]).await.unwrap();

    let app = app(state);
    let jwt = mint_jwt(&signing, owner.project_id, VOTER);
    let (status, body) = call(
        &app,
        "POST",
        &format!("{}/vote", board_item_uri(owner.project_id, &items[1])),
        Who::Bearer(&jwt),
        None,
    )
    .await;
    assert!(status.is_success(), "vote: {status} {body}");
    Seeded { owner, items }
}

#[sqlx::test(migrations = "../../migrations")]
async fn no_admin_response_names_a_user_or_a_users_list(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let seeded = seed_signals(&state, &pool, "privacy-owner@example.com", false).await;
    let owner = &seeded.owner;
    let me = Who::Admin(&owner.cookie);
    let item = &seeded.items[0];

    let mut responses: Vec<(String, StatusCode, Value)> = Vec::new();
    let mut record = |label: &str, (status, body): (StatusCode, Value)| {
        responses.push((label.to_string(), status, body));
    };
    record("priority", call(&app, "GET", &priority_uri(owner.project_id), me, None).await);
    record("settings GET", call(&app, "GET", &settings_uri(owner.project_id), me, None).await);
    record(
        "settings PATCH",
        call(&app, "PATCH", &settings_uri(owner.project_id), me, Some(json!({ "board_access": "signed_in" }))).await,
    );
    record("note PUT", call(&app, "PUT", &note_uri(item), me, Some(json!({ "body": "Planned." }))).await);
    record("note GET", call(&app, "GET", &note_uri(item), me, None).await);
    record("note DELETE", call(&app, "DELETE", &note_uri(item), me, None).await);
    for status in ["pending", "approved"] {
        record(
            &format!("moderation queue {status}"),
            call(&app, "GET", &format!("/api/v1/admin/feedback/moderation-queue?status={status}"), me, None).await,
        );
    }

    let subs: Vec<&str> = RANKERS.iter().chain(MARKERS.iter()).copied().chain([VOTER]).collect();
    for (label, status, body) in &responses {
        assert_eq!(*status, StatusCode::OK, "{label}: {body}");
        assert_no_identity_keys(body, label);
        let text = body.to_string();
        for sub in &subs {
            assert!(!text.contains(sub), "{label} names end user {sub}: {text}");
        }
    }

    // The priority view carries counts, not people: per item only the number
    // of rankers, votes and not-needed marks.
    let view = &responses[0].2;
    let per_item: Vec<&Value> = view["ranked"]
        .as_array()
        .unwrap()
        .iter()
        .chain(view["unranked"].as_array().unwrap())
        .collect();
    assert_eq!(per_item.len(), 5);
    for it in per_item {
        for key in ["rankers", "vote_count", "not_needed_count"] {
            assert!(it[key].is_u64(), "{key} is a bare count: {it}");
        }
    }
    assert!(view["contributing_rankers"].is_u64());
}

#[sqlx::test(migrations = "../../migrations")]
async fn the_priority_view_does_not_depend_on_who_holds_which_list(pool: PgPool) {
    let state = build_test_state(&pool);
    let app = app(&state);
    let plain = seed_signals(&state, &pool, "privacy-plain@example.com", false).await;
    let swapped = seed_signals(&state, &pool, "privacy-swapped@example.com", true).await;

    let view_of = |seeded: &Seeded| {
        let app = app.clone();
        let uri = priority_uri(seeded.owner.project_id);
        let cookie = seeded.owner.cookie.clone();
        let codes: Vec<String> = seeded.items.iter().map(|f| f.as_str().to_string()).collect();
        async move {
            let (status, mut view) = call(&app, "GET", &uri, Who::Admin(&cookie), None).await;
            assert_eq!(status, StatusCode::OK, "{view}");
            view.as_object_mut().unwrap().remove("computed_at");
            // Short codes differ between the two projects; name items by their
            // seeding position instead.
            let mut text = view.to_string();
            for (i, code) in codes.iter().enumerate() {
                text = text.replace(code.as_str(), &format!("ITEM-{i}"));
            }
            let mut view: Value = serde_json::from_str(&text).unwrap();
            // Unranked items are listed in short-code order, which is random
            // per project; compare them as a set.
            view["unranked"]
                .as_array_mut()
                .unwrap()
                .sort_by(|x, y| x["short_code"].as_str().cmp(&y["short_code"].as_str()));
            view
        }
    };
    let a = view_of(&plain).await;
    let b = view_of(&swapped).await;
    assert_eq!(a, b, "the owner's view is a function of the lists, not of who holds them");
}
