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

use feedbackmonk_anon::{AnonGate, DEFAULT_RATE_LIMIT_PER_HOUR};
use feedbackmonk_jwt::DEFAULT_IAT_LEEWAY_SECONDS;
use feedbackmonk_repository::{
    SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
    SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxRoadmapItemRepo,
    SqlxRoadmapVoteRepo, SqlxSigningKeyRepo, SqlxTenantRepo, SqlxTierQuotaRepo,
};

use feedbackmonk_api::email::{
    EmailNotifier, EnvSmtpConfig, EnvSmtpMailer, LettreEmailNotifier, Mailer, MailpitMailer,
};
use feedbackmonk_api::router::router as worker_a_router;
use feedbackmonk_api::state::AppState;
use feedbackmonk_api::{
    admin_feedback_routes, admin_roadmap_router, admin_tier_router, promote_router,
    roadmap_router, spawn_voting_cache_refresh, submission_router, widget_config_router,
    VotingCache,
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

    // P2: spawn the 60s roadmap voting-cache refresh tick. JoinHandle is
    // intentionally not held — process exit aborts the task. The cache
    // tolerates per-project refresh failures internally (logs WARN, keeps
    // prior payload).
    let _voting_cache_tick = spawn_voting_cache_refresh(
        state.voting_cache.clone(),
        Arc::clone(&state.projects),
        Arc::clone(&state.roadmap_items),
    );

    let app = build_app(state);

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
    let tier_quotas = Arc::new(SqlxTierQuotaRepo::new(pool.clone()));
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

    let jwt_iat_leeway_seconds: i64 = env::var("FEEDBACKMONK_JWT_LEEWAY_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_IAT_LEEWAY_SECONDS);

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
        jwt_iat_leeway_seconds,
        roadmap_items,
        roadmap_votes,
        voting_cache,
        started_at: Utc::now(),
        health,
        tier_quotas,
    })
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
                    .map(|s| s != "false")
                    .unwrap_or(true),
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
                .map(|s| s != "false")
                .unwrap_or(true);
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

fn build_app(state: AppState) -> Router {
    // FR-FBR-18: every request is wrapped in a span carrying a `request_id`
    // (UUIDv4) populated from `x-request-id` if the client supplied one, else
    // freshly generated. The TraceLayer emits structured INFO logs at request
    // start and response end with method/uri/status; downstream handler logs
    // automatically inherit the span's `request_id` field.
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().include_headers(false))
        .on_request(DefaultOnRequest::new())
        .on_response(DefaultOnResponse::new());

    let app = worker_a_router(state.clone())
        .merge(submission_router(state.clone()))
        .merge(admin_feedback_routes(state.clone()))
        .merge(widget_config_router(state.clone()))
        .merge(roadmap_router(state.clone()))
        .merge(admin_roadmap_router(state.clone()))
        .merge(admin_tier_router(state.clone()))
        .merge(promote_router(state));
    app.layer(PropagateRequestIdLayer::x_request_id())
        .layer(trace_layer)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}
