//! Admin feedback handlers (Contracts C7 + C8). All four endpoints sit
//! behind the `AdminSession` extractor and operate inside the session's
//! resolved `TenantScope` (DEC-FBR-03).
//!
//! Contracts implemented here:
//!   - `POST /api/v1/admin/feedback/{id}/transition` (Contract C7)
//!   - `POST /api/v1/admin/feedback/{id}/reply`      (Contract C7)
//!   - `GET  /api/v1/admin/feedback`                 (Contract C8 list)
//!   - `GET  /api/v1/admin/feedback/{id}`            (Contract C8 detail)
//!
//! The transition handler honours Contract C6 Hard Invariant #4 — the
//! `feedback.status` UPDATE and the `feedback_status_history` audit-row
//! INSERT land in the same DB transaction via the `_in_executor`
//! overloads added to both repository traits.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{legal_transitions_from, FeedbackId, FeedbackKind, FeedbackStatus};
use feedbackmonk_repository::{ProjectScope, ReplyVisibility, TenantScope};

use crate::auth::AdminSession;
use crate::email::{EmailContext, EmailKind, is_submitter_visible_transition};
use crate::error::ApiError;
use crate::state::AppState;

const REPLY_BODY_MIN: usize = 1;
const REPLY_BODY_MAX: usize = 16_384;
const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_LIST_LIMIT: u32 = 100;

/// Register the admin feedback routes under `/api/v1/admin/feedback`.
pub fn routes(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/feedback",
            get(list_admin_feedback),
        )
        .route(
            "/api/v1/admin/feedback/search",
            get(search_admin_feedback),
        )
        .route(
            "/api/v1/admin/feedback/:feedback_id",
            get(get_admin_feedback),
        )
        .route(
            "/api/v1/admin/feedback/:feedback_id/transition",
            post(transition_status),
        )
        .route(
            "/api/v1/admin/feedback/:feedback_id/reply",
            post(reply),
        )
        .with_state(state)
}

