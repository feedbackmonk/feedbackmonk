#![allow(clippy::doc_markdown)] // module-doc references HTTP verbs / header names verbatim without backticks
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // limit constants are bounded (<= 200)
#![allow(clippy::module_name_repetitions)] // public `roadmap_*Response`/`roadmap_router` names are intentional
#![allow(clippy::unused_async)] // axum handlers must be async even when the body has no .await branch

//! Public + admin roadmap HTTP handlers — Contract C15.
//!
//! 8 endpoints split between two routers (composed by `main.rs::build_app`):
//!
//! ```text
//! roadmap_router(state):
//!   GET    /api/v1/projects/{project_id}/roadmap?status=&limit=&offset=
//!   GET    /api/v1/projects/{project_id}/roadmap/top-voted?limit=N
//!   GET    /api/v1/projects/{project_id}/roadmap/items/{slug}
//!   POST   /api/v1/projects/{project_id}/roadmap/items/{slug}/vote
//!   DELETE /api/v1/projects/{project_id}/roadmap/items/{slug}/vote
//!
//! admin_roadmap_router(state):
//!   GET   /api/v1/admin/projects/{project_id}/roadmap
//!   POST  /api/v1/admin/projects/{project_id}/roadmap/items
//!   PATCH /api/v1/admin/projects/{project_id}/roadmap/items/{slug}
//! ```
//!
//! ## Auth-mode resolution at vote time (Contract C15)
//!
//! Mirrors the submission endpoint pattern.
//!
//! - `Authorization: Bearer <token>` present → call
//!   `feedbackmonk_jwt::verify_with_leeway` (aliased to `jwt_verify_with_leeway`
//!   on import per submission-handler convention). `voter_id = claims.sub`,
//!   `voter_mode = RoadmapVoterMode::Jwt`.
//! - Absent → read `X-Feedbackmonk-Anon-Cookie` (mint via
//!   `AnonGate::mint_cookie` if absent + emit `Set-Cookie`). Compute
//!   `AnonGate::token_hash(ip, cookie, project_id)`. `voter_id = hex(hash)`,
//!   `voter_mode = RoadmapVoterMode::Anon`. Pre-check `state.anon_gate.check`;
//!   429 + `Retry-After` on rate-limit.
//!
//! Canonical chokepoints — re-used, NEVER parallel-implemented.
//!
//! ## Public-scope minting
//!
//! Public endpoints are unauthenticated. They derive `&ProjectScope` via
//! `state.projects.open_for_submission(project_id)` — the allowlisted
//! pre-auth boundary (DEC-PODS-001) the submission endpoint already uses.
//!
//! Lineage:
//!   FR-FBR-11 (public roadmap) + FR-FBR-13 (voting + aggregator)
//!   Contract C15 (P2 plan §Interface Contracts)
//!   docs/planning/handoffs/p2-fanout-contracts.md §C15

use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::header::{AUTHORIZATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_anon::{AnonGate, ANON_COOKIE_HEADER};
use feedbackmonk_core::{RoadmapItem, RoadmapItemStatus, RoadmapVoterMode};
use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, JwtError};
use feedbackmonk_repository::{
    NewRoadmapItem, ProjectScope, RetractOutcome, RoadmapItemPatch, DEFAULT_RETRACTION_WINDOW,
};

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::roadmap_voting_cache::{aggregate_project, DEFAULT_TOP_VOTED_LIMIT, MAX_TOP_VOTED_LIMIT};
use crate::state::AppState;

/// Cookie attributes for the anon cookie when minted by a vote request.
/// Same Max-Age / SameSite / HttpOnly as the submission endpoint.
const ANON_COOKIE_MAX_AGE_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Vote retraction window — flex 30..=120s per pre-authorized self-mediation.
/// Setting matches `feedbackmonk_repository::DEFAULT_RETRACTION_WINDOW`.
pub const RETRACTION_WINDOW: Duration = DEFAULT_RETRACTION_WINDOW;

/// Max admin-list page size; protects the admin UI from runaway responses.
const MAX_ADMIN_LIST_LIMIT: u32 = 200;
const DEFAULT_ADMIN_LIST_LIMIT: u32 = 50;

