//! `feedbackr-tracing` — PII-scrubbing tracing-subscriber chokepoint (FR-FBR-10).
//!
//! `install_global_subscriber` is the SOLE public tracing-subscriber entry
//! point. Every log line emitted by the binary passes through the canonical
//! 20-pattern scrubber (`scrubber.rs`) at the WRITE boundary via a custom
//! `MakeWriter` (`layer.rs`). The `pii-scrub-audit` Verification Oracle
//! enforces this discipline at AST-grade:
//!
//! - **Probe A**: no `tracing_subscriber::fmt(`, `tracing_subscriber::registry(`,
//!   or `impl Layer<...> for ...` outside this crate.
//! - **Probe B**: SHA-256 of `CANONICAL_PATTERNS` matches
//!   `.claude/oracles/pii-scrub-audit/expected_hash.txt`.
//!
//! Three-leg defense (D-FBR-02): (1) this chokepoint, (2) the oracle,
//! (3) clippy + cargo-deny rules in the workspace.

use thiserror::Error;
use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::{SubscriberInitExt, TryInitError},
    EnvFilter,
};

pub mod scrubber;
mod layer;

pub use scrubber::scrub;

/// The test-time scrubbing writer factory. Production code uses the
/// stdout-backed version exclusively (constructed inside
/// `install_global_subscriber`). Tests can install their own subscriber
/// over a `SharedBufferScrubbing` to assert PII is scrubbed.
pub use layer::SharedBufferScrubbing;

/// Log level for `install_global_subscriber`. Maps onto
/// `tracing::Level` for the underlying `LevelFilter`. `RUST_LOG`, if set,
/// overrides this value at process startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for tracing::Level {
    fn from(l: LogLevel) -> Self {
        match l {
            LogLevel::Trace => tracing::Level::TRACE,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Error => tracing::Level::ERROR,
        }
    }
}

/// Log emission format. `Json` is the production default (structured for log
/// aggregators per FR-FBR-18); `Plain` is the human-friendly dev format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    Plain,
    #[default]
    Json,
}

/// Errors from `install_global_subscriber`.
#[derive(Debug, Error)]
pub enum TracingError {
    /// A subscriber was already installed for this process.
    #[error("global tracing subscriber already initialised: {0}")]
    Init(#[from] TryInitError),
    /// `RUST_LOG` was set but the value did not parse as a valid `EnvFilter`
    /// directive.
    #[error("invalid RUST_LOG directive: {0}")]
    Filter(String),
}

/// Install the global tracing subscriber for this process.
///
/// - Filter: if `RUST_LOG` is set, parses it as an `EnvFilter` directive;
///   otherwise applies a `LevelFilter` derived from `level`.
/// - Format: `LogFormat::Json` emits structured JSON with `current_span`
///   metadata (matches the P0 default); `LogFormat::Plain` emits the
///   ANSI-free human-friendly format.
/// - PII scrubbing: every emitted line passes through the canonical
///   20-pattern scrubber via `layer::StdoutScrubbing`. See module docs.
///
/// Idempotency: calling twice in the same process returns
/// `Err(TracingError::Init(_))` on the second call. Tests that need a
/// per-test subscriber should use `SharedBufferScrubbing` + their own
/// registry composition; `install_global_subscriber` is for the binary
/// entrypoint only.
pub fn install_global_subscriber(
    level: LogLevel,
    format: LogFormat,
) -> Result<(), TracingError> {
    let env_filter = match std::env::var("RUST_LOG") {
        Ok(s) if !s.trim().is_empty() => EnvFilter::try_new(&s)
            .map_err(|e| TracingError::Filter(e.to_string()))?,
        _ => EnvFilter::new(format!("{}", tracing::Level::from(level))),
    };

    let writer = layer::StdoutScrubbing;

    let registry = tracing_subscriber::registry().with(env_filter);

    match format {
        LogFormat::Json => registry
            .with(
                fmt::layer()
                    .json()
                    .with_current_span(true)
                    .with_span_list(false)
                    .with_writer(writer),
            )
            .try_init()
            .map_err(TracingError::Init),
        LogFormat::Plain => registry
            .with(fmt::layer().with_ansi(false).with_writer(writer))
            .try_init()
            .map_err(TracingError::Init),
    }
}
