//! Work-order API + approval state machine (Contract C22, FR-FBR-22 /
//! FR-FBR-25a). **THE security boundary between public feedback and code
//! execution.**
//!
//! This handler owns the C22 HTTP surface (project-scoped under
//! `/api/v1/projects/:project_id/work-orders`), the transition core (the legal
//! table + the authz matrix + **the approval gate**), and the runner
//! write-token verifier *seam* (Q14 — issuance is P5b).
//!
//! ## The five hard invariants (C22)
//!
//! 1. **No state ≥ `dispatched` without a recorded owner-authored `approve`
//!    event.** [`transition_work_order`] consults
//!    `WorkOrderEventRepo::has_approved_event` BEFORE opening the txn for any
//!    transition whose target [`WorkOrderState::is_execution_state`] — rejecting
//!    with [`WorkOrderTransitionError::ApprovalRequired`]. The
//!    `approval-gate-enforcement` Verification Oracle independently proves the
//!    same property from the ledger (detection-from-state, anti-reward-hacking).
//! 2. **Actor-role enforcement** per the authz matrix — owner-only events vs
//!    runner-only events, each only from the legal prior state.
//! 3. **Same-transaction event-row parity** — `update_state_in_executor` +
//!    `append_in_executor` inside one `pool.begin()` txn. Audit can never drift
//!    from state.
//! 4. `draft` entered only at autonomy Rung ≥ 1 (rung 0 never produces a work
//!    order); `reported → completed` is auto only at Rung ≥ 2.
//! 5. Terminal states (`completed`, `cancelled`) are immutable.
//!
//! ## Why a core fn the tests drive directly
//!
//! [`transition_work_order`] + [`create_work_order`] are `pub` so the
//! bypass-resistant `tests/work_order_state_machine.rs` corpus (Probe C of the
//! oracle) can attack the gate directly — asserting no `event_type`/path drives
//! state ≥ `dispatched` without a prior admin `approve` event, and that the
//! ledger never holds an orphan `dispatched`. A passing happy-path test is NOT
//! enough for this surface (Testability Gate Flag 1).

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use feedbackmonk_core::{
    is_legal_transition, ActionType, WorkOrderState, WorkOrderTransitionError,
};
use feedbackmonk_jwt::verify_with_leeway as jwt_verify_with_leeway;
use feedbackmonk_repository::{
    NewWorkOrder, NewWorkOrderEvent, ProjectScope, RepoError, WorkOrder, WorkOrderEvent,
    WorkOrderStatePatch,
};

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::state::AppState;

// ===========================================================================
// Runner write-token verifier SEAM (Q14)
// ===========================================================================

/// The signed class marker that distinguishes a **runner write-token** from an
/// end-user JWT. Both are `EdDSA` tokens minted from the SAME per-project
/// Ed25519 signing-key infrastructure (DEC-FBR-04) — the marker is what makes
/// the runner token a distinct *credential class* (Q14). A replayed end-user
/// JWT lacks it and is rejected at the runner endpoints.
///
/// **Issuance is P5b (FR-FBR-24).** P5a freezes only the verification/authz
/// seam; nothing in this repo mints a token carrying this scope. Single-sourced
/// here so the P5b minter and this verifier reference one constant.
pub const RUNNER_TOKEN_SCOPE: &str = "runner:write";

/// A verified runner identity (the token `sub`) bound to one project. Returned
/// by [`verify_runner_token`]; the runner endpoints stamp `sub` into
/// `work_orders.claimed_by_runner` + the event `actor_id`.
#[derive(Debug, Clone)]
pub struct RunnerIdentity {
    pub sub: String,
}

/// Read the signature-covered `scope` claim from a JWT payload. Returns `None`
/// on any structural/parse failure — callers MUST already have verified the
/// token via [`jwt_verify_with_leeway`] (which proves the signature covers
/// these exact payload bytes), so a present-and-equal scope claim is
/// trustworthy. Decoupled from `feedbackmonk_jwt` deliberately: the audited
/// crypto stays untouched; the class check is a thin authz layer on signed
/// bytes.
fn runner_scope_claim(token: &str) -> Option<String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let payload_b64 = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    let payload: serde_json::Map<String, JsonValue> = serde_json::from_slice(&bytes).ok()?;
    payload.get("scope")?.as_str().map(str::to_string)
}

/// Extract a `Bearer <token>` value from the `Authorization` header.
fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let stripped = value.strip_prefix("Bearer ")?;
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

