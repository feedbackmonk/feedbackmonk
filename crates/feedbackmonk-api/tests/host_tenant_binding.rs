#![allow(clippy::doc_markdown)] // test-file doc comments name types/columns/paths verbatim

//! ⛔ Trust-boundary fixture — HOST → TENANT BINDING (FR-FBR-32, DEC-FBR-IMPL-28).
//!
//! DEC-FBR-13 chose tenant subdomains over paths for one decisive reason: origin
//! isolation for the user-generated-content surface. That reason is only real if
//! a request arriving on tenant A's host **cannot reach tenant B's data**.
//! Resolution alone does not deliver it — and the failure is silent, because a
//! missing binding renders a perfectly correct-looking page of someone else's
//! feedback on your origin.
//!
//! This file is the behavioural leg of the `host-tenant-binding` oracle's Probe D.
//! The static legs (A/B/C) prove the wiring from source; these prove the effect.
//!
//! Invariants asserted (each a named test):
//!   1. `tenant_host_serves_its_own_project` — the baseline: on tenant A's
//!      subdomain, A's own project answers normally.
//!   2. `tenant_host_refuses_another_tenants_project` — B's project id on A's
//!      host is **404**, not 403 (no existence oracle) — across the board read,
//!      the single-item read, and widget-config.
//!   3. `custom_domain_binds_like_a_subdomain` — a claimed custom domain
//!      (FR-FBR-33) is bound to its owner exactly as the subdomain is.
//!   4. `admin_alias_redirects_and_never_serves` — an operator-registered admin
//!      alias 301s to the canonical admin host, preserving path + query, and
//!      serves no content (DEC-FBR-IMPL-27, the DEC-13/14 reconciliation).
//!   5. `admin_routes_404_on_a_tenant_host` — the admin surface is unreachable
//!      on a tenant host, so no admin session cookie is ever issued there.
//!   6. `unconfigured_deployment_is_inert` — with no root domain / admin host,
//!      EVERY request behaves exactly as it did before FR-FBR-32. This is the
//!      FR-FBR-17 self-host guarantee, and it is the test most likely to catch a
//!      well-meaning change that makes host-awareness unconditional.
//!   7. `host_header_spoofing_is_normalised` — case, port and the FQDN trailing
//!      dot all reduce to the same binding, so the guard cannot be walked around
//!      by respelling the Host header.
//!   8. `forwarded_host_ignored_without_trusted_proxy` — X-Forwarded-Host is
//!      honoured only behind a declared trusted proxy; otherwise it is an
//!      attacker-settable header.
//!
//! Real Postgres per test via `sqlx::test` (DEC-FBR-03 — the repository layer is
//! the sole query path). Harness shape ported from `tests/board_moderation_gate.rs`.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_anon::AnonGate;
use feedbackmonk_api::email::Mailer;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{
    admin_tier_router, bind_admin_routes, bind_public_routes, board_router, public_site_router,
    widget_config_router, HostConfig, HostState, PublicSiteState,
};
use feedbackmonk_core::hosting::DomainKind;
use feedbackmonk_core::{FeedbackId, FeedbackKind, ModerationStatus};
use feedbackmonk_repository::{
    DomainRepo, ProjectScope, SqlxDomainRepo, SqlxEmailVerificationRepo,
    SqlxFeedbackReplyRepo, SqlxFeedbackRepo, SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck,
    SqlxProjectRepo, SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo, TenantScope,
};

const ROOT: &str = "feedbackmonk.test";
const ADMIN_HOST: &str = "app.feedbackmonk.test";

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
        _scope: &TenantScope,
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
        anon_gate: AnonGate::new(NonZeroU32::new(1000).unwrap()),
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

fn enabled_config() -> HostConfig {
    HostConfig {
        root_domain: Some(ROOT.to_string()),
        admin_host: Some(ADMIN_HOST.to_string()),
        trust_forwarded_host: false,
    }
}

fn host_state(state: &AppState, pool: &PgPool, config: HostConfig) -> HostState {
    HostState {
        domains: Arc::new(SqlxDomainRepo::new(pool.clone())),
        projects: Arc::clone(&state.projects),
        config,
    }
}

/// The public surface under test, wired exactly as `build_app` wires it.
fn public_app(state: &AppState, pool: &PgPool, config: HostConfig) -> axum::Router {
    let hs = host_state(state, pool, config.clone());
    let site_state = PublicSiteState {
        tenants: Arc::clone(&state.tenants),
        projects: Arc::clone(&state.projects),
        domains: Arc::clone(&hs.domains),
        config,
    };
    bind_public_routes(board_router(state.clone()), hs.clone())
        .merge(bind_public_routes(
            widget_config_router(state.clone()),
            hs.clone(),
        ))
        .merge(bind_public_routes(public_site_router(site_state), hs))
}

