//! Deep read: clustered feedback + repo → recommend-only candidate
//! recommendations (FR-FBR-20; CLAUDE-C).
//!
//! Two layers, composed by [`deep_read`]:
//!   - **Deterministic floor** ([`deterministic_candidate`]) — always-on. One
//!     baseline candidate per *actionable* cluster, derived from the cluster's
//!     [`FeedbackKind`] via the canonical [`ActionType::from_feedback_kind`]
//!     mapping. No code grounding (`source_refs: []`), low confidence. This is
//!     the P5a clustering heuristic carried forward as the guaranteed signal.
//!   - **Injectable agent** ([`AnalystAgent`]) — optional. A code-grounded deep
//!     read that *augments* the floor (never replaces it). Mirrors the
//!     [`AgentCommand`](crate::agent::AgentCommand) injection discipline: the
//!     real spawn runs only in a manual/`--full` e2e; unit tests inject
//!     [`StubAnalystAgent`]. Agent failure is non-fatal — the floor stands.
//!
//! Everything here is **recommend-only**: a candidate cannot execute without a
//! downstream owner `approved` event (the approval gate, C22).

use async_trait::async_trait;

use feedbackmonk_core::{ActionType, FeedbackKind};

use super::{CandidateRecommendation, ClusterInput};
use crate::types::RepoContext;

/// Title cap — mirrors the API `recommendations.rs` `TITLE_MAX_CHARS`.
const TITLE_MAX_CHARS: usize = 512;
/// Body cap — mirrors the API `recommendations.rs` `BODY_MAX_CHARS`.
const BODY_MAX_CHARS: usize = 16_384;
/// Confidence of a deterministic baseline candidate: present but low — it is an
/// unverified floor, meant to be refined (by the agent) or reviewed by the owner.
const FLOOR_CONFIDENCE: f64 = 0.3;

/// The injectable deep-read agent (the BYO + test-injection seam, mirroring
/// [`AgentCommand`](crate::agent::AgentCommand)). The production impl spawns the
/// owner's agent against the repo; tests inject [`StubAnalystAgent`]. **No real
/// process spawn happens in unit tests.**
#[async_trait]
pub trait AnalystAgent: Send + Sync {
    /// Deep-read one cluster against the customer repo and return code-grounded,
    /// **recommend-only** candidates (`source_refs` as file/line pointers, never
    /// dumps). Return an empty `Vec` for "nothing to add beyond the floor".
    ///
    /// # Errors
    /// Any failure of the underlying read. Non-fatal to the sweep: [`deep_read`]
    /// keeps the deterministic floor when the agent errs.
    async fn analyze(
        &self,
        cluster: &ClusterInput,
        repo: &RepoContext,
    ) -> anyhow::Result<Vec<CandidateRecommendation>>;
}

/// A deterministic fake [`AnalystAgent`] for tests + the e2e dry-run. Spawns no
/// process — returns canned candidates (or a configured failure). This is the
/// injection point that keeps a real agent spawn out of unit tests.
#[derive(Debug, Clone, Default)]
pub struct StubAnalystAgent {
    /// Candidates the stub returns on success.
    pub candidates: Vec<CandidateRecommendation>,
    /// When set, `analyze` returns an error (to exercise floor-resilience).
    pub fail: bool,
}

impl StubAnalystAgent {
    /// A stub that returns `candidates` on every `analyze`.
    #[must_use]
    pub fn with_candidates(candidates: Vec<CandidateRecommendation>) -> Self {
        Self { candidates, fail: false }
    }

    /// A stub whose `analyze` always errors (to test floor-resilience).
    #[must_use]
    pub fn failing() -> Self {
        Self { candidates: Vec::new(), fail: true }
    }
}

#[async_trait]
impl AnalystAgent for StubAnalystAgent {
    async fn analyze(
        &self,
        _cluster: &ClusterInput,
        _repo: &RepoContext,
    ) -> anyhow::Result<Vec<CandidateRecommendation>> {
        if self.fail {
            anyhow::bail!("StubAnalystAgent configured to fail");
        }
        Ok(self.candidates.clone())
    }
}

/// Deep-read one cluster: the always-on deterministic floor, plus the optional
/// injectable agent's augmentation. Agent failure is non-fatal — the floor
/// stands (recommend-only resilience: a wrong/missing deep read can never lose
/// the baseline signal).
pub async fn deep_read(
    cluster: &ClusterInput,
    repo: &RepoContext,
    agent: Option<&dyn AnalystAgent>,
) -> Vec<CandidateRecommendation> {
    // Floor: the deterministic baseline (skipped for unclassified clusters).
    let mut candidates: Vec<CandidateRecommendation> =
        deterministic_candidate(cluster).into_iter().collect();

    // Augmentation: an injectable agent enriches with code-grounded candidates.
    if let Some(agent) = agent {
        if let Ok(extra) = agent.analyze(cluster, repo).await {
            candidates.extend(extra);
        }
        // An agent error is intentionally swallowed: the floor is the guarantee.
    }

    candidates
}

