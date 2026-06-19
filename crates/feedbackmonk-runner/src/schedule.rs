//! `--watch` tick scheduling + the `--sweep` analyst trigger seam (C26).
//!
//! The runner is poll-based (cron/systemd/CI-portable). `--watch` keeps one
//! process alive, running a poll tick every [`DEFAULT_WATCH_INTERVAL`]. `--sweep`
//! additionally runs the scheduled analyst deep-read once per tick.
//!
//! The analyst is Worker C's surface (`analyst::sweep`). To keep this module
//! decoupled from C's in-flight code (same-branch, parallel), the sweep is
//! injected as a [`SweepHook`] trait object rather than a direct call — `main.rs`
//! supplies the concrete hook that bridges to `analyst::sweep` once it lands.

use std::time::Duration;

use async_trait::async_trait;

use crate::agent::AgentCommand;
use crate::client::WorkOrderClient;
use crate::poll::{self, TickSummary};
use crate::types::RepoContext;

/// Default interval between poll ticks under `--watch`.
pub const DEFAULT_WATCH_INTERVAL: Duration = Duration::from_secs(30);

/// The analyst sweep, injected so this module does not depend on Worker C's
/// `analyst` module directly. `main.rs` wires the concrete bridge.
#[async_trait]
pub trait SweepHook: Send + Sync {
    /// Run one scheduled analyst deep-read sweep (recommend-only; egress through
    /// the sanitizer). Errors are logged by the caller, not fatal to the loop.
    ///
    /// # Errors
    /// Propagates the analyst's own failure (e.g. transport, deep-read error).
    async fn sweep(&self, client: &WorkOrderClient) -> anyhow::Result<()>;
}

/// Run ONE tick: a poll pass, then (if configured) the analyst sweep. A sweep
/// failure is logged and swallowed so it never aborts the implementer loop.
///
/// # Errors
/// Only a poll failure propagates (mirrors [`poll::run_once`]).
pub async fn run_tick(
    client: &WorkOrderClient,
    agent: &dyn AgentCommand,
    repo: &RepoContext,
    sweep: Option<&dyn SweepHook>,
) -> anyhow::Result<TickSummary> {
    let summary = poll::run_once(client, agent, repo).await?;
    if let Some(hook) = sweep {
        if let Err(e) = hook.sweep(client).await {
            tracing::warn!(target: "runner", error = %e, "analyst sweep failed (loop continues)");
        }
    }
    Ok(summary)
}

/// `--watch`: run a tick every `interval`. `max_ticks` bounds the loop (`None` =
/// run forever; tests + single-shot pass `Some(n)`).
///
/// # Errors
/// Propagates the first poll failure (transport down etc.); the caller decides
/// whether to exit or retry.
pub async fn watch_loop(
    client: &WorkOrderClient,
    agent: &dyn AgentCommand,
    repo: &RepoContext,
    interval: Duration,
    sweep: Option<&dyn SweepHook>,
    max_ticks: Option<usize>,
) -> anyhow::Result<()> {
    let mut ticks = 0usize;
    loop {
        let summary = run_tick(client, agent, repo, sweep).await?;
        tracing::info!(
            target: "runner",
            polled = summary.polled,
            reported = summary.reported,
            failed = summary.failed,
            "poll tick complete"
        );
        ticks += 1;
        if let Some(limit) = max_ticks {
            if ticks >= limit {
                return Ok(());
            }
        }
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::StubAgent;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct CountingSweep(Arc<AtomicUsize>);

    #[async_trait]
    impl SweepHook for CountingSweep {
        async fn sweep(&self, _client: &WorkOrderClient) -> anyhow::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    async fn empty_poll_server(pid: Uuid) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/projects/{pid}/runner/work-orders")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "items": [] })),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn watch_loop_runs_bounded_ticks_and_sweeps_each() {
        let pid = Uuid::new_v4();
        let server = empty_poll_server(pid).await;
        let client = WorkOrderClient::new(server.uri(), pid, "t");
        let agent = StubAgent::default();
        let repo = RepoContext { repo_path: ".".into() };
        let counter = Arc::new(AtomicUsize::new(0));
        let sweep = CountingSweep(counter.clone());

        watch_loop(
            &client,
            &agent,
            &repo,
            Duration::from_millis(1),
            Some(&sweep),
            Some(3),
        )
        .await
        .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 3, "one sweep per tick");
    }

    #[tokio::test]
    async fn run_tick_without_sweep_is_poll_only() {
        let pid = Uuid::new_v4();
        let server = empty_poll_server(pid).await;
        let client = WorkOrderClient::new(server.uri(), pid, "t");
        let agent = StubAgent::default();
        let repo = RepoContext { repo_path: ".".into() };
        let summary = run_tick(&client, &agent, &repo, None).await.unwrap();
        assert_eq!(summary.polled, 0);
    }
}
