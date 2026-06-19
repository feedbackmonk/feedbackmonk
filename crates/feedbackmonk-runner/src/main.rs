//! `feedbackmonk-runner` CLI entrypoint (Contract C26, FR-FBR-24 — P5b).
//!
//! Poll-based (cron/systemd/CI-portable; `--watch` for a long-running loop), NOT
//! webhook-driven — so it works for the owner's *local* repos with no public
//! endpoint. Subcommands:
//!
//!   feedbackmonk-runner poll [--watch] [--sweep] [--clusters <file>]
//!       drive dispatched orders; --sweep also runs the analyst (Worker C) over
//!       the clusters in <file>; --watch keeps the loop alive between ticks.
//!   feedbackmonk-runner mint-token --key <path> [--sub <label>]
//!       [--project <uuid>] [--ttl-seconds <n>]
//!       customer-side runner-token mint helper (Ed25519 sign; feedbackmonk
//!       never holds the private key — DEC-FBR-04).
//!
//! Auth + connection come from the environment: `FEEDBACKMONK_API_URL`,
//! `FEEDBACKMONK_PROJECT_ID`, `FEEDBACKMONK_RUNNER_TOKEN`. The repo the agent
//! runs in defaults to the CWD (override with `FEEDBACKMONK_REPO_PATH`); the
//! agent command defaults to `claude` (override `FEEDBACKMONK_AGENT_CMD`).

use std::path::Path;
use std::process::ExitCode;

use anyhow::Context;
use async_trait::async_trait;
use uuid::Uuid;

use feedbackmonk_runner::analyst::{self, ClusterInput};
use feedbackmonk_runner::default_agent::SpawnAgent;
use feedbackmonk_runner::schedule::{self, SweepHook, DEFAULT_WATCH_INTERVAL};
use feedbackmonk_runner::token_mint::{self, DEFAULT_TTL_SECONDS};
use feedbackmonk_runner::{RepoContext, WorkOrderClient};

fn usage() {
    eprintln!(
        "feedbackmonk-runner — autonomous implementer + analyst host (FR-FBR-23/24)\n\
         \n\
         USAGE:\n\
         \x20 feedbackmonk-runner poll [--watch] [--sweep] [--clusters <file>]\n\
         \x20 feedbackmonk-runner mint-token --key <path> [--sub <label>] [--project <uuid>] [--ttl-seconds <n>]\n\
         \n\
         poll: build the client from env and drive dispatched orders\n\
         \x20 --watch     keep the loop alive, one tick every 30s\n\
         \x20 --sweep     also run the analyst deep-read (needs --clusters)\n\
         \x20 --clusters  JSON file: [ClusterInput, ...] for the analyst sweep\n\
         \n\
         mint-token: sign a short-TTL runner write-token client-side\n\
         \x20 --key         path to the runner-class Ed25519 private seed (hex/base64)\n\
         \x20 --sub         runner label (default: feedbackmonk-runner)\n\
         \x20 --project     project UUID (default: FEEDBACKMONK_PROJECT_ID)\n\
         \x20 --ttl-seconds token lifetime (default: 86400)\n\
         \n\
         Env: FEEDBACKMONK_API_URL, FEEDBACKMONK_PROJECT_ID, FEEDBACKMONK_RUNNER_TOKEN\n\
         \x20    FEEDBACKMONK_REPO_PATH (default CWD), FEEDBACKMONK_AGENT_CMD (default claude)."
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        usage();
        return ExitCode::from(2);
    };

    match cmd {
        "poll" => block_on(cmd_poll(&args[1..])),
        "mint-token" => report(cmd_mint(&args[1..])),
        "-h" | "--help" | "help" => {
            usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown subcommand: {other}\n");
            usage();
            ExitCode::from(2)
        }
    }
}

// ---------------------------------------------------------------------------
// poll
// ---------------------------------------------------------------------------

async fn cmd_poll(flags: &[String]) -> anyhow::Result<()> {
    let watch = has_flag(flags, "--watch");
    let sweep_on = has_flag(flags, "--sweep");
    let clusters_path = flag_value(flags, "--clusters");

    let client = client_from_env()?;
    let agent = SpawnAgent::from_env();
    let repo = RepoContext {
        repo_path: std::env::var("FEEDBACKMONK_REPO_PATH").unwrap_or_else(|_| ".".to_string()),
    };

    let sweep_hook: Option<Box<dyn SweepHook>> = if sweep_on {
        build_sweep(clusters_path.as_deref(), &repo)?
    } else {
        None
    };
    let sweep_ref: Option<&dyn SweepHook> = sweep_hook.as_deref();

    if watch {
        schedule::watch_loop(
            &client,
            &agent,
            &repo,
            DEFAULT_WATCH_INTERVAL,
            sweep_ref,
            None,
        )
        .await?;
    } else {
        let summary = schedule::run_tick(&client, &agent, &repo, sweep_ref).await?;
        println!(
            "poll tick: polled={} reported={} failed={}",
            summary.polled, summary.reported, summary.failed
        );
    }
    Ok(())
}

