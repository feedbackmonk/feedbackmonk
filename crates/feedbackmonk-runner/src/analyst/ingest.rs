//! Ingestion: assemble the feedback-derived content → **sanitize (egress
//! chokepoint)** → attach routing → POST via the frozen `post_recommendation`
//! seam (FR-FBR-20, C27 25c; CLAUDE-C).
//!
//! ## The egress chokepoint
//!
//! Every feedback-derived recommendation field passes
//! [`sanitize_outbound`](crate::sanitizer::sanitize_outbound) before the wire —
//! the **second** outbound path the `feedback-as-data-audit` oracle (Probe B)
//! asserts (the first is the implementer's `result_ref`). The sanitizer
//! PII-scrubs, applies the secret denylist, and enforces references-not-dumps; a
//! rejected payload never reaches the network.
//!
//! ## Routing identifiers bypass the content sanitizer (deliberately)
//!
//! `cluster_id` / `sweep_id` are **trusted, host-supplied UUIDs** (from
//! [`ClusterInput`], fetched by the runner host — not feedback text). They are
//! attached AFTER sanitization because the canonical PII scrubber rewrites
//! UUID-shaped strings to `[uuid]`, which would destroy the `cluster_id` the
//! runner-host transport routes the ingestion URL on (collab DISC + MSG-C01 Q1).
//! Only feedback-derived content (`title`/`body`/`rationale`/`source_refs`) goes
//! through the chokepoint — which is exactly what 25c protects.
//!
//! ## The sink seam
//!
//! Egress goes through [`RecommendationSink`], a thin same-crate adapter over the
//! frozen [`WorkOrderClient::post_recommendation`]. Production passes a
//! `&WorkOrderClient`; tests inject a recorder — hermetic, no live HTTP, frozen
//! client untouched.

use async_trait::async_trait;
use serde_json::json;

use super::{CandidateRecommendation, ClusterInput};
use crate::client::WorkOrderClient;
use crate::sanitizer::sanitize_outbound;

/// The analyst's egress sink — abstracts the frozen
/// [`WorkOrderClient::post_recommendation`] so the analyst is hermetically
/// testable (a fake recorder in tests; the real client in production).
#[async_trait]
pub trait RecommendationSink: Send + Sync {
    /// POST one already-sanitized recommendation payload.
    ///
    /// # Errors
    /// A transport/HTTP failure from the underlying client.
    async fn post_recommendation(&self, payload: &serde_json::Value) -> anyhow::Result<()>;
}

#[async_trait]
impl RecommendationSink for WorkOrderClient {
    async fn post_recommendation(&self, payload: &serde_json::Value) -> anyhow::Result<()> {
        WorkOrderClient::post_recommendation(self, payload).await
    }
}

/// Build the **feedback-derived content** of the ingestion payload (the part that
/// MUST be sanitized): the API `IngestRequest` fields except the routing
/// identifiers. Matches `action_type` / `title` / `body` / `rationale` /
/// `source_refs` / `confidence`.
#[must_use]
pub fn build_content_payload(candidate: &CandidateRecommendation) -> serde_json::Value {
    json!({
        "action_type": candidate.action_type,
        "title": candidate.title,
        "body": candidate.body,
        "rationale": candidate.rationale,
        "source_refs": candidate.source_refs,
        "confidence": candidate.confidence,
    })
}

/// Assemble content → **sanitize (egress chokepoint)** → attach routing → POST.
///
/// The runner-host transport reads the top-level `cluster_id` to build the
/// cluster-scoped ingestion URL (collab MSG-C01 Q1, agreed with CLAUDE-B);
/// `IngestRequest` has no `deny_unknown_fields`, so the stray `cluster_id` /
/// `sweep_id` in the body are harmless if echoed.
///
/// # Errors
/// A [`SanitizeError`](crate::sanitizer::SanitizeError) (the content carried a
/// secret/source dump) or a transport error from `sink`.
pub async fn ingest(
    sink: &dyn RecommendationSink,
    cluster: &ClusterInput,
    candidate: &CandidateRecommendation,
) -> anyhow::Result<()> {
    let content = build_content_payload(candidate);
    // THE egress chokepoint — no feedback-derived content crosses the wire
    // un-sanitized.
    let mut payload = sanitize_outbound(&content)
        .map_err(|e| anyhow::anyhow!("egress sanitizer rejected recommendation: {e}"))?;
    // Attach trusted routing identifiers AFTER sanitization (see module docs:
    // the PII scrubber would mangle these UUIDs).
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("cluster_id".to_string(), json!(cluster.cluster_id));
        obj.insert("sweep_id".to_string(), json!(cluster.sweep_id));
    }
    sink.post_recommendation(&payload).await
}

