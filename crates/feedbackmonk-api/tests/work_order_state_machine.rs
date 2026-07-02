#![allow(clippy::doc_markdown)] // test doc comments name event_type / state strings verbatim

//! Work-order approval state machine — the **bypass-resistant** integration
//! corpus (Contract C22, FR-FBR-22 / FR-FBR-25a). This is **Probe C** of the
//! `approval-gate-enforcement` Verification Oracle: the drift-detection leg that
//! exercises the C22 invariant end-to-end against a real Postgres so the static
//! probes (state-machine source + handler) and live behaviour cannot silently
//! diverge.
//!
//! THE security boundary between public internet input and code execution is the
//! approval gate (C22 inv. 1): **no work order reaches an execution state
//! (`is_execution_state` — `dispatched` and beyond) without a prior
//! owner-authored `approve` event in the `work_order_events` ledger.** A passing
//! happy-path test does NOT prove the absence of bypass paths (Testability Gate
//! Flag 1, the highest plan-wide fidelity risk). These tests are adversarial:
//! they attack the gate directly (forced `approved` state with no approve event;
//! illegal skips; cross-actor authorship) and assert the ledger never holds an
//! orphan `dispatched`.
//!
//! Coverage:
//!   1. `happy_path_full_lifecycle` — create→approve→dispatch→claim→…→accept
//!   2. `forced_approved_state_without_approve_event_cannot_dispatch` — **the
//!      core bypass test**: the state column lies, the ledger is the truth
//!   3. `skip_to_execution_states_is_illegal` — draft→{claim,building,…} illegal
//!   4. `runner_cannot_author_owner_events` / `admin_cannot_author_runner_events`
//!   5. `terminal_states_are_immutable`
//!   6. `same_txn_parity_every_state_change_has_one_ledger_row`
//!   7. `ledger_never_holds_orphan_dispatched` (the oracle invariant, restated)
//!   8. `retry_loop_reuses_existing_approval` (failed→approved retry keeps gate open)
//!   9. runner-token seam (HTTP): runner write-token claims; end-user JWT → 403;
//!      no token → 401.

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
    create_work_order, transition_work_order, Actor, TransitionFailure, RUNNER_TOKEN_SCOPE,
};
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{work_order_admin_router, work_order_runner_router};
use feedbackmonk_core::{
    ActionType, FeedbackKind, KeyClass, WorkOrderState, WorkOrderTransitionError,
};
use feedbackmonk_repository::{
    NewRecommendation, ProjectScope, SqlxAnalysisSweepRepo,
    SqlxClusterRepo, SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
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
        runner_token_revocations: Arc::new(feedbackmonk_repository::SqlxRunnerTokenRevocationRepo::new(pool.clone())),
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
}

/// Seed a verified tenant + project + cluster + recommendation — everything a
/// work order needs to be created from.
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
    Seeded {
        scope,
        project_id: p.id,
        recommendation_id: rec.id,
    }
}

/// Register a fresh **runner-class** Ed25519 signing key (Contract C25); return
/// the dalek key so the caller can mint runner tokens verifiable by the project.
/// Runner-token verification selects only runner-class keys, so the seam test's
/// key MUST be runner-class.
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

/// Mint an EdDSA token. When `runner` is true it carries the `scope:
/// "runner:write"` class marker (a runner write-token); otherwise it is an
/// ordinary end-user JWT. A random `jti` is included.
fn mint_token(signing: &DalekSigningKey, project_id: Uuid, sub: &str, runner: bool) -> String {
    mint_token_jti(signing, project_id, sub, runner, &Uuid::new_v4().to_string())
}

/// Like [`mint_token`] but with a caller-chosen `jti` (so the revocation
/// round-trip can target a known token).
fn mint_token_jti(
    signing: &DalekSigningKey,
    project_id: Uuid,
    sub: &str,
    runner: bool,
    jti: &str,
) -> String {
    let header = json!({ "alg": "EdDSA", "typ": "JWT" });
    let now = Utc::now().timestamp();
    let mut payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
        "jti": jti,
    });
    if runner {
        payload["scope"] = json!(RUNNER_TOKEN_SCOPE);
    }
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig.to_bytes()))
}

