//! Recommendation read + analyst-ingestion API (FR-FBR-20, Contract C23/C24;
//! Worker B / CLAUDE-B).
//!
//! ## ULADP Agent Context Header
//!
//! **Purpose**: the seam a customer-side analyst (the P5b runner) writes
//! recommendations into, plus the read surface the owner review UI consumes.
//! Recommendation lifecycle: `proposed → approved | tweaked_approved | rejected
//! | superseded`. Worker A drives `approved`/`tweaked_approved` (via work-order
//! creation/transition); **Worker B owns create / ingest / supersede / reject /
//! read** (reject = proposed → rejected with NO work order).
//!
//! **File Index**:
//! - `RecommendationView` — the recommendation JSON wire shape (CLAUDE-B owns
//!   it per MSG-002 Q1 / MSG-003); `action_type` serialises `snake_case`.
//! - `validate_source_refs(&Value) -> Result<(), String>` — pure: the
//!   exfiltration defense (C24 case f). `source_refs` MUST be a list of
//!   **references** (file/line/doc pointers), NEVER a dump of file contents.
//! - `ingest_recommendation` — `POST .../clusters/:id/recommendations`.
//! - `supersede_recommendation` — `POST .../recommendations/:id/supersede`.
//! - `reject_recommendation` — `POST .../recommendations/:id/reject` (no work order).
//! - `list_recommendations` — `GET .../clusters/:id/recommendations`.
//! - `router(state)` — project-scoped routes, `AdminSession`, NO CORS.
//!
//! ## ⛔ Constraints & Business Rules
//!
//! - **`source_refs` are references, not dumps** (C24 case f) — the
//!   [`validate_source_refs`] gate is a real exfiltration defense, not a
//!   formality. An ingest that tries to smuggle `.env` contents (or any blob)
//!   into `source_refs` is rejected pre-DB.
//! - **Ingested text is DATA** — stored verbatim, never interpreted as an
//!   instruction (the runner/implementer that would *act* on it is P5b).
//! - P5a ingestion is behind `AdminSession` (NO CORS); runner-token auth = P5b.
//! - **DEC-FBR-03**: every write goes through `feedbackmonk-repository`.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use feedbackmonk_core::ActionType;
use feedbackmonk_repository::{NewRecommendation, ProjectScope, Recommendation};

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::handlers::work_orders::verify_runner_token;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// source_refs exfiltration defense (C24 case f) — pure + testable
// ---------------------------------------------------------------------------

/// Max number of references in one `source_refs` list.
const MAX_REFS: usize = 64;
/// Max chars of any single string inside `source_refs` (a *reference* is short:
/// `"src/auth.rs:10-20"`; a *dump* of file contents blows past this).
const MAX_REF_STR_CHARS: usize = 512;
/// Max serialized byte size of the whole `source_refs` value.
const MAX_REFS_TOTAL_BYTES: usize = 8 * 1024;

/// Object keys that smell like a content *dump* rather than a *reference*.
/// Their presence is rejected outright — a reference points AT evidence, it
/// never carries the evidence body.
const FORBIDDEN_REF_KEYS: &[&str] = &[
    "content",
    "contents",
    "body",
    "data",
    "file_contents",
    "filecontents",
    "text",
    "raw",
    "dump",
    "bytes",
    "blob",
    "payload",
    "source",
];