// ===========================================================================
// Response shapes (Contract C15)
// ===========================================================================

#[derive(Debug, Clone, Serialize)]
pub struct RoadmapItemResponse {
    pub slug: String,
    pub title: String,
    pub body: String,
    pub status: RoadmapItemStatus,
    pub vote_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoadmapListResponse {
    pub items: Vec<RoadmapItemResponse>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
    pub cached_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopVotedItemResponse {
    pub slug: String,
    pub title: String,
    pub status: RoadmapItemStatus,
    pub vote_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopVotedResponse {
    pub items: Vec<TopVotedItemResponse>,
    pub cached_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoteCastResponse {
    pub item_slug: String,
    pub voter_mode: RoadmapVoterMode,
    pub cast_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoteRetractResponse {
    pub item_slug: String,
    pub retracted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdminCreateRequest {
    pub slug: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub status: Option<RoadmapItemStatus>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdminPatchRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub status: Option<RoadmapItemStatus>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<RoadmapItemStatus>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TopVotedQuery {
    #[serde(default)]
    pub limit: Option<u32>,
}

// ===========================================================================
// Routers
// ===========================================================================

pub fn roadmap_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/projects/:project_id/roadmap", get(public_list))
        .route(
            "/api/v1/projects/:project_id/roadmap/top-voted",
            get(public_top_voted),
        )
        .route(
            "/api/v1/projects/:project_id/roadmap/items/:slug",
            get(public_detail),
        )
        .route(
            "/api/v1/projects/:project_id/roadmap/items/:slug/vote",
            post(public_vote_cast).delete(public_vote_retract),
        )
        .with_state(state)
}

pub fn admin_roadmap_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/projects/:project_id/roadmap",
            get(admin_list),
        )
        .route(
            "/api/v1/admin/projects/:project_id/roadmap/items",
            post(admin_create),
        )
        .route(
            "/api/v1/admin/projects/:project_id/roadmap/items/:slug",
            patch(admin_patch),
        )
        .with_state(state)
}

// ===========================================================================
// Public endpoints
// ===========================================================================

async fn public_list(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<RoadmapListResponse>, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let limit = clamp_list_limit(q.limit);
    let offset = q.offset.unwrap_or(0);

    let (items, total) = state
        .roadmap_items
        .list_public(&scope, q.status, limit, offset)
        .await?;

    // Pull vote counts from the cache (warming the slot lazily if cold) so
    // the list response carries `vote_count` for each item.
    state.voting_cache.touch_project(project_id).await;
    let cached_at = pick_cached_at(&state.voting_cache, project_id).await;

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let vote_count = state
            .voting_cache
            .read_item_count(project_id, item.id)
            .await;
        out.push(item_response(item, vote_count));
    }

    Ok(Json(RoadmapListResponse {
        items: out,
        total,
        limit,
        offset,
        cached_at,
    }))
}

async fn public_top_voted(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(q): Query<TopVotedQuery>,
) -> Result<Json<TopVotedResponse>, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let limit = q
        .limit
        .map_or(DEFAULT_TOP_VOTED_LIMIT as u32, |n| {
            n.min(MAX_TOP_VOTED_LIMIT as u32)
        }) as usize;

    // Cold-start warmer: if the project has never been seen, aggregate-on-read
    // inline so the first request doesn't return an empty list. Subsequent
    // requests are served from the cache regardless.
    if cache_is_cold(&state.voting_cache, project_id).await {
        match aggregate_project(
            state.roadmap_items.as_ref(),
            &scope,
            MAX_TOP_VOTED_LIMIT as i64,
        )
        .await
        {
            Ok(entry) => state.voting_cache.replace_project(project_id, entry).await,
            Err(_) => state.voting_cache.touch_project(project_id).await,
        }
    } else {
        state.voting_cache.touch_project(project_id).await;
    }

    let (top, cached_at) = state.voting_cache.read_top_voted(project_id, limit).await;

    // Resolve slugs/titles/statuses for the top-voted item_ids. One repo
    // round-trip per item is OK for a top-N (N <= 50).
    let mut items = Vec::with_capacity(top.len());
    for row in top {
        // Fetch the item via list_admin -> filter (cheap for N items vs
        // adding a get_by_id repo method that needs allowlisting). The
        // alternative is a single SQL with item_id IN (...); deferred to
        // P3 alongside the tier-state plumbing.
        let (all_items, _) = state
            .roadmap_items
            .list_admin(&scope, None, MAX_TOP_VOTED_LIMIT as u32, 0)
            .await?;
        if let Some(matched) = all_items.into_iter().find(|i| i.id == row.item_id) {
            items.push(TopVotedItemResponse {
                slug: matched.slug,
                title: matched.title,
                status: matched.status,
                vote_count: row.vote_count,
            });
        }
    }

    Ok(Json(TopVotedResponse { items, cached_at }))
}

async fn public_detail(
    State(state): State<AppState>,
    Path((project_id, slug)): Path<(Uuid, String)>,
) -> Result<Json<RoadmapItemResponse>, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let item = state.roadmap_items.get_by_slug(&scope, &slug).await?;
    let vote_count = state
        .roadmap_votes
        .vote_count_for_item(&scope, item.id)
        .await?;
    Ok(Json(item_response(item, vote_count)))
}

async fn public_vote_cast(
    State(state): State<AppState>,
    Path((project_id, slug)): Path<(Uuid, String)>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let item = state.roadmap_items.get_by_slug(&scope, &slug).await?;

    let (voter_id, voter_mode, set_cookie_header) = match resolve_voter(
        &state,
        &scope,
        project_id,
        &headers,
        addr,
    )
    .await
    {
        Ok(r) => r,
        Err(VoteResolveError::JwtError(e)) => return Ok(jwt_error_response(&e)),
        Err(VoteResolveError::RateLimited(retry)) => return Ok(rate_limited_response(retry)),
        Err(VoteResolveError::Api(api)) => return Err(api),
    };

    let vote = match state
        .roadmap_votes
        .cast(&scope, item.id, &voter_id, voter_mode)
        .await
    {
        Ok(v) => v,
        Err(feedbackmonk_repository::RepoError::Conflict) => {
            return Ok(already_voted_response());
        }
        Err(e) => return Err(e.into()),
    };

    let resp = Json(VoteCastResponse {
        item_slug: item.slug,
        voter_mode: vote.voter_mode,
        cast_at: vote.cast_at,
    });
    let mut response = (StatusCode::OK, resp).into_response();
    if let Some(cookie) = set_cookie_header {
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    Ok(response)
}

async fn public_vote_retract(
    State(state): State<AppState>,
    Path((project_id, slug)): Path<(Uuid, String)>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let item = state.roadmap_items.get_by_slug(&scope, &slug).await?;

    // Retract does NOT touch rate-limit (it's not an additional submission)
    // but it does need the same voter_id resolution to find the row.
    let (voter_id, _voter_mode, _set_cookie) = match resolve_voter_no_rate_limit(
        &state,
        &scope,
        project_id,
        &headers,
        addr,
    )
    .await
    {
        Ok(r) => r,
        Err(VoteResolveError::JwtError(e)) => return Ok(jwt_error_response(&e)),
        Err(VoteResolveError::RateLimited(_)) => unreachable!("no_rate_limit variant"),
        Err(VoteResolveError::Api(api)) => return Err(api),
    };

    match state
        .roadmap_votes
        .retract(&scope, item.id, &voter_id, RETRACTION_WINDOW)
        .await?
    {
        RetractOutcome::Removed { retracted_at } => {
            let resp = Json(VoteRetractResponse {
                item_slug: item.slug,
                retracted_at,
            });
            Ok((StatusCode::OK, resp).into_response())
        }
        RetractOutcome::NotFound => Ok(vote_not_found_response()),
        RetractOutcome::WindowExpired { .. } => Ok(retraction_window_expired_response()),
    }
}

// ===========================================================================
// Admin endpoints
// ===========================================================================

async fn admin_list(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<RoadmapListResponse>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    let limit = clamp_list_limit(q.limit);
    let offset = q.offset.unwrap_or(0);

    let (items, total) = state
        .roadmap_items
        .list_admin(&scope, q.status, limit, offset)
        .await?;

    state.voting_cache.touch_project(project_id).await;
    let cached_at = pick_cached_at(&state.voting_cache, project_id).await;

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let vote_count = state
            .voting_cache
            .read_item_count(project_id, item.id)
            .await;
        out.push(item_response(item, vote_count));
    }

    Ok(Json(RoadmapListResponse {
        items: out,
        total,
        limit,
        offset,
        cached_at,
    }))
}

async fn admin_create(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
    Json(req): Json<AdminCreateRequest>,
) -> Result<Json<RoadmapItemResponse>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;

