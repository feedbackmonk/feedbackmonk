//! `WorkOrderClient` — the runner's authenticated HTTP client against the
//! work-order API (Contract C25/C26). The method signatures FREEZE in Stage 0;
//! Worker B (Stage 1) wires the transport (reqwest) + the runner-token bearer
//! auth + retries.
//!
//! Auth: the client carries a customer-minted runner write-token (a short-TTL
//! EdDSA JWT, `scope:"runner:write"`, signed by a registered `runner`-class key
//! — DEC-FBR-04 keeps feedbackmonk private-key-free). Every request sends it as
//! `Authorization: Bearer <token>`. The structural blast-radius bound: a runner
//! token authorizes runner-only transitions and can NEVER author `approve`
//! (C22 inv. 2), so even full token compromise cannot create a dispatched order.
//!
//! ## Transport (Worker B, Stage 1)
//!
//! `reqwest` (async; rustls — no system OpenSSL). Every request attaches the
//! bearer; transient failures (connect/timeout, `429`, `5xx`) retry with
//! exponential backoff. Read-path note: the rich [`ClaimedOrder`] these methods
//! return needs a runner-token-accessible read endpoint (`GET work-orders` +
//! detail) that Stage 0 did not build — see MSG-B01. The transport targets the
//! documented runner URLs so it composes the moment those endpoints land; the
//! whole loop stays unit-testable against a `wiremock` server meanwhile.

use std::time::Duration;

use reqwest::Method;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::types::{ClaimedOrder, ResultRef};

/// Total send attempts (1 initial + 3 retries) for transient failures.
const MAX_ATTEMPTS: u32 = 4;
/// Base backoff; attempt N waits `BASE_BACKOFF_MS * 2^(N-1)` ms.
const BASE_BACKOFF_MS: u64 = 200;
/// Per-request timeout (a real `claude`-driven build reports via SEPARATE short
/// transitions, so no single request is long-running).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Authenticated client for one project's work-order API.
///
/// Stage-0 skeleton froze the connection coordinates + the bearer token + the
/// method signatures; Worker B (Stage 1) wired the `reqwest` transport. The
/// public surface (`new` / `bearer` / the four methods) is unchanged — the only
/// addition is a private `http` handle, so the frozen seam holds.
#[derive(Debug, Clone)]
pub struct WorkOrderClient {
    /// API base URL, e.g. `https://feedback.example.com` (no trailing slash).
    pub base_url: String,
    /// The project this runner serves.
    pub project_id: Uuid,
    /// The customer-minted runner write-token (sent as `Bearer`).
    runner_token: String,
    /// Shared connection pool (Worker B). Cheap to clone (Arc inside).
    http: reqwest::Client,
}

