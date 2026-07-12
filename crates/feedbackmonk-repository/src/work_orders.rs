//! Work-order repository (Contract C22/C23 backing methods).
//!
//! Owns CRUD on `work_orders` (migration 00014). Every method takes
//! `&ProjectScope` first (DEC-FBR-03). A work order is created in `draft`; its
//! `state` only ever changes through [`WorkOrderRepo::update_state_in_executor`],
//! which Worker A (Stage 1) composes IN THE SAME TRANSACTION as a
//! `work_order_events` append (C22 inv. 3 — audit can never drift from state).
//!
//! Stage 0 ships the foundation (create + get + list + the executor-aware
//! state setter). Worker A builds the legal-transition validation + the C22
//! authz matrix + the approval-gate enforcement (C22 inv. 1) on top — using
//! `feedbackmonk_core::work_order::{legal_transitions_from, WorkOrderState}`
//! and `WorkOrderEventRepo::has_approved_event` as the security predicate.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::{ActionType, WorkOrderState};

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;
use crate::work_order_events::NewWorkOrderEvent;

/// A work order — the contract between an APPROVED decision and a DISPATCHED
/// job (FR-FBR-22). `owner_overrides` carries the Q17 "tweak before approve"
/// edits (authoritative; overrides win at dispatch).
///
/// Provenance is optional since C31 (P6): `recommendation_id IS NULL` ⇔ the
/// order was owner-authored (no feedback grounding). The two FKs are always
/// both-`Some` or both-`None` (migration 00028 `work_orders_provenance_pair`).
/// `routing_label` targets the order at one runner identity (token `sub`);
/// `None` = first-claim-wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOrder {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub recommendation_id: Option<Uuid>,
    pub cluster_id: Option<Uuid>,
    pub action_type: ActionType,
    pub title: String,
    pub instructions: String,
    pub owner_overrides: Option<JsonValue>,
    pub autonomy_rung: i32,
    pub state: WorkOrderState,
    pub approved_by: Option<Uuid>,
    pub approved_at: Option<DateTime<Utc>>,
    pub dispatched_at: Option<DateTime<Utc>>,
    pub claimed_by_runner: Option<String>,
    pub result_ref: Option<JsonValue>,
    pub failure_reason: Option<String>,
    pub routing_label: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for [`WorkOrderRepo::create`]. Provenance (`recommendation_id` +
/// `cluster_id`) is both-`Some` (recommendation-derived) or both-`None`
/// (owner-authored, C31); a mixed pair is rejected with `Conflict` before the
/// DB CHECK would.
#[derive(Debug, Clone)]
pub struct NewWorkOrder<'a> {
    pub recommendation_id: Option<Uuid>,
    pub cluster_id: Option<Uuid>,
    pub action_type: ActionType,
    pub title: &'a str,
    pub instructions: &'a str,
    pub owner_overrides: Option<&'a JsonValue>,
    pub autonomy_rung: i32,
    pub routing_label: Option<&'a str>,
}

/// Optional column updates applied alongside a state change. Each `Some` field
/// is written; each `None` LEAVES THE EXISTING VALUE (via `COALESCE`) — these
/// columns are only ever set forward (approval stamp, dispatch stamp, claim,
/// result, failure), never cleared, so coalesce-on-none is the correct
/// monotonic semantics.
#[derive(Debug, Clone, Default)]
pub struct WorkOrderStatePatch<'a> {
    pub approved_by: Option<Uuid>,
    pub approved_at: Option<DateTime<Utc>>,
    pub dispatched_at: Option<DateTime<Utc>>,
    pub claimed_by_runner: Option<&'a str>,
    pub result_ref: Option<&'a JsonValue>,
    pub failure_reason: Option<&'a str>,
    /// Q17 authoritative "tweak before approve" edits, written when the owner
    /// authors the `approve` event (or carries an updated delta). `None`
    /// COALESCEs to the existing value (set-forward, like the other patch
    /// columns) — an owner who does not re-tweak at approve keeps the
    /// create-time overrides. Added additively in Stage 1 (Worker A) over the
    /// Stage 0 patch; existing call sites use `..Default::default()` unchanged.
    pub owner_overrides: Option<&'a JsonValue>,
    /// C31 §4 named-runner routing target (token `sub`), settable/overridable at
    /// approve (the Q17 tweak surface). `None` COALESCEs to the existing value
    /// (set-forward, like `owner_overrides`) — an owner who does not re-target at
    /// approve keeps the create-time `routing_label`. Added additively in Stage 1
    /// (Worker A, P6); existing call sites use `..Default::default()` unchanged.
    pub routing_label: Option<&'a str>,
}

