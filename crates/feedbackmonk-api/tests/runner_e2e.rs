#![allow(clippy::doc_markdown)] // doc comments name event_type / state strings verbatim

//! **Real end-to-end runner dry-run (DEC-001 / GATE 1).**
//!
//! This is the GATE-1 evidence the P5b handoff requires: a genuine
//! dispatch → claim → build(StubAgent) → report exercise driven by the ACTUAL
//! `feedbackmonk-runner` host loop (`poll::run_once`) over a REAL HTTP socket
//! against the REAL runner read/ingestion endpoints this worker (CLAUDE-E)
//! added to close the frozen-C26 gap — NOT a hermetic wiremock stand-in.
//!
//! It proves the two halves the wiremock test cannot:
//!   1. the new runner-token read endpoints actually SERVE the rich, frozen
//!      `ClaimedOrder` shape the runner's `WorkOrderClient` deserializes
//!      (trusted instruction layer + untrusted recommendation/cluster grounding
//!      assembled from the tenant-scoped repos), and
//!   2. the whole loop composes: poll (`GET /runner/work-orders?state=dispatched`)
//!      → claim → fetch detail (`GET /runner/work-orders/:id`) → implement
//!      (StubAgent — no real `claude`) → report, landing a SANITIZED `result_ref`.
//!
//! The agent is injected ([`StubAgent`]) so no real `claude` spawns — the
//! RCE-grade surface stays out of the test loop (Testability Gate discipline).

use std::num::NonZeroU32;
use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, Utc};
use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
use rand_core::OsRng;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::handlers::work_orders::{
    create_work_order, transition_work_order, Actor, RUNNER_TOKEN_SCOPE,
};
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{recommendation_admin_router, work_order_runner_router};
use feedbackmonk_core::{ActionType, FeedbackKind, KeyClass, WorkOrderState};
use feedbackmonk_repository::{
    NewRecommendation, ProjectScope, SqlxAnalysisSweepRepo, SqlxClusterRepo,
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxRecommendationRepo,
    SqlxRoadmapItemRepo, SqlxRoadmapVoteRepo, SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo,
    SqlxWorkOrderEventRepo, SqlxWorkOrderRepo, WorkOrderStatePatch,
};

use feedbackmonk_runner::{
    DiffStat, ImplementResult, RepoContext, ResultRef, StubAgent, Verification, WorkOrderClient,
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

// ----- Test wiring (mirrors work_order_state_machine.rs) -----------------------

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
    cluster_id: Uuid,
}

/// Seed a verified tenant + project + cluster (with a summary) + REAL feedback
/// members assigned to that cluster + a recommendation — everything the runner's
/// `ClaimedOrder` assembly joins together.
async fn seed_full(state: &AppState, email: &str) -> Seeded {
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
        .create(&scope, "Login broken", Some("Users cannot log in"), FeedbackKind::Bug, "agent")
        .await
        .unwrap();

    // Two REAL feedback members in the cluster — the rawest untrusted text the
    // ClaimedOrder carries as `member_bodies`.
    for (i, body) in ["login crashes on submit", "the submit button does nothing"]
        .into_iter()
        .enumerate()
    {
        let hash = [u8::try_from(i + 1).unwrap(); 32];
        let fid = state
            .feedback
            .submit_anonymous(&scope, &hash, None, body, None, FeedbackKind::Bug)
            .await
            .unwrap();
        state.feedback.set_cluster_id(&scope, &fid, Some(cluster.id)).await.unwrap();
    }

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
        cluster_id: cluster.id,
    }
}

/// Register a fresh **runner-class** Ed25519 signing key (Contract C25); return
/// the dalek key so the caller can mint runner tokens the project verifies.
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

