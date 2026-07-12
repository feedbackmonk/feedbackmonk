//! The autonomous implementer (FR-FBR-23) — the B→A seam.
//!
//! Worker B's runner loop calls [`implement`] on a claimed order; Worker A
//! fills it. The decomposition (Testability Gate Flag 1 mitigation) is:
//!   (a) **pure prompt-assembly** (`prompt::assemble`, hermetically corpus-tested),
//!   (b) an **injectable [`AgentCommand`]** (mocked in tests; real `claude` only
//!       in a manual/`--full` e2e dry-run),
//!   (c) **pure result-capture** (build the conclusions-only `ResultRef`, run it
//!       through `sanitizer::sanitize_outbound`).
//!
//! Stage-0: the signature is frozen (stub bails) so Worker B's loop compiles
//! against it today; Worker A finalizes the body + the C24 (g) corpus.

use crate::agent::AgentCommand;
use crate::types::{ClaimedOrder, ImplementResult, RepoContext, ResultRef};
use crate::{prompt, sanitizer};

/// Drive the implementer for one claimed order: assemble the envelope-disciplined
/// prompt (`prompt::assemble`), run the injectable `agent`, and capture a
/// sanitized conclusions-only result.
///
/// The Testability-Gate-Flag-1 decomposition (pure / injectable / pure):
///   1. **pure** prompt assembly — `prompt::assemble` (25b single chokepoint);
///   2. **injectable** agent — `agent.run` (`StubAgent` in tests; the real
///      `claude` spawn only in a manual/`--full` e2e dry-run);
///   3. **pure** result-capture — the returned `ResultRef` is run through
///      `sanitizer::sanitize_outbound` (25c references-not-dumps) BEFORE it can
///      reach the report path, so source/secrets never leave even if a steered
///      agent tried to smuggle them into the result.
///
/// # Errors
/// - Propagates any error from the injected `agent`.
/// - Returns the egress error if the captured result carries a source/secret
///   dump (`sanitizer::SanitizeError`, surfaced as `anyhow::Error`).
pub async fn implement(
    agent: &dyn AgentCommand,
    order: &ClaimedOrder,
    repo: &RepoContext,
) -> anyhow::Result<ImplementResult> {
    // 1. Pure assembly: feedback-derived text enters ONLY via the envelope.
    let prompt = prompt::assemble(order);

    // 2. Injectable agent: real work happens behind `AgentCommand` (mocked in
    //    tests; the RCE-grade `claude` spawn stays out of the inner loop).
    let result = agent.run(prompt, repo).await?;

    // 3. Pure capture: the egress chokepoint is the LAST thing that touches the
    //    result before it is returned to the runner loop for reporting.
    let value = serde_json::to_value(&result.result_ref)?;
    let sanitized = sanitizer::sanitize_outbound(&value)?;
    let result_ref: ResultRef = serde_json::from_value(sanitized)?;

    Ok(ImplementResult { result_ref })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::StubAgent;
    use crate::types::{DiffStat, RecommendationContext, Verification};
    use feedbackmonk_core::ActionType;
    use uuid::Uuid;

    fn sample_order() -> ClaimedOrder {
        ClaimedOrder {
            work_order_id: Uuid::nil(),
            project_id: Uuid::nil(),
            action_type: ActionType::BugFix,
            title: "Fix login".into(),
            instructions: "Fix the reported login bug.".into(),
            owner_overrides: None,
            recommendation: Some(RecommendationContext {
                body: "Login fails".into(),
                rationale: None,
                cluster_summary: "Login".into(),
                member_bodies: vec!["it breaks".into()],
                source_refs: serde_json::json!([]),
            }),
        }
    }

    fn result_ref(summary: &str) -> ResultRef {
        ResultRef {
            pr_url: None,
            branch: Some("fbm/wo-1".into()),
            commit: None,
            diff_stat: DiffStat { files: 1, insertions: 1, deletions: 0 },
            verification: Verification { tests_passed: true, finalize_status: "passed".into() },
            summary: summary.into(),
        }
    }

    #[tokio::test]
    async fn implement_returns_sanitized_conclusions() {
        let agent = StubAgent::with_result(ImplementResult {
            result_ref: result_ref("Fixed the null check in src/auth.rs:42."),
        });
        let repo = RepoContext { repo_path: ".".into() };
        let out = implement(&agent, &sample_order(), &repo).await.unwrap();
        assert_eq!(out.result_ref.branch.as_deref(), Some("fbm/wo-1"));
    }

    #[tokio::test]
    async fn implement_blocks_a_steered_secret_leak_at_egress() {
        // A steered agent tries to exfiltrate a secret in the result summary.
        let agent = StubAgent::with_result(ImplementResult {
            result_ref: result_ref("done; AWS_SECRET_ACCESS_KEY=AKIAIOSFODNN7EXAMPLEKEY"),
        });
        let repo = RepoContext { repo_path: ".".into() };
        let err = implement(&agent, &sample_order(), &repo).await.unwrap_err();
        assert!(
            err.to_string().contains("source/secret dump"),
            "egress must reject the leak: {err}"
        );
    }
}
