//! The swappable agent command — the BYO-agent seam (Q20) AND the test-injection
//! seam (Testability Gate mitigation), one abstraction.
//!
//! The runner drives a [`AgentCommand`]; the default production impl (Worker B,
//! Stage 1) spawns the owner's `claude` + ULDF against the repo, while a BYO
//! impl runs any configured command. Unit tests inject a fake so the
//! prompt-assembly + report-capture logic is exercised hermetically — the real
//! `claude` spawn runs only in a manual/`--full` e2e dry-run, never in unit
//! tests (the RCE-grade surface stays out of the inner loop).

use async_trait::async_trait;

use crate::types::{AssembledPrompt, ImplementResult, RepoContext};

/// Run an agent against a repository with an assembled (envelope-disciplined)
/// prompt, returning a conclusions-only [`ImplementResult`].
#[async_trait]
pub trait AgentCommand: Send + Sync {
    async fn run(
        &self,
        prompt: AssembledPrompt,
        repo: &RepoContext,
    ) -> anyhow::Result<ImplementResult>;
}

/// A deterministic fake agent for tests + the end-to-end dispatch→report
/// dry-run (Stage 1 GATE 1). It performs NO real work and spawns NO process —
/// it returns a canned, already-sanitized [`ImplementResult`]. This is the
/// injection point that keeps the real `claude` spawn out of unit tests.
#[derive(Debug, Clone, Default)]
pub struct StubAgent {
    pub result: Option<ImplementResult>,
}

impl StubAgent {
    #[must_use]
    pub fn with_result(result: ImplementResult) -> Self {
        Self { result: Some(result) }
    }
}

#[async_trait]
impl AgentCommand for StubAgent {
    async fn run(
        &self,
        _prompt: AssembledPrompt,
        _repo: &RepoContext,
    ) -> anyhow::Result<ImplementResult> {
        self.result
            .clone()
            .ok_or_else(|| anyhow::anyhow!("StubAgent has no canned result configured"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DiffStat, ResultRef, Verification};

    #[tokio::test]
    async fn stub_agent_returns_canned_result() {
        let result = ImplementResult {
            result_ref: ResultRef {
                pr_url: None,
                branch: Some("fbm/x".into()),
                commit: None,
                diff_stat: DiffStat { files: 1, insertions: 1, deletions: 0 },
                verification: Verification { tests_passed: true, finalize_status: "passed".into() },
                summary: "ok".into(),
            },
        };
        let agent = StubAgent::with_result(result.clone());
        let prompt = AssembledPrompt {
            instructions: "i".into(),
            untrusted_envelope: "e".into(),
        };
        let repo = RepoContext { repo_path: ".".into() };
        let got = agent.run(prompt, &repo).await.unwrap();
        assert_eq!(got, result);
    }
}