fn current_unix_timestamp() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    #[allow(clippy::cast_possible_wrap)]
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Verify a project-scoped runner write-token (Q14 seam). Two gates:
///   1. **Authenticity** — [`jwt_verify_with_leeway`] against the project's
///      active Ed25519 signing keys, with `aud == project_id` + strict `exp`
///      (the audited end-user verifier, reused verbatim).
///   2. **Class** — the signature-covered `scope` claim MUST equal
///      [`RUNNER_TOKEN_SCOPE`]. A valid end-user JWT (no such claim) is rejected
///      `403 Forbidden` — it authenticates a *person*, never the runner.
///
/// Returns the resolved [`ProjectScope`] (minted pre-auth from the path's
/// `project_id`, like the public submission path — the token, not a session, is
/// the credential) alongside the [`RunnerIdentity`].
async fn verify_runner_token(
    state: &AppState,
    project_id: Uuid,
    headers: &HeaderMap,
) -> Result<(ProjectScope, RunnerIdentity), ApiError> {
    let token = extract_bearer(headers).ok_or(ApiError::Unauthorized)?;

    // Pre-auth project resolution (DEC-PODS-001 boundary method): the runner is
    // authenticated by the TOKEN, not an admin session, so we mint the scope
    // straight from the path — exactly as the public submit handler does.
    let scope = state.projects.open_for_submission(project_id).await?;
    let active_keys = state.signing_keys.list_active(&scope).await?;
    let now_unix = current_unix_timestamp();

    let claims = jwt_verify_with_leeway(
        &token,
        project_id,
        &active_keys,
        now_unix,
        state.jwt_iat_leeway_seconds,
    )
    .map_err(|_| ApiError::Unauthorized)?;

    // THE class gate: reject anything that is not a runner write-token, even a
    // perfectly valid end-user JWT for this project.
    if runner_scope_claim(&token).as_deref() != Some(RUNNER_TOKEN_SCOPE) {
        return Err(ApiError::Forbidden);
    }

    Ok((scope, RunnerIdentity { sub: claims.sub }))
}

// ===========================================================================
// Actor model + the transition core
// ===========================================================================

/// The class of identity authoring a transition (C22 authz matrix, inv. 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorClass {
    /// Authenticated owner/admin (`AdminSession`).
    Admin,
    /// Project-scoped runner write-token (Q14).
    Runner,
    /// Server-side automation (`dispatch`; auto-`accept` at rung ≥ 2).
    System,
}

impl ActorClass {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Runner => "runner",
            Self::System => "system",
        }
    }
}

/// A concrete authenticated actor authoring a transition.
#[derive(Debug, Clone)]
pub enum Actor {
    /// Owner/admin; `id` is the tenant UUID stamped into `approved_by` +
    /// `actor_id`.
    Admin { id: Uuid },
    /// Runner; `sub` is the token subject stamped into `claimed_by_runner` +
    /// `actor_id`.
    Runner { sub: String },
    /// Server-side automation; no external identity.
    System,
}

impl Actor {
    fn class(&self) -> ActorClass {
        match self {
            Self::Admin { .. } => ActorClass::Admin,
            Self::Runner { .. } => ActorClass::Runner,
            Self::System => ActorClass::System,
        }
    }

    fn actor_id(&self) -> Option<String> {
        match self {
            Self::Admin { id } => Some(id.to_string()),
            Self::Runner { sub } => Some(sub.clone()),
            Self::System => None,
        }
    }
}

/// Resolve `(to_state, allowed_actors)` for an `event_type` given the current
/// `from` state, per the FROZEN C22 legal-transition + authz tables. Encodes
/// each event's from-constraint; a wrong prior state yields
/// [`WorkOrderTransitionError::IllegalTransition`]. Unknown `event_type` is
/// safe-denied (`ActorNotAuthorized`) — the HTTP handlers also allowlist per
/// endpoint, so unknown events never reach here in practice.
fn resolve_transition(
    event_type: &str,
    from: WorkOrderState,
) -> Result<(WorkOrderState, &'static [ActorClass]), WorkOrderTransitionError> {
    use ActorClass::{Admin, Runner, System};
    use WorkOrderState as S;

    // (required_from_ok, to_state, allowed actor classes)
    let illegal = |to: WorkOrderState| WorkOrderTransitionError::IllegalTransition { from, to };

    let resolved: (bool, WorkOrderState, &'static [ActorClass]) = match event_type {
        // ---- owner-authored (AdminSession) --------------------------------
        "approve" => (from == S::Draft, S::Approved, &[Admin]),
        "cancel" => (
            // cancel is legal from every non-terminal state EXCEPT reported
            // (whose owner exits are accept / request-changes / reject).
            !from.is_terminal() && from != S::Reported,
            S::Cancelled,
            &[Admin],
        ),
        "accept" => (from == S::Reported, S::Completed, &[Admin, System]),
        "request-changes" => (from == S::Reported, S::Building, &[Admin]),
        "reject" => (from == S::Reported, S::Failed, &[Admin]),
        "retry" => (from == S::Failed, S::Approved, &[Admin]),

        // ---- system ------------------------------------------------------
        "dispatch" => (from == S::Approved, S::Dispatched, &[System]),

        // ---- runner-authored (write-token) -------------------------------
        "claim" => (from == S::Dispatched, S::Claimed, &[Runner]),
        "building" => (from == S::Claimed, S::Building, &[Runner]),
        "verifying" => (from == S::Building, S::Verifying, &[Runner]),
        "reported" => (from == S::Verifying, S::Reported, &[Runner]),
        "failed" => (
            from == S::Building || from == S::Verifying,
            S::Failed,
            &[Runner],
        ),

        // Unknown event_type: safe-deny.
        _ => return Err(WorkOrderTransitionError::ActorNotAuthorized),
    };

    let (from_ok, to, allowed) = resolved;
    if !from_ok {
        return Err(illegal(to));
    }
    Ok((to, allowed))
}

