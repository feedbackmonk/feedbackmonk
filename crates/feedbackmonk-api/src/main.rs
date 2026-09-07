//! feedbackmonk API binary.
//!
//! Boot sequence:
//! 1. Load env (via parent process; we do NOT read .env here -- ops layer
//!    handles env injection in containers/dev shells).
//! 2. Connect Postgres.
//! 3. Construct repository handles + mailer (env-selected: Mailpit dev or SMTP prod).
//! 4. Build `AppState`.
//! 5. Compose Worker A router (+ Worker B's router when they merge it in).
//! 6. Bind `FEEDBACKMONK_PORT` and serve.
//!
//! Worker B merges their submission router by extending `build_state` +
//! `build_app` here -- coordinate via `channels/messages.md`.

use std::env;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use chrono::{Duration, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};

use feedbackmonk_anon::{
    AnonGate, IpGate, LoginGate, DEFAULT_LOGIN_RATE_LIMIT_PER_MIN, DEFAULT_PUBLIC_RATE_LIMIT_PER_MIN,
    DEFAULT_RATE_LIMIT_PER_HOUR,
};
use feedbackmonk_jwt::DEFAULT_IAT_LEEWAY_SECONDS;
use feedbackmonk_repository::{
    SqlxAnalysisSweepRepo, SqlxAttachmentRepo, SqlxBoardVoteRepo, SqlxClusterRepo,
    SqlxDomainRepo,
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxPasswordResetRepo, SqlxProjectRepo,
    SqlxRecommendationRepo,
    SqlxRoadmapItemRepo, SqlxRoadmapVoteRepo, SqlxRunnerTokenRepo, SqlxRunnerTokenRevocationRepo,
    SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo, SqlxWorkOrderEventRepo, SqlxWorkOrderRepo,
};

use feedbackmonk_api::email::{
    EmailNotifier, EnvSmtpConfig, EnvSmtpMailer, LettreEmailNotifier, Mailer, MailpitMailer,
};
use feedbackmonk_api::router::{health_router, router as worker_a_router};
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::translation::{DeepLTranslator, LibreTranslateTranslator, TranslationProvider};
use feedbackmonk_api::{
    account_recovery_router, admin_feedback_routes, admin_roadmap_router, admin_tier_router,
    apply_public_rate_limit, attachments_router, bind_admin_routes, bind_public_routes,
    board_router, capabilities_router, cluster_admin_router, domains_router,
    me_feedback_data_router, me_feedback_router, moderation_router, ops_router, parse_origins,
    promote_router, public_cors_layer, public_site_router, recommendation_admin_router,
    roadmap_router, runner_tokens_admin_router, solicitation_router, spawn_translation_worker,
    tenant_settings_router,
    spawn_voting_cache_refresh, submission_router, sweep_admin_router, widget_config_router,
    work_order_admin_router, work_order_runner_router, AccountRecoveryState, AttachmentState,
    DomainAdminState, HostConfig, HostState, MeFeedbackDataState, PublicRateLimit,
    PublicSiteState, VotingCache, DEFAULT_TRANSLATION_POLL_SECS,
    DEFAULT_TRANSLATION_TARGET_LANG,
};