/// Mint an EdDSA token. `runner=true` carries the `scope:"runner:write"` class
/// marker; otherwise an ordinary end-user JWT (no runner scope claim).
fn mint_token(signing: &DalekSigningKey, project_id: Uuid, sub: &str, runner: bool) -> String {
    let header = json!({ "alg": "EdDSA", "typ": "JWT" });
    let now = Utc::now().timestamp();
    let mut payload = json!({
        "sub": sub,
        "aud": project_id.to_string(),
        "iat": now,
        "exp": now + 300,
        "jti": Uuid::new_v4().to_string(),
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

/// Author an admin `approve` (opens the C22 gate) through the audited core.
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

/// Bind the runner read/write + ingestion routers to an ephemeral localhost port
/// and serve them on a background task. Returns `(base_url, join_handle)`.
async fn serve_app(state: &AppState) -> (String, tokio::task::JoinHandle<()>) {
    let app =
        work_order_runner_router(state.clone()).merge(recommendation_admin_router(state.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service()).await.unwrap();
    });
    (format!("http://{addr}"), handle)
}

fn stub_agent() -> StubAgent {
    StubAgent::with_result(ImplementResult {
        result_ref: ResultRef {
            pr_url: Some("https://git.example/pr/42".into()),
            branch: Some("fbm/wo-e2e".into()),
            commit: None,
            diff_stat: DiffStat { files: 2, insertions: 12, deletions: 3 },
            verification: Verification { tests_passed: true, finalize_status: "passed".into() },
            summary: "Restored the missing auth guard; 2 files".into(),
        },
    })
}

// ============================================================================
// 1. THE GATE-1 e2e: dispatch → claim → build(StubAgent) → report, REAL HTTP.
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn runner_e2e_dispatch_claim_build_report(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed_full(&state, "e2e-runner@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;

    // Drive the order to `dispatched` at autonomy rung 1 (so the runner's
    // `reported` does NOT auto-accept — the order rests at `reported`, the literal
    // GATE-1 terminal). approve + dispatch are authored through the audited core.
    let id = create_work_order(&state, &s.scope, s.recommendation_id, 1, None)
        .await
        .unwrap()
        .id;
    admin_approve(&state, &s.scope, id).await;
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

    let (base_url, server) = serve_app(&state).await;
    let token = mint_token(&signing, s.project_id, "runner-e2e", true);
    let client = WorkOrderClient::new(base_url, s.project_id, token);

    // (a) Poll the REAL read endpoint and assert it served the rich, frozen
    // ClaimedOrder shape (trusted instruction layer + untrusted grounding). A
    // GET poll does not transition the order, so the loop below still drives it.
    let orders = client.poll_dispatched().await.unwrap();
    assert_eq!(orders.len(), 1, "exactly the one dispatched order");
    let order = &orders[0];
    assert_eq!(order.work_order_id, id);
    assert_eq!(order.action_type, ActionType::BugFix);
    assert_eq!(order.title, "Fix the auth check");
    assert_eq!(order.instructions, "Restore the missing guard");
    // Untrusted, feedback-derived grounding assembled from the tenant-scoped repos.
    assert_eq!(order.recommendation.body, "Restore the missing guard");
    assert_eq!(order.recommendation.cluster_summary, "Users cannot log in");
    assert_eq!(order.recommendation.member_bodies.len(), 2, "both cluster members");
    assert!(order
        .recommendation
        .member_bodies
        .iter()
        .any(|b| b == "login crashes on submit"));
    assert_eq!(order.recommendation.source_refs, json!([{ "path": "src/auth.rs", "lines": "10-20" }]));

    // (b) Drive the REAL loop end-to-end: poll → claim → fetch detail → build →
    // verifying → reported (result_ref through the egress sanitizer).
    let agent = stub_agent();
    let repo = RepoContext { repo_path: ".".into() };
    let summary = feedbackmonk_runner::poll::run_once(&client, &agent, &repo).await.unwrap();
    assert_eq!(summary.polled, 1);
    assert_eq!(summary.reported, 1, "driven to reported");
    assert_eq!(summary.failed, 0, "no per-order failures: {:?}", summary.errors);

    // The order rests at `reported` with the SANITIZED result_ref persisted.
    let wo = state.work_orders.get(&s.scope, id).await.unwrap();
    assert_eq!(wo.state, WorkOrderState::Reported, "rung-1 order rests at reported");
    let rr = wo.result_ref.expect("result_ref persisted on reported");
    assert_eq!(rr["branch"], "fbm/wo-e2e");
    assert_eq!(rr["verification"]["tests_passed"], true);
    assert_eq!(rr["diff_stat"]["files"], 2);

    // The ledger chains the full runner lifecycle.
    let ledger = state.work_order_events.list_for_work_order(&s.scope, id).await.unwrap();
    let events: Vec<&str> = ledger.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(
        events,
        ["approve", "dispatch", "claim", "building", "verifying", "reported"]
    );

    server.abort();
}

// ============================================================================
// 2. Runner-authed analyst ingestion (DEC-001 / MSG-C01 Q2) over REAL HTTP.
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn runner_e2e_analyst_ingestion_via_runner_token(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed_full(&state, "e2e-ingest@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;

    let (base_url, server) = serve_app(&state).await;
    let token = mint_token(&signing, s.project_id, "analyst-e2e", true);
    let client = WorkOrderClient::new(base_url, s.project_id, token);

    // The payload Worker C produces: a top-level `cluster_id` routes the URL (B↔C
    // seam, MSG-C01 Q1); the rest is the `IngestRequest` body.
    let payload = json!({
        "cluster_id": s.cluster_id.to_string(),
        "action_type": "bug_fix",
        "title": "Fix the login crash",
        "body": "Restore the auth guard removed in the refactor",
        "rationale": "Multiple users locked out",
        "source_refs": [{ "path": "src/auth.rs", "lines": "10-20" }],
        "confidence": 0.7,
        "sweep_id": null,
    });
    client.post_recommendation(&payload).await.unwrap();

    // A new `proposed` recommendation now exists in the cluster (alongside the
    // one seed_full created), proving the runner-token ingestion path lands.
    let recs = state.recommendations.list_for_cluster(&s.scope, s.cluster_id).await.unwrap();
    assert!(
        recs.iter()
            .any(|r| r.title == "Fix the login crash" && r.status == "proposed"),
        "runner-token ingestion must create a proposed recommendation"
    );

    server.abort();
}

// ============================================================================
// 3. The new runner READ endpoints reject a non-runner credential (authz).
// ============================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn runner_read_endpoints_reject_end_user_jwt(pool: PgPool) {
    let state = build_test_state(&pool);
    let s = seed_full(&state, "e2e-authz@example.com").await;
    let signing = seed_signing_key(&state, &s.scope).await;

    let (base_url, server) = serve_app(&state).await;

    // A valid END-USER JWT (no runner scope claim) must NOT read the runner
    // surface — the new endpoints are behind the SAME audited verify path.
    let end_user = mint_token(&signing, s.project_id, "end-user-1", false);
    let client = WorkOrderClient::new(base_url, s.project_id, end_user);
    let err = client.poll_dispatched().await.unwrap_err();
    assert!(
        err.to_string().contains("403"),
        "an end-user JWT must be forbidden on the runner read endpoint; got: {err}"
    );

    server.abort();
}