    validate_slug(&req.slug)?;
    validate_title(&req.title)?;
    validate_body(&req.body)?;

    let status = req.status.unwrap_or_default();
    let input = NewRoadmapItem {
        slug: &req.slug,
        title: &req.title,
        body: &req.body,
        status,
        origin_feedback_id: None,
        created_by: session.scope.tenant_id(),
    };
    let created = match state.roadmap_items.create(&scope, &input).await {
        Ok(item) => item,
        Err(feedbackmonk_repository::RepoError::Conflict) => {
            return Err(ApiError::Conflict(format!(
                r#"{{"error":"SlugTaken","slug":"{}"}}"#,
                req.slug
            )));
        }
        Err(e) => return Err(e.into()),
    };

    Ok(Json(item_response(created, 0)))
}

async fn admin_patch(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, slug)): Path<(Uuid, String)>,
    Json(req): Json<AdminPatchRequest>,
) -> Result<Json<RoadmapItemResponse>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;

    if let Some(t) = req.title.as_deref() {
        validate_title(t)?;
    }
    if let Some(b) = req.body.as_deref() {
        validate_body(b)?;
    }

    let patch = RoadmapItemPatch {
        title: req.title.as_deref(),
        body: req.body.as_deref(),
        status: req.status,
    };
    let updated = state.roadmap_items.update(&scope, &slug, &patch).await?;
    let vote_count = state.roadmap_votes.vote_count_for_item(&scope, updated.id).await?;
    Ok(Json(item_response(updated, vote_count)))
}