/// Outcome of a committed transition.
#[derive(Debug, Clone)]
pub struct TransitionOutcome {
    pub audit_id: Uuid,
    pub from_state: WorkOrderState,
    pub to_state: WorkOrderState,
}

/// The failure surface of the transition core. `Rule` carries the FROZEN
/// state-machine error so `tests/work_order_state_machine.rs` asserts the exact
/// variant (the bypass-resistance corpus); `NotFound`/`Internal` cover scope +
/// DB failures. The HTTP handlers map this to [`ApiError`] at the edge so
/// `error.rs` stays untouched.
#[derive(Debug)]
pub enum TransitionFailure {
    Rule(WorkOrderTransitionError),
    NotFound,
    Internal(String),
}

impl From<RepoError> for TransitionFailure {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound => Self::NotFound,
            other => Self::Internal(format!("repository error: {other:?}")),
        }
    }
}

impl From<sqlx::Error> for TransitionFailure {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(format!("database error: {e}"))
    }
}

impl From<TransitionFailure> for ApiError {
    fn from(f: TransitionFailure) -> Self {
        match f {
            TransitionFailure::Rule(
                e @ (WorkOrderTransitionError::IllegalTransition { .. }
                | WorkOrderTransitionError::TerminalState),
            ) => Self::Conflict(transition_error_body(&e)),
            // The approval gate + actor-authz denials are 403. The body is
            // deliberately generic (no gate-leak): a 403 does not disclose
            // whether approval was missing or the actor was wrong.
            TransitionFailure::Rule(
                WorkOrderTransitionError::ApprovalRequired { .. }
                | WorkOrderTransitionError::ActorNotAuthorized,
            ) => Self::Forbidden,
            TransitionFailure::NotFound => Self::NotFound,
            TransitionFailure::Internal(m) => Self::Internal(m),
        }
    }
}

fn transition_error_body(e: &WorkOrderTransitionError) -> String {
    let (code, from, to) = match e {
        WorkOrderTransitionError::IllegalTransition { from, to } => {
            ("IllegalTransition", Some(*from), Some(*to))
        }
        WorkOrderTransitionError::TerminalState => ("TerminalState", None, None),
        WorkOrderTransitionError::ApprovalRequired { to } => ("ApprovalRequired", None, Some(*to)),
        WorkOrderTransitionError::ActorNotAuthorized => ("ActorNotAuthorized", None, None),
    };
    json!({
        "error": code,
        "from_state": from.map(WorkOrderState::as_db_str),
        "to_state": to.map(WorkOrderState::as_db_str),
    })
    .to_string()
}

