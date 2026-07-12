#![allow(clippy::doc_markdown)] // test doc comments name event_type / state strings verbatim

//! Named-runner routing — the C31 §5 / D-P6-3 integration corpus (P6).
//!
//! Routing is COORDINATION, not a trust boundary: an optional `routing_label`
//! targets a work order at one runner identity (the verified token `sub`). The
//! server enforces it in two places, both keyed on the *verified* token sub
//! (never a client-supplied value):
//!   - **Poll** `GET .../runner/work-orders?state=dispatched` — a labeled order
//!     is returned only to its named runner; an unlabeled order to every runner
//!     (first-claim-wins).
//!   - **Claim** — the same predicate; a mismatch is `409 {"error":"routing_mismatch"}`.
//!
//! A runner token still cannot author `approve` (C25), so a mis-targeted claim
//! is a benign 409, never a privilege escalation. These tests prove the server
//! filters/enforces on the sub, that unlabeled stays first-claim-wins, and that
//! a `routing_label` set at approve overrides the create-time value.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, Utc};
use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
use rand_core::OsRng;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::handlers::work_orders::{
    create_work_order, transition_work_order, Actor, RUNNER_TOKEN_SCOPE,
};
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{work_order_admin_router, work_order_runner_router};
use feedbackmonk_core::{ActionType, FeedbackKind, KeyClass, WorkOrderState};
use feedbackmonk_repository::{
    NewRecommendation, ProjectScope, SqlxAnalysisSweepRepo, SqlxClusterRepo,
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxRecommendationRepo,
    SqlxRoadmapItemRepo, SqlxRoadmapVoteRepo, SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo,
    SqlxWorkOrderEventRepo, SqlxWorkOrderRepo, WorkOrderStatePatch,
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
        clusters: Arc::new(SqlxClusterRepo::new(pool.clone())),
        recommendations: Arc::new(SqlxRecommendationRepo::new(pool.clone())),
        analysis_sweeps: Arc::new(SqlxAnalysisSweepRepo::new(pool.clone())),
        work_orders: Arc::new(SqlxWorkOrderRepo::new(pool.clone())),
        work_order_events: Arc::new(SqlxWorkOrderEventRepo::new(pool.clone())),
        runner_tokens: Arc::new(feedbackmonk_repository::SqlxRunnerTokenRepo::new(pool.clone())),
        runner_token_revocations: Arc::new(
            feedbackmonk_repository::SqlxRunnerTokenRevocationRepo::new(pool.clone()),
        ),
        jwt_iat_leeway_seconds: 5,
        roadmap_items: Arc::new(SqlxRoadmapItemRepo::new(pool.clone())),
        roadmap_votes: Arc::new(SqlxRoadmapVoteRepo::new(pool.clone())),
        board_votes: Arc::new(feedbackmonk_repository::SqlxBoardVoteRepo::new(pool.clone())),
        voting_cache: feedbackmonk_api::VotingCache::new(),
        started_at: Utc::now(),
        health: SqlxHealthCheck::new(pool.clone()),
        tier_quotas: Arc::new(SqlxTierQuotaRepo::new(pool.clone())),
    }
}

struct Seeded {
    scope: ProjectScope,
    project_id: Uuid,
    recommendation_id: Uuid,
    cookie: String,
}

/// Seed a verified tenant + project + cluster + recommendation, plus an admin
/// session cookie (so the approve-override test can drive the admin surface).
async fn seed(state: &AppState, email: &str) -> Seeded {
    let tenant = state.tenants.create(email, "hash").await.unwrap();
    let tscope = state.tenants.scope_for(tenant.id).await.unwrap();
    state.tenants.mark_verified(&tscope).await.unwrap();
    let p = state
        .projects
        .create(&tscope, "Proj", &format!("p-{}", &tenant.id.to_string()[..8]))
        .await
        .unwrap();
    let scope = state.projects.open(&tscope, p.id).await.unwrap();
    let cluster = state
        .clusters
        .create(&scope, "Cluster", None, FeedbackKind::Bug, "agent")
        .await
        .unwrap();
    let refs = json!([{ "path": "src/auth.rs", "lines": "10-20" }]);
    let rec = state
        .recommendations
        .create(
            &scope,
            NewRecommendation {
                cluster_id: cluster.id,
                sweep_id: None,
                action_type: ActionType::BugFix,
                title: "Fix the auth check",
                body: "Restore the missing guard",
                rationale: Some("Many users locked out"),
                source_refs: &refs,
                confidence: 0.8,
            },
        )
        .await
        .unwrap();
    let cookie = feedbackmonk_api::auth::issue_session_cookie(
        tenant.id,
        i64::from(tenant.session_epoch),
        state.session_secret.as_ref(),
    );
    let cookie = cookie.to_string().split(';').next().unwrap().to_string();
    Seeded {
        scope,
        project_id: p.id,
        recommendation_id: rec.id,
        cookie,
    }
}