// Boot sequence: a flat sequence of env reads + wiring, not branching logic.
#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> Result<()> {
    init_tracing()?;

    let port: u16 = env::var("FEEDBACKMONK_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(14304);

    // FEEDBACKMONK_BIND_ADDR controls which interface the api binary
    // listens on. Default 127.0.0.1 preserves the dev-machine pattern
    // (don't expose the api to the whole LAN during `cargo run`).
    // Self-host docker-compose sets this to 0.0.0.0 so the admin-ui
    // edge container (separate IP in the docker network) can reach the
    // api via the service-name DNS (see deploy/docker/docker-compose.yml
    // and docs/operations/SELFHOST_ENV.md — Contract C21).
    let bind_addr: IpAddr = env::var("FEEDBACKMONK_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1".to_string())
        .parse()
        .context("FEEDBACKMONK_BIND_ADDR is not a valid IP address (try 127.0.0.1 for local, 0.0.0.0 for docker)")?;

    let pool = connect_pg().await?;

    // The translation provider is built ONCE and used by two consumers: the
    // FR-FBR-30 translate-after-accept worker (spawned below) and the FR-FBR-40
    // outbound email path (inside `build_state`, which is why it is constructed
    // here rather than beside the worker). `None` — the default posture,
    // DEC-FBR-IMPL-26 — means neither consumer has a path to any backend.
    let translation_provider = build_translation_provider()?;
    feedbackmonk_repository::TranslationFlag::set(translation_provider.is_some());

    let state = build_state(pool, translation_provider.clone())?;

    // Gap #1: attachment upload sub-router state — its own state type (NOT
    // AppState) so attachments add zero edits to AppState constructors. The
    // object store is env-selected (local FS default for self-host;
    // S3-compatible for SaaS/MinIO — docs/operations/SELFHOST_ENV.md C21).
    let attachment_state = AttachmentState {
        projects: Arc::clone(&state.projects),
        attachments: Arc::new(SqlxAttachmentRepo::new(state.pool.clone())),
        storage: feedbackmonk_api::storage::from_env()
            .context("failed to configure attachment object store")?,
        signing_keys: Arc::clone(&state.signing_keys),
        jwt_iat_leeway_seconds: state.jwt_iat_leeway_seconds,
    };

    // Phase A A1/A5: me_feedback erasure + export sub-router state. Its own
    // state type (NOT AppState) for the same reason as AttachmentState — the
    // attachment repo + object store must not ripple through every
    // `AppState { … }` construction site. Shares the attachment_state handles.
    let me_feedback_data_state = MeFeedbackDataState {
        projects: Arc::clone(&state.projects),
        signing_keys: Arc::clone(&state.signing_keys),
        feedback: Arc::clone(&state.feedback),
        feedback_replies: Arc::clone(&state.feedback_replies),
        attachments: Arc::clone(&attachment_state.attachments),
        storage: Arc::clone(&attachment_state.storage),
        jwt_iat_leeway_seconds: state.jwt_iat_leeway_seconds,
    };

    // Scrutiny P1-1: admin account-recovery sub-router state (password reset +
    // verify-email resend). Its own state type (NOT AppState) so the new
    // password_resets repo + reset TTL don't ripple through every AppState
    // construction site — same pattern as AttachmentState / MeFeedbackDataState.
    // Logout runs on AppState (needs AdminSession) and is wired into the Worker
    // A router directly.
    let reset_ttl_hours: i64 = env::var("FEEDBACKMONK_RESET_TOKEN_TTL_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|h| *h >= 1)
        .unwrap_or(1);
    let account_recovery_state = AccountRecoveryState {
        tenants: Arc::clone(&state.tenants),
        password_resets: Arc::new(SqlxPasswordResetRepo::new(state.pool.clone())),
        email_verifications: Arc::clone(&state.email_verifications),
        mailer: Arc::clone(&state.mailer),
        login_gate: state.login_gate.clone(),
        public_url: Arc::clone(&state.public_url),
        reset_token_ttl: Duration::hours(reset_ttl_hours),
        verify_token_ttl: state.verify_token_ttl,
    };

    // P2: spawn the 60s roadmap voting-cache refresh tick. JoinHandle is
    // intentionally not held — process exit aborts the task. The cache
    // tolerates per-project refresh failures internally (logs WARN, keeps
    // prior payload).
    let _voting_cache_tick = spawn_voting_cache_refresh(
        state.voting_cache.clone(),
        Arc::clone(&state.projects),
        Arc::clone(&state.roadmap_items),
    );

    // FR-FBR-30: translate-after-accept worker. The provider DEFAULTS OFF
    // (DEC-FBR-IMPL-26) — translation egress is a conscious, opt-in, disclosed
    // choice (docs/operations/SELFHOST_ENV.md C21). When off (`None`), no worker
    // is spawned and submits stamp NO `translation_status` (the global flag stays
    // false). When a provider is configured, submits stamp `pending` and this
    // background worker drains the queue off the request path (DEC-FBR-IMPL-25
    // D3). JoinHandle intentionally not held — process exit aborts the task,
    // exactly like the voting-cache tick above.
    if let Some(provider) = translation_provider {
        let target_lang = env::var("FEEDBACKMONK_TRANSLATION_TARGET_LANG")
            .unwrap_or_else(|_| DEFAULT_TRANSLATION_TARGET_LANG.to_string());
        let poll_secs = env::var("FEEDBACKMONK_TRANSLATION_POLL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TRANSLATION_POLL_SECS);
        tracing::info!(
            target_lang = %target_lang,
            poll_secs,
            "translation provider ENABLED — spawning translate-after-accept worker (FR-FBR-30)"
        );
        let _translation_tick = spawn_translation_worker(
            provider,
            Arc::clone(&state.feedback),
            target_lang,
            poll_secs,
        );
    }

    // CORS allowlist for the public, credentialed widget endpoints (submission
    // + attachments). Customer sites embed the widget cross-origin; their
    // origins must be listed here or the browser blocks the preflight. Unset =>
    // no cross-origin origin allowed (secure default). See `cors.rs` /
    // DEC-FBR-IMPL-09.
    let cors_origins = parse_origins(&env::var("FEEDBACKMONK_CORS_ORIGINS").unwrap_or_default());
    if cors_origins.is_empty() {
        tracing::warn!(
            "FEEDBACKMONK_CORS_ORIGINS is unset/empty — cross-origin widget embeds will be \
             blocked by the browser. Set it to the customer origin(s), e.g. https://gitcellar.com"
        );
    } else {
        tracing::info!(origins = ?cors_origins, "CORS allowlist for public widget endpoints");
    }

    // FR-FBR-32/33: host->tenant resolution + the custom-domain surface.
    // Sub-state types (NOT AppState fields) for the same reason
    // AttachmentState/MeFeedbackDataState are: a new repo handle must not
    // ripple through every `AppState { … }` construction site.
    //
    // `HostConfig::from_env` reads FEEDBACKMONK_ROOT_DOMAIN +
    // FEEDBACKMONK_ADMIN_HOST. With neither set the whole layer is a
    // pass-through and self-host behaviour is unchanged (DEC-FBR-IMPL-28).
    let host_config = HostConfig::from_env(state.trusted_proxy_hops);
    if host_config.is_enabled() {
        tracing::info!(
            root_domain = ?host_config.root_domain,
            admin_host = ?host_config.admin_host,
            trust_forwarded_host = host_config.trust_forwarded_host,
            "host-based tenant resolution ENABLED (FR-FBR-32)"
        );
    } else {
        tracing::info!(
            "host-based tenant resolution inert (FEEDBACKMONK_ROOT_DOMAIN /              FEEDBACKMONK_ADMIN_HOST unset) — single-tenant/self-host posture"
        );
    }
    let domain_repo = Arc::new(SqlxDomainRepo::new(state.pool.clone()));
    let host_state = HostState {
        domains: Arc::clone(&domain_repo) as Arc<dyn feedbackmonk_repository::DomainRepo>,
        projects: Arc::clone(&state.projects),
        config: host_config.clone(),
    };
    let public_site_state = PublicSiteState {
        tenants: Arc::clone(&state.tenants),
        projects: Arc::clone(&state.projects),
        domains: Arc::clone(&host_state.domains),
        config: host_config.clone(),
    };
    let domain_admin_state = DomainAdminState {
        app: state.clone(),
        domains: Arc::clone(&host_state.domains),
        config: host_config,
    };

    let app = build_app(
        state,
        attachment_state,
        me_feedback_data_state,
        account_recovery_state,
        &host_state,
        public_site_state,
        domain_admin_state,
        &cors_origins,
    );

    let addr: SocketAddr = (bind_addr, port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "feedbackmonk-api listening");
    // `into_make_service_with_connect_info` injects `ConnectInfo<SocketAddr>`
    // as a request extension so the submission handler can hash client IP
    // into its anon-mode token (FR-FBR-06). Without this the handler emits
    // "Missing request extension: ConnectInfo<SocketAddr>" at runtime even
    // though the routes compile.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Initialise tracing per FR-FBR-18 + FR-FBR-10.
///
/// Delegates to `feedbackmonk_tracing::install_global_subscriber`, the
/// workspace-wide PII-scrubbing chokepoint. `FEEDBACKMONK_LOG_FORMAT=json`
/// (production default) emits structured JSON; `FEEDBACKMONK_LOG_FORMAT=text`
/// is the human-friendly dev format. `RUST_LOG`, if set, overrides the
/// `level` argument (parsed inside `install_global_subscriber`).
///
/// The `pii-scrub-audit` Verification Oracle (Probe A) forbids any other
/// `tracing_subscriber::fmt()` / `registry()` / `impl Layer<...> for ...`
/// elsewhere in the workspace.
fn init_tracing() -> Result<()> {
    let format = match std::env::var("FEEDBACKMONK_LOG_FORMAT")
        .unwrap_or_else(|_| "json".to_string())
        .as_str()
    {
        "text" | "plain" => feedbackmonk_tracing::LogFormat::Plain,
        _ => feedbackmonk_tracing::LogFormat::Json,
    };
    feedbackmonk_tracing::install_global_subscriber(
        feedbackmonk_tracing::LogLevel::Info,
        format,
    )
    .context("failed to install global tracing subscriber")?;
    Ok(())
}

async fn connect_pg() -> Result<PgPool> {
    let url = env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&url)
        .await
        .context("failed to connect to Postgres")?;
    Ok(pool)
}

// A flat repo-wiring + env-parsing constructor, not complex logic — the
// `too_many_lines` heuristic does not fit an `AppState` assembly function.
#[allow(clippy::too_many_lines)]
fn build_state(
    pool: PgPool,
    translation_provider: Option<Arc<dyn TranslationProvider>>,
) -> Result<AppState> {
    let tenants = Arc::new(SqlxTenantRepo::new(pool.clone()));
    let projects = Arc::new(SqlxProjectRepo::new(pool.clone()));
    let signing_keys = Arc::new(SqlxSigningKeyRepo::new(pool.clone()));
    let feedback = Arc::new(SqlxFeedbackRepo::new(pool.clone()));
    let feedback_history = Arc::new(SqlxFeedbackStatusHistoryRepo::new(pool.clone()));
    let feedback_replies = Arc::new(SqlxFeedbackReplyRepo::new(pool.clone()));
    let email_verifications = Arc::new(SqlxEmailVerificationRepo::new(pool.clone()));
    let roadmap_items = Arc::new(SqlxRoadmapItemRepo::new(pool.clone()));
    let roadmap_votes = Arc::new(SqlxRoadmapVoteRepo::new(pool.clone()));
    let board_votes = Arc::new(SqlxBoardVoteRepo::new(pool.clone()));
    let tier_quotas = Arc::new(SqlxTierQuotaRepo::new(pool.clone()));
    // P5a: agentic feedback resolution loop repositories (Contracts C22/C23).
    let clusters = Arc::new(SqlxClusterRepo::new(pool.clone()));
    let recommendations = Arc::new(SqlxRecommendationRepo::new(pool.clone()));
    let analysis_sweeps = Arc::new(SqlxAnalysisSweepRepo::new(pool.clone()));
    let work_orders = Arc::new(SqlxWorkOrderRepo::new(pool.clone()));
    let work_order_events = Arc::new(SqlxWorkOrderEventRepo::new(pool.clone()));
    // P5b: runner-token lifecycle registry + append-only revocation denylist
    // (Contract C25).
    let runner_tokens = Arc::new(SqlxRunnerTokenRepo::new(pool.clone()));
    let runner_token_revocations = Arc::new(SqlxRunnerTokenRevocationRepo::new(pool.clone()));
    let voting_cache = VotingCache::new();
    let health = SqlxHealthCheck::new(pool.clone());

    let mailer = build_mailer()?;
    let email_notifier = build_email_notifier(
        Arc::clone(&tenants) as Arc<dyn feedbackmonk_repository::TenantRepo>,
        translation_provider,
    )?;
    let session_secret = load_session_secret()?;
    let public_url = env::var("FEEDBACKMONK_PUBLIC_URL")
        .unwrap_or_else(|_| "http://localhost:14304".to_string());

    let ttl_hours: i64 = env::var("FEEDBACKMONK_VERIFY_TOKEN_TTL_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);

    let anon_quota: u32 = env::var("FEEDBACKMONK_ANON_RATE_LIMIT_PER_HOUR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_RATE_LIMIT_PER_HOUR);
    let anon_quota = NonZeroU32::new(anon_quota)
        .context("FEEDBACKMONK_ANON_RATE_LIMIT_PER_HOUR must be > 0")?;
    let anon_gate = AnonGate::new(anon_quota);

    let login_quota: u32 = env::var("FEEDBACKMONK_LOGIN_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_LOGIN_RATE_LIMIT_PER_MIN);
    let login_quota = NonZeroU32::new(login_quota)
        .context("FEEDBACKMONK_LOGIN_RATE_LIMIT_PER_MIN must be > 0")?;
    let login_gate = LoginGate::new(login_quota);

    // Class-level per-IP DoS ceiling for every public route (P0-2).
    let public_quota: u32 = env::var("FEEDBACKMONK_PUBLIC_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PUBLIC_RATE_LIMIT_PER_MIN);
    let public_quota = NonZeroU32::new(public_quota)
        .context("FEEDBACKMONK_PUBLIC_RATE_LIMIT_PER_MIN must be > 0")?;
    let ip_gate = IpGate::new(public_quota);

    // Trusted reverse-proxy hops for client-IP resolution (P1-2). Default 0 =
    // trust no X-Forwarded-For (secure default); set 1 behind a single LB.
    let trusted_proxy_hops: usize = env::var("FEEDBACKMONK_TRUSTED_PROXY_HOPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let jwt_iat_leeway_seconds: i64 = env::var("FEEDBACKMONK_JWT_LEEWAY_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_IAT_LEEWAY_SECONDS);

    // DEC-FBR-IMPL-11: ops mutation surface. Unset/empty ⇒ disabled (the
    // endpoint returns 404), so deployments that don't opt in expose nothing.
    let ops_token: Option<Arc<str>> = env::var("FEEDBACKMONK_OPS_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(Arc::from);
    if ops_token.is_some() {
        tracing::info!("ops mutation endpoint ENABLED (FEEDBACKMONK_OPS_TOKEN set)");
    }

    Ok(AppState {
        pool,
        tenants,
        projects,
        signing_keys,
        feedback,
        feedback_history,
        feedback_replies,
        email_verifications,
        mailer,
        email_notifier,
        session_secret: Arc::new(session_secret),
        public_url: Arc::from(public_url.as_str()),
        verify_token_ttl: Duration::hours(ttl_hours),
        anon_gate,
        login_gate,
        ip_gate,
        trusted_proxy_hops,
        jwt_iat_leeway_seconds,
        roadmap_items,
        roadmap_votes,
        board_votes,
        voting_cache,
        started_at: Utc::now(),
        health,
        tier_quotas,
        ops_token,
        clusters,
        recommendations,
        analysis_sweeps,
        work_orders,
        work_order_events,
        runner_tokens,
        runner_token_revocations,
    })
}

/// Build the translation provider from env (FR-FBR-30, DEC-FBR-IMPL-26).
///
/// `FEEDBACKMONK_TRANSLATION_PROVIDER` selects the backend and DEFAULTS to
/// `off`. `off` returns `None` (no worker spawned, no pending-stamping — the
/// default-off privacy posture). `deepl` requires
/// `FEEDBACKMONK_TRANSLATION_DEEPL_API_KEY`. Mirrors `build_mailer()`.
///
/// Do NOT change the default away from `off`, or remove the `off` option,
/// without re-opening DEC-FBR-IMPL-26 (the egress-consent decision).
fn build_translation_provider() -> Result<Option<Arc<dyn TranslationProvider>>> {
    let mode = env::var("FEEDBACKMONK_TRANSLATION_PROVIDER").unwrap_or_else(|_| "off".to_string());
    match mode.trim().to_ascii_lowercase().as_str() {
        "" | "off" => Ok(None),
        "deepl" => {
            let key = env::var("FEEDBACKMONK_TRANSLATION_DEEPL_API_KEY").context(
                "FEEDBACKMONK_TRANSLATION_DEEPL_API_KEY is required when \
                 FEEDBACKMONK_TRANSLATION_PROVIDER=deepl",
            )?;
            Ok(Some(Arc::new(DeepLTranslator::new(key)?)))
        }
        "libretranslate" => {
            // No-egress option (DEC-FBR-IMPL-26): an operator-supplied, self-hosted
            // LibreTranslate endpoint. Optional api key for instances that require one.
            let url = env::var("FEEDBACKMONK_TRANSLATION_LIBRETRANSLATE_URL").context(
                "FEEDBACKMONK_TRANSLATION_LIBRETRANSLATE_URL is required when \
                 FEEDBACKMONK_TRANSLATION_PROVIDER=libretranslate (e.g. http://libretranslate:5000)",
            )?;
            let key = env::var("FEEDBACKMONK_TRANSLATION_LIBRETRANSLATE_API_KEY").ok();
            Ok(Some(Arc::new(LibreTranslateTranslator::new(&url, key)?)))
        }
        other => Err(anyhow::anyhow!(
            "FEEDBACKMONK_TRANSLATION_PROVIDER must be 'off', 'deepl', or 'libretranslate', got {other}"
        )),
    }
}

fn build_mailer() -> Result<Arc<dyn Mailer>> {
    let mode = env::var("FEEDBACKMONK_MAILER").unwrap_or_else(|_| "mailpit".to_string());
    let from = env::var("FEEDBACKMONK_SMTP_FROM").unwrap_or_else(|_| "no-reply@feedbackmonk.local".into());
    match mode.as_str() {
        "mailpit" => {
            let host = env::var("FEEDBACKMONK_MAILPIT_HOST").unwrap_or_else(|_| "localhost".into());
            let port = env::var("FEEDBACKMONK_MAILPIT_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1025);
            Ok(Arc::new(MailpitMailer::new(&host, port, &from)?))
        }
        "smtp" => {
            let cfg = EnvSmtpConfig {
                host: env::var("FEEDBACKMONK_SMTP_HOST").context("FEEDBACKMONK_SMTP_HOST")?,
                port: env::var("FEEDBACKMONK_SMTP_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(587),
                user: env::var("FEEDBACKMONK_SMTP_USER").context("FEEDBACKMONK_SMTP_USER")?,
                pass: env::var("FEEDBACKMONK_SMTP_PASS").context("FEEDBACKMONK_SMTP_PASS")?,
                from,
                starttls: env::var("FEEDBACKMONK_SMTP_STARTTLS")
                    .map_or(true, |s| s != "false"),
            };
            Ok(Arc::new(EnvSmtpMailer::new(cfg)?))
        }
        other => Err(anyhow::anyhow!(
            "FEEDBACKMONK_MAILER must be 'mailpit' or 'smtp', got {other}"
        )),
    }
}

/// `translation_provider` is the FR-FBR-40 outbound-translation backend, and is
/// `None` in the default posture — the notifier then has no path to any provider
/// at all, whatever a tenant has ticked (DEC-FBR-IMPL-26).
fn build_email_notifier(
    tenants: Arc<dyn feedbackmonk_repository::TenantRepo>,
    translation_provider: Option<Arc<dyn TranslationProvider>>,
) -> Result<Arc<dyn EmailNotifier>> {
    let mode = env::var("FEEDBACKMONK_MAILER").unwrap_or_else(|_| "mailpit".to_string());
    let from = env::var("FEEDBACKMONK_SMTP_FROM").unwrap_or_else(|_| "no-reply@feedbackmonk.local".into());
    match mode.as_str() {
        "mailpit" => {
            let host = env::var("FEEDBACKMONK_MAILPIT_HOST").unwrap_or_else(|_| "localhost".into());
            let port = env::var("FEEDBACKMONK_MAILPIT_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1025);
            Ok(Arc::new(
                LettreEmailNotifier::mailpit(tenants, &host, port, &from)?
                    .with_translator(translation_provider),
            ))
        }
        "smtp" => {
            // Reuse the env-driven SMTP relay; we only need the lettre
            // transport, not the EnvSmtpMailer wrapper.
            use lettre::{AsyncSmtpTransport, Tokio1Executor};
            use lettre::transport::smtp::authentication::Credentials;
            let host = env::var("FEEDBACKMONK_SMTP_HOST").context("FEEDBACKMONK_SMTP_HOST")?;
            let port: u16 = env::var("FEEDBACKMONK_SMTP_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(587);
            let user = env::var("FEEDBACKMONK_SMTP_USER").context("FEEDBACKMONK_SMTP_USER")?;
            let pass = env::var("FEEDBACKMONK_SMTP_PASS").context("FEEDBACKMONK_SMTP_PASS")?;
            let starttls = env::var("FEEDBACKMONK_SMTP_STARTTLS")
                .map_or(true, |s| s != "false");
            let builder = if starttls {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)?
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&host)?
            };
            let transport = builder
                .port(port)
                .credentials(Credentials::new(user, pass))
                .build();
            Ok(Arc::new(
                LettreEmailNotifier::from_transport(tenants, transport, &from)
                    .with_translator(translation_provider),
            ))
        }
        other => Err(anyhow::anyhow!(
            "FEEDBACKMONK_MAILER must be 'mailpit' or 'smtp', got {other}"
        )),
    }
}

fn load_session_secret() -> Result<[u8; 32]> {
    let hex_str = env::var("FEEDBACKMONK_SESSION_SECRET")
        .context("FEEDBACKMONK_SESSION_SECRET not set (expected 64 hex chars)")?;
    let trimmed = hex_str.trim();
    if trimmed.len() != 64 {
        anyhow::bail!(
            "FEEDBACKMONK_SESSION_SECRET must be 64 hex chars (32 bytes); got {} chars",
            trimmed.len()
        );
    }
    let mut out = [0u8; 32];
    for (i, chunk) in trimmed.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(chunk).context("non-utf8 in session secret")?;
        out[i] = u8::from_str_radix(s, 16).context("non-hex in session secret")?;
    }
    Ok(out)
}

// A flat router-composition function, not complex logic -- `too_many_arguments`
// does not fit an assembly seam whose whole job is to receive the sub-states.
#[allow(clippy::too_many_arguments)]
fn build_app(
    state: AppState,
    attachment_state: AttachmentState,
    me_feedback_data_state: MeFeedbackDataState,
    account_recovery_state: AccountRecoveryState,
    host_state: &HostState,
    public_site_state: PublicSiteState,
    domain_admin_state: DomainAdminState,
    cors_origins: &[String],
) -> Router {
    // FR-FBR-18: every request is wrapped in a span carrying a `request_id`
    // (UUIDv4) populated from `x-request-id` if the client supplied one, else
    // freshly generated. The TraceLayer emits structured INFO logs at request
    // start and response end with method/uri/status; downstream handler logs
    // automatically inherit the span's `request_id` field.
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().include_headers(false))
        .on_request(DefaultOnRequest::new())
        .on_response(DefaultOnResponse::new());

    // CORS applies ONLY to the public credentialed widget endpoints (submission
    // + attachments) — applied per-router before `.merge()` so it never leaks
    // onto admin/operator routes. `widget-config` is intentionally excluded: it
    // is fetched with `credentials: "omit"` and stays `*`-public (project brand
    // metadata only). See `cors.rs` + DEC-FBR-04 / DEC-FBR-IMPL-09.
    let cors = public_cors_layer(cors_origins);

    // Class-level per-IP DoS ceiling (P0-2). EVERY public router below is
    // wrapped by `apply_public_rate_limit` — the `public-route-ceiling` oracle
    // enforces this so a future public route cannot silently skip the floor.
    let prl = PublicRateLimit::new(state.ip_gate.clone(), state.trusted_proxy_hops);

    // FR-FBR-32 (DEC-FBR-IMPL-28): two binding layers over the SAME routers we
    // already had — nothing is migrated or duplicated.
    //
    //   `bind_public_routes` — resolves the Host, stashes the HostScope, and on
    //   a tenant-bound host REFUSES (404) any `project_id` in the path that
    //   belongs to a different tenant. This is what makes the subdomain split
    //   real; DEC-FBR-13 chose subdomains for origin isolation, and resolution
    //   without this restriction would render tenant B's user-generated content
    //   on tenant A's origin while every page still looked correct.
    //
    //   `bind_admin_routes` — refuses admin routes on a tenant-bound host, so
    //   the admin console (and its session cookie) exists on exactly ONE origin.
    //
    // The `host-tenant-binding` Verification Oracle asserts from this function
    // that every public router still carries the public guard and every admin
    // router the admin guard — the same anti-treadmill shape
    // `public-route-ceiling` uses for the rate-limit floor. A new public route
    // added without a wrapper is the regression it exists to catch.
    //
    // With no root domain / admin host configured, BOTH layers are
    // pass-throughs and behaviour is byte-identical to pre-FR-FBR-32.
    let hs = host_state;

    let app = health_router(state.clone())
        // Health probes are deliberately NOT bound: an orchestrator may probe
        // on any hostname the deployment answers on, and health carries no
        // tenant data. See `router::health_router`.
        .merge(bind_admin_routes(worker_a_router(state.clone()), hs.clone()))
        // Scrutiny P1-1: password-reset request/confirm + verify-email resend.
        // Public (unauthenticated) + email-triggering, so rate-limited via the
        // shared LoginGate inside the handlers. No CORS (admin/tenant surface,
        // same posture as login/signup in worker_a_router — not a widget embed).
        // Admin-bound: an account-recovery mail must not be triggerable from a
        // tenant's own public origin.
        .merge(bind_admin_routes(
            account_recovery_router(account_recovery_state),
            hs.clone(),
        ))
        .merge(bind_public_routes(
            apply_public_rate_limit(
                submission_router(state.clone()).layer(cors.clone()),
                prl.clone(),
            ),
            hs.clone(),
        ))
        .merge(bind_admin_routes(admin_feedback_routes(state.clone()), hs.clone()))
        .merge(bind_public_routes(widget_config_router(state.clone()), hs.clone()))
        .merge(bind_public_routes(
            apply_public_rate_limit(roadmap_router(state.clone()), prl.clone()),
            hs.clone(),
        ))
        .merge(bind_admin_routes(admin_roadmap_router(state.clone()), hs.clone()))
        .merge(bind_admin_routes(admin_tier_router(state.clone()), hs.clone()))
        // Operator-only tier + brand-override mutation (DEC-FBR-IMPL-11). No
        // CORS layer (called server-side / via curl, never a browser embed);
        // guarded by the OpsAuth bearer token (404 when token unset).
        .merge(bind_admin_routes(ops_router(state.clone()), hs.clone()))
        .merge(bind_public_routes(me_feedback_router(state.clone()), hs.clone()))
        // Phase A A1/A5: erasure + export on the me_feedback path — merged
        // WITHOUT CORS, same posture as the read subtree above (JWT end-user
        // surface driven by the consumer's own client, not a browser embed).
        .merge(bind_public_routes(
            me_feedback_data_router(me_feedback_data_state),
            hs.clone(),
        ))
        // GitCellar in-app solicitation (FR-FBR-28/27): durable per-user
        // solicitation state (JWT end-user surface; merged WITHOUT CORS, like
        // me_feedback — driven by the consumer's own client) + public
        // capability discovery (`GET /api/v1/capabilities`, metadata-only).
        .merge(bind_public_routes(solicitation_router(state.clone()), hs.clone()))
        // Capability discovery carries no tenant data and no project id — it is
        // deployment metadata, answerable on any host, so it is left unbound.
        .merge(capabilities_router(state.clone()))
        // FR-FBR-32/33: host-rooted discovery + the edge's on-demand-TLS
        // authorisation seam. Public, and bound like every other public router
        // (the guard is what populates the HostScope `/site` reads).
        .merge(bind_public_routes(
            apply_public_rate_limit(public_site_router(public_site_state), prl.clone()),
            hs.clone(),
        ))
        // FR-FBR-33: tenant subdomain + custom-domain claim/release. Admin
        // surface (AdminSession, no CORS); the tier gate fires inside the claim
        // handler.
        .merge(bind_admin_routes(domains_router(domain_admin_state), hs.clone()))
        // FR-FBR-38 / C38: tenant Language settings. Admin-only, one origin.
        .merge(bind_admin_routes(
            tenant_settings_router(state.clone()),
            hs.clone(),
        ))
        .merge(bind_public_routes(
            apply_public_rate_limit(
                attachments_router(attachment_state).layer(cors.clone()),
                prl.clone(),
            ),
            hs.clone(),
        ))
        // P5a (Contract C22, Worker A): work-order API + approval state machine.
        // Admin routes behind AdminSession; runner routes behind the runner
        // write-token seam (Q14). BOTH merge WITHOUT `.layer(cors)` — these are
        // admin + server-to-server surfaces, never browser embeds. CORS stays
        // ONLY on the public submit/attachments routers above (Ripple Analysis
        // flags accidental CORS-exposure of admin/runner endpoints).
        .merge(bind_admin_routes(work_order_admin_router(state.clone()), hs.clone()))
        .merge(bind_admin_routes(work_order_runner_router(state.clone()), hs.clone()))
        // P5b (Contract C25, Worker D consumes): runner-token lifecycle
        // (list/register/revoke) behind AdminSession. Merged WITHOUT
        // `.layer(cors)` — admin surface, never a browser embed. Only the
        // public submit/attachments routers above carry CORS (Ripple Analysis
        // flags accidental CORS-exposure of admin endpoints).
        .merge(bind_admin_routes(runner_tokens_admin_router(state.clone()), hs.clone()))
        // P5a (Contract C23/C24, Worker B): clustering/sweep/recommendation
        // admin surface (merge/split, sweep trigger+digest, recommendation
        // ingestion + read). AdminSession; merged WITHOUT `.layer(cors)` — admin
        // surface, never a browser embed. Clustering-on-submit adds NO new
        // external route (it hooks the existing public submit handler).
        .merge(bind_admin_routes(cluster_admin_router(state.clone()), hs.clone()))
        .merge(bind_admin_routes(recommendation_admin_router(state.clone()), hs.clone()))
        .merge(bind_admin_routes(sweep_admin_router(state.clone()), hs.clone()))
        // Public Feedback Board + Moderation Gate (Contracts C28/C29):
        //   board_router is the PUBLIC approved-only board read — merged WITH
        //   `.layer(cors)`, matching the submit/attachments public surface
        //   (`cors-allowlist-enforcement`). moderation_router is the admin
        //   moderate + queue + board-settings surface — merged WITHOUT CORS
        //   (AdminSession, never a browser embed; Ripple Analysis flags
        //   accidental CORS-exposure of admin endpoints).
        .merge(bind_public_routes(
            apply_public_rate_limit(
                board_router(state.clone()).layer(cors.clone()),
                prl.clone(),
            ),
            hs.clone(),
        ))
        .merge(bind_admin_routes(moderation_router(state.clone()), hs.clone()))
        .merge(bind_admin_routes(promote_router(state), hs.clone()));
    app.layer(PropagateRequestIdLayer::x_request_id())
        .layer(trace_layer)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}