/// Mint a runner write-token with NO `jti` claim (scrutiny P2-10). Structurally
/// valid + signed + carries the runner scope marker, but omits the revocation
/// key — `verify_runner_token` must refuse it (401) since it would be
/// unrevocable.
fn mint_runner_token_without_jti(
    signing: &DalekSigningKey,
    project_id: Uuid,
    sub: &str,
) -> String {
    let header = json!({ "alg": "EdDSA", "typ": "JWT" });
    let now = Utc::now().timestamp();
    let payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
        "scope": RUNNER_TOKEN_SCOPE,
        // deliberately NO "jti"
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = signing.sign(signing_input.as_bytes());
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig.to_bytes()))
}

async fn make_draft(state: &AppState, s: &Seeded, rung: i32) -> Uuid {
    create_work_order(state, &s.scope, s.recommendation_id, rung, None)
        .await
        .unwrap()
        .id
}

/// Author an admin `approve` (opens the gate) and assert it commits.
async fn admin_approve(state: &AppState, scope: &ProjectScope, id: Uuid) {
    let admin = Uuid::new_v4();
    transition_work_order(
        state,
        scope,
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
}

// ============================================================================
// 1. Happy path — the full lifecycle commits in order.
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn happy_path_full_lifecycle(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-happy@example.com").await;
    let id = make_draft(&state, &s, 1).await;

    admin_approve(&state, &s.scope, id).await;
    // system dispatch (gate now open).
    transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() }).await.unwrap();
    // runner lifecycle.
    let runner = Actor::Runner { sub: "runner-1".into() };
    transition_work_order(&state, &s.scope, id, "claim", &runner, None,
        WorkOrderStatePatch { claimed_by_runner: Some("runner-1"), ..Default::default() }).await.unwrap();
    transition_work_order(&state, &s.scope, id, "building", &runner, None, WorkOrderStatePatch::default()).await.unwrap();
    transition_work_order(&state, &s.scope, id, "verifying", &runner, None, WorkOrderStatePatch::default()).await.unwrap();
    transition_work_order(&state, &s.scope, id, "reported", &runner, None, WorkOrderStatePatch::default()).await.unwrap();
    // owner accepts → completed.
    transition_work_order(&state, &s.scope, id, "accept", &Actor::Admin { id: Uuid::new_v4() }, None, WorkOrderStatePatch::default()).await.unwrap();

    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Completed);

    let ledger = state.work_order_events.list_for_work_order(&s.scope, id).await.unwrap();
    let events: Vec<&str> = ledger.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(events, ["approve", "dispatch", "claim", "building", "verifying", "reported", "accept"]);
    // The gate was open before the first execution state (`dispatched`).
    assert!(state.work_order_events.has_approved_event(&s.scope, id).await.unwrap());
}

// ============================================================================
// 2. THE bypass test — a forged `approved` state column cannot dispatch.
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn forced_approved_state_without_approve_event_cannot_dispatch(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-bypass@example.com").await;
    let id = make_draft(&state, &s, 1).await;

    // Simulate a bypass/corruption: force the STATE COLUMN to `approved`
    // directly via the repo WITHOUT appending an owner-authored `approve`
    // event. The state column now lies; the ledger is empty of approvals.
    let mut tx = pool.begin().await.unwrap();
    state
        .work_orders
        .update_state_in_executor(&s.scope, &mut tx, id, WorkOrderState::Approved, WorkOrderStatePatch::default())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!state.work_order_events.has_approved_event(&s.scope, id).await.unwrap());

    // dispatch (approved→dispatched is a LEGAL edge) MUST still be refused —
    // the gate reads the LEDGER, never trusts the state column alone.
    let err = transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch::default()).await.unwrap_err();
    assert!(
        matches!(err, TransitionFailure::Rule(WorkOrderTransitionError::ApprovalRequired { to }) if to == WorkOrderState::Dispatched),
        "execution state must require a prior owner-authored approve event; got {err:?}"
    );

    // No orphan dispatched in the ledger, and the order never advanced.
    let ledger = state.work_order_events.list_for_work_order(&s.scope, id).await.unwrap();
    assert!(ledger.iter().all(|e| e.to_state != WorkOrderState::Dispatched), "no orphan dispatched row");
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Approved, "state stays approved; dispatch rolled back / never ran");
}