// ===========================================================================
// Voter-resolution helpers (mirrors submission endpoint pattern)
// ===========================================================================

enum VoteResolveError {
    JwtError(JwtError),
    RateLimited(u64),
    Api(ApiError),
}

impl From<ApiError> for VoteResolveError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}
impl From<feedbackmonk_repository::RepoError> for VoteResolveError {
    fn from(e: feedbackmonk_repository::RepoError) -> Self {
        Self::Api(e.into())
    }
}

async fn resolve_voter(
    state: &AppState,
    scope: &ProjectScope,
    project_id: Uuid,
    headers: &HeaderMap,
    addr: SocketAddr,
) -> Result<(String, RoadmapVoterMode, Option<HeaderValue>), VoteResolveError> {
    if let Some(token) = extract_bearer(headers) {
        let active_keys = state.signing_keys.list_active(scope).await?;
        let now_unix = current_unix_timestamp();
        let claims = jwt_verify_with_leeway(
            &token,
            project_id,
            &active_keys,
            now_unix,
            state.jwt_iat_leeway_seconds,
        )
        .map_err(VoteResolveError::JwtError)?;
        Ok((claims.sub, RoadmapVoterMode::Jwt, None))
    } else {
        let client_ip = addr.ip().to_string();
        let (cookie_value, set_cookie_header) = resolve_anon_cookie(headers);
        let token_hash = AnonGate::token_hash(&client_ip, &cookie_value, project_id);

        match state.anon_gate.check(&token_hash, project_id) {
            Ok(_) => {}
            Err(feedbackmonk_anon::RateLimitError::Exceeded { retry_after_seconds }) => {
                return Err(VoteResolveError::RateLimited(retry_after_seconds));
            }
        }
        let voter_id = hex_encode(&token_hash);
        Ok((voter_id, RoadmapVoterMode::Anon, set_cookie_header))
    }
}