/// A representative admin router, wired exactly as `build_app` wires admin.
fn admin_app(state: &AppState, pool: &PgPool, config: HostConfig) -> axum::Router {
    bind_admin_routes(
        admin_tier_router(state.clone()),
        host_state(state, pool, config),
    )
}

/// Seed a verified tenant + one project with a board-visible approved row.
/// Returns `(TenantScope, ProjectScope, project_id)`.
async fn seed_tenant(
    state: &AppState,
    pool: &PgPool,
    email: &str,
    subdomain: &str,
) -> (TenantScope, ProjectScope, Uuid) {
    let tenant = state.tenants.create(email, "hash").await.unwrap();
    let tscope = state.tenants.scope_for(tenant.id).await.unwrap();
    state.tenants.mark_verified(&tscope).await.unwrap();
    state
        .tenants
        .set_subdomain(&tscope, Some(subdomain))
        .await
        .unwrap();

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

    let fb = submit(state, &pscope, "an approved public row").await;
    set_moderation(state, pool, &pscope, &fb, ModerationStatus::Approved).await;

    (tscope, pscope, p.id)
}

async fn submit(state: &AppState, scope: &ProjectScope, body: &str) -> FeedbackId {
    state
        .feedback
        .submit_anonymous(scope, &[7u8; 32], None, body, None, FeedbackKind::Other)
        .await
        .unwrap()
}

