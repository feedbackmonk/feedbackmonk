//! Runner-transition reporting (C26 loop step 5) — the runner's lifecycle
//! transitions `building → verifying → reported`, plus the `failed` exit.
//!
//! **Egress discipline (Worker B consumes Worker A's chokepoint).** The
//! implementer path already sanitizes inside `implementer::implement`, but this
//! module is the runner's *direct* outbound path for the `result_ref` POST, so
//! it routes the payload through [`sanitizer::sanitize_outbound`] one more time
//! before the wire. Sanitizing already-clean conclusions is idempotent; the
//! point is that **every outbound POST this module makes provably passes the one
//! egress chokepoint** — exactly what `feedback-as-data-audit` Probe B asserts.
//! A `result_ref` the sanitizer rejects (a leaked source/secret dump) fails the
//! report rather than crossing the wire.

use crate::client::WorkOrderClient;
use crate::sanitizer::sanitize_outbound;
use crate::types::ResultRef;
use uuid::Uuid;

/// `claimed → building` — the runner has started the implementation run.
///
/// # Errors
/// Transport/HTTP failure.
pub async fn report_building(client: &WorkOrderClient, work_order_id: Uuid) -> anyhow::Result<()> {
    client
        .runner_transition(work_order_id, "building", None, None)
        .await
}

/// `building → verifying` — the implementation landed; verification is running.
///
/// # Errors
/// Transport/HTTP failure.
pub async fn report_verifying(client: &WorkOrderClient, work_order_id: Uuid) -> anyhow::Result<()> {
    client
        .runner_transition(work_order_id, "verifying", None, None)
        .await
}

/// `verifying → reported` — the terminal runner success, carrying the
/// conclusions-only `result_ref`. The payload is routed through the egress
/// sanitizer (Worker A's chokepoint) before the POST.
///
/// # Errors
/// [`anyhow::Error`] if the egress sanitizer rejects the payload (a source/
/// secret dump never crosses the wire), or on transport/HTTP failure.
pub async fn report_reported(
    client: &WorkOrderClient,
    work_order_id: Uuid,
    result_ref: &ResultRef,
) -> anyhow::Result<()> {
    let sanitized = sanitize_clean(result_ref)?;
    client
        .runner_transition(work_order_id, "reported", Some(&sanitized), None)
        .await
}

/// `building | verifying → failed` — the implementation or verification failed.
/// `reason` is a short status string (the failure summary is sanitized as part
/// of the runner-transition body the API stores).
///
/// # Errors
/// Transport/HTTP failure.
pub async fn report_failed(
    client: &WorkOrderClient,
    work_order_id: Uuid,
    reason: &str,
) -> anyhow::Result<()> {
    // The failure reason is runner-authored status text (not feedback-derived),
    // but route it through the PII scrubber leg defensively so a path/email that
    // leaked into an error string is scrubbed before storage.
    let scrubbed = crate::sanitizer::scrub_pii(reason);
    client
        .runner_transition(work_order_id, "failed", None, Some(&scrubbed))
        .await
}

/// Route a `ResultRef` through the egress chokepoint and round-trip it back to
/// the typed shape. The sanitizer preserves structure (it scrubs string leaves /
/// rejects dumps), so the cleaned value deserializes back into `ResultRef`.
fn sanitize_clean(result_ref: &ResultRef) -> anyhow::Result<ResultRef> {
    let value = serde_json::to_value(result_ref)?;
    let cleaned = sanitize_outbound(&value)
        .map_err(|e| anyhow::anyhow!("egress sanitizer rejected result_ref: {e}"))?;
    Ok(serde_json::from_value(cleaned)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DiffStat, Verification};

    fn sample_ref() -> ResultRef {
        ResultRef {
            pr_url: Some("https://git.example/pr/1".into()),
            branch: Some("fbm/wo-1".into()),
            commit: None,
            diff_stat: DiffStat { files: 2, insertions: 9, deletions: 1 },
            verification: Verification { tests_passed: true, finalize_status: "passed".into() },
            summary: "Fixed null deref".into(),
        }
    }

    #[test]
    fn sanitize_clean_preserves_reference_shape() {
        let cleaned = sanitize_clean(&sample_ref()).unwrap();
        assert_eq!(cleaned.branch.as_deref(), Some("fbm/wo-1"));
        assert_eq!(cleaned.diff_stat.files, 2);
        assert!(cleaned.verification.tests_passed);
    }

    #[test]
    fn sanitize_clean_scrubs_pii_in_summary() {
        let mut r = sample_ref();
        r.summary = "reported by alice@example.com".into();
        let cleaned = sanitize_clean(&r).unwrap();
        assert!(!cleaned.summary.contains("alice@example.com"), "got: {}", cleaned.summary);
    }

    #[tokio::test]
    async fn report_reported_posts_sanitized_result_ref() {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let pid = uuid::Uuid::new_v4();
        let wo = uuid::Uuid::new_v4();
        Mock::given(method("POST"))
            .and(path(format!(
                "/api/v1/projects/{pid}/work-orders/{wo}/runner-transition"
            )))
            .and(body_partial_json(serde_json::json!({ "event_type": "reported" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let client = WorkOrderClient::new(server.uri(), pid, "t");
        report_reported(&client, wo, &sample_ref()).await.unwrap();
    }
}
