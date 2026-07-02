//! Work-order event repository (Contract C22 backing methods) — APPEND-ONLY.
//!
//! Owns the `work_order_events` ledger (migration 00014): the append-only audit
//! trail + state-machine record. This trait deliberately exposes NO update or
//! delete method — the ledger is immutable, the record the
//! `approval-gate-enforcement` oracle trusts (mirrors `feedback_status_history`).
//!
//! Every state change Worker A's handler performs writes a row here IN THE SAME
//! TRANSACTION as the `work_orders.state` update (C22 inv. 3) via
//! [`WorkOrderEventRepo::append_in_executor`]. [`WorkOrderEventRepo::has_approved_event`]
//! is THE security predicate (C22 inv. 1): true iff an owner-authored `approve`
//! event exists for the order — the gate that must hold before any execution
//! state.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::WorkOrderState;

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

/// One immutable ledger row. `from_state` is `None` only for a genesis event
/// (work-order creation: `None -> draft`); every transition carries both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOrderEvent {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub work_order_id: Uuid,
    pub from_state: Option<WorkOrderState>,
    pub to_state: WorkOrderState,
    pub event_type: String,
    pub actor: String,
    pub actor_id: Option<String>,
    pub detail: Option<JsonValue>,
    pub at: DateTime<Utc>,
}

/// One ledger append. Grouped to keep the call sites readable.
#[derive(Debug, Clone)]
pub struct NewWorkOrderEvent<'a> {
    pub work_order_id: Uuid,
    pub from_state: Option<WorkOrderState>,
    pub to_state: WorkOrderState,
    /// One of: create | approve | dispatch | claim | building | verifying |
    /// reported | failed | accept | request-changes | reject | retry | cancel.
    pub event_type: &'a str,
    /// `admin` | `runner` | `system` (C22 authz matrix).
    pub actor: &'a str,
    pub actor_id: Option<&'a str>,
    pub detail: Option<&'a JsonValue>,
}

#[async_trait]
pub trait WorkOrderEventRepo: Send + Sync {
    /// Append a ledger row (own transaction). The `work_order_id` MUST belong
    /// to `scope` (resolved within scope first; `NotFound` otherwise). Returns
    /// the inserted row id.
    async fn append(&self, scope: &ProjectScope, event: NewWorkOrderEvent<'_>) -> Result<Uuid>;

    /// Same-transaction variant of [`append`] — appends a ledger row on a
    /// caller-supplied executor without touching `work_orders.state`.
    ///
    /// **Production transitions use [`crate::WorkOrderRepo::transition_in_executor`]
    /// instead** — it writes the state column AND this ledger row in one
    /// executor, so audit can never drift from state (C22 inv. 3, scrutiny
    /// P1-4). This standalone append remains for genesis/asymmetric ledger
    /// writes that are not paired with a state change.
    async fn append_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        event: NewWorkOrderEvent<'_>,
    ) -> Result<Uuid>;

    /// List a work order's ledger oldest-first (state-machine replay order).
    /// Cross-tenant scopes see an empty Vec.
    async fn list_for_work_order(
        &self,
        scope: &ProjectScope,
        work_order_id: Uuid,
    ) -> Result<Vec<WorkOrderEvent>>;

    /// **THE security predicate (C22 inv. 1 / FR-FBR-25a).** True iff an
    /// owner-authored `approve` event (`event_type='approve' AND actor='admin'`)
    /// exists for the work order within scope. Worker A's transition handler
    /// consults this BEFORE allowing any transition into an execution state
    /// (`WorkOrderState::is_execution_state`); the `approval-gate-enforcement`
    /// oracle independently proves the same property from the ledger.
    async fn has_approved_event(
        &self,
        scope: &ProjectScope,
        work_order_id: Uuid,
    ) -> Result<bool>;
}

#[derive(Clone)]
pub struct SqlxWorkOrderEventRepo {
    pool: PgPool,
}

impl SqlxWorkOrderEventRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkOrderEventRepo for SqlxWorkOrderEventRepo {
    async fn append(&self, scope: &ProjectScope, event: NewWorkOrderEvent<'_>) -> Result<Uuid> {
        // Resolve the work order WITHIN scope so a cross-tenant work_order_id
        // cannot trigger a ledger row under a sibling tenant.
        sqlx::query!(
            r#"
            SELECT id FROM work_orders
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            event.work_order_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        let inserted = sqlx::query!(
            r#"
            INSERT INTO work_order_events
                (tenant_id, project_id, work_order_id, from_state, to_state,
                 event_type, actor, actor_id, detail)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id
            "#,
            scope.tenant_id(),
            scope.project_id(),
            event.work_order_id,
            event.from_state.map(WorkOrderState::as_db_str),
            event.to_state.as_db_str(),
            event.event_type,
            event.actor,
            event.actor_id,
            event.detail,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(inserted.id)
    }

    async fn append_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        event: NewWorkOrderEvent<'_>,
    ) -> Result<Uuid> {
        sqlx::query!(
            r#"
            SELECT id FROM work_orders
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            event.work_order_id,
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(RepoError::NotFound)?;

        let inserted = sqlx::query!(
            r#"
            INSERT INTO work_order_events
                (tenant_id, project_id, work_order_id, from_state, to_state,
                 event_type, actor, actor_id, detail)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id
            "#,
            scope.tenant_id(),
            scope.project_id(),
            event.work_order_id,
            event.from_state.map(WorkOrderState::as_db_str),
            event.to_state.as_db_str(),
            event.event_type,
            event.actor,
            event.actor_id,
            event.detail,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(inserted.id)
    }

    async fn list_for_work_order(
        &self,
        scope: &ProjectScope,
        work_order_id: Uuid,
    ) -> Result<Vec<WorkOrderEvent>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, tenant_id, project_id, work_order_id, from_state,
                   to_state, event_type, actor, actor_id, detail, at
            FROM work_order_events
            WHERE tenant_id = $1 AND project_id = $2 AND work_order_id = $3
            ORDER BY at ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            work_order_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| WorkOrderEvent {
                id: r.id,
                tenant_id: r.tenant_id,
                project_id: r.project_id,
                work_order_id: r.work_order_id,
                from_state: r.from_state.as_deref().map(WorkOrderState::from_db_str),
                to_state: WorkOrderState::from_db_str(&r.to_state),
                event_type: r.event_type,
                actor: r.actor,
                actor_id: r.actor_id,
                detail: r.detail,
                at: r.at,
            })
            .collect())
    }