#[async_trait]
pub trait WorkOrderRepo: Send + Sync {
    /// Create a work order in `draft`. The `recommendation_id` and `cluster_id`
    /// MUST belong to `scope` (resolved within scope first; `NotFound`
    /// otherwise). No `work_order_events` row is written here — the ledger
    /// starts at the first transition (mirrors `feedback.status` starting at
    /// `submitted` with no genesis history row). Worker A's handler appends the
    /// `approve`/`cancel` events.
    async fn create(&self, scope: &ProjectScope, wo: NewWorkOrder<'_>) -> Result<WorkOrder>;

    /// Fetch one work order by id within scope.
    async fn get(&self, scope: &ProjectScope, work_order_id: Uuid) -> Result<WorkOrder>;

    /// List work orders for the project, newest-first. Optional `state` and
    /// `cluster_id` filters. Cross-tenant scopes see an empty Vec.
    async fn list(
        &self,
        scope: &ProjectScope,
        state: Option<&str>,
        cluster_id: Option<Uuid>,
    ) -> Result<Vec<WorkOrder>>;

    /// List work orders visible to a NAMED runner (C31 §5, D-P6-3). Identical to
    /// [`WorkOrderRepo::list`] but adds the routing predicate
    /// `routing_label IS NULL OR routing_label = <runner_sub>`: an unlabeled
    /// order is visible to every runner (first-claim-wins), a labeled order only
    /// to the runner whose verified token `sub` matches. The admin
    /// [`WorkOrderRepo::list`] stays untouched (owners see every order). Routing
    /// is coordination, not a trust boundary — the claim handler enforces the
    /// same predicate as the authoritative gate (a 409 on mismatch).
    async fn list_for_runner(
        &self,
        scope: &ProjectScope,
        state: Option<&str>,
        cluster_id: Option<Uuid>,
        runner_sub: &str,
    ) -> Result<Vec<WorkOrder>>;

    /// **The combined state+ledger transition primitive (C22 inv. 3).** On a
    /// caller-supplied connection/transaction, performs BOTH the
    /// `work_orders.state` UPDATE (to `event.to_state`, + optional column
    /// `patch`) AND the `work_order_events` ledger INSERT — so the state change
    /// and its audit row can NEVER drift apart. This is the ONE method
    /// production transition handlers call: same-transaction parity is
    /// guaranteed by construction, not by the caller remembering to pair two
    /// separate setters (scrutiny P1-4). Mirrors
    /// `FeedbackRepo::moderate_in_executor`'s shape.
    ///
    /// The state target and the ledgered `work_order_id` are taken from
    /// `event` (single source of truth — the caller cannot pass a `to_state`
    /// that disagrees with the audit row). Returns the inserted ledger row id.
    ///
    /// This primitive does NOT validate transition legality or actor authz —
    /// that is the handler's responsibility (it checks `legal_transitions_from`
    /// + the C22 authz matrix + `has_approved_event` BEFORE calling this). The
    /// repository only enforces tenant/project scope: `NotFound` if the work
    /// order is absent or out of scope (the UPDATE's scope predicate is the
    /// guard — a zero-row UPDATE short-circuits before the ledger append).
    async fn transition_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        patch: WorkOrderStatePatch<'_>,
        event: NewWorkOrderEvent<'_>,
    ) -> Result<Uuid>;

