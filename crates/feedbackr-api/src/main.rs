//! Feedbackr API binary.
//!
//! Boot sequence:
//! 1. Load env (via parent process; we do NOT read .env here -- ops layer
//!    handles env injection in containers/dev shells).
//! 2. Connect Postgres.
//! 3. Construct repository handles + mailer (env-selected: Mailpit dev or SMTP prod).
//! 4. Build `AppState`.
//! 5. Compose Worker A router (+ Worker B's router when they merge it in).
//! 6. Bind `FEEDBACKR_PORT` and serve.
//!
//! Worker B merges their submission router by extending `build_state` +
//! `build_app` here -- coordinate via `channels/messages.md`.

use std::env;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use chrono::{Duration, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};

use feedbackr_anon::{AnonGate, DEFAULT_RATE_LIMIT_PER_HOUR};
use feedbackr_jwt::DEFAULT_IAT_LEEWAY_SECONDS;
use feedbackr_repository::{
    SqlxEmailVerificationRepo, SqlxFeedbackRepo, SqlxHealthCheck, SqlxProjectRepo,
    SqlxSigningKeyRepo, SqlxTenantRepo,
};

use feedbackr_api::email::{EnvSmtpConfig, EnvSmtpMailer, Mailer, MailpitMailer};
use feedbackr_api::router::router as worker_a_router;
use feedbackr_api::state::AppState;
use feedbackr_api::submission_router;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let port: u16 = env::var("FEEDBACKR_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(14304);

    let pool = connect_pg().await?;
    let state = build_state(pool)?;
    let app = build_app(state);

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "feedbackr-api listening");
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

/// Initialise tracing per FR-FBR-18. `FEEDBACKR_LOG_FORMAT=json` (default in
/// prod-style deployments) emits structured JSON suitable for log aggregators;
/// `FEEDBACKR_LOG_FORMAT=text` is the human-friendly dev format. Log level is
/// controlled by `RUST_LOG` (default `info`).
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt = std::env::var("FEEDBACKR_LOG_FORMAT").unwrap_or_else(|_| "json".to_string());
    match fmt.as_str() {
        "text" => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .init();
        }
        _ => {
            // JSON is the default — structured for log aggregators per
            // FR-FBR-18. Includes the request_id span field automatically
            // when the request lives inside the TraceLayer's span (below).
            tracing_subscriber::fmt()
                .json()
                .with_current_span(true)
                .with_span_list(false)
                .with_env_filter(filter)
                .init();
        }
    }
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
    let email_verifications = Arc::new(SqlxEmailVerificationRepo::new(pool.clone()));
    let health = SqlxHealthCheck::new(pool.clone());

    let mailer = build_mailer()?;
    let session_secret = load_session_secret()?;
    let public_url = env::var("FEEDBACKR_PUBLIC_URL")
        .unwrap_or_else(|_| "http://localhost:14304".to_string());

    let ttl_hours: i64 = env::var("FEEDBACKR_VERIFY_TOKEN_TTL_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);

    let anon_quota: u32 = env::var("FEEDBACKR_ANON_RATE_LIMIT_PER_HOUR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_RATE_LIMIT_PER_HOUR);
    let anon_quota = NonZeroU32::new(anon_quota)
        .context("FEEDBACKR_ANON_RATE_LIMIT_PER_HOUR must be > 0")?;
    let anon_gate = AnonGate::new(anon_quota);

    let jwt_iat_leeway_seconds: i64 = env::var("FEEDBACKR_JWT_LEEWAY_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_IAT_LEEWAY_SECONDS);

    Ok(AppState {
        pool,
        tenants,
        projects,
        signing_keys,
        feedback,
        email_verifications,
        mailer,
        session_secret: Arc::new(session_secret),
        public_url: Arc::from(public_url.as_str()),
        verify_token_ttl: Duration::hours(ttl_hours),
        anon_gate,
        jwt_iat_leeway_seconds,
        started_at: Utc::now(),
        health,
    })
}

fn build_mailer() -> Result<Arc<dyn Mailer>> {
    let mode = env::var("FEEDBACKR_MAILER").unwrap_or_else(|_| "mailpit".to_string());
    let from = env::var("FEEDBACKR_SMTP_FROM").unwrap_or_else(|_| "no-reply@feedbackr.local".into());
    match mode.as_str() {
        "mailpit" => {
            let host = env::var("FEEDBACKR_MAILPIT_HOST").unwrap_or_else(|_| "localhost".into());
            let port = env::var("FEEDBACKR_MAILPIT_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1025);
            Ok(Arc::new(MailpitMailer::new(&host, port, &from)?))
        }
        "smtp" => {
            let cfg = EnvSmtpConfig {
                host: env::var("FEEDBACKR_SMTP_HOST").context("FEEDBACKR_SMTP_HOST")?,
                port: env::var("FEEDBACKR_SMTP_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(587),
                user: env::var("FEEDBACKR_SMTP_USER").context("FEEDBACKR_SMTP_USER")?,
                pass: env::var("FEEDBACKR_SMTP_PASS").context("FEEDBACKR_SMTP_PASS")?,
                from,
                starttls: env::var("FEEDBACKR_SMTP_STARTTLS")
                    .map(|s| s != "false")
                    .unwrap_or(true),
            };
            Ok(Arc::new(EnvSmtpMailer::new(cfg)?))
        }
        other => Err(anyhow::anyhow!(
            "FEEDBACKR_MAILER must be 'mailpit' or 'smtp', got {other}"
        )),
    }
}

fn load_session_secret() -> Result<[u8; 32]> {
    let hex_str = env::var("FEEDBACKR_SESSION_SECRET")
        .context("FEEDBACKR_SESSION_SECRET not set (expected 64 hex chars)")?;
    let trimmed = hex_str.trim();
    if trimmed.len() != 64 {
        anyhow::bail!(
            "FEEDBACKR_SESSION_SECRET must be 64 hex chars (32 bytes); got {} chars",
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

    let app = worker_a_router(state.clone()).merge(submission_router(state));
    app.layer(PropagateRequestIdLayer::x_request_id())
        .layer(trace_layer)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}