// ============================================================================
// 3. Skipping straight to an execution state is illegal (gate never reached).
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn skip_to_execution_states_is_illegal(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-skip@example.com").await;
    let id = make_draft(&state, &s, 1).await;

    // From `draft`, every execution-bound event is an illegal transition — the
    // legal table has no edge, so it fails before the gate even matters.
    for (ev, actor) in [
        ("claim", Actor::Runner { sub: "r".into() }),
        ("building", Actor::Runner { sub: "r".into() }),
        ("verifying", Actor::Runner { sub: "r".into() }),
        ("reported", Actor::Runner { sub: "r".into() }),
        ("dispatch", Actor::System),
        ("accept", Actor::Admin { id: Uuid::new_v4() }),
    ] {
        let err = transition_work_order(&state, &s.scope, id, ev, &actor, None, WorkOrderStatePatch::default())
            .await
            .unwrap_err();
        assert!(
            matches!(err, TransitionFailure::Rule(WorkOrderTransitionError::IllegalTransition { .. })),
            "{ev} from draft must be IllegalTransition; got {err:?}"
        );
    }
    // The order never left draft; the ledger is empty.
    assert!(state.work_order_events.list_for_work_order(&s.scope, id).await.unwrap().is_empty());
}

// ============================================================================
// 4. Cross-actor authorship is rejected (authz matrix, inv. 2).
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn runner_cannot_author_owner_events(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-runner-owner@example.com").await;
    let id = make_draft(&state, &s, 1).await;

    // A runner attempting the owner-only `approve` — even though draft→approved
    // is a legal edge — must be refused on authz. (If this passed, a runner
    // could open its own gate: the literal RCE path.)
    let err = transition_work_order(&state, &s.scope, id, "approve", &Actor::Runner { sub: "r".into() }, None,
        WorkOrderStatePatch::default()).await.unwrap_err();
    assert!(matches!(err, TransitionFailure::Rule(WorkOrderTransitionError::ActorNotAuthorized)), "got {err:?}");
    // The gate stays CLOSED.
    assert!(!state.work_order_events.has_approved_event(&s.scope, id).await.unwrap());
}

#[sqlx::test(migrations = "../../migrations")]
async fn admin_cannot_author_runner_events(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-admin-runner@example.com").await;
    let id = make_draft(&state, &s, 1).await;
    admin_approve(&state, &s.scope, id).await;
    transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() }).await.unwrap();

    // An admin attempting the runner-only `claim` from `dispatched` — legal
    // edge, wrong actor class → ActorNotAuthorized.
    let err = transition_work_order(&state, &s.scope, id, "claim", &Actor::Admin { id: Uuid::new_v4() }, None,
        WorkOrderStatePatch::default()).await.unwrap_err();
    assert!(matches!(err, TransitionFailure::Rule(WorkOrderTransitionError::ActorNotAuthorized)), "got {err:?}");
}

// ============================================================================
// 5. Terminal states are immutable (inv. 5).
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn terminal_states_are_immutable(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-terminal@example.com").await;
    let id = make_draft(&state, &s, 1).await;
    // draft → cancelled (terminal).
    transition_work_order(&state, &s.scope, id, "cancel", &Actor::Admin { id: Uuid::new_v4() }, None,
        WorkOrderStatePatch::default()).await.unwrap();

    for ev in ["approve", "dispatch", "cancel", "retry"] {
        let err = transition_work_order(&state, &s.scope, id, ev, &Actor::Admin { id: Uuid::new_v4() }, None,
            WorkOrderStatePatch::default()).await.unwrap_err();
        assert!(
            matches!(err, TransitionFailure::Rule(WorkOrderTransitionError::TerminalState)),
            "{ev} on a cancelled order must be TerminalState; got {err:?}"
        );
    }
}