/// Register a runner-class Ed25519 key; return the dalek key for minting.
async fn seed_signing_key(state: &AppState, scope: &ProjectScope) -> DalekSigningKey {
    let signing = DalekSigningKey::generate(&mut OsRng);
    let pk: [u8; 32] = signing.verifying_key().to_bytes();
    state
        .signing_keys
        .register_with_class(scope, &pk, "runner-key", KeyClass::Runner)
        .await
        .unwrap();
    signing
}

/// Mint a runner write-token with the given `sub` (a fresh jti each time).
fn mint_runner_token(signing: &DalekSigningKey, project_id: Uuid, sub: &str) -> String {
    let header = json!({ "alg": "EdDSA", "typ": "JWT" });
    let now = Utc::now().timestamp();
    let payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
        "jti": Uuid::new_v4().to_string(),
        "scope": RUNNER_TOKEN_SCOPE,
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig.to_bytes()))
}

/// Drive a derived order to `dispatched` at rung 1 with an optional routing label.
async fn make_dispatched(state: &AppState, s: &Seeded, routing_label: Option<&str>) -> Uuid {
    let id = create_work_order(state, &s.scope, s.recommendation_id, 1, None, routing_label)
        .await
        .unwrap()
        .id;
    let admin = Uuid::new_v4();
    transition_work_order(
        state,
        &s.scope,
        id,
        "approve",
        &Actor::Admin { id: admin },
        None,
        WorkOrderStatePatch {
            approved_by: Some(admin),
            approved_at: Some(Utc::now()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    transition_work_order(
        state,
        &s.scope,
        id,
        "dispatch",
        &Actor::System,
        None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() },
    )
    .await
    .unwrap();
    id
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&to_bytes(body, 256 * 1024).await.unwrap()).unwrap()
}

/// The work_order_ids in a runner poll response (`{ items: [ClaimedOrder] }`).
fn poll_ids(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["work_order_id"].as_str().unwrap().to_string())
        .collect()
}