/// Bridge `--sweep` to Worker C's `analyst::sweep`. The runner host sources the
/// cluster inputs (the frozen `WorkOrderClient` has no cluster-read — MSG-B01 /
/// MSG-C01 Q3); for the dry-run they come from a `--clusters <file>`.
struct MainSweep {
    clusters: Vec<ClusterInput>,
    repo: RepoContext,
}

#[async_trait]
impl SweepHook for MainSweep {
    async fn sweep(&self, client: &WorkOrderClient) -> anyhow::Result<()> {
        // `None` agent → the deterministic analyst floor (recommend-only); the
        // egress sink is the client (it implements RecommendationSink).
        let tally = analyst::sweep(client, None, &self.clusters, &self.repo).await;
        println!(
            "analyst sweep: clusters_seen={} proposed={} skipped={} failed={}",
            tally.clusters_seen, tally.proposed, tally.skipped, tally.failed
        );
        Ok(())
    }
}

fn build_sweep(
    clusters_path: Option<&str>,
    repo: &RepoContext,
) -> anyhow::Result<Option<Box<dyn SweepHook>>> {
    let Some(path) = clusters_path else {
        eprintln!(
            "--sweep given without --clusters <file>: no runner cluster-read endpoint exists yet \
             (see collab MSG-B01); skipping the analyst sweep this run."
        );
        return Ok(None);
    };
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading clusters file {path}"))?;
    let clusters: Vec<ClusterInput> =
        serde_json::from_str(&raw).with_context(|| format!("parsing clusters file {path}"))?;
    Ok(Some(Box::new(MainSweep {
        clusters,
        repo: repo.clone(),
    })))
}

fn client_from_env() -> anyhow::Result<WorkOrderClient> {
    let base = std::env::var("FEEDBACKMONK_API_URL")
        .context("FEEDBACKMONK_API_URL not set (e.g. https://feedback.example.com)")?;
    let pid = std::env::var("FEEDBACKMONK_PROJECT_ID").context("FEEDBACKMONK_PROJECT_ID not set")?;
    let project_id =
        Uuid::parse_str(&pid).context("FEEDBACKMONK_PROJECT_ID is not a valid UUID")?;
    let token =
        std::env::var("FEEDBACKMONK_RUNNER_TOKEN").context("FEEDBACKMONK_RUNNER_TOKEN not set")?;
    Ok(WorkOrderClient::new(base, project_id, token))
}

// ---------------------------------------------------------------------------
// mint-token
// ---------------------------------------------------------------------------

fn cmd_mint(flags: &[String]) -> anyhow::Result<()> {
    let key = flag_value(flags, "--key").context("mint-token requires --key <path>")?;
    let sub = flag_value(flags, "--sub").unwrap_or_else(|| "feedbackmonk-runner".to_string());
    let project_id = match flag_value(flags, "--project") {
        Some(p) => Uuid::parse_str(&p).context("--project is not a valid UUID")?,
        None => {
            let p = std::env::var("FEEDBACKMONK_PROJECT_ID")
                .context("--project not given and FEEDBACKMONK_PROJECT_ID not set")?;
            Uuid::parse_str(&p).context("FEEDBACKMONK_PROJECT_ID is not a valid UUID")?
        }
    };
    let ttl = match flag_value(flags, "--ttl-seconds") {
        Some(s) => s.parse::<i64>().context("--ttl-seconds must be an integer")?,
        None => DEFAULT_TTL_SECONDS,
    };

    let minted = token_mint::mint_from_key_file(Path::new(&key), &sub, project_id, ttl)?;
    // The token alone goes to stdout so it is capturable:
    //   export FEEDBACKMONK_RUNNER_TOKEN="$(feedbackmonk-runner mint-token --key …)"
    println!("{}", minted.token);
    eprintln!(
        "minted runner token: jti={} exp_unix={} (sub={sub}, aud={project_id}, ttl={ttl}s)",
        minted.jti, minted.expires_at_unix
    );
    eprintln!(
        "register for admin visibility (optional): POST /api/v1/projects/{project_id}/runner-tokens \
         {{\"jti\":\"{}\",\"label\":\"{sub}\"}}",
        minted.jti
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// arg + result helpers
// ---------------------------------------------------------------------------

fn has_flag(flags: &[String], name: &str) -> bool {
    flags.iter().any(|f| f == name)
}

fn flag_value(flags: &[String], name: &str) -> Option<String> {
    flags
        .iter()
        .position(|f| f == name)
        .and_then(|i| flags.get(i + 1))
        .cloned()
}

/// Run an async command body on a fresh runtime, mapping its `Result` to an exit
/// code.
fn block_on<F>(fut: F) -> ExitCode
where
    F: std::future::Future<Output = anyhow::Result<()>>,
{
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: failed to start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    report(rt.block_on(fut))
}

fn report(result: anyhow::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