/// The deterministic baseline candidate for a cluster (the always-on floor).
///
/// Returns `None` for an unclassified cluster (`Other` → `NoAction`): the floor
/// never defaults a cluster into a proposed *executable* action — that mirrors
/// [`ActionType::from_feedback_kind`]'s own inert-default discipline.
#[must_use]
pub fn deterministic_candidate(cluster: &ClusterInput) -> Option<CandidateRecommendation> {
    let action_type = ActionType::from_feedback_kind(cluster.kind);
    if action_type == ActionType::NoAction {
        return None;
    }

    let report_count = cluster.member_bodies.len();
    let title = floor_title(cluster);
    let summary = cluster.summary.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let body = floor_body(&cluster.label, summary, cluster.kind, report_count);
    let rationale = Some(format!(
        "Deterministic clustering floor: {report_count} feedback report(s) grouped as {}.",
        cluster.kind.as_str()
    ));

    Some(CandidateRecommendation {
        action_type,
        title,
        body,
        // References, never dumps (C24 f): the floor grounds nothing in code.
        source_refs: serde_json::json!([]),
        rationale,
        confidence: FLOOR_CONFIDENCE,
    })
}

/// A non-empty, length-capped title (the API requires 1..=512 chars).
fn floor_title(cluster: &ClusterInput) -> String {
    let trimmed = truncate_chars(cluster.label.trim(), TITLE_MAX_CHARS);
    if trimmed.is_empty() {
        format!("Review {} cluster", cluster.kind.as_str())
    } else {
        trimmed
    }
}

/// Recommend-only baseline body. Summarizes the cluster; explicitly advisory.
fn floor_body(label: &str, summary: Option<&str>, kind: FeedbackKind, count: usize) -> String {
    let summary_part = summary.map_or_else(
        || "No cluster summary is available yet.".to_string(),
        |s| format!("Summary: {s}."),
    );
    let body = format!(
        "{count} grouped report(s) describe a {} concern: \"{label}\". {summary_part} \
         This is a deterministic baseline recommendation from the always-on clustering floor — \
         advisory only; review and refine before approval. \
         (Recommend-only: nothing runs without an owner approval.)",
        kind.as_str()
    );
    truncate_chars(&body, BODY_MAX_CHARS)
}

/// Truncate to at most `max` characters (char-safe, not byte-safe).
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn cluster(kind: FeedbackKind, label: &str, summary: Option<&str>) -> ClusterInput {
        ClusterInput {
            cluster_id: Uuid::new_v4(),
            sweep_id: None,
            label: label.to_string(),
            summary: summary.map(str::to_string),
            kind,
            member_bodies: vec!["report one".into(), "report two".into()],
        }
    }

    fn repo() -> RepoContext {
        RepoContext { repo_path: ".".into() }
    }

    #[test]
    fn floor_maps_kind_to_action_and_grounds_nothing() {
        let c = cluster(FeedbackKind::Bug, "Login broken", Some("many users"));
        let cand = deterministic_candidate(&c).expect("bug cluster is actionable");
        assert_eq!(cand.action_type, ActionType::BugFix);
        assert_eq!(cand.title, "Login broken");
        assert_eq!(cand.source_refs, serde_json::json!([]));
        assert!((0.0..=1.0).contains(&cand.confidence));
        assert!(!cand.body.is_empty() && cand.body.chars().count() <= BODY_MAX_CHARS);
    }

    #[test]
    fn floor_skips_unclassified_clusters() {
        let c = cluster(FeedbackKind::Other, "Misc", None);
        assert!(deterministic_candidate(&c).is_none());
    }

    #[test]
    fn floor_title_falls_back_when_label_blank() {
        let c = cluster(FeedbackKind::Feature, "   ", None);
        let cand = deterministic_candidate(&c).unwrap();
        assert!(!cand.title.trim().is_empty());
    }

    #[test]
    fn floor_title_and_body_are_capped() {
        let huge = "x".repeat(TITLE_MAX_CHARS + 50);
        let c = cluster(FeedbackKind::Bug, &huge, None);
        let cand = deterministic_candidate(&c).unwrap();
        assert_eq!(cand.title.chars().count(), TITLE_MAX_CHARS);
        assert!(cand.body.chars().count() <= BODY_MAX_CHARS);
    }

    #[tokio::test]
    async fn deep_read_without_agent_is_just_the_floor() {
        let c = cluster(FeedbackKind::Bug, "Login broken", None);
        let out = deep_read(&c, &repo(), None).await;
        assert_eq!(out.len(), 1);
    }

    #[tokio::test]
    async fn deep_read_appends_agent_candidates() {
        let c = cluster(FeedbackKind::Bug, "Login broken", None);
        let extra = CandidateRecommendation {
            action_type: ActionType::Enhancement,
            title: "Harden retry".into(),
            body: "grounded".into(),
            rationale: None,
            source_refs: serde_json::json!(["src/auth.rs:5-9"]),
            confidence: 0.6,
        };
        let agent = StubAnalystAgent::with_candidates(vec![extra]);
        let out = deep_read(&c, &repo(), Some(&agent)).await;
        assert_eq!(out.len(), 2, "floor + agent candidate");
    }

    #[tokio::test]
    async fn deep_read_survives_agent_failure() {
        let c = cluster(FeedbackKind::Bug, "Login broken", None);
        let agent = StubAnalystAgent::failing();
        let out = deep_read(&c, &repo(), Some(&agent)).await;
        assert_eq!(out.len(), 1, "agent erred → only the floor remains");
    }

    #[tokio::test]
    async fn deep_read_on_unclassified_with_no_agent_is_empty() {
        let c = cluster(FeedbackKind::Other, "Misc", None);
        let out = deep_read(&c, &repo(), None).await;
        assert!(out.is_empty());
    }
}