// ---------- Contract C7: transition ----------

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionRequest {
    pub to_status: FeedbackStatus,
    pub reason_note: Option<String>,
    pub duplicate_of: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionResponse {
    pub feedback_id: String,
    pub from_status: FeedbackStatus,
    pub to_status: FeedbackStatus,
    pub transitioned_at: DateTime<Utc>,
    pub audit_id: Uuid,
    pub email_queued: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionErrorBody {
    pub error: &'static str,
    pub from_status: Option<FeedbackStatus>,
    pub to_status: Option<FeedbackStatus>,
}

pub async fn transition_status(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
    Json(req): Json<TransitionRequest>,
) -> Result<Json<TransitionResponse>, ApiError> {
    let project_scope = sole_project_scope(&state, &session.scope).await?;
    let fb_id = FeedbackId::from(feedback_id);
    let duplicate_of = req
        .duplicate_of
        .as_deref()
        .map(|s| FeedbackId::from(s.to_string()));
    let outcome = perform_transition(
        &state,
        &project_scope,
        &fb_id,
        req.to_status,
        req.reason_note.as_deref(),
        duplicate_of.as_ref(),
        session.scope.tenant_id(),
    )
    .await?;
    Ok(Json(outcome))
}

/// Core transition logic, decoupled from axum extractors so tests can
/// drive it without building an HTTP request.
#[allow(clippy::too_many_lines)]
pub(crate) async fn perform_transition(
    state: &AppState,
    project_scope: &ProjectScope,
    feedback_id: &FeedbackId,
    to_status: FeedbackStatus,
    reason_note: Option<&str>,
    duplicate_of: Option<&FeedbackId>,
    transitioned_by: Uuid,
) -> Result<TransitionResponse, ApiError> {
    // Read the current row to recover `from_status` for the state-machine
    // check + submitter email for the post-commit email send. This read is
    // outside the transaction; the UPDATE inside the txn does `FOR UPDATE`
    // so any concurrent UPDATE between the two reads still rolls back.
    let (feedback, _history) = state
        .feedback
        .get_with_history(project_scope, feedback_id)
        .await?;
    let from_status = feedback.status;

    if to_status == from_status {
        return Err(api_transition_error(
            "IllegalTransition",
            from_status,
            to_status,
        ));
    }
    if !legal_transitions_from(from_status).contains(&to_status) {
        return Err(api_transition_error(
            "IllegalTransition",
            from_status,
            to_status,
        ));
    }
    if to_status == FeedbackStatus::Duplicate {
        let Some(target) = duplicate_of else {
            return Err(api_transition_error(
                "DuplicateRequiresTarget",
                from_status,
                to_status,
            ));
        };
        if *target == feedback.short_code {
            return Err(api_transition_error(
                "DuplicateSelfReference",
                from_status,
                to_status,
            ));
        }
    }

    // Same-transaction status update + audit row insert. Both repo methods
    // are scope-bound; cross-tenant feedback_id / duplicate_of fail before
    // either write commits.
    let mut tx = state.pool.begin().await?;

    let actual_from = state
        .feedback
        .update_status_in_executor(project_scope, &mut tx, feedback_id, to_status)
        .await?;

    // The locked-row read inside update_status_in_executor returns the
    // pre-update status. If a concurrent transition raced ours, the row's
    // status may have moved between the outer get_with_history and the
    // FOR UPDATE read; we re-validate against the locked value.
    if !legal_transitions_from(actual_from).contains(&to_status) || actual_from == to_status {
        // Roll back; do not write the audit row.
        tx.rollback().await.ok();
        return Err(api_transition_error(
            "IllegalTransition",
            actual_from,
            to_status,
        ));
    }

    let audit_id = state
        .feedback_history
        .append_in_executor(
            project_scope,
            &mut tx,
            feedback_id,
            actual_from,
            to_status,
            reason_note,
            duplicate_of,
            transitioned_by,
        )
        .await
        .map_err(|e| match e {
            feedbackmonk_repository::RepoError::NotFound => ApiError::Conflict(json_err(
                "DuplicateTargetMissing",
                Some(actual_from),
                Some(to_status),
            )),
            other => ApiError::from(other),
        })?;

    tx.commit().await?;

    tracing::info!(
        target: "admin",
        feedback_id = %feedback_id,
        from_status = %actual_from.as_db_str(),
        to_status = %to_status.as_db_str(),
        "feedback status transition committed"
    );

    // Post-commit: email the submitter if this is a submitter-visible
    // transition and we have an address on file. Mail failure does NOT
    // roll the DB back — the email is a notification, not a precondition.
    let mut email_queued = false;
    if is_submitter_visible_transition(to_status) {
        let ctx = EmailContext {
            feedback_id: feedback_id.clone(),
            submitter_email: feedback.end_user_email.clone(),
            body_excerpt: None,
            reply_body: None,
        };
        let kind = EmailKind::StatusChange {
            from: actual_from,
            to: to_status,
            reason_note: reason_note.map(str::to_string),
        };
        match state.email_notifier.send_email(project_scope.tenant(), kind, ctx).await {
            Ok(outcome) => email_queued = outcome.was_queued(),
            Err(e) => {
                tracing::warn!(
                    target: "email",
                    feedback_id = %feedback_id,
                    error = %e,
                    "status-change email failed (transition still committed)"
                );
            }
        }
    }

    Ok(TransitionResponse {
        feedback_id: feedback_id.as_str().to_string(),
        from_status: actual_from,
        to_status,
        transitioned_at: Utc::now(),
        audit_id,
        email_queued,
    })
}

fn api_transition_error(
    code: &'static str,
    from: FeedbackStatus,
    to: FeedbackStatus,
) -> ApiError {
    ApiError::Conflict(json_err(code, Some(from), Some(to)))
}

fn json_err(code: &str, from: Option<FeedbackStatus>, to: Option<FeedbackStatus>) -> String {
    // ApiError::Conflict carries a String body message. We serialise the
    // structured form so the wire JSON satisfies Contract C7's 409 shape.
    serde_json::to_string(&TransitionErrorBody {
        error: match code {
            "IllegalTransition" => "IllegalTransition",
            "DuplicateRequiresTarget" => "DuplicateRequiresTarget",
            "DuplicateTargetMissing" => "DuplicateTargetMissing",
            "DuplicateSelfReference" => "DuplicateSelfReference",
            _ => "TransitionError",
        },
        from_status: from,
        to_status: to,
    })
    .unwrap_or_else(|_| String::from(code))
}

// ---------- Contract C7: reply ----------

#[derive(Debug, Clone, Deserialize)]
pub struct ReplyRequest {
    pub body: String,
    pub visibility: ReplyVisibilityWire,
}

/// Wire form of `ReplyVisibility` — distinct so the JSON spelling can
/// evolve independently if needed.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplyVisibilityWire {
    Public,
    Internal,
}

impl From<ReplyVisibilityWire> for ReplyVisibility {
    fn from(v: ReplyVisibilityWire) -> Self {
        match v {
            ReplyVisibilityWire::Public => Self::Public,
            ReplyVisibilityWire::Internal => Self::Internal,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplyResponse {
    pub reply_id: Uuid,
    pub feedback_id: String,
    pub visibility: ReplyVisibilityWire,
    pub created_at: DateTime<Utc>,
    pub email_queued: bool,
}

pub async fn reply(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
    Json(req): Json<ReplyRequest>,
) -> Result<Json<ReplyResponse>, ApiError> {
    if req.body.len() < REPLY_BODY_MIN || req.body.len() > REPLY_BODY_MAX {
        return Err(ApiError::BadRequest(format!(
            "body must be {REPLY_BODY_MIN}..={REPLY_BODY_MAX} chars"
        )));
    }
    let project_scope = sole_project_scope(&state, &session.scope).await?;
    let fb_id = FeedbackId::from(feedback_id);

    // Resolve feedback row (scope-checked) so we can email the submitter
    // if visibility is public.
    let (feedback, _history) = state
        .feedback
        .get_with_history(&project_scope, &fb_id)
        .await?;

    let visibility: ReplyVisibility = req.visibility.into();
    let inserted = state
        .feedback_replies
        .create(
            &project_scope,
            &fb_id,
            &req.body,
            visibility,
            session.scope.tenant_id(),
        )
        .await?;

    let mut email_queued = false;
    if visibility == ReplyVisibility::Public {
        let ctx = EmailContext {
            feedback_id: fb_id.clone(),
            submitter_email: feedback.end_user_email.clone(),
            body_excerpt: None,
            reply_body: Some(req.body.clone()),
        };
        let kind = EmailKind::PublicReply { reply_id: inserted.id };
        match state.email_notifier.send_email(project_scope.tenant(), kind, ctx).await {
            Ok(outcome) => email_queued = outcome.was_queued(),
            Err(e) => {
                tracing::warn!(
                    target: "email",
                    feedback_id = %fb_id,
                    error = %e,
                    "public-reply email failed (reply still committed)"
                );
            }
        }
    }

    Ok(Json(ReplyResponse {
        reply_id: inserted.id,
        feedback_id: fb_id.as_str().to_string(),
        visibility: req.visibility,
        created_at: inserted.created_at,
        email_queued,
    }))
}

// ---------- Contract C8: list ----------

#[derive(Debug, Clone, Deserialize)]
pub struct ListParams {
    pub status: Option<FeedbackStatus>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListResponse {
    pub items: Vec<FeedbackListItemWire>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackListItemWire {
    pub feedback_id: String,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body_excerpt: String,
    pub submitted_at: DateTime<Utc>,
    pub submitter_label: String,
    pub reply_count: i64,
}

pub async fn list_admin_feedback(
    State(state): State<AppState>,
    session: AdminSession,
    Query(params): Query<ListParams>,
) -> Result<Json<ListResponse>, ApiError> {
    let project_scope = sole_project_scope(&state, &session.scope).await?;
    let limit = params.limit.unwrap_or(DEFAULT_LIST_LIMIT).min(MAX_LIST_LIMIT);
    let offset = params.offset.unwrap_or(0);
    let (items, total) = state
        .feedback
        .list_for_admin(&project_scope, params.status, limit, offset)
        .await?;

    // Replace Stage-1's hard-zero reply_count with the real count per row.
    let mut wire_items = Vec::with_capacity(items.len());
    for it in items {
        let count = state
            .feedback_replies
            .count_for_feedback(&project_scope, &it.feedback_id)
            .await
            .unwrap_or(0);
        wire_items.push(FeedbackListItemWire {
            feedback_id: it.feedback_id.as_str().to_string(),
            kind: it.kind,
            status: it.status,
            body_excerpt: it.body_excerpt,
            submitted_at: it.submitted_at,
            submitter_label: format_submitter_label(it.submitter_email.as_deref(), it.is_anonymous),
            reply_count: count,
        });
    }

    Ok(Json(ListResponse {
        items: wire_items,
        total,
        limit,
        offset,
    }))
}

// ---------- Gap #3: admin full-text search ----------

#[derive(Debug, Clone, Deserialize)]
pub struct SearchParams {
    /// Raw user query — passed to `websearch_to_tsquery` (forgiving syntax).
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// `GET /api/v1/admin/feedback/search?q=...` — tenant-scoped full-text search
/// over feedback bodies (GitCellar parity gap #3). Shares the Contract C8 list
/// response shape so the admin UI renders results with the same table. A blank
/// `q` short-circuits to an empty page (no DB round-trip) so the UI can mount
/// the box before the user has typed anything.
pub async fn search_admin_feedback(
    State(state): State<AppState>,
    session: AdminSession,
    Query(params): Query<SearchParams>,
) -> Result<Json<ListResponse>, ApiError> {
    let limit = params.limit.unwrap_or(DEFAULT_LIST_LIMIT).min(MAX_LIST_LIMIT);
    let offset = params.offset.unwrap_or(0);

    let query = params.q.unwrap_or_default();
    if query.trim().is_empty() {
        return Ok(Json(ListResponse {
            items: Vec::new(),
            total: 0,
            limit,
            offset,
        }));
    }

    let project_scope = sole_project_scope(&state, &session.scope).await?;
    let (items, total) = state
        .feedback
        .search_for_admin(&project_scope, &query, limit, offset)
        .await?;

    // Same reply_count enrichment as list_admin_feedback — the repository
    // search method returns hard-zero; the HTTP layer resolves the real count.
    let mut wire_items = Vec::with_capacity(items.len());
    for it in items {
        let count = state
            .feedback_replies
            .count_for_feedback(&project_scope, &it.feedback_id)
            .await
            .unwrap_or(0);
        wire_items.push(FeedbackListItemWire {
            feedback_id: it.feedback_id.as_str().to_string(),
            kind: it.kind,
            status: it.status,
            body_excerpt: it.body_excerpt,
            submitted_at: it.submitted_at,
            submitter_label: format_submitter_label(it.submitter_email.as_deref(), it.is_anonymous),
            reply_count: count,
        });
    }

    Ok(Json(ListResponse {
        items: wire_items,
        total,
        limit,
        offset,
    }))
}

// ---------- Contract C8: detail ----------

#[derive(Debug, Clone, Serialize)]
pub struct StatusHistoryEntryWire {
    pub from_status: FeedbackStatus,
    pub to_status: FeedbackStatus,
    pub reason_note: Option<String>,
    pub duplicate_of_feedback_id: Option<String>,
    pub transitioned_by: String,
    pub transitioned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplyEntryWire {
    pub reply_id: Uuid,
    pub body: String,
    pub visibility: ReplyVisibilityWire,
    pub author: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
pub enum SubmitterWire {
    Authenticated {
        sub: Option<String>,
        email: Option<String>,
        name: Option<String>,
    },
    Anonymous {
        email: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackDetailResponse {
    pub feedback_id: String,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body: String,
    pub submitted_at: DateTime<Utc>,
    pub submitter: SubmitterWire,
    pub external_metadata: Option<serde_json::Value>,
    pub status_history: Vec<StatusHistoryEntryWire>,
    pub replies: Vec<ReplyEntryWire>,
}

pub async fn get_admin_feedback(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
) -> Result<Json<FeedbackDetailResponse>, ApiError> {
    let project_scope = sole_project_scope(&state, &session.scope).await?;
    let fb_id = FeedbackId::from(feedback_id);
    let (feedback, history) = state.feedback.get_with_history(&project_scope, &fb_id).await?;
    let replies = state
        .feedback_replies
        .list_for_feedback(&project_scope, &fb_id)
        .await?;

    // Admin label resolution: in P0/P1 the only "admin user" identity is
    // the tenant itself. If transitioned_by/author_user_id matches the
    // current tenant id, surface their email; otherwise surface a
    // placeholder until tenant_users exists.
    let tenant_row = state.tenants.get(&session.scope).await?;
    let admin_email_for = |uuid: Uuid| -> String {
        if uuid == session.scope.tenant_id() {
            tenant_row.email.clone()
        } else {
            format!("(unknown admin: {uuid})")
        }
    };

    // Resolve duplicate_of UUID -> short_code per row. Cheap N+1 against
    // a typically-tiny history; the alternative is a server-side JOIN that
    // pulls down feedback.short_code alongside each history row (a
    // pre-authorized widening on StatusHistoryRow we deliberately defer).
    let mut history_wire = Vec::with_capacity(history.len());
    for h in history {
        let duplicate_of_short = if let Some(dup_id) = h.duplicate_of_feedback_id {
            // Walk via feedback_repo would re-fetch; we resolve via a
            // single scope-bound short_code lookup. Skip-on-error: a
            // dangling reference (ON DELETE SET NULL) just renders as None.
            resolve_short_code(&state, &project_scope, dup_id).await
        } else {
            None
        };
        history_wire.push(StatusHistoryEntryWire {
            from_status: h.from_status,
            to_status: h.to_status,
            reason_note: h.reason_note,
            duplicate_of_feedback_id: duplicate_of_short,
            transitioned_by: admin_email_for(h.transitioned_by),
            transitioned_at: h.transitioned_at,
        });
    }

    let replies_wire: Vec<ReplyEntryWire> = replies
        .into_iter()
        .map(|r| ReplyEntryWire {
            reply_id: r.id,
            body: r.body,
            visibility: match r.visibility {
                ReplyVisibility::Public => ReplyVisibilityWire::Public,
                ReplyVisibility::Internal => ReplyVisibilityWire::Internal,
            },
            author: admin_email_for(r.author_user_id),
            created_at: r.created_at,
        })
        .collect();

    let submitter = if feedback.end_user_sub.is_some() {
        SubmitterWire::Authenticated {
            sub: feedback.end_user_sub.clone(),
            email: feedback.end_user_email.clone(),
            name: feedback.end_user_name.clone(),
        }
    } else {
        SubmitterWire::Anonymous {
            email: feedback.end_user_email.clone(),
        }
    };

    Ok(Json(FeedbackDetailResponse {
        feedback_id: feedback.short_code.as_str().to_string(),
        kind: feedback.kind,
        status: feedback.status,
        body: feedback.body,
        submitted_at: feedback.accepted_at,
        submitter,
        external_metadata: feedback.external_metadata,
        status_history: history_wire,
        replies: replies_wire,
    }))
}

// ---------- helpers ----------

/// Map (`submitter_email`, `is_anonymous`) -> the formatted
/// `submitter_label` string per Contract C8 list response.
fn format_submitter_label(email: Option<&str>, is_anonymous: bool) -> String {
    match (is_anonymous, email) {
        (false, Some(e)) => e.to_string(),
        (false, None) => "authenticated".to_string(),
        (true, Some(e)) => format!("anonymous (email: {e})"),
        (true, None) => "anonymous".to_string(),
    }
}

/// P0/P1 only support one project per tenant in practice; the admin
/// endpoints scope to the tenant's first project. Future work: per-project
/// admin URLs (FR-FBR-15 / P3 work). For now, return Conflict if the
/// tenant has zero projects so the caller can surface a setup error.
async fn sole_project_scope(
    state: &AppState,
    scope: &TenantScope,
) -> Result<ProjectScope, ApiError> {
    let projects = state.projects.list_for_tenant(scope).await?;
    let first = projects.first().ok_or_else(|| {
        ApiError::Conflict(
            r#"{"error":"NoProject","detail":"tenant has no projects; create one first"}"#
                .to_string(),
        )
    })?;
    Ok(state.projects.open(scope, first.id).await?)
}

/// Resolve a feedback UUID PK to its public `short_code`, within scope.
/// Returns `None` when the row is outside scope or has been deleted
/// (`feedback_status_history.duplicate_of_feedback_id` is `ON DELETE SET
/// NULL`).
async fn resolve_short_code(
    state: &AppState,
    scope: &ProjectScope,
    uuid: Uuid,
) -> Option<String> {
    // Use list_recent's full-row read and filter in memory — cheap for the
    // typical history size. The repository layer is the only path to
    // feedback rows; we deliberately do not add a "find by id" repo
    // method just for this lookup until usage justifies it.
    let recent = state.feedback.list_recent(scope, 1000).await.ok()?;
    recent
        .into_iter()
        .find(|f| f.id == uuid)
        .map(|f| f.short_code.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use chrono::Duration;
    use feedbackmonk_anon::{AnonGate, DEFAULT_RATE_LIMIT_PER_HOUR};
    use feedbackmonk_repository::{
        SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
        SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxSigningKeyRepo,
        SqlxTenantRepo,
    };
    use sqlx::PgPool;
    use std::num::NonZeroU32;

    use crate::email::send::RecordingEmailNotifier;
    use crate::email::Mailer;

    struct StubMailer;
    #[async_trait::async_trait]
    impl Mailer for StubMailer {
        async fn send_verify_email(&self, _to: &str, _link: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn build_test_state(pool: &PgPool) -> (AppState, Arc<RecordingEmailNotifier>) {
        let tenants = Arc::new(SqlxTenantRepo::new(pool.clone()));
        let projects = Arc::new(SqlxProjectRepo::new(pool.clone()));
        let signing_keys = Arc::new(SqlxSigningKeyRepo::new(pool.clone()));
        let feedback = Arc::new(SqlxFeedbackRepo::new(pool.clone()));
        let feedback_history = Arc::new(SqlxFeedbackStatusHistoryRepo::new(pool.clone()));
        let feedback_replies = Arc::new(SqlxFeedbackReplyRepo::new(pool.clone()));
        let email_verifications = Arc::new(SqlxEmailVerificationRepo::new(pool.clone()));
        let recorder = Arc::new(RecordingEmailNotifier::new());

        let state = AppState {
            pool: pool.clone(),
            tenants,
            projects,
            signing_keys,
            feedback,
            feedback_history,
            feedback_replies,
            email_verifications,
            mailer: Arc::new(StubMailer),
            email_notifier: Arc::clone(&recorder) as Arc<dyn crate::email::EmailNotifier>,
            session_secret: Arc::new([0u8; 32]),
            public_url: Arc::from("http://localhost:14304"),
            verify_token_ttl: Duration::hours(24),
            anon_gate: AnonGate::new(NonZeroU32::new(DEFAULT_RATE_LIMIT_PER_HOUR).unwrap()),
            jwt_iat_leeway_seconds: 5,
            // P2 fields — required by AppState; the admin-feedback tests don't
            // exercise these surfaces (see docs/test-modifications/
            // 20260514-p2-appstate-roadmap-fields.md for the Read-Only-Tests
            // mode justification).
            roadmap_items: Arc::new(feedbackmonk_repository::SqlxRoadmapItemRepo::new(
                pool.clone(),
            )),
            roadmap_votes: Arc::new(feedbackmonk_repository::SqlxRoadmapVoteRepo::new(
                pool.clone(),
            )),
            voting_cache: crate::roadmap_voting_cache::VotingCache::new(),
            started_at: Utc::now(),
            health: SqlxHealthCheck::new(pool.clone()),
            // P3 Stage 1 fixture extension — see
            // docs/test-modifications/20260514-p3-appstate-tier-quotas.md.
            // Admin transition/reply tests don't exercise tier caps; this
            // tenant defaults to Free (1 project / 50 feedback) which is
            // well above the seed counts in every admin_feedback test.
            tier_quotas: Arc::new(feedbackmonk_repository::SqlxTierQuotaRepo::new(
                pool.clone(),
            )),
        };
        (state, recorder)
    }

    async fn seed_project_scope(state: &AppState, email: &str) -> ProjectScope {
        let t = state.tenants.create(email, "h").await.unwrap();
        let scope = state.tenants.scope_for(t.id).await.unwrap();
        // Verify the tenant so `AdminSession` extraction (used downstream
        // by tests that bypass the extractor) would also accept it.
        state.tenants.mark_verified(&scope).await.unwrap();
        let p = state.projects.create(&scope, "Proj", "proj").await.unwrap();
        state.projects.open(&scope, p.id).await.unwrap()
    }

    async fn seed_feedback_with_email(
        state: &AppState,
        scope: &ProjectScope,
        email: Option<&str>,
    ) -> FeedbackId {
        state
            .feedback
            .submit_anonymous(
                scope,
                &[1u8; 32],
                email,
                "submission body",
                FeedbackKind::Bug,
            )
            .await
            .unwrap()
    }

    // ---- Hard invariants for Contract C6 (5 named tests) ----

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_illegal_transition_fails_before_db_write(pool: PgPool) {
        let (state, _rec) = build_test_state(&pool);
        let scope = seed_project_scope(&state, "illegal@example.com").await;
        let fb_id = seed_feedback_with_email(&state, &scope, Some("submitter@example.com")).await;

        // Submitted -> Shipped is illegal (must go via Triaged or
        // InProgress).
        let err = perform_transition(
            &state,
            &scope,
            &fb_id,
            FeedbackStatus::Shipped,
            None,
            None,
            scope.tenant_id(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ApiError::Conflict(ref m) if m.contains("IllegalTransition")));

        // Row + history unchanged.
        let (fb, history) = state
            .feedback
            .get_with_history(&scope, &fb_id)
            .await
            .unwrap();
        assert_eq!(fb.status, FeedbackStatus::Submitted);
        assert!(history.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_to_duplicate_requires_target(pool: PgPool) {
        let (state, _rec) = build_test_state(&pool);
        let scope = seed_project_scope(&state, "dup-req@example.com").await;
        let fb_id = seed_feedback_with_email(&state, &scope, Some("submitter@example.com")).await;

        let err = perform_transition(
            &state,
            &scope,
            &fb_id,
            FeedbackStatus::Duplicate,
            None,
            None,
            scope.tenant_id(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, ApiError::Conflict(ref m) if m.contains("DuplicateRequiresTarget")),
            "expected DuplicateRequiresTarget, got {err:?}"
        );

        // No audit row written.
        let (_fb, history) = state
            .feedback
            .get_with_history(&scope, &fb_id)
            .await
            .unwrap();
        assert!(history.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_duplicate_of_cross_tenant_rejected(pool: PgPool) {
        let (state, _rec) = build_test_state(&pool);
        let s1 = seed_project_scope(&state, "ct-dup-1@example.com").await;
        let s2 = seed_project_scope(&state, "ct-dup-2@example.com").await;
        let fb_a = seed_feedback_with_email(&state, &s1, Some("a@example.com")).await;
        let fb_b = state
            .feedback
            .submit_anonymous(&s2, &[9u8; 32], None, "other tenant", FeedbackKind::Other)
            .await
            .unwrap();

        // s1 attempts to mark fb_a duplicate of fb_b (lives in s2).
        let err = perform_transition(
            &state,
            &s1,
            &fb_a,
            FeedbackStatus::Duplicate,
            None,
            Some(&fb_b),
            s1.tenant_id(),
        )
        .await
        .unwrap_err();

        // Either reported as DuplicateTargetMissing (audit-row append's
        // scope check rejected fb_b) OR as NotFound. Both are acceptable
        // — the test asserts no successful audit-row write.
        assert!(
            matches!(err, ApiError::Conflict(ref m) if m.contains("DuplicateTargetMissing"))
                || matches!(err, ApiError::NotFound),
            "expected scope-check failure, got {err:?}"
        );

        // fb_a status unchanged.
        let (fb, history) = state
            .feedback
            .get_with_history(&s1, &fb_a)
            .await
            .unwrap();
        assert_eq!(fb.status, FeedbackStatus::Submitted);
        assert!(history.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_audit_row_in_same_txn(pool: PgPool) {
        let (state, _rec) = build_test_state(&pool);
        let scope = seed_project_scope(&state, "atomic@example.com").await;
        let fb_id = seed_feedback_with_email(&state, &scope, Some("submitter@example.com")).await;

        // Inject a failure between the status UPDATE and the audit-row
        // INSERT by supplying an invalid duplicate_of for a Triaged
        // transition. The same-txn invariant is exercised by the bad
        // input path: even though we DID update the status column in the
        // executor, the audit_row's scope check fails and the txn rolls
        // back -- so BOTH the status update AND the absence of audit row
        // hold together.
        let nowhere = FeedbackId::from("FB-NOWHERE".to_string());
        // Submitted -> Duplicate is legal, but duplicate_of is bogus.
        let err = perform_transition(
            &state,
            &scope,
            &fb_id,
            FeedbackStatus::Duplicate,
            None,
            Some(&nowhere),
            scope.tenant_id(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, ApiError::Conflict(_) | ApiError::NotFound),
            "expected failure for bogus duplicate_of, got {err:?}"
        );

        // Post-rollback: feedback.status is still Submitted (the UPDATE
        // in the txn rolled back along with the failed audit-row INSERT).
        let (fb, history) = state
            .feedback
            .get_with_history(&scope, &fb_id)
            .await
            .unwrap();
        assert_eq!(
            fb.status,
            FeedbackStatus::Submitted,
            "status update must roll back when audit row insert fails"
        );
        assert!(
            history.is_empty(),
            "audit row must roll back along with status update"
        );

        // Then verify happy-path: a clean transition lands BOTH a status
        // update and an audit row atomically.
        let outcome = perform_transition(
            &state,
            &scope,
            &fb_id,
            FeedbackStatus::Triaged,
            Some("looks legit"),
            None,
            scope.tenant_id(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.from_status, FeedbackStatus::Submitted);
        assert_eq!(outcome.to_status, FeedbackStatus::Triaged);

        let (fb_after, history_after) = state
            .feedback
            .get_with_history(&scope, &fb_id)
            .await
            .unwrap();
        assert_eq!(fb_after.status, FeedbackStatus::Triaged);
        assert_eq!(history_after.len(), 1);
        assert_eq!(history_after[0].from_status, FeedbackStatus::Submitted);
        assert_eq!(history_after[0].to_status, FeedbackStatus::Triaged);
        assert_eq!(history_after[0].id, outcome.audit_id);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn test_noop_transition_rejected(pool: PgPool) {
        let (state, _rec) = build_test_state(&pool);
        let scope = seed_project_scope(&state, "noop@example.com").await;
        let fb_id = seed_feedback_with_email(&state, &scope, Some("submitter@example.com")).await;

        // from == to (Submitted -> Submitted) must fail with
        // IllegalTransition rather than silently succeeding.
        let err = perform_transition(
            &state,
            &scope,
            &fb_id,
            FeedbackStatus::Submitted,
            None,
            None,
            scope.tenant_id(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ApiError::Conflict(ref m) if m.contains("IllegalTransition")));

        // No audit row.
        let (_fb, history) = state.feedback.get_with_history(&scope, &fb_id).await.unwrap();
        assert!(history.is_empty());
    }

    // ---- Email integration tests ----

    #[sqlx::test(migrations = "../../migrations")]
    async fn submitter_visible_transition_queues_email_when_address_present(pool: PgPool) {
        let (state, rec) = build_test_state(&pool);
        let scope = seed_project_scope(&state, "email-vis@example.com").await;
        let fb_id = seed_feedback_with_email(&state, &scope, Some("submitter@example.com")).await;

        let out = perform_transition(
            &state,
            &scope,
            &fb_id,
            FeedbackStatus::Triaged,
            None,
            None,
            scope.tenant_id(),
        )
        .await
        .unwrap();

        assert!(out.email_queued);
        let sent = rec.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn submitter_without_email_skips_send(pool: PgPool) {
        let (state, rec) = build_test_state(&pool);
        let scope = seed_project_scope(&state, "noemail@example.com").await;
        let fb_id = seed_feedback_with_email(&state, &scope, None).await;

        let out = perform_transition(
            &state,
            &scope,
            &fb_id,
            FeedbackStatus::Triaged,
            None,
            None,
            scope.tenant_id(),
        )
        .await
        .unwrap();

        assert!(!out.email_queued);
        assert!(rec.sent.lock().unwrap().is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn reopen_transition_does_not_email(pool: PgPool) {
        let (state, rec) = build_test_state(&pool);
        let scope = seed_project_scope(&state, "reopen@example.com").await;
        let fb_id = seed_feedback_with_email(&state, &scope, Some("submitter@example.com")).await;

        // Walk Submitted -> Triaged -> WontFix -> Submitted (re-open).
        perform_transition(&state, &scope, &fb_id, FeedbackStatus::Triaged, None, None, scope.tenant_id()).await.unwrap();
        perform_transition(&state, &scope, &fb_id, FeedbackStatus::WontFix, None, None, scope.tenant_id()).await.unwrap();
        // Now re-open: Submitted is admin-internal, no email.
        let out = perform_transition(&state, &scope, &fb_id, FeedbackStatus::Submitted, None, None, scope.tenant_id()).await.unwrap();
        assert!(!out.email_queued);

        // Sent count from the two submitter-visible transitions only.
        let sent = rec.sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
    }

    // ---- Unit tests of helpers ----

    #[test]
    fn format_submitter_label_cases() {
        assert_eq!(format_submitter_label(Some("a@b"), false), "a@b");
        assert_eq!(format_submitter_label(None, false), "authenticated");
        assert_eq!(format_submitter_label(Some("c@d"), true), "anonymous (email: c@d)");
        assert_eq!(format_submitter_label(None, true), "anonymous");
    }
}