    /// Set `state = to_state` (+ optional column patch) on a caller-supplied
    /// connection/transaction WITHOUT writing a ledger row.
    ///
    /// **Prefer [`WorkOrderRepo::transition_in_executor`] for every production
    /// transition** — it writes the state and the `work_order_events` audit row
    /// in the same executor, so audit can never drift from state (C22 inv. 3).
    /// This unpaired setter exists only for legitimate ASYMMETRIC composition:
    /// the bypass-resistance corpus (`tests/work_order_state_machine.rs`)
    /// deliberately forges a state-column value with NO matching approve event
    /// to prove the ledger-authoritative approval gate still refuses. Do NOT
    /// call it from a handler.
    ///
    /// This primitive validates neither transition legality nor actor authz;
    /// the repository only enforces tenant/project scope. `NotFound` if the
    /// work order is absent or out of scope.
    async fn update_state_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        work_order_id: Uuid,
        to_state: WorkOrderState,
        patch: WorkOrderStatePatch<'_>,
    ) -> Result<()>;
}

#[derive(Clone)]
pub struct SqlxWorkOrderRepo {
    pool: PgPool,
}

impl SqlxWorkOrderRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Internal row shape (`action_type` / `state` held as the DB string form, decoded
/// to the typed enums by [`map_work_order`]).
struct WorkOrderRow {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    recommendation_id: Option<Uuid>,
    cluster_id: Option<Uuid>,
    action_type: String,
    title: String,
    instructions: String,
    owner_overrides: Option<JsonValue>,
    autonomy_rung: i32,
    state: String,
    approved_by: Option<Uuid>,
    approved_at: Option<DateTime<Utc>>,
    dispatched_at: Option<DateTime<Utc>>,
    claimed_by_runner: Option<String>,
    result_ref: Option<JsonValue>,
    failure_reason: Option<String>,
    routing_label: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn map_work_order(row: WorkOrderRow) -> WorkOrder {
    WorkOrder {
        id: row.id,
        tenant_id: row.tenant_id,
        project_id: row.project_id,
        recommendation_id: row.recommendation_id,
        cluster_id: row.cluster_id,
        action_type: ActionType::from_db_str(&row.action_type),
        title: row.title,
        instructions: row.instructions,
        owner_overrides: row.owner_overrides,
        autonomy_rung: row.autonomy_rung,
        state: WorkOrderState::from_db_str(&row.state),
        approved_by: row.approved_by,
        approved_at: row.approved_at,
        dispatched_at: row.dispatched_at,
        claimed_by_runner: row.claimed_by_runner,
        result_ref: row.result_ref,
        failure_reason: row.failure_reason,
        routing_label: row.routing_label,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

#[async_trait]
impl WorkOrderRepo for SqlxWorkOrderRepo {
    async fn create(&self, scope: &ProjectScope, wo: NewWorkOrder<'_>) -> Result<WorkOrder> {
        match (wo.recommendation_id, wo.cluster_id) {
            // Recommendation-derived: resolve the recommendation WITHIN scope
            // (it carries the cluster_id linkage; a cross-tenant
            // recommendation_id cannot anchor a work order under a sibling
            // tenant), and the supplied cluster_id must match its cluster
            // (defense against a mismatched cross-cluster work order).
            (Some(rec_id), Some(cluster_id)) => {
                let rec = sqlx::query!(
                    r#"
                    SELECT cluster_id FROM recommendations
                    WHERE tenant_id = $1 AND project_id = $2 AND id = $3
                    "#,
                    scope.tenant_id(),
                    scope.project_id(),
                    rec_id,
                )
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;

                if rec.cluster_id != cluster_id {
                    return Err(RepoError::Conflict);
                }
            }
            // Owner-authored: no provenance to resolve (C31 / D-P6-2).
            (None, None) => {}
            // Half-null provenance is unrepresentable (migration 00028 CHECK);
            // reject before the DB would.
            _ => return Err(RepoError::Conflict),
        }

        let row = sqlx::query_as!(
            WorkOrderRow,
            r#"
            INSERT INTO work_orders
                (tenant_id, project_id, recommendation_id, cluster_id,
                 action_type, title, instructions, owner_overrides, autonomy_rung,
                 routing_label)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, tenant_id, project_id, recommendation_id, cluster_id,
                      action_type, title, instructions, owner_overrides,
                      autonomy_rung, state, approved_by, approved_at,
                      dispatched_at, claimed_by_runner, result_ref,
                      failure_reason, routing_label, created_at, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            wo.recommendation_id,
            wo.cluster_id,
            wo.action_type.as_db_str(),
            wo.title,
            wo.instructions,
            wo.owner_overrides,
            wo.autonomy_rung,
            wo.routing_label,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(map_work_order(row))
    }

    async fn get(&self, scope: &ProjectScope, work_order_id: Uuid) -> Result<WorkOrder> {
        let row = sqlx::query_as!(
            WorkOrderRow,
            r#"
            SELECT id, tenant_id, project_id, recommendation_id, cluster_id,
                   action_type, title, instructions, owner_overrides,
                   autonomy_rung, state, approved_by, approved_at,
                   dispatched_at, claimed_by_runner, result_ref,
                   failure_reason, routing_label, created_at, updated_at
            FROM work_orders
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            work_order_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;
        Ok(map_work_order(row))
    }

    async fn list(
        &self,
        scope: &ProjectScope,
        state: Option<&str>,
        cluster_id: Option<Uuid>,
    ) -> Result<Vec<WorkOrder>> {
        let rows = sqlx::query_as!(
            WorkOrderRow,
            r#"
            SELECT id, tenant_id, project_id, recommendation_id, cluster_id,
                   action_type, title, instructions, owner_overrides,
                   autonomy_rung, state, approved_by, approved_at,
                   dispatched_at, claimed_by_runner, result_ref,
                   failure_reason, routing_label, created_at, updated_at
            FROM work_orders
            WHERE tenant_id = $1 AND project_id = $2
              AND ($3::text IS NULL OR state = $3)
              AND ($4::uuid IS NULL OR cluster_id = $4)
            ORDER BY created_at DESC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            state,
            cluster_id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(map_work_order).collect())
    }

    async fn list_for_runner(
        &self,
        scope: &ProjectScope,
        state: Option<&str>,
        cluster_id: Option<Uuid>,
        runner_sub: &str,
    ) -> Result<Vec<WorkOrder>> {
        let rows = sqlx::query_as!(
            WorkOrderRow,
            r#"
            SELECT id, tenant_id, project_id, recommendation_id, cluster_id,
                   action_type, title, instructions, owner_overrides,
                   autonomy_rung, state, approved_by, approved_at,
                   dispatched_at, claimed_by_runner, result_ref,
                   failure_reason, routing_label, created_at, updated_at
            FROM work_orders
            WHERE tenant_id = $1 AND project_id = $2
              AND ($3::text IS NULL OR state = $3)
              AND ($4::uuid IS NULL OR cluster_id = $4)
              AND (routing_label IS NULL OR routing_label = $5)
            ORDER BY created_at DESC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            state,
            cluster_id,
            runner_sub,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(map_work_order).collect())
    }

    async fn transition_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        patch: WorkOrderStatePatch<'_>,
        event: NewWorkOrderEvent<'_>,
    ) -> Result<Uuid> {
        // 1. Advance the state column (+ monotonic column patch). The scope
        // predicate is the tenant/project guard: a zero-row UPDATE means the
        // work order is absent or out of scope, and we short-circuit BEFORE
        // appending a ledger row (so a cross-tenant id can never orphan an
        // audit row). `event.to_state` is the single source of truth for the
        // target — the ledger row below records the same value by construction.
        let result = sqlx::query!(
            r#"
            UPDATE work_orders
            SET state = $4,
                approved_by = COALESCE($5, approved_by),
                approved_at = COALESCE($6, approved_at),
                dispatched_at = COALESCE($7, dispatched_at),
                claimed_by_runner = COALESCE($8, claimed_by_runner),
                result_ref = COALESCE($9, result_ref),
                failure_reason = COALESCE($10, failure_reason),
                owner_overrides = COALESCE($11, owner_overrides),
                routing_label = COALESCE($12, routing_label),
                updated_at = now()
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            event.work_order_id,
            event.to_state.as_db_str(),
            patch.approved_by,
            patch.approved_at,
            patch.dispatched_at,
            patch.claimed_by_runner,
            patch.result_ref,
            patch.failure_reason,
            patch.owner_overrides,
            patch.routing_label,
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }

        // 2. Append the immutable ledger row in the SAME executor (C22 inv. 3).
        // The work order is already proven in-scope by the UPDATE above, so no
        // redundant scope-resolution SELECT is needed here.
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

    async fn update_state_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        work_order_id: Uuid,
        to_state: WorkOrderState,
        patch: WorkOrderStatePatch<'_>,
    ) -> Result<()> {
        let result = sqlx::query!(
            r#"
            UPDATE work_orders
            SET state = $4,
                approved_by = COALESCE($5, approved_by),
                approved_at = COALESCE($6, approved_at),
                dispatched_at = COALESCE($7, dispatched_at),
                claimed_by_runner = COALESCE($8, claimed_by_runner),
                result_ref = COALESCE($9, result_ref),
                failure_reason = COALESCE($10, failure_reason),
                owner_overrides = COALESCE($11, owner_overrides),
                routing_label = COALESCE($12, routing_label),
                updated_at = now()
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            work_order_id,
            to_state.as_db_str(),
            patch.approved_by,
            patch.approved_at,
            patch.dispatched_at,
            patch.claimed_by_runner,
            patch.result_ref,
            patch.failure_reason,
            patch.owner_overrides,
            patch.routing_label,
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::{ClusterRepo, SqlxClusterRepo};
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::recommendations::{NewRecommendation, RecommendationRepo, SqlxRecommendationRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use feedbackmonk_core::FeedbackKind;
    use serde_json::json;

    struct Seeded {
        scope: ProjectScope,
        recommendation_id: Uuid,
        cluster_id: Uuid,
    }

    async fn seed(pool: &PgPool, email: &str) -> Seeded {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let crepo = SqlxClusterRepo::new(pool.clone());
        let rrepo = SqlxRecommendationRepo::new(pool.clone());
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
        Seeded { scope, recommendation_id: rec.id, cluster_id: cluster.id }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_starts_in_draft(pool: PgPool) {
        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let s = seed(&pool, "wo-draft@example.com").await;

        let wo = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: Some(s.recommendation_id),
                    cluster_id: Some(s.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "Fix the bug",
                    instructions: "Do the thing",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(wo.state, WorkOrderState::Draft);
        assert_eq!(wo.autonomy_rung, 1);
        assert!(wo.approved_by.is_none());

        let got = repo.get(&s.scope, wo.id).await.unwrap();
        assert_eq!(got, wo);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_state_in_executor_advances_state_and_patches(pool: PgPool) {
        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let s = seed(&pool, "wo-advance@example.com").await;
        let wo = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: Some(s.recommendation_id),
                    cluster_id: Some(s.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "Fix",
                    instructions: "Do",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: None,
                },
            )
            .await
            .unwrap();

        let approver = Uuid::new_v4();
        let mut tx = pool.begin().await.unwrap();
        repo.update_state_in_executor(
            &s.scope,
            &mut tx,
            wo.id,
            WorkOrderState::Approved,
            WorkOrderStatePatch {
                approved_by: Some(approver),
                approved_at: Some(Utc::now()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let got = repo.get(&s.scope, wo.id).await.unwrap();
        assert_eq!(got.state, WorkOrderState::Approved);
        assert_eq!(got.approved_by, Some(approver));
        assert!(got.approved_at.is_some());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn transition_in_executor_writes_state_and_ledger_atomically(pool: PgPool) {
        use crate::work_order_events::{SqlxWorkOrderEventRepo, WorkOrderEventRepo};

        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let events = SqlxWorkOrderEventRepo::new(pool.clone());
        let s = seed(&pool, "wo-transition@example.com").await;
        let wo = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: Some(s.recommendation_id),
                    cluster_id: Some(s.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "Fix",
                    instructions: "Do",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: None,
                },
            )
            .await
            .unwrap();

        let approver = Uuid::new_v4();
        let mut tx = pool.begin().await.unwrap();
        let audit_id = repo
            .transition_in_executor(
                &s.scope,
                &mut tx,
                WorkOrderStatePatch {
                    approved_by: Some(approver),
                    approved_at: Some(Utc::now()),
                    ..Default::default()
                },
                NewWorkOrderEvent {
                    work_order_id: wo.id,
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
        tx.commit().await.unwrap();

        // State advanced + patch applied.
        let got = repo.get(&s.scope, wo.id).await.unwrap();
        assert_eq!(got.state, WorkOrderState::Approved);
        assert_eq!(got.approved_by, Some(approver));

        // ...and the ledger holds exactly the paired row (same audit id).
        let ledger = events.list_for_work_order(&s.scope, wo.id).await.unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].id, audit_id);
        assert_eq!(ledger[0].to_state, WorkOrderState::Approved);
        assert_eq!(ledger[0].event_type, "approve");
        assert_eq!(ledger[0].actor, "admin");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn transition_in_executor_cross_tenant_writes_nothing(pool: PgPool) {
        use crate::work_order_events::{SqlxWorkOrderEventRepo, WorkOrderEventRepo};

        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let events = SqlxWorkOrderEventRepo::new(pool.clone());
        let s1 = seed(&pool, "wo-tx-owner1@example.com").await;
        let s2 = seed(&pool, "wo-tx-owner2@example.com").await;
        let wo = repo
            .create(
                &s1.scope,
                NewWorkOrder {
                    recommendation_id: Some(s1.recommendation_id),
                    cluster_id: Some(s1.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "Fix",
                    instructions: "Do",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: None,
                },
            )
            .await
            .unwrap();

        // s2 cannot transition s1's work order: the scoped UPDATE matches no
        // row → NotFound, and NO orphan ledger row is appended.
        let mut tx = pool.begin().await.unwrap();
        let err = repo
            .transition_in_executor(
                &s2.scope,
                &mut tx,
                WorkOrderStatePatch::default(),
                NewWorkOrderEvent {
                    work_order_id: wo.id,
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
        tx.rollback().await.unwrap();

        assert!(events.list_for_work_order(&s1.scope, wo.id).await.unwrap().is_empty());
        assert_eq!(repo.get(&s1.scope, wo.id).await.unwrap().state, WorkOrderState::Draft);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cross_tenant_get_and_create_rejected(pool: PgPool) {
        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let s1 = seed(&pool, "wo-owner1@example.com").await;
        let s2 = seed(&pool, "wo-owner2@example.com").await;

        let wo = repo
            .create(
                &s1.scope,
                NewWorkOrder {
                    recommendation_id: Some(s1.recommendation_id),
                    cluster_id: Some(s1.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "Fix",
                    instructions: "Do",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: None,
                },
            )
            .await
            .unwrap();

        // s2 cannot read s1's work order.
        assert!(matches!(repo.get(&s2.scope, wo.id).await.unwrap_err(), RepoError::NotFound));

        // s2 cannot create a work order anchored on s1's recommendation.
        let err = repo
            .create(
                &s2.scope,
                NewWorkOrder {
                    recommendation_id: Some(s1.recommendation_id),
                    cluster_id: Some(s1.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "x",
                    instructions: "y",
                    owner_overrides: None,
                    autonomy_rung: 0,
                    routing_label: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }

    // ---- C31 (P6): owner-authored provenance + routing_label ----------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn owner_authored_create_with_no_provenance_succeeds(pool: PgPool) {
        // C31 / D-P6-2: an owner-authored order has NO feedback grounding —
        // both provenance FKs are None. It still enters `draft`.
        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let s = seed(&pool, "wo-owner-authored@example.com").await;

        let wo = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: None,
                    cluster_id: None,
                    action_type: ActionType::Enhancement,
                    title: "Owner-authored task",
                    instructions: "Do exactly this",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: Some("ci-runner"),
                },
            )
            .await
            .unwrap();
        assert_eq!(wo.state, WorkOrderState::Draft);
        assert!(wo.recommendation_id.is_none());
        assert!(wo.cluster_id.is_none());
        assert_eq!(wo.routing_label.as_deref(), Some("ci-runner"));

        let got = repo.get(&s.scope, wo.id).await.unwrap();
        assert_eq!(got, wo);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mixed_provenance_pair_is_rejected_before_the_db(pool: PgPool) {
        // Half-null provenance is unrepresentable (migration 00028 CHECK); the
        // repo rejects it with `Conflict` BEFORE the DB CHECK would fire.
        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let s = seed(&pool, "wo-mixed-pair@example.com").await;

        // recommendation_id Some, cluster_id None.
        let err = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: Some(s.recommendation_id),
                    cluster_id: None,
                    action_type: ActionType::BugFix,
                    title: "x",
                    instructions: "y",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Conflict), "mixed pair must Conflict; got {err:?}");

        // cluster_id Some, recommendation_id None (the other half).
        let err = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: None,
                    cluster_id: Some(s.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "x",
                    instructions: "y",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Conflict), "mixed pair must Conflict; got {err:?}");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn routing_label_is_set_forward_by_the_patch(pool: PgPool) {
        // C31 §4: a patch `routing_label = Some` overwrites; `None` COALESCEs to
        // the existing value (set-forward, like owner_overrides).
        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let s = seed(&pool, "wo-routing-setfwd@example.com").await;
        let wo = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: Some(s.recommendation_id),
                    cluster_id: Some(s.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "Fix",
                    instructions: "Do",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: Some("runner-create"),
                },
            )
            .await
            .unwrap();

        // A patch with routing_label = Some overrides the create-time value.
        let mut tx = pool.begin().await.unwrap();
        repo.update_state_in_executor(
            &s.scope,
            &mut tx,
            wo.id,
            WorkOrderState::Approved,
            WorkOrderStatePatch {
                approved_by: Some(Uuid::new_v4()),
                approved_at: Some(Utc::now()),
                routing_label: Some("runner-approve"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            repo.get(&s.scope, wo.id).await.unwrap().routing_label.as_deref(),
            Some("runner-approve")
        );

        // A subsequent patch with routing_label = None keeps the existing value.
        let mut tx = pool.begin().await.unwrap();
        repo.update_state_in_executor(
            &s.scope,
            &mut tx,
            wo.id,
            WorkOrderState::Dispatched,
            WorkOrderStatePatch { dispatched_at: Some(Utc::now()), ..Default::default() },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            repo.get(&s.scope, wo.id).await.unwrap().routing_label.as_deref(),
            Some("runner-approve"),
            "None routing_label must COALESCE to the existing value"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_for_runner_filters_by_routing_label(pool: PgPool) {
        // C31 §5 / D-P6-3: an unlabeled order is visible to every runner; a
        // labeled order only to the runner whose sub matches.
        let repo = SqlxWorkOrderRepo::new(pool.clone());
        let s = seed(&pool, "wo-runner-poll@example.com").await;

        // Unlabeled (first-claim-wins) order.
        let unlabeled = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: Some(s.recommendation_id),
                    cluster_id: Some(s.cluster_id),
                    action_type: ActionType::BugFix,
                    title: "Unlabeled",
                    instructions: "Any runner",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: None,
                },
            )
            .await
            .unwrap();
        // Labeled to "runner-A".
        let labeled = repo
            .create(
                &s.scope,
                NewWorkOrder {
                    recommendation_id: None,
                    cluster_id: None,
                    action_type: ActionType::BugFix,
                    title: "Labeled",
                    instructions: "Only runner-A",
                    owner_overrides: None,
                    autonomy_rung: 1,
                    routing_label: Some("runner-A"),
                },
            )
            .await
            .unwrap();

        // runner-A sees BOTH (unlabeled + its own labeled).
        let for_a = repo.list_for_runner(&s.scope, None, None, "runner-A").await.unwrap();
        let ids_a: Vec<Uuid> = for_a.iter().map(|w| w.id).collect();
        assert!(ids_a.contains(&unlabeled.id));
        assert!(ids_a.contains(&labeled.id));

        // runner-B sees ONLY the unlabeled one (the labeled order is invisible).
        let for_b = repo.list_for_runner(&s.scope, None, None, "runner-B").await.unwrap();
        let ids_b: Vec<Uuid> = for_b.iter().map(|w| w.id).collect();
        assert!(ids_b.contains(&unlabeled.id));
        assert!(!ids_b.contains(&labeled.id), "runner-B must NOT see runner-A's labeled order");

        // The admin `list` is untouched — owners see every order regardless.
        let admin = repo.list(&s.scope, None, None).await.unwrap();
        assert_eq!(admin.len(), 2);
    }
}
