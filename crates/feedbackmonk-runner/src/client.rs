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

use uuid::Uuid;

use crate::types::{ClaimedOrder, ResultRef};

/// Authenticated client for one project's work-order API.
///
/// Stage-0 skeleton: holds the connection coordinates + the bearer token. The
/// transport is wired by Worker B; the methods below bail until then so the seam
/// is frozen (callers compile against the final signatures today).
#[derive(Debug, Clone)]
pub struct WorkOrderClient {
    /// API base URL, e.g. `https://feedback.example.com` (no trailing slash).
    pub base_url: String,
    /// The project this runner serves.
    pub project_id: Uuid,
    /// The customer-minted runner write-token (sent as `Bearer`).
    runner_token: String,
}

impl WorkOrderClient {
    #[must_use]
    pub fn new(base_url: impl Into<String>, project_id: Uuid, runner_token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            project_id,
            runner_token: runner_token.into(),
        }
    }

    /// The bearer value Worker B will attach to every request.
    #[must_use]
    pub fn bearer(&self) -> &str {
        &self.runner_token
    }

    /// `GET /work-orders?state=dispatched` — the poll step (C26 loop step 1).
    ///
    /// # Errors
    /// Stage-0 stub: returns an error until Worker B wires the transport.
    pub async fn poll_dispatched(&self) -> anyhow::Result<Vec<ClaimedOrder>> {
        anyhow::bail!("WorkOrderClient::poll_dispatched is wired by Worker B (Stage 1)")
    }

    /// `POST /work-orders/:id/claim` — dispatched → claimed (C26 loop step 2).
    ///
    /// # Errors
    /// Stage-0 stub.
    pub async fn claim(&self, _work_order_id: Uuid) -> anyhow::Result<ClaimedOrder> {
        anyhow::bail!("WorkOrderClient::claim is wired by Worker B (Stage 1)")
    }

    /// `POST /work-orders/:id/runner-transition` — building / verifying /
    /// reported / failed (C26 loop step 5). On `reported`, `result_ref` MUST
    /// have already passed the egress sanitizer (Worker A's `sanitize_outbound`).
    ///
    /// # Errors
    /// Stage-0 stub.
    pub async fn runner_transition(
        &self,
        _work_order_id: Uuid,
        _event_type: &str,
        _result_ref: Option<&ResultRef>,
        _failure_reason: Option<&str>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("WorkOrderClient::runner_transition is wired by Worker B (Stage 1)")
    }

    /// `POST /recommendations` (ingestion seam) — the analyst egress path (C26).
    /// The payload MUST have passed the egress sanitizer first (Worker C routes
    /// its recommendation bodies through Worker A's `sanitize_outbound`).
    ///
    /// # Errors
    /// Stage-0 stub.
    pub async fn post_recommendation(&self, _payload: &serde_json::Value) -> anyhow::Result<()> {
        anyhow::bail!("WorkOrderClient::post_recommendation is wired by Worker C (Stage 1)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_holds_coordinates_and_bearer() {
        let pid = Uuid::new_v4();
        let c = WorkOrderClient::new("https://x.example", pid, "tok-abc");
        assert_eq!(c.base_url, "https://x.example");
        assert_eq!(c.project_id, pid);
        assert_eq!(c.bearer(), "tok-abc");
    }
}