/// **The transition core (C22 inv. 1–5).** Validates, enforces the approval
/// gate, and commits the state change + the ledger row in ONE transaction.
///
/// Order (each step a hard gate):
///   1. Load the work order (scope-bound). Absent/cross-tenant → `NotFound`.
///   2. **Terminal check (inv. 5)** — a `completed`/`cancelled` order rejects
///      every transition.
///   3. **Resolve `to` + allowed actors (legal table + authz, inv. 1/2)** — a
///      wrong prior state → `IllegalTransition`.
///   4. **Actor authz (inv. 2)** — the actor's class must be in the allowed set
///      → else `ActorNotAuthorized`.
///   5. Defense-in-depth: `is_legal_transition(from, to)` must also hold.
///   6. **THE approval gate (inv. 1)** — if `to.is_execution_state()`, the
///      ledger MUST already carry an owner-authored `approve` event
///      (`has_approved_event`); else `ApprovalRequired`. Checked BEFORE the txn
///      opens AND audited (the ledger is the source of truth the oracle reads).
///   7. **Same-txn parity (inv. 3)** — `update_state_in_executor` +
///      `append_in_executor` in one `pool.begin()`; the event append failing
///      rolls the state change back.
pub async fn transition_work_order(
    state: &AppState,
    scope: &ProjectScope,
    work_order_id: Uuid,
    event_type: &str,
    actor: &Actor,
    detail: Option<&JsonValue>,
    patch: WorkOrderStatePatch<'_>,
) -> Result<TransitionOutcome, TransitionFailure> {
    // 1. Load (scope-bound).
    let wo = state.work_orders.get(scope, work_order_id).await?;
    let from = wo.state;

    // 2. Terminal (inv. 5).
    if from.is_terminal() {
        return Err(TransitionFailure::Rule(WorkOrderTransitionError::TerminalState));
    }

    // 3. Resolve target + allowed actors (legal table + authz; inv. 1/2).
    let (to, allowed) = resolve_transition(event_type, from).map_err(TransitionFailure::Rule)?;

    // 4. Actor authz (inv. 2).
    if !allowed.contains(&actor.class()) {
        return Err(TransitionFailure::Rule(
            WorkOrderTransitionError::ActorNotAuthorized,
        ));
    }

    // 5. Defense-in-depth: the resolved edge must be in the frozen table.
    if !is_legal_transition(from, to) {
        return Err(TransitionFailure::Rule(
            WorkOrderTransitionError::IllegalTransition { from, to },
        ));
    }

    // 6. THE approval gate (inv. 1) — pre-DB-check AND audited.
    if to.is_execution_state()
        && !state
            .work_order_events
            .has_approved_event(scope, work_order_id)
            .await?
    {
        return Err(TransitionFailure::Rule(
            WorkOrderTransitionError::ApprovalRequired { to },
        ));
    }

    // 7. Same-txn parity (inv. 3): state update + ledger row, atomic.
    let mut tx = state.pool.begin().await?;
    state
        .work_orders
        .update_state_in_executor(scope, &mut tx, work_order_id, to, patch)
        .await?;
    let audit_id = state
        .work_order_events
        .append_in_executor(
            scope,
            &mut tx,
            NewWorkOrderEvent {
                work_order_id,
                from_state: Some(from),
                to_state: to,
                event_type,
                actor: actor.class().as_db_str(),
                actor_id: actor.actor_id().as_deref(),
                detail,
            },
        )
        .await?;
    tx.commit().await?;

    tracing::info!(
        target: "work_order",
        %work_order_id,
        from = from.as_db_str(),
        to = to.as_db_str(),
        event_type,
        actor = actor.class().as_db_str(),
        "work-order transition committed"
    );

    Ok(TransitionOutcome {
        audit_id,
        from_state: from,
        to_state: to,
    })
}

/// Create a `draft` work order from a recommendation (inv. 4 rung-gate).
/// `autonomy_rung` must be ≥ 1 (rung 0 never produces a work order) and ≤ 3.
/// The recommendation (and its cluster) are resolved within scope by the repo;
/// a cross-tenant `recommendation_id` → `NotFound`.
pub async fn create_work_order(
    state: &AppState,
    scope: &ProjectScope,
    recommendation_id: Uuid,
    autonomy_rung: i32,
    owner_overrides: Option<&JsonValue>,
) -> Result<WorkOrder, ApiError> {
    // inv. 4: rung 0 never produces a work order.
    if !(1..=3).contains(&autonomy_rung) {
        return Err(ApiError::BadRequest(
            "autonomy_rung must be between 1 and 3 (rung 0 never produces a work order)".into(),
        ));
    }

    // Read the recommendation (scope-bound) to derive the order's fields. The
    // recommendation is provenance; the work order is the order (Q17).
    let rec = state.recommendations.get(scope, recommendation_id).await?;

    let wo = state
        .work_orders
        .create(
            scope,
            NewWorkOrder {
                recommendation_id: rec.id,
                cluster_id: rec.cluster_id,
                action_type: rec.action_type,
                title: &rec.title,
                instructions: &rec.body,
                owner_overrides,
                autonomy_rung,
            },
        )
        .await?;
    Ok(wo)
}

// ===========================================================================
// Response structs (the A → C seam — Interface Contract 1)
// ===========================================================================