/// Like `resolve_voter` but skips the rate-limit check. Retract is not a
/// new submission; rate-limiting it would block users from undoing accidental
/// votes during a burst.
async fn resolve_voter_no_rate_limit(
    state: &AppState,
    scope: &ProjectScope,
    project_id: Uuid,
    headers: &HeaderMap,
    addr: SocketAddr,
) -> Result<(String, RoadmapVoterMode, Option<HeaderValue>), VoteResolveError> {
    if let Some(token) = extract_bearer(headers) {
        let active_keys = state.signing_keys.list_active(scope).await?;
        let now_unix = current_unix_timestamp();
        let claims = jwt_verify_with_leeway(
            &token,
            project_id,
            &active_keys,
            now_unix,
            state.jwt_iat_leeway_seconds,
        )
        .map_err(VoteResolveError::JwtError)?;
        Ok((claims.sub, RoadmapVoterMode::Jwt, None))
    } else {
        let client_ip = addr.ip().to_string();
        let (cookie_value, set_cookie_header) = resolve_anon_cookie(headers);
        let token_hash = AnonGate::token_hash(&client_ip, &cookie_value, project_id);
        let voter_id = hex_encode(&token_hash);
        Ok((voter_id, RoadmapVoterMode::Anon, set_cookie_header))
    }
}

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let stripped = value.strip_prefix("Bearer ")?;
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

fn resolve_anon_cookie(headers: &HeaderMap) -> (String, Option<HeaderValue>) {
    if let Some(existing) = headers.get(ANON_COOKIE_HEADER).and_then(|v| v.to_str().ok()) {
        if !existing.is_empty() {
            return (existing.to_string(), None);
        }
    }
    let minted = AnonGate::mint_cookie();
    let set_cookie = format!(
        "{ANON_COOKIE_HEADER}={minted}; Path=/api/v1; Max-Age={ANON_COOKIE_MAX_AGE_SECONDS}; HttpOnly; SameSite=Lax"
    );
    let header_value = HeaderValue::from_str(&set_cookie).ok();
    (minted, header_value)
}

fn current_unix_timestamp() -> i64 {
    #[allow(clippy::cast_possible_wrap)]
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").expect("write to String never fails");
    }
    s
}

// ===========================================================================
// Validation helpers
// ===========================================================================

fn clamp_list_limit(requested: Option<u32>) -> u32 {
    requested.map_or(DEFAULT_ADMIN_LIST_LIMIT, |n| {
        n.clamp(1, MAX_ADMIN_LIST_LIMIT)
    })
}

fn validate_slug(s: &str) -> Result<(), ApiError> {
    if s.is_empty() || s.len() > 80 {
        return Err(ApiError::BadRequest(format!(
            r#"{{"error":"InvalidSlug","slug":"{s}"}}"#
        )));
    }
    if s.starts_with('-') || s.ends_with('-') {
        return Err(ApiError::BadRequest(format!(
            r#"{{"error":"InvalidSlug","slug":"{s}"}}"#
        )));
    }
    if s.contains("--") {
        return Err(ApiError::BadRequest(format!(
            r#"{{"error":"InvalidSlug","slug":"{s}"}}"#
        )));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::BadRequest(format!(
            r#"{{"error":"InvalidSlug","slug":"{s}"}}"#
        )));
    }
    Ok(())
}

fn validate_title(s: &str) -> Result<(), ApiError> {
    let len = s.chars().count();
    if len == 0 {
        return Err(ApiError::BadRequest("title must be non-empty".into()));
    }
    if len > 200 {
        return Err(ApiError::BadRequest(
            "title exceeds 200 characters".into(),
        ));
    }
    Ok(())
}

fn validate_body(s: &str) -> Result<(), ApiError> {
    let len = s.chars().count();
    if len == 0 {
        return Err(ApiError::BadRequest("body must be non-empty".into()));
    }
    if len > 16384 {
        return Err(ApiError::PayloadTooLarge(
            "body exceeds 16384 characters".into(),
        ));
    }
    Ok(())
}

// ===========================================================================
// Response builders + small helpers
// ===========================================================================

fn item_response(item: RoadmapItem, vote_count: i64) -> RoadmapItemResponse {
    RoadmapItemResponse {
        slug: item.slug,
        title: item.title,
        body: item.body,
        status: item.status,
        vote_count,
        created_at: item.created_at,
        updated_at: item.updated_at,
    }
}