    async fn has_approved_event(
        &self,
        scope: &ProjectScope,
        work_order_id: Uuid,
    ) -> Result<bool> {
        let row = sqlx::query!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM work_order_events
                WHERE tenant_id = $1 AND project_id = $2 AND work_order_id = $3
                  AND event_type = 'approve' AND actor = 'admin'
            ) AS "exists!"
            "#,
            scope.tenant_id(),
            scope.project_id(),
            work_order_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.exists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::{ClusterRepo, SqlxClusterRepo};
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::recommendations::{NewRecommendation, RecommendationRepo, SqlxRecommendationRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use crate::work_orders::{NewWorkOrder, SqlxWorkOrderRepo, WorkOrderRepo};
    use feedbackmonk_core::{ActionType, FeedbackKind};
    use serde_json::json;

    struct Seeded {
        scope: ProjectScope,
        work_order_id: Uuid,
    }

    async fn seed(pool: &PgPool, email: &str) -> Seeded {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let crepo = SqlxClusterRepo::new(pool.clone());
        let rrepo = SqlxRecommendationRepo::new(pool.clone());
        let worepo = SqlxWorkOrderRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let tscope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&tscope, "Proj", "proj").await.unwrap();
        let scope = prepo.open(&tscope, p.id).await.unwrap();
        let cluster = crepo
            .create(&scope, "Cluster", None, FeedbackKind::Bug, "agent")
            .await
            .unwrap();
        let refs = json!([]);
        let rec = rrepo
            .create(
                &scope,
                NewRecommendation {
                    cluster_id: cluster.id,
                    sweep_id: None,
                    action_type: ActionType::BugFix,
                    title: "Fix",
                    body: "Body",
                    rationale: None,
                    source_refs: &refs,
                    confidence: 0.5,
                },
            )
            .await
            .unwrap();
        let wo = worepo
            .create(
                &scope,
                NewWorkOrder {
                    recommendation_id: rec.id,
                    cluster_id: cluster.id,
                    action_type: ActionType::BugFix,
                    title: "Fix",
                    instructions: "Do",
                    owner_overrides: None,
                    autonomy_rung: 1,
                },
            )
            .await
            .unwrap();
        Seeded { scope, work_order_id: wo.id }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn append_then_list_round_trip(pool: PgPool) {
        let repo = SqlxWorkOrderEventRepo::new(pool.clone());
        let s = seed(&pool, "woe-rt@example.com").await;

        repo.append(
            &s.scope,
            NewWorkOrderEvent {
                work_order_id: s.work_order_id,
                from_state: Some(WorkOrderState::Draft),
                to_state: WorkOrderState::Approved,
                event_type: "approve",
                actor: "admin",
                actor_id: Some("admin-uuid"),
                detail: None,
            },
        )
        .await
        .unwrap();

        let rows = repo.list_for_work_order(&s.scope, s.work_order_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].from_state, Some(WorkOrderState::Draft));
        assert_eq!(rows[0].to_state, WorkOrderState::Approved);
        assert_eq!(rows[0].event_type, "approve");
        assert_eq!(rows[0].actor, "admin");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn has_approved_event_tracks_owner_approval(pool: PgPool) {
        let repo = SqlxWorkOrderEventRepo::new(pool.clone());
        let s = seed(&pool, "woe-approve@example.com").await;

        // No approval yet (the security gate is CLOSED).
        assert!(!repo.has_approved_event(&s.scope, s.work_order_id).await.unwrap());

        // A RUNNER-authored approve does NOT count (authz: only admin approves).
        repo.append(
            &s.scope,
            NewWorkOrderEvent {
                work_order_id: s.work_order_id,
                from_state: Some(WorkOrderState::Draft),
                to_state: WorkOrderState::Approved,
                event_type: "approve",
                actor: "runner",
                actor_id: Some("runner-1"),
                detail: None,
            },
        )
        .await
        .unwrap();
        assert!(
            !repo.has_approved_event(&s.scope, s.work_order_id).await.unwrap(),
            "a runner-authored approve must NOT satisfy the owner-approval gate"
        );

        // An ADMIN-authored approve OPENS the gate.
        repo.append(
            &s.scope,
            NewWorkOrderEvent {
                work_order_id: s.work_order_id,
                from_state: Some(WorkOrderState::Draft),
                to_state: WorkOrderState::Approved,
                event_type: "approve",
                actor: "admin",
                actor_id: Some("owner-1"),
                detail: None,
            },
        )
        .await
        .unwrap();
        assert!(repo.has_approved_event(&s.scope, s.work_order_id).await.unwrap());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn append_cross_tenant_work_order_rejected(pool: PgPool) {
        let repo = SqlxWorkOrderEventRepo::new(pool.clone());
        let s1 = seed(&pool, "woe-owner1@example.com").await;
        let s2 = seed(&pool, "woe-owner2@example.com").await;

        // s2 cannot append to s1's work order.
        let err = repo
            .append(
                &s2.scope,
                NewWorkOrderEvent {
                    work_order_id: s1.work_order_id,
                    from_state: Some(WorkOrderState::Draft),
                    to_state: WorkOrderState::Approved,
                    event_type: "approve",
                    actor: "admin",
                    actor_id: None,
                    detail: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
        // s2's view of s1's ledger is empty.
        assert!(repo.list_for_work_order(&s2.scope, s1.work_order_id).await.unwrap().is_empty());
    }
}
