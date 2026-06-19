//! Analyst runtime — the customer-side scheduled deep-read (FR-FBR-20, C26 —
//! P5b; CLAUDE-C).
//!
//! P5a shipped the recommendation *ingestion seam* + the deterministic
//! clustering heuristic; it deferred the **deep read** ("P5a provides the seam;
//! the deep read is P5b"). This module is that deep read, hosted in the runner
//! and triggered by `feedbackmonk-runner poll --sweep` (Worker B wires the CLI).
//!
//! ## Shape
//!
//! The runner host hands the analyst a `Vec<`[`ClusterInput`]`]` (the frozen
//! [`WorkOrderClient`](crate::WorkOrderClient) has no cluster-read method, so
//! the host fetches the cluster data and passes it in — the B↔C seam, collab
//! MSG-C01 Q3). For each actionable cluster [`deep_read`](deep_read::deep_read)
//! produces code-grounded, **recommend-only** [`CandidateRecommendation`]s, and
//! [`ingest`](ingest::ingest) routes each through the egress sanitizer
//! ([`sanitize_outbound`](crate::sanitizer::sanitize_outbound)) before POSTing
//! it via the frozen [`post_recommendation`](crate::WorkOrderClient::post_recommendation)
//! seam.
//!
//! ## Safety floor (why this is low-risk despite reading public feedback)
//!
//! - **Recommend-only.** Output is advisory. A wrong or manufactured
//!   recommendation **cannot execute** — it requires a downstream owner
//!   `approved` event (the approval gate, C22). The analyst can only *propose*.
//! - **Deterministic floor is always-on.** Every actionable cluster gets a
//!   deterministic baseline candidate ([`from_feedback_kind`](feedbackmonk_core::ActionType::from_feedback_kind));
//!   an optional injectable agent ([`AnalystAgent`]) *augments*, never replaces,
//!   and its failure is non-fatal (the floor stands).
//! - **`source_refs` are references, never dumps** (C24 case f). The floor emits
//!   `[]`; the agent emits file/line pointers; the egress sanitizer rejects any
//!   content dump.
//! - **One egress chokepoint.** Every outbound payload passes
//!   `sanitize_outbound` — the second outbound path `feedback-as-data-audit`
//!   Probe B asserts (the first is the implementer's `result_ref`).

pub mod deep_read;
pub mod ingest;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{ActionType, FeedbackKind};

use crate::types::RepoContext;

pub use deep_read::{deep_read, deterministic_candidate, AnalystAgent, StubAnalystAgent};
pub use ingest::{ingest, RecommendationSink};

/// One actionable cluster handed to the analyst by the runner host.
///
/// The frozen [`WorkOrderClient`](crate::WorkOrderClient) exposes no cluster
/// read, so the host fetches these (admin read or a future runner read endpoint)
/// and passes them to [`sweep`]. It also deserializes from a `--clusters <file>`
/// for the GATE-1 dry-run (CLAUDE-B), hence `Deserialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterInput {
    /// The cluster the recommendation anchors to (drives the ingestion URL).
    pub cluster_id: Uuid,
    /// The sweep this read belongs to (provenance), if any.
    pub sweep_id: Option<Uuid>,
    /// The cluster's short label (used for the recommendation title).
    pub label: String,
    /// The cluster summary, if one has been generated.
    pub summary: Option<String>,
    /// The cluster's feedback taxonomy (drives the deterministic action type).
    pub kind: FeedbackKind,
    /// Verbatim member feedback bodies — untrusted, public-reported text. Used
    /// only as grounding for the deep read; never dumped into `source_refs`.
    pub member_bodies: Vec<String>,
}

/// Tally of one analyst sweep. Counts only (Eq-friendly for assertions); a
/// resilient sweep never aborts — individual ingest failures are counted, not
/// propagated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepTally {
    /// Clusters examined.
    pub clusters_seen: usize,
    /// Recommendations successfully sanitized + posted.
    pub proposed: usize,
    /// Clusters that produced no candidate (e.g. unclassified → `NoAction`).
    pub skipped: usize,
    /// Candidates that failed the egress sanitizer or the transport.
    pub failed: usize,
}

/// Run the analyst over `clusters`: deep-read each, then sanitize + ingest every
/// candidate (recommend-only). Resilient — a per-candidate sanitizer/transport
/// failure is tallied in [`SweepTally::failed`] and the sweep continues.
///
/// `sink` is the egress; production passes a `&WorkOrderClient` (which implements
/// [`RecommendationSink`]); tests inject a recorder. `agent` is the optional
/// injectable deep-read; `None` runs the deterministic floor alone.
pub async fn sweep(
    sink: &dyn RecommendationSink,
    agent: Option<&dyn AnalystAgent>,
    clusters: &[ClusterInput],
    repo: &RepoContext,
) -> SweepTally {
    let mut tally = SweepTally::default();
    for cluster in clusters {
        tally.clusters_seen += 1;
        let candidates = deep_read::deep_read(cluster, repo, agent).await;
        if candidates.is_empty() {
            tally.skipped += 1;
            continue;
        }
        for candidate in &candidates {
            match ingest::ingest(sink, cluster, candidate).await {
                Ok(()) => tally.proposed += 1,
                Err(_) => tally.failed += 1,
            }
        }
    }
    tally
}