// ============================================================================
// 1. Poll filtering — a labeled order is visible only to the matching sub.
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn labeled_order_visible_only_to_matching_runner_sub(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "routing-poll@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;

    let labeled = make_dispatched(&state, &s, Some("runner-A")).await;
    let unlabeled = make_dispatched(&state, &s, None).await;

    let app = work_order_runner_router(state.clone());
    let poll = |token: String| {
        Request::get(format!(
            "/api/v1/projects/{}/runner/work-orders?state=dispatched",
            s.project_id
        ))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
    };

    // runner-A sees BOTH (its labeled order + the unlabeled one).
    let token_a = mint_runner_token(&signing, s.project_id, "runner-A");
    let resp = app.clone().oneshot(poll(token_a)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ids_a = poll_ids(&body_json(resp.into_body()).await);
    assert!(ids_a.contains(&labeled.to_string()), "runner-A must see its labeled order");
    assert!(ids_a.contains(&unlabeled.to_string()), "runner-A must see the unlabeled order");

    // runner-B sees ONLY the unlabeled one.
    let token_b = mint_runner_token(&signing, s.project_id, "runner-B");
    let resp = app.clone().oneshot(poll(token_b)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ids_b = poll_ids(&body_json(resp.into_body()).await);
    assert!(ids_b.contains(&unlabeled.to_string()), "runner-B must see the unlabeled order");
    assert!(
        !ids_b.contains(&labeled.to_string()),
        "runner-B must NOT see runner-A's labeled order"
    );
}

// ============================================================================
// 2. Claim enforcement — a wrong sub is refused 409 routing_mismatch; the
//    matching sub claims; unlabeled = first-claim-wins.
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn claim_by_wrong_sub_is_409_matching_sub_claims(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "routing-claim@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;
    let id = make_dispatched(&state, &s, Some("runner-A")).await;

    let app = work_order_runner_router(state.clone());
    let claim = |token: String| {
        Request::post(format!(
            "/api/v1/projects/{}/work-orders/{}/claim",
            s.project_id, id
        ))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
    };

    // Wrong sub → 409 { "error": "routing_mismatch" }, order NOT claimed.
    let token_b = mint_runner_token(&signing, s.project_id, "runner-B");
    let resp = app.clone().oneshot(claim(token_b)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT, "wrong sub must 409");
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["error"], "routing_mismatch");
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Dispatched, "mismatched claim must not transition");

    // Matching sub → 200, order claimed by runner-A.
    let token_a = mint_runner_token(&signing, s.project_id, "runner-A");
    let resp = app.clone().oneshot(claim(token_a)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "matching sub must claim");
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Claimed);
    assert_eq!(wo.claimed_by_runner.as_deref(), Some("runner-A"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn unlabeled_order_is_first_claim_wins(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "routing-unlabeled@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;
    let id = make_dispatched(&state, &s, None).await;

    let app = work_order_runner_router(state.clone());
    let claim = |token: String| {
        Request::post(format!(
            "/api/v1/projects/{}/work-orders/{}/claim",
            s.project_id, id
        ))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
    };

    // runner-X claims first → 200, sticks.
    let token_x = mint_runner_token(&signing, s.project_id, "runner-X");
    let resp = app.clone().oneshot(claim(token_x)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "first claim on an unlabeled order wins");
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.claimed_by_runner.as_deref(), Some("runner-X"));

    // runner-Y's later claim is refused — NOT on routing (unlabeled), but
    // because the order already left `dispatched` (illegal transition → 409).
    let token_y = mint_runner_token(&signing, s.project_id, "runner-Y");
    let resp = app.clone().oneshot(claim(token_y)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT, "second claim loses");
    let body = body_json(resp.into_body()).await;
    assert_ne!(
        body["error"], "routing_mismatch",
        "an unlabeled order never 409s on routing — it lost the race, not the label"
    );
    // The winner still holds it.
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.claimed_by_runner.as_deref(), Some("runner-X"));
}

// ============================================================================
// 3. routing_label set at approve overrides the create-time value (C31 §4).
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn routing_label_set_at_approve_overrides_create_time(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "routing-approve-override@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;

    // Create a derived draft labeled for runner-A at create time (rung 1 so
    // approve does NOT auto-dispatch — we drive dispatch after).
    let id = create_work_order(&state, &s.scope, s.recommendation_id, 1, None, Some("runner-A"))
        .await
        .unwrap()
        .id;

    // Approve via the HTTP admin handler, overriding routing_label → runner-B.
    let admin_app = work_order_admin_router(state.clone());
    let approve_req = Request::post(format!(
        "/api/v1/projects/{}/work-orders/{}/approve",
        s.project_id, id
    ))
    .header("content-type", "application/json")
    .header("cookie", s.cookie.clone())
    .body(Body::from(json!({ "routing_label": "runner-B" }).to_string()))
    .unwrap();
    let resp = admin_app.oneshot(approve_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "approve must succeed");

    // The order's routing target is now runner-B (approve overrode create-time).
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.routing_label.as_deref(), Some("runner-B"));

    // Dispatch it, then prove poll visibility follows the NEW label.
    transition_work_order(
        &state,
        &s.scope,
        id,
        "dispatch",
        &Actor::System,
        None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() },
    )
    .await
    .unwrap();

    let runner_app = work_order_runner_router(state.clone());
    let poll = |token: String| {
        Request::get(format!(
            "/api/v1/projects/{}/runner/work-orders?state=dispatched",
            s.project_id
        ))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
    };

    // runner-B (the overridden target) sees it.
    let token_b = mint_runner_token(&signing, s.project_id, "runner-B");
    let resp = runner_app.clone().oneshot(poll(token_b)).await.unwrap();
    let ids_b = poll_ids(&body_json(resp.into_body()).await);
    assert!(ids_b.contains(&id.to_string()), "runner-B (overridden label) must see it");

    // runner-A (the stale create-time target) no longer sees it.
    let token_a = mint_runner_token(&signing, s.project_id, "runner-A");
    let resp = runner_app.clone().oneshot(poll(token_a)).await.unwrap();
    let ids_a = poll_ids(&body_json(resp.into_body()).await);
    assert!(
        !ids_a.contains(&id.to_string()),
        "runner-A must NOT see it after approve re-targeted to runner-B"
    );
}
