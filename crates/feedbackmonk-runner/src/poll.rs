//! The runner host loop (C26 loop, FR-FBR-24) — one poll tick:
//!
//! ```text
//! 1. GET /runner/work-orders?state=dispatched   → [orders]
//! 2. for each: claim   (dispatched → claimed) + fetch detail
//! 3. report `building`
//! 4. ImplementResult = implementer::implement(agent, order, repo)   ← Worker A
//! 5. report `verifying` then `reported {result_ref}`               (sanitized)
//!    on any failure → report `failed {reason}`
//! ```
//!
//! The loop **never reimplements prompt assembly or result-capture** — it calls
//! Worker A's `implementer::implement` and reports the returned `ResultRef`
//! (GUIDE §7.2 composition rule). The `agent` is injectable: production passes
//! the spawn-based [`crate::default_agent::SpawnAgent`]; tests + the GATE 1
//! dry-run inject [`crate::agent::StubAgent`], so no real `claude` runs in unit
//! tests. Per-order failures are isolated — one bad order reports `failed` and
//! the tick continues with the rest.

use crate::agent::AgentCommand;
use crate::client::WorkOrderClient;
use crate::types::{ClaimedOrder, RepoContext};
use crate::{claim, implementer, report};

/// What one poll tick did (for `--watch` logging + test assertions).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickSummary {
    /// Orders returned by the dispatched poll.
    pub polled: usize,
    /// Orders driven to `reported` (implement + verification succeeded).
    pub reported: usize,
    /// Orders driven to `failed` (claim/implement/report error on that order).
    pub failed: usize,
    /// Per-order error strings (the order still got a `failed` transition when
    /// possible; an error here means even that could not be recorded).
    pub errors: Vec<String>,
}

/// Run ONE poll tick: poll dispatched orders and drive each end-to-end.
///
/// # Errors
/// Only the *poll* step can fail the whole tick (no orders to work). Per-order
/// failures are recorded in [`TickSummary`], not propagated.
pub async fn run_once(
    client: &WorkOrderClient,
    agent: &dyn AgentCommand,
    repo: &RepoContext,
) -> anyhow::Result<TickSummary> {
    let orders = client.poll_dispatched().await?;
    let mut summary = TickSummary {
        polled: orders.len(),
        ..Default::default()
    };
    if orders.is_empty() {
        tracing::debug!(target: "runner", "poll tick: no dispatched orders");
        return Ok(summary);
    }

    for order in orders {
        let work_order_id = order.work_order_id;
        match drive_order(client, agent, repo, order).await {
            Ok(()) => summary.reported += 1,
            Err(e) => {
                summary.failed += 1;
                tracing::warn!(
                    target: "runner",
                    %work_order_id,
                    error = %e,
                    "work order failed during this tick"
                );
                // Best-effort: record a `failed` transition so the order does not
                // hang in an execution state. If even that fails, capture it.
                if let Err(report_err) =
                    report::report_failed(client, work_order_id, &e.to_string()).await
                {
                    summary
                        .errors
                        .push(format!("{work_order_id}: {e} (and failed to report: {report_err})"));
                }
            }
        }
    }
    Ok(summary)
}

/// Drive ONE order through `claim → building → implement → verifying →
/// reported`. Any step returning `Err` aborts THIS order (the caller reports
/// `failed`); the implementer is Worker A's `implement`, called verbatim.
async fn drive_order(
    client: &WorkOrderClient,
    agent: &dyn AgentCommand,
    repo: &RepoContext,
    order: ClaimedOrder,
) -> anyhow::Result<()> {
    // 2/3. Claim the dispatched order + fetch its full detail.
    let claimed = claim::claim_order(client, order.work_order_id).await?;
    let work_order_id = claimed.work_order_id;

    // 3. building.
    report::report_building(client, work_order_id).await?;

    // 4. Implement (Worker A owns the body — assembly + agent run + sanitize).
    let result = implementer::implement(agent, &claimed, repo).await?;

    // 5. verifying → reported (result_ref routed through the egress chokepoint).
    report::report_verifying(client, work_order_id).await?;
    report::report_reported(client, work_order_id, &result.result_ref).await?;
    tracing::info!(target: "runner", %work_order_id, "work order reported");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::StubAgent;
    use crate::types::{DiffStat, ImplementResult, ResultRef, Verification};
    use uuid::Uuid;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn order_json(pid: Uuid, wo: Uuid) -> serde_json::Value {
        serde_json::json!({
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
        })
    }

    /// Stub the four endpoints a single-order tick touches.
    async fn mount_order(server: &MockServer, pid: Uuid, wo: Uuid) {
        // poll
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/projects/{pid}/runner/work-orders")))
            .and(query_param("state", "dispatched"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "items": [order_json(pid, wo)] })),
            )
            .mount(server)
            .await;
        // claim (POST) — returns a TransitionResponse the client ignores
        Mock::given(method("POST"))
            .and(path(format!("/api/v1/projects/{pid}/work-orders/{wo}/claim")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(server)
            .await;
        // detail (GET) — the runner re-fetch after claim
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/projects/{pid}/runner/work-orders/{wo}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(order_json(pid, wo)))
            .mount(server)
            .await;
        // every runner-transition (building/verifying/reported/failed)
        Mock::given(method("POST"))
            .and(path(format!(
                "/api/v1/projects/{pid}/work-orders/{wo}/runner-transition"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn tick_with_no_orders_is_a_clean_noop() {
        let server = MockServer::start().await;
        let pid = Uuid::new_v4();
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/projects/{pid}/runner/work-orders")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "items": [] })),
            )
            .mount(&server)
            .await;
        let client = WorkOrderClient::new(server.uri(), pid, "t");
        let agent = StubAgent::default();
        let repo = RepoContext { repo_path: ".".into() };
        let summary = run_once(&client, &agent, &repo).await.unwrap();
        assert_eq!(summary, TickSummary { polled: 0, ..Default::default() });
    }

    #[tokio::test]
    async fn order_with_canned_result_drives_to_reported() {
        // The GATE 1 dry-run as a hermetic unit test: dispatch → claim →
        // build(StubAgent) → report, against a wiremock API. Drives Worker A's
        // `implementer::implement` with an injected StubAgent (no real `claude`),
        // and asserts the order reaches `reported`.
        let server = MockServer::start().await;
        let pid = Uuid::new_v4();
        let wo = Uuid::new_v4();
        mount_order(&server, pid, wo).await;

        let canned = ImplementResult {
            result_ref: ResultRef {
                pr_url: None,
                branch: Some("fbm/wo".into()),
                commit: None,
                diff_stat: DiffStat { files: 1, insertions: 3, deletions: 0 },
                verification: Verification { tests_passed: true, finalize_status: "passed".into() },
                summary: "ok".into(),
            },
        };
        let client = WorkOrderClient::new(server.uri(), pid, "t");
        let agent = StubAgent::with_result(canned);
        let repo = RepoContext { repo_path: ".".into() };

        let summary = run_once(&client, &agent, &repo).await.unwrap();
        assert_eq!(summary.polled, 1);
        assert_eq!(summary.reported, 1, "order should reach `reported`");
        assert_eq!(summary.failed, 0);
        assert!(summary.errors.is_empty(), "errors: {:?}", summary.errors);
    }
}