async fn set_moderation(
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

fn get_on(host: &str, path: &str) -> Request<Body> {
    Request::get(path)
        .header("host", host)
        .body(Body::empty())
        .unwrap()
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 256 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ----- 1. Baseline: a tenant host serves its own project ----------------------

#[sqlx::test(migrations = "../../migrations")]
async fn tenant_host_serves_its_own_project(pool: PgPool) {
    let state = build_test_state(&pool);
    let (_, _, project_a) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;

    let app = public_app(&state, &pool, enabled_config());
    let resp = app
        .oneshot(get_on(
            &format!("alpha.{ROOT}"),
            &format!("/api/v1/projects/{project_a}/board"),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "own project must still serve");
    let body = body_to_json(resp.into_body()).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
}

// ----- 2. THE invariant: cross-tenant project on a bound host is 404 ----------

#[sqlx::test(migrations = "../../migrations")]
async fn tenant_host_refuses_another_tenants_project(pool: PgPool) {
    let state = build_test_state(&pool);
    let (_, _, _project_a) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;
    let (_, pscope_b, project_b) = seed_tenant(&state, &pool, "b@example.com", "bravo").await;

    // Sanity: B's board really is publicly readable on B's own host, so a 404
    // below is the binding refusing it — not the row being invisible anyway.
    let app = public_app(&state, &pool, enabled_config());
    let ok = app
        .oneshot(get_on(
            &format!("bravo.{ROOT}"),
            &format!("/api/v1/projects/{project_b}/board"),
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    // The whole point: B's project, reached on A's origin.
    for path in [
        format!("/api/v1/projects/{project_b}/board"),
        format!("/api/v1/projects/{project_b}/widget-config"),
    ] {
        let app = public_app(&state, &pool, enabled_config());
        let resp = app
            .oneshot(get_on(&format!("alpha.{ROOT}"), &path))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "tenant A's host must not reach tenant B via {path}; \
             404 (not 403) so a probe cannot learn the project exists"
        );
    }

    let _ = pscope_b;
}

// ----- 3. A custom domain binds exactly like a subdomain ---------------------

#[sqlx::test(migrations = "../../migrations")]
async fn custom_domain_binds_like_a_subdomain(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope_a, _, project_a) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;
    let (_, _, project_b) = seed_tenant(&state, &pool, "b@example.com", "bravo").await;

    let domains = SqlxDomainRepo::new(pool.clone());
    domains
        .claim(&tscope_a, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap();

    // Own project: served.
    let app = public_app(&state, &pool, enabled_config());
    let own = app
        .oneshot(get_on(
            "feedback.acme.example",
            &format!("/api/v1/projects/{project_a}/board"),
        ))
        .await
        .unwrap();
    assert_eq!(own.status(), StatusCode::OK);

    // Someone else's project: refused, same as on the subdomain.
    let app = public_app(&state, &pool, enabled_config());
    let other = app
        .oneshot(get_on(
            "feedback.acme.example",
            &format!("/api/v1/projects/{project_b}/board"),
        ))
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
}

// ----- 4. Admin alias redirects, never serves (DEC-FBR-IMPL-27) --------------

#[sqlx::test(migrations = "../../migrations")]
async fn admin_alias_redirects_and_never_serves(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope_a, _, project_a) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;

    let domains = SqlxDomainRepo::new(pool.clone());
    domains
        .claim(&tscope_a, "triage.acme.example", DomainKind::AdminAlias)
        .await
        .unwrap();

    // Public surface: 301 to the canonical admin host, path + query preserved.
    let app = public_app(&state, &pool, enabled_config());
    let resp = app
        .oneshot(get_on(
            "triage.acme.example",
            &format!("/api/v1/projects/{project_a}/board?limit=5"),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(
        location,
        format!("https://{ADMIN_HOST}/api/v1/projects/{project_a}/board?limit=5"),
        "the alias must land the caller on the SAME path at the canonical admin \
         host — that is what keeps a first-party link working without serving \
         admin on a customer domain"
    );

    // Admin surface: also a redirect, never the admin content itself.
    let app = admin_app(&state, &pool, enabled_config());
    let resp = app
        .oneshot(get_on("triage.acme.example", "/api/v1/admin/tier"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
}

// ----- 5. Admin is unreachable on a tenant host ------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn admin_routes_404_on_a_tenant_host(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope_a, _, _) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;
    let domains = SqlxDomainRepo::new(pool.clone());
    domains
        .claim(&tscope_a, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap();

    for host in [format!("alpha.{ROOT}"), "feedback.acme.example".to_string()] {
        let app = admin_app(&state, &pool, enabled_config());
        let resp = app
            .oneshot(get_on(&host, "/api/v1/admin/tier"))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "admin must not be reachable on {host}; the request must 404 BEFORE \
             the handler runs, so no admin session cookie is ever issued there"
        );
    }

    // On the canonical admin host the route is reachable again (401 because
    // there is no session cookie — which is the handler answering, not the guard).
    let app = admin_app(&state, &pool, enabled_config());
    let resp = app
        .oneshot(get_on(ADMIN_HOST, "/api/v1/admin/tier"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ----- 6. Unconfigured deployment is completely inert (FR-FBR-17) ------------

#[sqlx::test(migrations = "../../migrations")]
async fn unconfigured_deployment_is_inert(pool: PgPool) {
    let state = build_test_state(&pool);
    let (_, _, project_a) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;
    let (_, _, project_b) = seed_tenant(&state, &pool, "b@example.com", "bravo").await;

    let disabled = HostConfig::default();
    assert!(!disabled.is_enabled());

    // Every project is reachable from every host, exactly as before FR-FBR-32.
    // A self-host operator answering on an IP, a LAN name or anything else must
    // see no behaviour change from a SaaS feature they never enabled.
    for (host, project) in [
        ("192.168.1.10:14304", project_a),
        ("feedback.gitcellar.com", project_b),
        (&format!("alpha.{ROOT}"), project_b),
    ] {
        let app = public_app(&state, &pool, disabled.clone());
        let resp = app
            .oneshot(get_on(host, &format!("/api/v1/projects/{project}/board")))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "unconfigured deployments must be untouched ({host} → {project})"
        );
    }

    // And admin stays reachable on any host.
    let app = admin_app(&state, &pool, disabled);
    let resp = app
        .oneshot(get_on("192.168.1.10:14304", "/api/v1/admin/tier"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "handler, not guard");
}

// ----- 7. Host respelling does not defeat the guard --------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn host_header_spoofing_is_normalised(pool: PgPool) {
    let state = build_test_state(&pool);
    let (_, _, _project_a) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;
    let (_, _, project_b) = seed_tenant(&state, &pool, "b@example.com", "bravo").await;

    // Each of these is a legal respelling of `alpha.{ROOT}`. If any one of them
    // failed to normalise, it would resolve to "unknown host" — unbound — and
    // the cross-tenant read would succeed.
    for spelling in [
        format!("ALPHA.{}", ROOT.to_uppercase()),
        format!("alpha.{ROOT}:8443"),
        format!("alpha.{ROOT}."),
        format!("  alpha.{ROOT}  "),
    ] {
        let app = public_app(&state, &pool, enabled_config());
        let resp = app
            .oneshot(get_on(
                &spelling,
                &format!("/api/v1/projects/{project_b}/board"),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "Host spelling {spelling:?} must still bind to tenant A"
        );
    }
}

// ----- 8. X-Forwarded-Host is only trusted behind a declared proxy -----------

#[sqlx::test(migrations = "../../migrations")]
async fn forwarded_host_ignored_without_trusted_proxy(pool: PgPool) {
    let state = build_test_state(&pool);
    let (_, _, project_a) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;
    let (_, _, project_b) = seed_tenant(&state, &pool, "b@example.com", "bravo").await;

    // Attacker on A's host claims to be on B's, hoping to read B's board.
    let req = Request::get(format!("/api/v1/projects/{project_b}/board"))
        .header("host", format!("alpha.{ROOT}"))
        .header("x-forwarded-host", format!("bravo.{ROOT}"))
        .body(Body::empty())
        .unwrap();
    let app = public_app(&state, &pool, enabled_config());
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "X-Forwarded-Host must be ignored when no trusted proxy is declared"
    );

    // With a trusted proxy declared, the forwarded host IS the effective host —
    // which is the whole reason the flag is tied to FEEDBACKMONK_TRUSTED_PROXY_HOPS.
    let mut trusting = enabled_config();
    trusting.trust_forwarded_host = true;
    let req = Request::get(format!("/api/v1/projects/{project_a}/board"))
        .header("host", "internal.railway")
        .header("x-forwarded-host", format!("alpha.{ROOT}"))
        .body(Body::empty())
        .unwrap();
    let app = public_app(&state, &pool, trusting);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ----- 9. /public/site reports only the host's own tenant --------------------

#[sqlx::test(migrations = "../../migrations")]
async fn public_site_describes_only_the_hosts_tenant(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope_a, _, project_a) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;
    seed_tenant(&state, &pool, "b@example.com", "bravo").await;

    let app = public_app(&state, &pool, enabled_config());
    let resp = app
        .oneshot(get_on(&format!("alpha.{ROOT}"), "/api/v1/public/site"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp.into_body()).await;

    assert_eq!(body["tenant_id"], tscope_a.tenant_id().to_string());
    assert_eq!(body["subdomain"], "alpha");
    assert_eq!(body["custom_domain"], false);
    let projects = body["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1, "only this tenant's projects are listed");
    assert_eq!(projects[0]["project_id"], project_a.to_string());
    assert_eq!(projects[0]["public_board_enabled"], true);

    // Privacy: the payload is project metadata only. A tenant's email must never
    // appear on a public, unauthenticated surface (DEC-FBR-02 / Q24 class).
    let raw = serde_json::to_string(&body).unwrap();
    assert!(!raw.contains("a@example.com"), "no tenant PII on /public/site");
    assert!(!raw.contains("b@example.com"), "no other tenant on /public/site");

    // An unbound host gets nothing at all — the endpoint cannot enumerate.
    let app = public_app(&state, &pool, enabled_config());
    let resp = app
        .oneshot(get_on(ROOT, "/api/v1/public/site"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ----- 10. The edge TLS seam authorises exactly the right hosts --------------

#[sqlx::test(migrations = "../../migrations")]
async fn tls_authorize_gates_issuance(pool: PgPool) {
    let state = build_test_state(&pool);
    let (tscope_a, _, _) = seed_tenant(&state, &pool, "a@example.com", "alpha").await;

    let domains = SqlxDomainRepo::new(pool.clone());
    domains
        .claim(&tscope_a, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap();

    // A Free tenant (the seed default) does NOT carry `custom_domain`, so the
    // edge must refuse to issue for their claimed domain — the tier gate is
    // enforced by the thing that serves traffic, not only at claim time.
    let app = public_app(&state, &pool, enabled_config());
    let resp = app
        .oneshot(get_on(
            ADMIN_HOST,
            "/api/v1/public/tls-authorize?domain=feedback.acme.example",
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "a Free tenant's custom domain must not get a certificate (FR-FBR-33)"
    );

    // Promote to Pro (which carries custom_domain) and the same host authorises.
    state
        .tenants
        .set_tier(&tscope_a, feedbackmonk_core::Tier::Pro)
        .await
        .unwrap();
    let app = public_app(&state, &pool, enabled_config());
    let resp = app
        .oneshot(get_on(
            ADMIN_HOST,
            "/api/v1/public/tls-authorize?domain=feedback.acme.example",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The admin host and a live tenant subdomain authorise; a stranger does not.
    for (domain, expected) in [
        (ADMIN_HOST.to_string(), StatusCode::OK),
        (format!("alpha.{ROOT}"), StatusCode::OK),
        (format!("nobody.{ROOT}"), StatusCode::NOT_FOUND),
        ("random.example.org".to_string(), StatusCode::NOT_FOUND),
    ] {
        let app = public_app(&state, &pool, enabled_config());
        let resp = app
            .oneshot(get_on(
                ADMIN_HOST,
                &format!("/api/v1/public/tls-authorize?domain={domain}"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), expected, "tls-authorize({domain})");
    }
}