// ============================================================================
// 6 + 7. Same-txn parity + no orphan dispatched (the oracle invariant).
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn same_txn_parity_and_no_orphan_dispatched(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-parity@example.com").await;
    let id = make_draft(&state, &s, 1).await;
    admin_approve(&state, &s.scope, id).await;
    transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() }).await.unwrap();
    transition_work_order(&state, &s.scope, id, "cancel", &Actor::Admin { id: Uuid::new_v4() }, None,
        WorkOrderStatePatch::default()).await.unwrap();

    let ledger = state.work_order_events.list_for_work_order(&s.scope, id).await.unwrap();
    // Parity: three committed transitions → exactly three ledger rows; the
    // final row's to_state equals the live state column.
    assert_eq!(ledger.len(), 3);
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(ledger.last().unwrap().to_state, wo.state);
    // Every from→to in the ledger is a contiguous chain (no gaps).
    for pair in ledger.windows(2) {
        assert_eq!(pair[1].from_state, Some(pair[0].to_state), "ledger must chain contiguously");
    }
    // The oracle invariant restated behaviourally: any row reaching an
    // execution state is preceded by an owner-authored approve event.
    let approved = state.work_order_events.has_approved_event(&s.scope, id).await.unwrap();
    let reaches_exec = ledger.iter().any(|e| e.to_state.is_execution_state());
    assert!(!reaches_exec || approved, "execution state present without approve event in ledger");
}

// ============================================================================
// 8. retry loop reuses the existing approval (gate stays open).
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn retry_loop_keeps_gate_open(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-retry@example.com").await;
    let id = make_draft(&state, &s, 1).await;
    admin_approve(&state, &s.scope, id).await;
    let runner = Actor::Runner { sub: "r".into() };
    transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() }).await.unwrap();
    transition_work_order(&state, &s.scope, id, "claim", &runner, None,
        WorkOrderStatePatch { claimed_by_runner: Some("r"), ..Default::default() }).await.unwrap();
    transition_work_order(&state, &s.scope, id, "building", &runner, None, WorkOrderStatePatch::default()).await.unwrap();
    transition_work_order(&state, &s.scope, id, "failed", &runner, None,
        WorkOrderStatePatch { failure_reason: Some("boom"), ..Default::default() }).await.unwrap();
    // owner retry: failed → approved (admin, NOT a new `approve` event).
    transition_work_order(&state, &s.scope, id, "retry", &Actor::Admin { id: Uuid::new_v4() }, None,
        WorkOrderStatePatch::default()).await.unwrap();
    // dispatch again: the ORIGINAL approve event still satisfies the gate.
    transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch::default()).await.unwrap();
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Dispatched);
}

// ============================================================================
// 9. Runner write-token seam (HTTP) — Q14.
// ============================================================================

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&to_bytes(body, 64 * 1024).await.unwrap()).unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn runner_token_seam_accepts_runner_rejects_end_user_and_anon(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-runner-seam@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;

    // Drive an order to `dispatched` (gate open) so `claim` is a legal edge.
    let id = make_draft(&state, &s, 1).await;
    admin_approve(&state, &s.scope, id).await;
    transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() }).await.unwrap();

    let app = work_order_runner_router(state.clone()).merge(work_order_admin_router(state.clone()));
    let claim_url = format!("/api/v1/projects/{}/work-orders/{}/claim", s.project_id, id);

    let claim_req = |bearer: Option<String>| {
        let mut b = Request::post(&claim_url).header("content-type", "application/json");
        if let Some(t) = bearer {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    };

    // (a) No token → 401.
    let resp = app.clone().oneshot(claim_req(None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "no token must 401");

    // (b) A valid END-USER JWT (no runner scope claim) → 403 (authenticates a
    // person, never the runner — the privilege boundary).
    let end_user = mint_token(&signing, s.project_id, "end-user-1", false);
    let resp = app.clone().oneshot(claim_req(Some(end_user))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "an end-user JWT must NOT authorize a runner claim");

    // (c) A runner write-token (scope=runner:write) → 200, order claimed.
    let runner_token = mint_token(&signing, s.project_id, "runner-7", true);
    let resp = app.clone().oneshot(claim_req(Some(runner_token))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "a runner write-token must claim");
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["to_state"], "claimed");

    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Claimed);
    assert_eq!(wo.claimed_by_runner.as_deref(), Some("runner-7"));
}

