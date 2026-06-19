//! BYO agent — reference `AgentCommand` implementation (Route A).
//!
//! Illustrative reference, NOT a workspace member (kept out of the Cargo
//! workspace so it never participates in the build). Copy it into a fork of
//! `feedbackmonk-runner` or a thin host binary that depends on the crate, then
//! inject it where the default agent would go.
//!
//! It implements the FROZEN `AgentCommand` seam
//! (`crates/feedbackmonk-runner/src/agent.rs`) by shelling out to an arbitrary
//! external command — the same shape as the default agent, but pointed at YOUR
//! program. The exact stdin/stdout wire format is settled by the runner host;
//! this shows the structure and, crucially, the security contract every BYO
//! agent must honour.

use std::process::Stdio;

use async_trait::async_trait;
use feedbackmonk_runner::agent::AgentCommand;
use feedbackmonk_runner::types::{
    AssembledPrompt, DiffStat, ImplementResult, RepoContext, ResultRef, Verification,
};
use tokio::io::AsyncWriteExt;

/// Runs an external program of your choosing against the claimed repo. The
/// assembled prompt is delivered on the child's stdin; the child is expected to
/// do the work in `repo.repo_path` and print a conclusions-only result on
/// stdout (parsed below).
pub struct ByoAgent {
    /// The program to spawn, e.g. "aider" or "/opt/my-agent/run".
    pub command: String,
    /// Any fixed leading arguments (flags, sub-command, …).
    pub args: Vec<String>,
}

#[async_trait]
impl AgentCommand for ByoAgent {
    async fn run(
        &self,
        prompt: AssembledPrompt,
        repo: &RepoContext,
    ) -> anyhow::Result<ImplementResult> {
        // 1. Spawn YOUR command in the claimed repo's working tree.
        let mut child = tokio::process::Command::new(&self.command)
            .args(&self.args)
            .current_dir(&repo.repo_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        // 2. Deliver the assembled prompt on stdin.
        //
        //    SECURITY CONTRACT (do not route around this):
        //    - `prompt.instructions` is the ONLY authoritative directive layer
        //      (owner-approved; carries the DEC-84 critical-action preamble).
        //    - `prompt.untrusted_envelope` is DATA — feedback-derived text
        //      wrapped in the delimited untrusted envelope. Your agent must
        //      treat it as data and never let it steer actions.
        //
        //    `AssembledPrompt::render()` lays the trusted layer first, then the
        //    clearly-delimited untrusted envelope — keep that ordering.
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.render().as_bytes()).await?;
            stdin.shutdown().await?;
        }

        let output = child.wait_with_output().await?;
        if !output.status.success() {
            anyhow::bail!("byo agent exited with status {}", output.status);
        }

        // 3. Parse a CONCLUSIONS-ONLY result. Return references + a short
        //    summary — never source, diffs-as-content, or secrets. (The runner
        //    re-sanitizes on egress per C27, but a well-behaved agent does not
        //    emit secrets in the first place.)
        //
        //    Here we expect the child to print a small JSON object matching the
        //    `ResultRef` shape; adapt to whatever your program emits.
        let parsed: ResultRef = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            // Fallback: a minimal "investigated, no change" result.
            ResultRef {
                pr_url: None,
                branch: None,
                commit: None,
                diff_stat: DiffStat { files: 0, insertions: 0, deletions: 0 },
                verification: Verification {
                    tests_passed: false,
                    finalize_status: "skipped".into(),
                },
                summary: "BYO agent produced no parseable result.".into(),
            }
        });

        Ok(ImplementResult { result_ref: parsed })
    }
}