fn jwt_error_response(err: &JwtError) -> Response {
    let body = Json(json!({ "error": err.variant_name() }));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

fn rate_limited_response(retry_after_seconds: u64) -> Response {
    let body = Json(json!({
        "error": "RateLimitExceeded",
        "retry_after_seconds": retry_after_seconds,
    }));
    let mut response = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
    if let Ok(v) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
        response.headers_mut().insert("Retry-After", v);
    }
    response
}

fn already_voted_response() -> Response {
    let body = Json(json!({ "error": "AlreadyVoted" }));
    (StatusCode::CONFLICT, body).into_response()
}

fn vote_not_found_response() -> Response {
    let body = Json(json!({ "error": "VoteNotFound" }));
    (StatusCode::NOT_FOUND, body).into_response()
}

fn retraction_window_expired_response() -> Response {
    let body = Json(json!({ "error": "RetractionWindowExpired" }));
    (StatusCode::FORBIDDEN, body).into_response()
}

async fn cache_is_cold(
    cache: &crate::roadmap_voting_cache::VotingCache,
    project_id: Uuid,
) -> bool {
    !cache.known_projects().await.contains(&project_id)
}

async fn pick_cached_at(
    cache: &crate::roadmap_voting_cache::VotingCache,
    project_id: Uuid,
) -> Option<DateTime<Utc>> {
    let (_, cached_at) = cache.read_top_voted(project_id, 0).await;
    cached_at
}

// ===========================================================================
// Unit tests — validation helpers (handler integration is in the workspace
// test suite once the API binary is wired)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_slug_accepts_canonical_kebab() {
        validate_slug("dark-mode").unwrap();
        validate_slug("a").unwrap();
        validate_slug("abc-123-def").unwrap();
    }

    #[test]
    fn validate_slug_rejects_edge_cases() {
        assert!(validate_slug("").is_err(), "empty");
        assert!(validate_slug(&"x".repeat(81)).is_err(), "too long");
        assert!(validate_slug("-leading").is_err(), "leading hyphen");
        assert!(validate_slug("trailing-").is_err(), "trailing hyphen");
        assert!(validate_slug("double--hyphen").is_err(), "consecutive");
        assert!(validate_slug("UPPER").is_err(), "uppercase");
        assert!(validate_slug("under_score").is_err(), "underscore");
        assert!(validate_slug("with space").is_err(), "space");
    }

    #[test]
    fn validate_title_caps_at_200() {
        validate_title("a").unwrap();
        validate_title(&"x".repeat(200)).unwrap();
        assert!(validate_title("").is_err());
        assert!(validate_title(&"x".repeat(201)).is_err());
    }

    #[test]
    fn validate_body_caps_at_16384() {
        validate_body("body").unwrap();
        validate_body(&"x".repeat(16384)).unwrap();
        assert!(validate_body("").is_err());
        assert!(matches!(
            validate_body(&"x".repeat(16385)).unwrap_err(),
            ApiError::PayloadTooLarge(_)
        ));
    }

    #[test]
    fn clamp_list_limit_uses_defaults_and_caps() {
        assert_eq!(clamp_list_limit(None), DEFAULT_ADMIN_LIST_LIMIT);
        assert_eq!(clamp_list_limit(Some(0)), 1, "zero clamps up to 1");
        assert_eq!(clamp_list_limit(Some(10_000)), MAX_ADMIN_LIST_LIMIT);
        assert_eq!(clamp_list_limit(Some(25)), 25);
    }

    #[test]
    fn hex_encode_is_lowercase_64_chars() {
        let bytes = [0xAB; 32];
        let s = hex_encode(&bytes);
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(&s[..4], "abab");
    }

    #[test]
    fn extract_bearer_present_and_empty() {
        let mut h = HeaderMap::new();
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_str("Bearer abc.def.ghi").unwrap(),
        );
        assert_eq!(extract_bearer(&h).as_deref(), Some("abc.def.ghi"));

        let mut h2 = HeaderMap::new();
        h2.insert(AUTHORIZATION, HeaderValue::from_str("Bearer ").unwrap());
        assert!(extract_bearer(&h2).is_none(), "empty token rejected");

        let h3 = HeaderMap::new();
        assert!(extract_bearer(&h3).is_none(), "absent rejected");
    }
}