/// JSON view of a work order. `snake_case` keys (serde default) to match the
/// existing admin handlers; `state` / `action_type` carry their own enum
/// casing (`kebab-case` / `snake_case`). Worker C mirrors this into
/// `types.gen.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderView {
    pub id: Uuid,
    pub recommendation_id: Uuid,
    pub cluster_id: Uuid,
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<WorkOrder> for WorkOrderView {
    fn from(w: WorkOrder) -> Self {
        Self {
            id: w.id,
            recommendation_id: w.recommendation_id,
            cluster_id: w.cluster_id,
            action_type: w.action_type,
            title: w.title,
            instructions: w.instructions,
            owner_overrides: w.owner_overrides,
            autonomy_rung: w.autonomy_rung,
            state: w.state,
            approved_by: w.approved_by,
            approved_at: w.approved_at,
            dispatched_at: w.dispatched_at,
            claimed_by_runner: w.claimed_by_runner,
            result_ref: w.result_ref,
            failure_reason: w.failure_reason,
            created_at: w.created_at,
            updated_at: w.updated_at,
        }
    }
}

/// JSON view of one append-only ledger row.
#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderEventView {
    pub id: Uuid,
    pub from_state: Option<WorkOrderState>,
    pub to_state: WorkOrderState,
    pub event_type: String,
    pub actor: String,
    pub actor_id: Option<String>,
    pub detail: Option<JsonValue>,
    pub at: DateTime<Utc>,
}

impl From<WorkOrderEvent> for WorkOrderEventView {
    fn from(e: WorkOrderEvent) -> Self {
        Self {
            id: e.id,
            from_state: e.from_state,
            to_state: e.to_state,
            event_type: e.event_type,
            actor: e.actor,
            actor_id: e.actor_id,
            detail: e.detail,
            at: e.at,
        }
    }
}

/// `GET /work-orders` response.
#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderListResponse {
    pub items: Vec<WorkOrderView>,
}

/// `GET /work-orders/:id` response — the order plus its full event ledger.
#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderDetailResponse {
    #[serde(flatten)]
    pub work_order: WorkOrderView,
    pub events: Vec<WorkOrderEventView>,
}

/// Response for every transition endpoint (approve / transition / claim /
/// runner-transition).
#[derive(Debug, Clone, Serialize)]
pub struct TransitionResponse {
    pub work_order_id: Uuid,
    pub from_state: WorkOrderState,
    pub to_state: WorkOrderState,
    pub event_type: String,
    pub audit_id: Uuid,
    /// Set when a runner `reported` auto-accepts at autonomy rung ≥ 2 (inv. 4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_accepted: Option<bool>,
}