impl WorkOrderClient {
    #[must_use]
    pub fn new(base_url: impl Into<String>, project_id: Uuid, runner_token: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url: base_url.into(),
            project_id,
            runner_token: runner_token.into(),
            http,
        }
    }

    /// The bearer value Worker B attaches to every request.
    #[must_use]
    pub fn bearer(&self) -> &str {
        &self.runner_token
    }

    /// `GET /work-orders?state=dispatched` — the poll step (C26 loop step 1).
    ///
    /// Tolerates either a `{ "items": [...] }` envelope (matching the existing
    /// list handlers) or a bare array of `ClaimedOrder`.
    ///
    /// # Errors
    /// Transport/HTTP failure, or a response that is not a `ClaimedOrder` list.
    pub async fn poll_dispatched(&self) -> anyhow::Result<Vec<ClaimedOrder>> {
        let url = format!("{}/runner/work-orders?state=dispatched", self.project_base());
        let resp = self.send(Method::GET, &url, None).await?;
        let value: Value = resp.json().await?;
        let arr = match value {
            Value::Object(mut map) => map.remove("items").unwrap_or(Value::Array(vec![])),
            other => other,
        };
        Ok(serde_json::from_value(arr)?)
    }

    /// `POST /work-orders/:id/claim` — dispatched → claimed (C26 loop step 2),
    /// then fetch the order detail (step 3) so the caller gets the full trusted
    /// + untrusted context needed to drive the implementer.
    ///
    /// # Errors
    /// Transport/HTTP failure on either the claim transition or the detail fetch.
    pub async fn claim(&self, work_order_id: Uuid) -> anyhow::Result<ClaimedOrder> {
        let claim_url = format!("{}/work-orders/{}/claim", self.project_base(), work_order_id);
        // The claim handler returns a TransitionResponse (not the rich order); we
        // only need it to have succeeded.
        self.send(Method::POST, &claim_url, None).await?;
        self.fetch_order(work_order_id).await
    }

    /// `POST /work-orders/:id/runner-transition` — building / verifying /
    /// reported / failed (C26 loop step 5). On `reported`, `result_ref` MUST
    /// have already passed the egress sanitizer (Worker A's `sanitize_outbound`).
    ///
    /// # Errors
    /// Transport/HTTP failure, or a non-success status from the API.
    pub async fn runner_transition(
        &self,
        work_order_id: Uuid,
        event_type: &str,
        result_ref: Option<&ResultRef>,
        failure_reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/work-orders/{}/runner-transition",
            self.project_base(),
            work_order_id
        );
        let mut body = Map::new();
        body.insert("event_type".into(), Value::String(event_type.to_string()));
        if let Some(r) = result_ref {
            body.insert("result_ref".into(), serde_json::to_value(r)?);
        }
        if let Some(reason) = failure_reason {
            body.insert("failure_reason".into(), Value::String(reason.to_string()));
        }
        self.send(Method::POST, &url, Some(&Value::Object(body))).await?;
        Ok(())
    }

    /// `POST /runner/clusters/:cluster_id/recommendations` (ingestion seam) —
    /// the analyst egress path (C26). The payload MUST have passed the egress
    /// sanitizer first (Worker C routes its recommendation bodies through Worker
    /// A's `sanitize_outbound`). Per the B↔C seam (MSG-C01 Q1) the payload
    /// carries a top-level `cluster_id`, which routes the URL; the API ignores
    /// the stray field in the body.
    ///
    /// # Errors
    /// Missing/invalid `cluster_id` in the payload, or transport/HTTP failure.
    pub async fn post_recommendation(&self, payload: &Value) -> anyhow::Result<()> {
        let cluster_id = payload
            .get("cluster_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow::anyhow!("post_recommendation payload missing top-level string `cluster_id`")
            })?;
        let url = format!(
            "{}/runner/clusters/{}/recommendations",
            self.project_base(),
            cluster_id
        );
        self.send(Method::POST, &url, Some(payload)).await?;
        Ok(())
    }

    // -- internals ----------------------------------------------------------

    /// `GET /runner/work-orders/:id` (runner detail) → the full [`ClaimedOrder`].
    async fn fetch_order(&self, work_order_id: Uuid) -> anyhow::Result<ClaimedOrder> {
        let url = format!("{}/runner/work-orders/{}", self.project_base(), work_order_id);
        let resp = self.send(Method::GET, &url, None).await?;
        Ok(resp.json().await?)
    }

    /// `{base_url}/api/v1/projects/{project_id}` — the project-scoped URL root.
    fn project_base(&self) -> String {
        format!("{}/api/v1/projects/{}", self.base_url, self.project_id)
    }

    /// Send one request with the bearer attached, retrying transient failures
    /// (connect/timeout, `429`, `5xx`) with exponential backoff. A non-transient
    /// non-2xx status surfaces the body for diagnosis.
    async fn send(
        &self,
        method: Method,
        url: &str,
        body: Option<&Value>,
    ) -> anyhow::Result<reqwest::Response> {
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            let mut req = self
                .http
                .request(method.clone(), url)
                .bearer_auth(&self.runner_token);
            if let Some(b) = body {
                req = req.json(b);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    let retryable = status.as_u16() == 429 || status.is_server_error();
                    if retryable && attempt < MAX_ATTEMPTS {
                        Self::backoff(attempt).await;
                        continue;
                    }
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("{method} {url} -> HTTP {status}: {text}");
                }
                Err(e) => {
                    let retryable = e.is_timeout() || e.is_connect() || e.is_request();
                    if retryable && attempt < MAX_ATTEMPTS {
                        Self::backoff(attempt).await;
                        continue;
                    }
                    return Err(
                        anyhow::Error::new(e).context(format!("{method} {url} transport error"))
                    );
                }
            }
        }
    }

    /// Exponential backoff before retry attempt `attempt` (1-based).
    async fn backoff(attempt: u32) {
        let millis = BASE_BACKOFF_MS.saturating_mul(1u64 << (attempt - 1));
        tokio::time::sleep(Duration::from_millis(millis)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DiffStat, Verification};

    #[test]
    fn client_holds_coordinates_and_bearer() {
        let pid = Uuid::new_v4();
        let c = WorkOrderClient::new("https://x.example", pid, "tok-abc");
        assert_eq!(c.base_url, "https://x.example");
        assert_eq!(c.project_id, pid);
        assert_eq!(c.bearer(), "tok-abc");
    }

    #[test]
    fn project_base_builds_scoped_url() {
        let pid = Uuid::nil();
        let c = WorkOrderClient::new("https://x.example", pid, "t");
        assert_eq!(
            c.project_base(),
            "https://x.example/api/v1/projects/00000000-0000-0000-0000-000000000000"
        );
    }

    #[tokio::test]
    async fn poll_dispatched_lists_orders_from_items_envelope() {
        use wiremock::matchers::{header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let pid = Uuid::new_v4();
        let wo = Uuid::new_v4();
        let body = serde_json::json!({
            "items": [{
                "work_order_id": wo,
                "project_id": pid,
                "action_type": "bug_fix",
                "title": "Null check",
                "instructions": "Fix the null deref",
                "owner_overrides": null,
                "recommendation": {
                    "body": "users hit a crash",
                    "rationale": null,
                    "cluster_summary": "crash cluster",
                    "member_bodies": ["it crashes"],
                    "source_refs": []
                }
            }]
        });
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/projects/{pid}/runner/work-orders")))
            .and(query_param("state", "dispatched"))
            .and(header("authorization", "Bearer tok-xyz"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let c = WorkOrderClient::new(server.uri(), pid, "tok-xyz");
        let orders = c.poll_dispatched().await.unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].work_order_id, wo);
        assert_eq!(
            orders[0].recommendation.as_ref().unwrap().cluster_summary,
            "crash cluster"
        );
    }

    #[tokio::test]
    async fn runner_transition_posts_event_and_result_ref() {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let pid = Uuid::new_v4();
        let wo = Uuid::new_v4();
        Mock::given(method("POST"))
            .and(path(format!(
                "/api/v1/projects/{pid}/work-orders/{wo}/runner-transition"
            )))
            .and(body_partial_json(serde_json::json!({ "event_type": "reported" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let c = WorkOrderClient::new(server.uri(), pid, "t");
        let rr = ResultRef {
            pr_url: None,
            branch: Some("fbm/x".into()),
            commit: None,
            diff_stat: DiffStat { files: 1, insertions: 2, deletions: 0 },
            verification: Verification { tests_passed: true, finalize_status: "passed".into() },
            summary: "done".into(),
        };
        c.runner_transition(wo, "reported", Some(&rr), None).await.unwrap();
    }

    #[tokio::test]
    async fn post_recommendation_routes_cluster_from_payload() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let pid = Uuid::new_v4();
        let cluster = Uuid::new_v4();
        Mock::given(method("POST"))
            .and(path(format!(
                "/api/v1/projects/{pid}/runner/clusters/{cluster}/recommendations"
            )))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let c = WorkOrderClient::new(server.uri(), pid, "t");
        let payload = serde_json::json!({ "cluster_id": cluster, "body": "do x" });
        c.post_recommendation(&payload).await.unwrap();
    }

    #[tokio::test]
    async fn post_recommendation_rejects_missing_cluster_id() {
        let c = WorkOrderClient::new("https://x.example", Uuid::nil(), "t");
        let payload = serde_json::json!({ "body": "no cluster here" });
        let err = c.post_recommendation(&payload).await.unwrap_err();
        assert!(err.to_string().contains("cluster_id"));
    }

    #[tokio::test]
    async fn server_error_retries_then_surfaces() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let pid = Uuid::new_v4();
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/projects/{pid}/runner/work-orders")))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let c = WorkOrderClient::new(server.uri(), pid, "t");
        let err = c.poll_dispatched().await.unwrap_err();
        assert!(err.to_string().contains("503"), "got: {err}");
    }
}