/// A recording egress sink — the hermetic test double for `WorkOrderClient` (no
/// live HTTP). Captures every posted (already-sanitized) payload. Shared by the
/// `analyst` module's tests so the `post_recommendation` impl lives in exactly
/// one file alongside `sanitize_outbound` (keeps `feedback-as-data-audit` Probe B
/// — "every file that POSTs references the sanitizer" — satisfied).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct RecordingSink {
    pub posted: std::sync::Mutex<Vec<serde_json::Value>>,
    pub fail: bool,
}

#[cfg(test)]
#[async_trait]
impl RecommendationSink for RecordingSink {
    async fn post_recommendation(&self, payload: &serde_json::Value) -> anyhow::Result<()> {
        if self.fail {
            anyhow::bail!("recording sink configured to fail");
        }
        self.posted.lock().unwrap().push(payload.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feedbackmonk_core::{ActionType, FeedbackKind};
    use uuid::Uuid;

    fn cluster() -> ClusterInput {
        ClusterInput {
            cluster_id: Uuid::new_v4(),
            sweep_id: Some(Uuid::new_v4()),
            label: "Login broken".into(),
            summary: None,
            kind: FeedbackKind::Bug,
            member_bodies: vec![],
        }
    }

    fn candidate(body: &str) -> CandidateRecommendation {
        CandidateRecommendation {
            action_type: ActionType::BugFix,
            title: "Fix login".into(),
            body: body.into(),
            rationale: Some("grouped reports".into()),
            source_refs: json!(["src/auth.rs:10-20"]),
            confidence: 0.5,
        }
    }

    #[test]
    fn content_payload_has_ingest_request_fields_and_no_routing() {
        let p = build_content_payload(&candidate("body text"));
        assert_eq!(p["action_type"], "bug_fix");
        assert_eq!(p["title"], "Fix login");
        assert_eq!(p["body"], "body text");
        assert_eq!(p["source_refs"], json!(["src/auth.rs:10-20"]));
        assert_eq!(p["confidence"], 0.5);
        // Routing identifiers are NOT in the content (attached post-sanitize).
        assert!(p.get("cluster_id").is_none());
        assert!(p.get("sweep_id").is_none());
    }

    #[tokio::test]
    async fn ingest_attaches_routing_ids_that_survive_sanitization() {
        // The PII scrubber rewrites UUIDs; cluster_id/sweep_id must still arrive
        // intact (attached after the chokepoint) so the transport can route.
        let sink = RecordingSink::default();
        let c = cluster();
        ingest(&sink, &c, &candidate("body")).await.unwrap();

        let posted = sink.posted.lock().unwrap();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0]["cluster_id"], json!(c.cluster_id));
        assert_eq!(posted[0]["sweep_id"], json!(c.sweep_id));
    }

    #[tokio::test]
    async fn ingest_routes_content_through_sanitize_outbound() {
        // A PII-bearing body proves the egress chokepoint ran before the wire:
        // the posted body must have the email scrubbed.
        let sink = RecordingSink::default();
        let c = cluster();
        ingest(&sink, &c, &candidate("reach me at leak@example.com"))
            .await
            .unwrap();

        let posted = sink.posted.lock().unwrap();
        let body = posted[0]["body"].as_str().unwrap();
        assert!(
            !body.contains("leak@example.com"),
            "ingest must route content through sanitize_outbound: {body}"
        );
    }

    #[tokio::test]
    async fn ingest_rejects_a_secret_dump_at_the_chokepoint() {
        // A candidate whose body smuggles key material must be rejected by the
        // egress sanitizer (Worker A's secret denylist) — never posted.
        let sink = RecordingSink::default();
        let c = cluster();
        let mut cand = candidate("here is the key");
        cand.body =
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmU=\n-----END OPENSSH PRIVATE KEY-----"
                .into();
        let err = ingest(&sink, &c, &cand).await.unwrap_err();
        assert!(
            err.to_string().contains("egress sanitizer rejected"),
            "secret dump must be rejected: {err}"
        );
        assert!(sink.posted.lock().unwrap().is_empty(), "rejected payload is never posted");
    }

    #[tokio::test]
    async fn ingest_propagates_transport_failure() {
        let sink = RecordingSink { fail: true, ..Default::default() };
        let c = cluster();
        let err = ingest(&sink, &c, &candidate("body")).await.unwrap_err();
        assert!(err.to_string().contains("fail"));
    }
}