/// A code-grounded, recommend-only candidate the deep read proposes for a
/// cluster. Mirrors the API `IngestRequest` body shape; carries no `cluster_id`
/// (that comes from the [`ClusterInput`] at ingestion). `f64` confidence → no
/// `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateRecommendation {
    /// The action class (`bug_fix` | `feature_implementation` | `enhancement` |
    /// `investigation` | `no_action`). Serialises `snake_case`.
    pub action_type: ActionType,
    /// Short recommendation title (≤ 512 chars — matches the API cap).
    pub title: String,
    /// Recommendation prose (1..=16384 chars). Feedback-derived DATA; the egress
    /// sanitizer PII-scrubs it before the wire.
    pub body: String,
    /// Optional rationale (why this action).
    pub rationale: Option<String>,
    /// Grounding references — file/line/doc POINTERS, never file contents
    /// (C24 f). The floor emits `[]`.
    pub source_refs: serde_json::Value,
    /// Analyst confidence in `[0.0, 1.0]`.
    pub confidence: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    // The recording sink lives in `ingest` (the one file alongside
    // `sanitize_outbound`) so this module never mentions the egress method name —
    // keeps `feedback-as-data-audit` Probe B's per-file assertion satisfied.
    use super::ingest::RecordingSink;

    fn cluster(kind: FeedbackKind, label: &str) -> ClusterInput {
        ClusterInput {
            cluster_id: Uuid::new_v4(),
            sweep_id: Some(Uuid::new_v4()),
            label: label.to_string(),
            summary: Some("many users affected".to_string()),
            kind,
            member_bodies: vec!["it crashes on login".into(), "same here".into()],
        }
    }

    fn repo() -> RepoContext {
        RepoContext { repo_path: ".".into() }
    }

    #[tokio::test]
    async fn sweep_proposes_for_actionable_clusters_and_skips_no_action() {
        let sink = RecordingSink::default();
        let clusters = vec![
            cluster(FeedbackKind::Bug, "Login broken"),
            cluster(FeedbackKind::Feature, "Dark mode"),
            cluster(FeedbackKind::Other, "Misc chatter"), // → NoAction → skipped
        ];

        let tally = sweep(&sink, None, &clusters, &repo()).await;

        assert_eq!(tally.clusters_seen, 3);
        assert_eq!(tally.proposed, 2, "two actionable clusters → two recommendations");
        assert_eq!(tally.skipped, 1, "the Other/NoAction cluster is skipped");
        assert_eq!(tally.failed, 0);
        assert_eq!(sink.posted.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn swept_payloads_are_recommend_only_and_carry_cluster_id() {
        let sink = RecordingSink::default();
        let c = cluster(FeedbackKind::Bug, "Login broken");
        let cluster_id = c.cluster_id;

        let tally = sweep(&sink, None, std::slice::from_ref(&c), &repo()).await;
        assert_eq!(tally.proposed, 1);

        let posted = sink.posted.lock().unwrap();
        let p = &posted[0];
        // Cluster routing field is present for B's transport URL (MSG-C01 Q1).
        assert_eq!(p["cluster_id"], serde_json::json!(cluster_id));
        // Deterministic floor: actionable action type, empty source_refs (no dumps).
        assert_eq!(p["action_type"], "bug_fix");
        assert_eq!(p["source_refs"], serde_json::json!([]));
        // Recommend-only: a confidence in range, never an execution directive.
        let conf = p["confidence"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&conf));
    }

    #[tokio::test]
    async fn agent_augments_the_floor_without_replacing_it() {
        let sink = RecordingSink::default();
        let extra = CandidateRecommendation {
            action_type: ActionType::Enhancement,
            title: "Refine the login retry".into(),
            body: "Code-grounded deep-read candidate.".into(),
            rationale: Some("agent inspected src/auth.rs".into()),
            source_refs: serde_json::json!(["src/auth.rs:10-20"]),
            confidence: 0.7,
        };
        let agent = StubAnalystAgent::with_candidates(vec![extra]);
        let c = cluster(FeedbackKind::Bug, "Login broken");

        let tally = sweep(&sink, Some(&agent), std::slice::from_ref(&c), &repo()).await;
        // Floor candidate + the agent's candidate both posted (augments, not replaces).
        assert_eq!(tally.proposed, 2);
    }

    #[tokio::test]
    async fn agent_failure_keeps_the_deterministic_floor() {
        let sink = RecordingSink::default();
        let agent = StubAnalystAgent::failing();
        let c = cluster(FeedbackKind::Feature, "Dark mode");

        let tally = sweep(&sink, Some(&agent), std::slice::from_ref(&c), &repo()).await;
        // Agent erred, but the floor still produced + posted one recommendation.
        assert_eq!(tally.proposed, 1);
        assert_eq!(tally.failed, 0);
    }
}