// ===========================================================================
// Request bodies
// ===========================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct CreateWorkOrderRequest {
    pub recommendation_id: Uuid,
    pub autonomy_rung: i32,
    #[serde(default)]
    pub owner_overrides: Option<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApproveRequest {
    #[serde(default)]
    pub owner_overrides: Option<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OwnerTransitionRequest {
    /// One of: cancel | accept | request-changes | reject | retry.
    pub event_type: String,
    /// For `request-changes`, carries the new `owner_overrides` delta (Q17).
    #[serde(default)]
    pub detail: Option<JsonValue>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RunnerTransitionRequest {
    /// One of: building | verifying | reported | failed.
    pub event_type: String,
    #[serde(default)]
    pub detail: Option<JsonValue>,
    /// Set on `reported` — the runner's result reference (never a dump).
    #[serde(default)]
    pub result_ref: Option<JsonValue>,
    /// Set on `failed`.
    #[serde(default)]
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListQuery {
    pub state: Option<String>,
    pub cluster_id: Option<Uuid>,
}

// Owner / runner event allowlists per endpoint — defense-in-depth alongside the
// core's authz matrix (a wrong event for the endpoint is a 400, not a 403/409).
const OWNER_TRANSITION_EVENTS: &[&str] = &["cancel", "accept", "request-changes", "reject", "retry"];
const RUNNER_TRANSITION_EVENTS: &[&str] = &["building", "verifying", "reported", "failed"];

// ===========================================================================
// Admin HTTP handlers (behind AdminSession; project-scoped URL)
// ===========================================================================

/// Resolve the `ProjectScope` for a project-scoped admin URL: the
/// `AdminSession` proves the tenant, `projects.open` validates the path's
/// `project_id` belongs to that tenant (cross-tenant → `NotFound`).
async fn admin_scope(
    state: &AppState,
    session: &AdminSession,
    project_id: Uuid,
) -> Result<ProjectScope, ApiError> {
    Ok(state.projects.open(&session.scope, project_id).await?)
}

/// `POST /api/v1/projects/:project_id/work-orders` — create a draft (admin).
pub async fn create(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
    Json(req): Json<CreateWorkOrderRequest>,
) -> Result<Json<WorkOrderView>, ApiError> {
    let scope = admin_scope(&state, &session, project_id).await?;
    let wo = create_work_order(
        &state,
        &scope,
        req.recommendation_id,
        req.autonomy_rung,
        req.owner_overrides.as_ref(),
    )
    .await?;
    Ok(Json(WorkOrderView::from(wo)))
}

/// `GET /api/v1/projects/:project_id/work-orders` — list (admin). Filters:
/// `state`, `cluster_id`.
pub async fn list(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<WorkOrderListResponse>, ApiError> {
    let scope = admin_scope(&state, &session, project_id).await?;
    let items = state
        .work_orders
        .list(&scope, q.state.as_deref(), q.cluster_id)
        .await?
        .into_iter()
        .map(WorkOrderView::from)
        .collect();
    Ok(Json(WorkOrderListResponse { items }))
}

/// `GET /api/v1/projects/:project_id/work-orders/:id` — detail + ledger (admin).
pub async fn detail(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, work_order_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<WorkOrderDetailResponse>, ApiError> {
    let scope = admin_scope(&state, &session, project_id).await?;
    let wo = state.work_orders.get(&scope, work_order_id).await?;
    let events = state
        .work_order_events
        .list_for_work_order(&scope, work_order_id)
        .await?
        .into_iter()
        .map(WorkOrderEventView::from)
        .collect();
    Ok(Json(WorkOrderDetailResponse {
        work_order: WorkOrderView::from(wo),
        events,
    }))
}

/// `POST /api/v1/projects/:project_id/work-orders/:id/approve` — **the owner
/// approval gate** (admin). Writes the `approve` event that opens C22 inv. 1,
/// stamps `approved_by`/`approved_at` + the Q17 `owner_overrides`, and projects
/// the recommendation status (`approved` / `tweaked_approved`).
pub async fn approve(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, work_order_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ApproveRequest>,
) -> Result<Json<TransitionResponse>, ApiError> {
    let scope = admin_scope(&state, &session, project_id).await?;
    let admin_id = session.scope.tenant_id();

    // Read first to recover the recommendation_id for the status projection +
    // an early 404.
    let wo = state.work_orders.get(&scope, work_order_id).await?;

    let tweaked = req.owner_overrides.as_ref().is_some_and(|v| !v.is_null());
    let now = Utc::now();
    let patch = WorkOrderStatePatch {
        approved_by: Some(admin_id),
        approved_at: Some(now),
        owner_overrides: req.owner_overrides.as_ref(),
        ..Default::default()
    };

    let outcome = transition_work_order(
        &state,
        &scope,
        work_order_id,
        "approve",
        &Actor::Admin { id: admin_id },
        req.owner_overrides.as_ref(),
        patch,
    )
    .await
    .map_err(ApiError::from)?;

    // Post-commit, best-effort: project the recommendation status. The
    // work-order ledger is authoritative; this denormalized projection lagging
    // never fails an approval (mirrors the email-after-commit pattern).
    let rec_status = if tweaked { "tweaked_approved" } else { "approved" };
    if let Err(e) = state
        .recommendations
        .set_status(&scope, wo.recommendation_id, rec_status)
        .await
    {
        tracing::warn!(
            target: "work_order",
            %work_order_id,
            recommendation_id = %wo.recommendation_id,
            error = ?e,
            "recommendation status projection failed (approval still committed)"
        );
    }

    Ok(Json(TransitionResponse {
        work_order_id,
        from_state: outcome.from_state,
        to_state: outcome.to_state,
        event_type: "approve".into(),
        audit_id: outcome.audit_id,
        auto_accepted: None,
    }))
}

/// `POST /api/v1/projects/:project_id/work-orders/:id/transition` — owner
/// transitions: cancel / accept / request-changes / reject / retry (admin).
pub async fn owner_transition(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, work_order_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<OwnerTransitionRequest>,
) -> Result<Json<TransitionResponse>, ApiError> {
    if !OWNER_TRANSITION_EVENTS.contains(&req.event_type.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "event_type must be one of {OWNER_TRANSITION_EVENTS:?}; got {:?}",
            req.event_type
        )));
    }
    let scope = admin_scope(&state, &session, project_id).await?;
    let admin_id = session.scope.tenant_id();

    let outcome = transition_work_order(
        &state,
        &scope,
        work_order_id,
        &req.event_type,
        &Actor::Admin { id: admin_id },
        req.detail.as_ref(),
        WorkOrderStatePatch::default(),
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(TransitionResponse {
        work_order_id,
        from_state: outcome.from_state,
        to_state: outcome.to_state,
        event_type: req.event_type,
        audit_id: outcome.audit_id,
        auto_accepted: None,
    }))
}

// ===========================================================================
// Runner HTTP handlers (behind the runner write-token seam; NO CORS)
// ===========================================================================

/// `POST /api/v1/projects/:project_id/work-orders/:id/claim` — runner claims a
/// dispatched order (runner-only). **P5b drives this**; in P5a it is
/// authz-guarded + exercised only by tests / a no-op probe.
pub async fn claim(
    State(state): State<AppState>,
    Path((project_id, work_order_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<TransitionResponse>, ApiError> {
    let (scope, runner) = verify_runner_token(&state, project_id, &headers).await?;
    let patch = WorkOrderStatePatch {
        claimed_by_runner: Some(&runner.sub),
        ..Default::default()
    };
    let outcome = transition_work_order(
        &state,
        &scope,
        work_order_id,
        "claim",
        &Actor::Runner { sub: runner.sub.clone() },
        None,
        patch,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(TransitionResponse {
        work_order_id,
        from_state: outcome.from_state,
        to_state: outcome.to_state,
        event_type: "claim".into(),
        audit_id: outcome.audit_id,
        auto_accepted: None,
    }))
}

/// `POST /api/v1/projects/:project_id/work-orders/:id/runner-transition` —
/// runner lifecycle: building / verifying / reported / failed (runner-only).
/// On `reported` at autonomy rung ≥ 2 the handler auto-authors `accept` (inv. 4
/// — system-authored `reported → completed`).
pub async fn runner_transition(
    State(state): State<AppState>,
    Path((project_id, work_order_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(req): Json<RunnerTransitionRequest>,
) -> Result<Json<TransitionResponse>, ApiError> {
    if !RUNNER_TRANSITION_EVENTS.contains(&req.event_type.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "event_type must be one of {RUNNER_TRANSITION_EVENTS:?}; got {:?}",
            req.event_type
        )));
    }
    let (scope, runner) = verify_runner_token(&state, project_id, &headers).await?;

    let patch = WorkOrderStatePatch {
        result_ref: req.result_ref.as_ref(),
        failure_reason: req.failure_reason.as_deref(),
        ..Default::default()
    };
    let outcome = transition_work_order(
        &state,
        &scope,
        work_order_id,
        &req.event_type,
        &Actor::Runner { sub: runner.sub.clone() },
        req.detail.as_ref(),
        patch,
    )
    .await
    .map_err(ApiError::from)?;

    // inv. 4: `reported → completed` is auto only at autonomy rung ≥ 2. The
    // owner does it otherwise (via /transition `accept`).
    let mut auto_accepted = None;
    if req.event_type == "reported" {
        let wo = state.work_orders.get(&scope, work_order_id).await?;
        if wo.autonomy_rung >= 2 {
            transition_work_order(
                &state,
                &scope,
                work_order_id,
                "accept",
                &Actor::System,
                Some(&json!({ "reason": "auto-accept at autonomy rung >= 2" })),
                WorkOrderStatePatch::default(),
            )
            .await
            .map_err(ApiError::from)?;
            auto_accepted = Some(true);
        }
    }

    Ok(Json(TransitionResponse {
        work_order_id,
        from_state: outcome.from_state,
        to_state: outcome.to_state,
        event_type: req.event_type,
        audit_id: outcome.audit_id,
        auto_accepted,
    }))
}

// ===========================================================================
// Routers (merged WITHOUT .layer(cors) in main.rs)
// ===========================================================================

/// Admin work-order routes — behind `AdminSession`. **No CORS** (admin surface,
/// never a browser embed).
pub fn work_order_admin_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/projects/:project_id/work-orders",
            post(create).get(list),
        )
        .route(
            "/api/v1/projects/:project_id/work-orders/:work_order_id",
            get(detail),
        )
        .route(
            "/api/v1/projects/:project_id/work-orders/:work_order_id/approve",
            post(approve),
        )
        .route(
            "/api/v1/projects/:project_id/work-orders/:work_order_id/transition",
            post(owner_transition),
        )
        .with_state(state)
}

/// Runner work-order routes — behind the runner write-token seam (Q14). **No
/// CORS** (server-to-server runner surface, never a browser embed).
pub fn work_order_runner_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/projects/:project_id/work-orders/:work_order_id/claim",
            post(claim),
        )
        .route(
            "/api/v1/projects/:project_id/work-orders/:work_order_id/runner-transition",
            post(runner_transition),
        )
        .with_state(state)
}

