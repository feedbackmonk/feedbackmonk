//! The default production [`AgentCommand`] — spawns the owner's coding agent
//! (Claude Code + ULDF by default) against the claimed repo (the BYO seam, Q20).
//!
//! **This is the only place a real process is spawned.** Unit tests + the GATE 1
//! dry-run inject [`crate::agent::StubAgent`] instead, so the RCE-grade spawn
//! never runs in the inner loop (Testability Gate mitigation). The spawn here is
//! exercised only by a manual / `--full` e2e run.
//!
//! ## BYO contract (documented in `docs/operations/RUNNER_PROTOCOL.md`)
//!
//! The runner invokes a configured command in the repo working tree, delivers
//! the assembled (envelope-disciplined) prompt on **stdin**, and reads the
//! agent's conclusions-only result from **stdout**: the agent MUST, on success,
//! print a final line
//!
//! ```text
//! FEEDBACKMONK_RESULT_REF: {"branch":"…","diff_stat":{…},"verification":{…},"summary":"…"}
//! ```
//!
//! whose JSON deserializes into [`ResultRef`] (references + conclusions only —
//! the egress sanitizer rejects dumps downstream). Any BYO agent that honours
//! this contract drops in by setting `FEEDBACKMONK_AGENT_CMD`.

use async_trait::async_trait;

use crate::agent::AgentCommand;
use crate::types::{AssembledPrompt, ImplementResult, RepoContext, ResultRef};

/// Env var overriding the spawned command (default `claude`).
pub const AGENT_CMD_ENV: &str = "FEEDBACKMONK_AGENT_CMD";
/// Env var overriding the command args (whitespace-split; default empty).
pub const AGENT_ARGS_ENV: &str = "FEEDBACKMONK_AGENT_ARGS";
/// The sentinel prefixing the agent's machine-readable result line on stdout.
pub const RESULT_REF_SENTINEL: &str = "FEEDBACKMONK_RESULT_REF:";

/// Spawns a coding agent as a subprocess, feeding the prompt on stdin and
/// parsing a [`ResultRef`] from stdout (the BYO seam + the default impl).
#[derive(Debug, Clone)]
pub struct SpawnAgent {
    /// The program to run (default `claude`).
    pub command: String,
    /// Arguments passed to the program.
    pub args: Vec<String>,
}

impl Default for SpawnAgent {
    fn default() -> Self {
        Self::from_env()
    }
}

impl SpawnAgent {
    /// Build from the environment: `FEEDBACKMONK_AGENT_CMD` (default `claude`)
    /// and `FEEDBACKMONK_AGENT_ARGS` (whitespace-split; default none).
    #[must_use]
    pub fn from_env() -> Self {
        let command = std::env::var(AGENT_CMD_ENV).unwrap_or_else(|_| "claude".to_string());
        let args = std::env::var(AGENT_ARGS_ENV)
            .ok()
            .map(|s| s.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        Self { command, args }
    }
}

#[async_trait]
impl AgentCommand for SpawnAgent {
    async fn run(
        &self,
        prompt: AssembledPrompt,
        repo: &RepoContext,
    ) -> anyhow::Result<ImplementResult> {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        let mut child = Command::new(&self.command)
            .args(&self.args)
            .current_dir(&repo.repo_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn agent `{}`: {e}", self.command))?;

        // Deliver the assembled prompt on stdin (the untrusted envelope is part
        // of `render()` — the agent never sees feedback text outside it).
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.render().as_bytes()).await?;
            stdin.shutdown().await?;
        }

        let output = child.wait_with_output().await?;
        if !output.status.success() {
            anyhow::bail!(
                "agent `{}` exited with status {}",
                self.command,
                output.status
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let result_ref = parse_result_ref(&stdout)?;
        Ok(ImplementResult { result_ref })
    }
}

/// Extract the [`ResultRef`] from the agent's stdout: the LAST line beginning
/// with [`RESULT_REF_SENTINEL`], whose remainder parses as `ResultRef`. Pure +
/// testable (the spawn itself is exercised only in e2e).
///
/// # Errors
/// No sentinel line present, or the JSON after it does not deserialize into a
/// `ResultRef`.
pub fn parse_result_ref(stdout: &str) -> anyhow::Result<ResultRef> {
    let line = stdout
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix(RESULT_REF_SENTINEL))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "agent did not emit a `{RESULT_REF_SENTINEL}` result line on stdout"
            )
        })?;
    serde_json::from_str(line.trim()).map_err(|e| {
        anyhow::anyhow!("agent `{RESULT_REF_SENTINEL}` line is not a valid ResultRef: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_result_ref_from_sentinel_line() {
        let stdout = "\
            some build chatter\n\
            more logs\n\
            FEEDBACKMONK_RESULT_REF: {\"branch\":\"fbm/wo-1\",\"diff_stat\":{\"files\":2,\"insertions\":10,\"deletions\":1},\"verification\":{\"tests_passed\":true,\"finalize_status\":\"passed\"},\"summary\":\"fixed it\"}\n\
            trailing line\n";
        let r = parse_result_ref(stdout).unwrap();
        assert_eq!(r.branch.as_deref(), Some("fbm/wo-1"));
        assert_eq!(r.diff_stat.files, 2);
        assert!(r.verification.tests_passed);
        assert_eq!(r.summary, "fixed it");
        assert!(r.pr_url.is_none());
    }

    #[test]
    fn last_sentinel_line_wins() {
        let stdout = "\
            FEEDBACKMONK_RESULT_REF: {\"diff_stat\":{\"files\":0,\"insertions\":0,\"deletions\":0},\"verification\":{\"tests_passed\":false,\"finalize_status\":\"skipped\"},\"summary\":\"first\"}\n\
            FEEDBACKMONK_RESULT_REF: {\"diff_stat\":{\"files\":1,\"insertions\":1,\"deletions\":0},\"verification\":{\"tests_passed\":true,\"finalize_status\":\"passed\"},\"summary\":\"final\"}\n";
        let r = parse_result_ref(stdout).unwrap();
        assert_eq!(r.summary, "final");
    }

    #[test]
    fn missing_sentinel_is_an_error() {
        let err = parse_result_ref("just some logs, no result").unwrap_err();
        assert!(err.to_string().contains(RESULT_REF_SENTINEL));
    }

    #[test]
    fn malformed_result_json_is_an_error() {
        let err = parse_result_ref("FEEDBACKMONK_RESULT_REF: {not json}").unwrap_err();
        assert!(err.to_string().contains("ResultRef"));
    }

    #[test]
    fn from_env_defaults_to_claude() {
        // Note: relies on the env var being unset in the test process.
        let agent = SpawnAgent {
            command: "claude".into(),
            args: vec![],
        };
        assert_eq!(agent.command, "claude");
    }
}