/// **Exfiltration defense (C24 case f).** Validate that `source_refs` is a list
/// of *references* (grounding pointers the analyst inspected), never a dump of
/// file contents that would carry a secret into analyst/owner-reachable
/// context. Returns `Err(reason)` describing the first violation.
///
/// Rules:
/// - must be a JSON array of at most [`MAX_REFS`] items, total serialized size
///   at most [`MAX_REFS_TOTAL_BYTES`];
/// - each item is either a short string pointer (≤ [`MAX_REF_STR_CHARS`]) or an
///   object with a non-empty string anchor (`path` | `file` | `url` | `doc`);
/// - object items carry no [`FORBIDDEN_REF_KEYS`] and no string value longer
///   than [`MAX_REF_STR_CHARS`]; nested arrays/objects (a smuggling vector)
///   are rejected.
pub fn validate_source_refs(value: &JsonValue) -> Result<(), String> {
    let arr = value
        .as_array()
        .ok_or_else(|| "source_refs must be a JSON array of references".to_string())?;

    if arr.len() > MAX_REFS {
        return Err(format!("source_refs may list at most {MAX_REFS} references"));
    }
    // Cheap total-size cap (defends against many small-but-not-tiny entries).
    let serialized = value.to_string();
    if serialized.len() > MAX_REFS_TOTAL_BYTES {
        return Err(format!(
            "source_refs exceeds {MAX_REFS_TOTAL_BYTES} bytes — references, not dumps"
        ));
    }

    for (i, item) in arr.iter().enumerate() {
        match item {
            JsonValue::String(s) => {
                if s.chars().count() > MAX_REF_STR_CHARS {
                    return Err(format!(
                        "source_refs[{i}] string exceeds {MAX_REF_STR_CHARS} chars — that is a dump, not a reference"
                    ));
                }
            }
            JsonValue::Object(map) => {
                let mut has_anchor = false;
                for (key, val) in map {
                    let key_lc = key.to_ascii_lowercase();
                    if FORBIDDEN_REF_KEYS.contains(&key_lc.as_str()) {
                        return Err(format!(
                            "source_refs[{i}] key {key:?} is a content-dump field — references must point AT evidence, not carry it"
                        ));
                    }
                    match val {
                        JsonValue::String(s) => {
                            if s.chars().count() > MAX_REF_STR_CHARS {
                                return Err(format!(
                                    "source_refs[{i}].{key} exceeds {MAX_REF_STR_CHARS} chars — that is a dump, not a reference"
                                ));
                            }
                            if matches!(key_lc.as_str(), "path" | "file" | "url" | "doc")
                                && !s.trim().is_empty()
                            {
                                has_anchor = true;
                            }
                        }
                        JsonValue::Number(_) | JsonValue::Bool(_) | JsonValue::Null => {}
                        JsonValue::Array(_) | JsonValue::Object(_) => {
                            return Err(format!(
                                "source_refs[{i}].{key} must be a scalar — nested structures are a smuggling vector"
                            ));
                        }
                    }
                }
                if !has_anchor {
                    return Err(format!(
                        "source_refs[{i}] needs a non-empty string anchor (path | file | url | doc)"
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "source_refs[{i}] must be a string pointer or an object with an anchor"
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire shape (CLAUDE-B owns it — MSG-002 Q1 / MSG-003)
// ---------------------------------------------------------------------------

/// Recommendation JSON wire shape. `action_type` serialises `snake_case`
/// (`feedbackmonk_core::ActionType`); `status` is the DB-pinned value set
/// (`proposed | approved | tweaked_approved | rejected | superseded`).
#[derive(Debug, Clone, Serialize)]
pub struct RecommendationView {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub sweep_id: Option<Uuid>,
    pub action_type: ActionType,
    pub title: String,
    pub body: String,
    pub rationale: Option<String>,
    pub source_refs: JsonValue,
    pub confidence: f64,
    pub status: String,
    pub generated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl From<Recommendation> for RecommendationView {
    fn from(r: Recommendation) -> Self {
        Self {
            id: r.id,
            cluster_id: r.cluster_id,
            sweep_id: r.sweep_id,
            action_type: r.action_type,
            title: r.title,
            body: r.body,
            rationale: r.rationale,
            source_refs: r.source_refs,
            confidence: r.confidence,
            status: r.status,
            generated_at: r.generated_at,
            created_at: r.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Ingestion + lifecycle
// ---------------------------------------------------------------------------

/// Title length cap (matches the `recommendations.title` 1..=512 DB CHECK).
const TITLE_MAX_CHARS: usize = 512;
/// Body cap — ingested recommendations are prose, not file dumps.
const BODY_MAX_CHARS: usize = 16_384;

#[derive(Debug, Clone, Deserialize)]
pub struct IngestRequest {
    /// `bug_fix | feature_implementation | enhancement | investigation | no_action`.
    pub action_type: ActionType,
    pub title: String,
    pub body: String,
    pub rationale: Option<String>,
    /// JSON array of grounding REFERENCES — validated by [`validate_source_refs`].
    pub source_refs: JsonValue,
    pub confidence: f64,
    /// Optional provenance sweep this recommendation came from.
    pub sweep_id: Option<Uuid>,
}

/// Shared ingestion core: validate + create a `proposed` recommendation within
/// an ALREADY-RESOLVED scope. Both the admin (`AdminSession`) and runner
/// (runner-token) entrypoints funnel through this, so the `source_refs` exfil
/// gate (C24 case f) + the title/body/confidence caps run IDENTICALLY
/// regardless of credential class. All validation is pre-DB.
async fn ingest_into_scope(
    state: &AppState,
    scope: &ProjectScope,
    cluster_id: Uuid,
    req: IngestRequest,
) -> Result<RecommendationView, ApiError> {
    // ----- validation (all pre-DB) -----
    let title = req.title.trim();
    if title.is_empty() || title.chars().count() > TITLE_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "title must be 1..={TITLE_MAX_CHARS} chars"
        )));
    }
    if req.body.is_empty() || req.body.chars().count() > BODY_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "body must be 1..={BODY_MAX_CHARS} chars"
        )));
    }
    if !(0.0..=1.0).contains(&req.confidence) {
        return Err(ApiError::BadRequest(
            "confidence must be between 0.0 and 1.0".into(),
        ));
    }
    // THE exfiltration defense — references, not dumps.
    validate_source_refs(&req.source_refs)
        .map_err(|reason| ApiError::BadRequest(format!("invalid source_refs: {reason}")))?;

    let rationale = req.rationale.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let rec = state
        .recommendations
        .create(
            scope,
            NewRecommendation {
                cluster_id,
                sweep_id: req.sweep_id,
                action_type: req.action_type,
                title,
                body: &req.body,
                rationale,
                source_refs: &req.source_refs,
                confidence: req.confidence,
            },
        )
        .await?;

    Ok(rec.into())
}

/// `POST /api/v1/projects/:project_id/clusters/:cluster_id/recommendations` —
/// ingest a `proposed` recommendation for a cluster (admin). The `source_refs`
/// exfil gate runs BEFORE any DB write (C24 case f).
pub async fn ingest_recommendation(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, cluster_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<RecommendationView>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    Ok(Json(ingest_into_scope(&state, &scope, cluster_id, req).await?))
}

/// `POST /api/v1/projects/:project_id/runner/clusters/:cluster_id/recommendations`
/// — the runner-authed analyst ingestion path (DEC-001 / MSG-C01 Q2). Same exfil
/// gate + caps as the admin path (shared [`ingest_into_scope`]); the only
/// difference is the credential class — a runner write-token instead of an
/// `AdminSession`. Runner-token; NO CORS. A dedicated `/runner/` path so it does
/// not overlap the admin ingestion route (`.merge()` forbids overlap) and the
/// existing `AdminSession` path keeps working unchanged.
pub async fn runner_ingest_recommendation(
    State(state): State<AppState>,
    Path((project_id, cluster_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(req): Json<IngestRequest>,
) -> Result<Json<RecommendationView>, ApiError> {
    let (scope, _runner) = verify_runner_token(&state, project_id, &headers).await?;
    Ok(Json(ingest_into_scope(&state, &scope, cluster_id, req).await?))
}

/// `POST /api/v1/projects/:project_id/recommendations/:rec_id/supersede` —
/// mark a recommendation `superseded` (retained for history). Worker B owns
/// supersede; Worker A owns approve/tweak/reject (via work-order creation).
pub async fn supersede_recommendation(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, rec_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RecommendationView>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    let rec = state
        .recommendations
        .set_status(&scope, rec_id, "superseded")
        .await?;
    Ok(Json(rec.into()))
}

/// `POST /api/v1/projects/:project_id/recommendations/:rec_id/reject` — mark a
/// recommendation `rejected` (proposed → rejected; NO work order is created —
/// that distinguishes it from Worker A's approve/tweak path which creates a work
/// order). Owner action surfaced by the review UI (FR-FBR-21). Any request body
/// (e.g. `{reason}`) is accepted but unused in P5a — there is no rejection-reason
/// column yet (a P5b addition); the body is simply not consumed.
pub async fn reject_recommendation(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, rec_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RecommendationView>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    let rec = state
        .recommendations
        .set_status(&scope, rec_id, "rejected")
        .await?;
    Ok(Json(rec.into()))
}

/// `GET /api/v1/projects/:project_id/clusters/:cluster_id/recommendations` —
/// list a cluster's recommendations, newest-first.
pub async fn list_recommendations(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, cluster_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<RecommendationView>>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    let recs = state
        .recommendations
        .list_for_cluster(&scope, cluster_id)
        .await?;
    Ok(Json(recs.into_iter().map(RecommendationView::from).collect()))
}

/// Recommendation admin routes. Mounted by `build_app` WITHOUT the public CORS
/// layer.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/projects/:project_id/clusters/:cluster_id/recommendations",
            get(list_recommendations).post(ingest_recommendation),
        )
        // Runner-authed analyst ingestion (DEC-001 / MSG-C01 Q2). Dedicated
        // `/runner/` path — runner-token + NO CORS; does NOT overlap the admin
        // ingestion route above.
        .route(
            "/api/v1/projects/:project_id/runner/clusters/:cluster_id/recommendations",
            post(runner_ingest_recommendation),
        )
        .route(
            "/api/v1/projects/:project_id/recommendations/:rec_id/supersede",
            post(supersede_recommendation),
        )
        .route(
            "/api/v1/projects/:project_id/recommendations/:rec_id/reject",
            post(reject_recommendation),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_string_and_object_references() {
        let refs = json!([
            "src/auth.rs:10-20",
            {"path": "src/login.rs", "lines": "5-9", "note": "missing guard"},
            {"url": "https://docs.example.com/auth"}
        ]);
        assert!(validate_source_refs(&refs).is_ok());
    }

    #[test]
    fn empty_array_is_ok() {
        assert!(validate_source_refs(&json!([])).is_ok());
    }

    #[test]
    fn rejects_non_array() {
        assert!(validate_source_refs(&json!({"path": "x"})).is_err());
        assert!(validate_source_refs(&json!("src/x.rs")).is_err());
    }

    #[test]
    fn rejects_content_dump_key() {
        // The exfil probe: smuggling .env contents via a `content` field.
        let refs = json!([{"path": ".env", "content": "SECRET_KEY=hunter2..."}]);
        let err = validate_source_refs(&refs).unwrap_err();
        assert!(err.contains("content-dump"), "got: {err}");
    }

    #[test]
    fn rejects_oversize_string_dump() {
        let huge = "x".repeat(MAX_REF_STR_CHARS + 1);
        let refs = json!([huge]);
        let err = validate_source_refs(&refs).unwrap_err();
        assert!(err.contains("dump"), "got: {err}");
    }

    #[test]
    fn rejects_object_without_anchor() {
        let refs = json!([{"note": "no path here"}]);
        assert!(validate_source_refs(&refs).is_err());
    }

    #[test]
    fn rejects_nested_structure_smuggling() {
        let refs = json!([{"path": "a.rs", "lines": ["1", "2", "lots", "of", "data"]}]);
        let err = validate_source_refs(&refs).unwrap_err();
        assert!(err.contains("smuggling"), "got: {err}");
    }
}