// ===========================================================================
// Unit tests (pure helpers — DB-backed coverage lives in
// tests/work_order_state_machine.rs, Probe C)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    #[test]
    fn approve_resolves_draft_to_approved_admin_only() {
        let (to, allowed) = resolve_transition("approve", WorkOrderState::Draft).unwrap();
        assert_eq!(to, WorkOrderState::Approved);
        assert_eq!(allowed, &[ActorClass::Admin]);
    }

    #[test]
    fn approve_from_non_draft_is_illegal() {
        let err = resolve_transition("approve", WorkOrderState::Approved).unwrap_err();
        assert!(matches!(err, WorkOrderTransitionError::IllegalTransition { .. }));
    }

    #[test]
    fn dispatch_is_system_only_from_approved() {
        let (to, allowed) = resolve_transition("dispatch", WorkOrderState::Approved).unwrap();
        assert_eq!(to, WorkOrderState::Dispatched);
        assert_eq!(allowed, &[ActorClass::System]);
    }

    #[test]
    fn runner_lifecycle_events_are_runner_only() {
        for (ev, from, to) in [
            ("claim", WorkOrderState::Dispatched, WorkOrderState::Claimed),
            ("building", WorkOrderState::Claimed, WorkOrderState::Building),
            ("verifying", WorkOrderState::Building, WorkOrderState::Verifying),
            ("reported", WorkOrderState::Verifying, WorkOrderState::Reported),
        ] {
            let (resolved_to, allowed) = resolve_transition(ev, from).unwrap();
            assert_eq!(resolved_to, to);
            assert_eq!(allowed, &[ActorClass::Runner], "{ev} must be runner-only");
        }
    }

    #[test]
    fn accept_allows_admin_or_system_for_auto_accept() {
        let (to, allowed) = resolve_transition("accept", WorkOrderState::Reported).unwrap();
        assert_eq!(to, WorkOrderState::Completed);
        assert!(allowed.contains(&ActorClass::Admin));
        assert!(allowed.contains(&ActorClass::System));
        assert!(!allowed.contains(&ActorClass::Runner), "a runner must never accept");
    }

    #[test]
    fn cancel_legal_from_non_terminal_except_reported() {
        assert!(resolve_transition("cancel", WorkOrderState::Draft).is_ok());
        assert!(resolve_transition("cancel", WorkOrderState::Approved).is_ok());
        // reported's owner exits are accept/request-changes/reject, not cancel.
        assert!(matches!(
            resolve_transition("cancel", WorkOrderState::Reported).unwrap_err(),
            WorkOrderTransitionError::IllegalTransition { .. }
        ));
    }

    #[test]
    fn failed_reachable_from_building_or_verifying_runner_only() {
        assert!(resolve_transition("failed", WorkOrderState::Building).is_ok());
        assert!(resolve_transition("failed", WorkOrderState::Verifying).is_ok());
        assert!(matches!(
            resolve_transition("failed", WorkOrderState::Claimed).unwrap_err(),
            WorkOrderTransitionError::IllegalTransition { .. }
        ));
    }

    #[test]
    fn retry_resolves_failed_to_approved_admin_only() {
        let (to, allowed) = resolve_transition("retry", WorkOrderState::Failed).unwrap();
        assert_eq!(to, WorkOrderState::Approved);
        assert_eq!(allowed, &[ActorClass::Admin]);
    }

    #[test]
    fn unknown_event_type_is_safe_denied() {
        assert!(matches!(
            resolve_transition("delete-the-auth-check", WorkOrderState::Draft).unwrap_err(),
            WorkOrderTransitionError::ActorNotAuthorized
        ));
    }

    #[test]
    fn runner_scope_claim_reads_signed_scope() {
        // A minimal JWT payload carrying the runner class marker.
        let payload = json!({ "sub": "runner-1", "scope": RUNNER_TOKEN_SCOPE, "aud": "p" });
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let token = format!("HEADER.{payload_b64}.SIG");
        assert_eq!(runner_scope_claim(&token).as_deref(), Some(RUNNER_TOKEN_SCOPE));
    }

    #[test]
    fn runner_scope_claim_absent_on_end_user_jwt() {
        // An end-user JWT has no `scope` claim.
        let payload = json!({ "sub": "user-1", "aud": "p", "email": "u@example.com" });
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let token = format!("HEADER.{payload_b64}.SIG");
        assert_eq!(runner_scope_claim(&token), None);
    }

    #[test]
    fn runner_scope_claim_malformed_is_none() {
        assert_eq!(runner_scope_claim("not-a-jwt"), None);
        assert_eq!(runner_scope_claim("a.!!!.c"), None);
    }

    #[test]
    fn transition_error_body_is_machine_parseable() {
        let body = transition_error_body(&WorkOrderTransitionError::IllegalTransition {
            from: WorkOrderState::Draft,
            to: WorkOrderState::Dispatched,
        });
        let v: JsonValue = serde_json::from_str(&body).unwrap();
        assert_eq!(v["error"], "IllegalTransition");
        assert_eq!(v["from_state"], "draft");
        assert_eq!(v["to_state"], "dispatched");
    }
}
