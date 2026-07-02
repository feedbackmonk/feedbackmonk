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
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxPasswordResetRepo, SqlxProjectRepo,
    SqlxRecommendationRepo,
    SqlxRoadmapItemRepo, SqlxRoadmapVoteRepo, SqlxRunnerTokenRepo, SqlxRunnerTokenRevocationRepo,
    SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo, SqlxWorkOrderEventRepo, SqlxWorkOrderRepo,
};

use feedbackmonk_api::email::{
    EmailNotifier, EnvSmtpConfig, EnvSmtpMailer, LettreEmailNotifier, Mailer, MailpitMailer,
};
use feedbackmonk_api::router::router as worker_a_router;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::translation::{DeepLTranslator, LibreTranslateTranslator, TranslationProvider};
use feedbackmonk_api::{
    account_recovery_router, admin_feedback_routes, admin_roadmap_router, admin_tier_router,
    apply_public_rate_limit, attachments_router, board_router, capabilities_router,
    cluster_admin_router,
    me_feedback_data_router, me_feedback_router, moderation_router, ops_router, parse_origins,
    promote_router, public_cors_layer, recommendation_admin_router, roadmap_router,
    runner_tokens_admin_router, solicitation_router, spawn_translation_worker,
    spawn_voting_cache_refresh, submission_router, sweep_admin_router, widget_config_router,
    work_order_admin_router, work_order_runner_router, AccountRecoveryState, AttachmentState,
    MeFeedbackDataState, PublicRateLimit, VotingCache, DEFAULT_TRANSLATION_POLL_SECS,
    DEFAULT_TRANSLATION_TARGET_LANG,
};

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
    let state = build_state(pool)?;

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
    let translation_provider = build_translation_provider()?;
    feedbackmonk_repository::TranslationFlag::set(translation_provider.is_some());
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

    let app = build_app(
        state,
        attachment_state,
        me_feedback_data_state,
        account_recovery_state,
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
fn build_state(pool: PgPool) -> Result<AppState> {
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
    let email_notifier = build_email_notifier(Arc::clone(&tenants) as Arc<dyn feedbackmonk_repository::TenantRepo>)?;
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

fn build_email_notifier(
    tenants: Arc<dyn feedbackmonk_repository::TenantRepo>,
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
            Ok(Arc::new(LettreEmailNotifier::mailpit(tenants, &host, port, &from)?))
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
            Ok(Arc::new(LettreEmailNotifier::from_transport(tenants, transport, &from)))
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

fn build_app(
    state: AppState,
    attachment_state: AttachmentState,
    me_feedback_data_state: MeFeedbackDataState,
    account_recovery_state: AccountRecoveryState,
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

    let app = worker_a_router(state.clone())
        // Scrutiny P1-1: password-reset request/confirm + verify-email resend.
        // Public (unauthenticated) + email-triggering, so rate-limited via the
        // shared LoginGate inside the handlers. No CORS (admin/tenant surface,
        // same posture as login/signup in worker_a_router — not a widget embed).
        .merge(account_recovery_router(account_recovery_state))
        .merge(apply_public_rate_limit(
            submission_router(state.clone()).layer(cors.clone()),
            prl.clone(),
        ))
        .merge(admin_feedback_routes(state.clone()))
        .merge(widget_config_router(state.clone()))
        .merge(apply_public_rate_limit(
            roadmap_router(state.clone()),
            prl.clone(),
        ))
        .merge(admin_roadmap_router(state.clone()))
        .merge(admin_tier_router(state.clone()))
        // Operator-only tier + brand-override mutation (DEC-FBR-IMPL-11). No
        // CORS layer (called server-side / via curl, never a browser embed);
        // guarded by the OpsAuth bearer token (404 when token unset).
        .merge(ops_router(state.clone()))
        .merge(me_feedback_router(state.clone()))
        // Phase A A1/A5: erasure + export on the me_feedback path — merged
        // WITHOUT CORS, same posture as the read subtree above (JWT end-user
        // surface driven by the consumer's own client, not a browser embed).
        .merge(me_feedback_data_router(me_feedback_data_state))
        // GitCellar in-app solicitation (FR-FBR-28/27): durable per-user
        // solicitation state (JWT end-user surface; merged WITHOUT CORS, like
        // me_feedback — driven by the consumer's own client) + public
        // capability discovery (`GET /api/v1/capabilities`, metadata-only).
        .merge(solicitation_router(state.clone()))
        .merge(capabilities_router(state.clone()))
        .merge(apply_public_rate_limit(
            attachments_router(attachment_state).layer(cors.clone()),
            prl.clone(),
        ))
        // P5a (Contract C22, Worker A): work-order API + approval state machine.
        // Admin routes behind AdminSession; runner routes behind the runner
        // write-token seam (Q14). BOTH merge WITHOUT `.layer(cors)` — these are
        // admin + server-to-server surfaces, never browser embeds. CORS stays
        // ONLY on the public submit/attachments routers above (Ripple Analysis
        // flags accidental CORS-exposure of admin/runner endpoints).
        .merge(work_order_admin_router(state.clone()))
        .merge(work_order_runner_router(state.clone()))
        // P5b (Contract C25, Worker D consumes): runner-token lifecycle
        // (list/register/revoke) behind AdminSession. Merged WITHOUT
        // `.layer(cors)` — admin surface, never a browser embed. Only the
        // public submit/attachments routers above carry CORS (Ripple Analysis
        // flags accidental CORS-exposure of admin endpoints).
        .merge(runner_tokens_admin_router(state.clone()))
        // P5a (Contract C23/C24, Worker B): clustering/sweep/recommendation
        // admin surface (merge/split, sweep trigger+digest, recommendation
        // ingestion + read). AdminSession; merged WITHOUT `.layer(cors)` — admin
        // surface, never a browser embed. Clustering-on-submit adds NO new
        // external route (it hooks the existing public submit handler).
        .merge(cluster_admin_router(state.clone()))
        .merge(recommendation_admin_router(state.clone()))
        .merge(sweep_admin_router(state.clone()))
        // Public Feedback Board + Moderation Gate (Contracts C28/C29):
        //   board_router is the PUBLIC approved-only board read — merged WITH
        //   `.layer(cors)`, matching the submit/attachments public surface
        //   (`cors-allowlist-enforcement`). moderation_router is the admin
        //   moderate + queue + board-settings surface — merged WITHOUT CORS
        //   (AdminSession, never a browser embed; Ripple Analysis flags
        //   accidental CORS-exposure of admin endpoints).
        .merge(apply_public_rate_limit(
            board_router(state.clone()).layer(cors.clone()),
            prl.clone(),
        ))
        .merge(moderation_router(state.clone()))
        .merge(promote_router(state));
    app.layer(PropagateRequestIdLayer::x_request_id())
        .layer(trace_layer)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}