// ============================================================================
// 9b. Scrutiny P2-10 — a runner token with NO `jti` is rejected at the seam.
//     A jti-less token would skip the revocation denylist entirely (unrevocable),
//     so it must be refused BEFORE any transition.
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn runner_token_without_jti_is_rejected(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-runner-nojti@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;

    // Drive an order to `dispatched` so `claim` would be a legal edge if allowed.
    let id = make_draft(&state, &s, 1).await;
    admin_approve(&state, &s.scope, id).await;
    transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() }).await.unwrap();

    let app = work_order_runner_router(state.clone());
    let claim_url = format!("/api/v1/projects/{}/work-orders/{}/claim", s.project_id, id);
    let token = mint_runner_token_without_jti(&signing, s.project_id, "runner-nojti");
    let req = Request::post(&claim_url)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "a runner token with no jti must be rejected (unrevocable)"
    );
    // The order must NOT have transitioned.
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Dispatched);
}

// ============================================================================
// 10. inv. 4 — create rejects rung 0; auto-accept at rung ≥ 2.
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn rung_zero_never_produces_a_work_order(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-rung0@example.com").await;
    let err = create_work_order(&state, &s.scope, s.recommendation_id, 0, None).await.unwrap_err();
    assert!(matches!(err, feedbackmonk_api::ApiError::BadRequest(_)), "rung 0 must be rejected; got {err:?}");
}

// ============================================================================
// 11. Runner-token mint / verify / revoke round-trip (Contract C25, P5b).
//     THE GATE 0 acceptance test: a customer-minted runner token verifies, and
//     revoking its `jti` kills it at the auth seam BEFORE any state transition —
//     while a different (unrevoked) token from the same key still works
//     (revocation is jti-scoped, not key-scoped).
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn runner_token_mint_verify_revoke_round_trip(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed(&state, "wo-runner-revoke@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;

    // Drive an order to `dispatched` so `claim` is a legal edge.
    let id = make_draft(&state, &s, 1).await;
    admin_approve(&state, &s.scope, id).await;
    transition_work_order(&state, &s.scope, id, "dispatch", &Actor::System, None,
        WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() }).await.unwrap();

    let app = work_order_runner_router(state.clone());
    let claim_url = format!("/api/v1/projects/{}/work-orders/{}/claim", s.project_id, id);
    let claim_req = |bearer: String| {
        Request::post(&claim_url)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {bearer}"))
            .body(Body::empty())
            .unwrap()
    };

    // MINT a runner token with a known jti, then REVOKE that jti (the owner's
    // kill-switch for a leaked token).
    let revoked_jti = Uuid::new_v4().to_string();
    let revoked_token = mint_token_jti(&signing, s.project_id, "runner-rev", true, &revoked_jti);
    state
        .runner_token_revocations
        .revoke(&s.scope, &revoked_jti, Some("leaked"))
        .await
        .unwrap();

    // VERIFY: the revoked token is rejected at the auth seam (401), BEFORE any
    // transition — the order stays `dispatched` (no claim).
    let resp = app.clone().oneshot(claim_req(revoked_token)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "a revoked runner token must 401");
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(
        wo.state,
        WorkOrderState::Dispatched,
        "a revoked token must not transition the order"
    );

    // A DIFFERENT, unrevoked token minted from the SAME runner key still works —
    // revocation is jti-scoped, not key-scoped (and proves mint+verify happy path).
    let good_token = mint_token(&signing, s.project_id, "runner-ok", true);
    let resp = app.clone().oneshot(claim_req(good_token)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "an unrevoked runner token must still claim");
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Claimed);
}
