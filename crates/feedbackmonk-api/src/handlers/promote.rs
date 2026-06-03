//! # Promote-to-roadmap handler (FR-FBR-12, Contract C16)
//!
//! ## ULADP Agent Context Header
//!
//! **Purpose**: implement the admin one-shot "promote this feature feedback
//! to the public roadmap" action. Adds a new row in `roadmap_items` (Worker
//! B's migration 00006), atomically transitions the source feedback to
//! `Duplicate` with `transition_reason = "promoted to roadmap"`, and is
//! idempotent on the `roadmap_items.origin_feedback_id` UNIQUE constraint.
//!
//! **File Index**:
//! - `render_roadmap_title(&str) -> String` — pure-function byte-for-byte
//!   port from gitcellar; collapses whitespace, truncates to
//!   [`TITLE_MAX_CHARS`].
//! - `render_roadmap_body(&str) -> String` — pure-function byte-for-byte
//!   port from gitcellar; the **single rendering point** for the public
//!   roadmap item body and the Q24-privacy-invariant test guard.
//! - `truncate_with_ellipsis(&str, usize) -> String` — internal helper
//!   reused by the title renderer.
//! - `TITLE_MAX_CHARS` — title-truncation cap (80 chars, matching
//!   gitcellar).
//! - `promote_handler` — wires Contract C16 against Worker B's roadmap repo
//!   (lands once `docs/planning/handoffs/p2-fanout-contracts.md` freezes).
//! - `#[cfg(test)] mod tests` — 6 ported Q24/render tests + integration
//!   test for the handler.
//!
//! **Public API**:
//! - `routes(state: AppState) -> Router` — mounts
//!   `POST /api/v1/admin/feedback/:feedback_id/promote` under the existing
//!   admin namespace.
//! - `render_roadmap_title`, `render_roadmap_body` — re-exported so other
//!   crates (admin-ui via type-mirror, or downstream tests) can render
//!   without going through HTTP.
//!
//! ## ⛔ Constraints & Business Rules — Q24 byte-for-byte invariant
//!
//! [`render_roadmap_title`] and [`render_roadmap_body`] (and the
//! `truncate_with_ellipsis` helper they share) are **byte-for-byte ports
//! from `gitcellar-cloud/src/feedback/roadmap_promote.rs`** lines 119–150.
//! The six Q24/render tests in `mod tests` are **byte-for-byte ports** from
//! gitcellar lines 340–416 — test names, assertion text, and helper
//! functions identical to the gitcellar source.
//!
//! These functions and tests defend the **Q24 privacy invariant**
//! (FR-FBR-12 / DEC-FBR-02 brand promise): the public roadmap item body
//! MUST contain the feedback message verbatim with NO `FB-<digits>`
//! reference and NO `submitted by …` framing on top of it. The
//! byte-for-byte fidelity to gitcellar source IS the safety mechanism.
//!
//! **DO NOT** "tidy" the tests. **DO NOT** merge assertions. **DO NOT**
//! replace `assert!` with `assert_eq!` even when the latter is cleaner. The
//! failure messages themselves contain the string `"Q24 violation"` — that
//! signal is load-bearing in regression logs.
//!
//! Any future refactor that "cleans up" these tests IS the Q24 regression
//! mode this rule exists to defend against. If you need to change the
//! renderers, port them again from gitcellar source (DEC-FBR-07 keeps that
//! repo READ-ONLY) — do not edit them in place.
//!
//! ## Decision Log
//!
//! - **Q24 byte-for-byte invariant**: `render_roadmap_body` +
//!   `render_roadmap_title` + `truncate_with_ellipsis` + the 6 named tests
//!   are byte-for-byte ports from gitcellar. DO NOT modify in place. A
//!   future refactor that "tidies" the test IS the Q24 regression mode.
//!   Anchor: DEC-FBR-02 brand promise.
//! - **Idempotency mechanism**: `roadmap_items.origin_feedback_id UNIQUE`
//!   (Worker B's migration 00006) + `get_existing_promotion(scope,
//!   feedback_id)` re-fetch on UNIQUE violation. Mirrors gitcellar's
//!   `get_roadmap_mapping_by_feedback` pattern.
//! - **Atomic same-txn transition** (Contract C6 Hard Invariant #4):
//!   roadmap insert + feedback status update + audit-history append run in
//!   one DB transaction via `_in_executor` overloads on Worker B's repos.
//! - **Module-local, not a separate `promote.md`**: ULADP Agent Context
//!   Header lives inline at the top of this file because the module is a
//!   single file (one .rs); a separate README would split the source of
//!   truth and risk drift. Tier-1 / Tier-2 inline-header pattern matches
//!   `admin_feedback.rs`'s module doc style.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{FeedbackId, FeedbackKind, FeedbackStatus, RoadmapItemStatus};
use feedbackmonk_repository::{NewRoadmapItem, ProjectScope, RepoError, TenantScope};

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::state::AppState;

/// Title-truncation cap — long feedback messages get truncated for the
/// public roadmap-item title; the full message goes in the body.
///
/// Byte-for-byte from gitcellar `roadmap_promote.rs` line 59.
pub const TITLE_MAX_CHARS: usize = 80;

/// Render the public roadmap item title from the feedback message.
///
/// Q24 invariant: NO `FB-N` reference, NO submitter username. Title is just
/// the message (trimmed + truncated). Tests assert this byte-for-byte.
///
/// **Byte-for-byte port from gitcellar `roadmap_promote.rs` lines 119–129.
/// DO NOT modify — see module-level Q24 invariant.**
pub fn render_roadmap_title(message: &str) -> String {
    // Single-line by replacing newlines so the issue title doesn't get an
    // accidental linebreak. Collapse whitespace runs to keep visual neatness.
    let collapsed = message
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    truncate_with_ellipsis(&collapsed, TITLE_MAX_CHARS)
}

/// Render the public roadmap item body. The body is the feedback message
/// verbatim with a short framing header — NO submitter PII, NO `FB-N`
/// reference. This single renderer is the entry point both for production
/// `promote_handler` and for the inline Q24 privacy-invariant unit tests.
///
/// **Byte-for-byte port from gitcellar `roadmap_promote.rs` lines 136–141.
/// DO NOT modify — see module-level Q24 invariant.**
pub fn render_roadmap_body(message: &str) -> String {
    format!(
        "Posted from a feedback submission.\n\n{}\n\n---\n\nReact with 👍 if you'd like to see this prioritized.",
        message.trim()
    )
}

/// **Byte-for-byte port from gitcellar `roadmap_promote.rs` lines 143–150.
/// DO NOT modify — see module-level Q24 invariant.**
fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}

// ---------- Contract C16: promote ----------

/// Wire `POST /api/v1/admin/feedback/{feedback_id}/promote` under the
/// admin namespace. Mirrors `admin_feedback::routes` shape so the
/// `build_app` merge is mechanical.
pub fn routes(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/feedback/:feedback_id/promote",
            post(promote_handler),
        )
        .with_state(state)
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromoteRequest {
    pub slug: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromoteResponse {
    pub roadmap_item_id: Uuid,
    pub roadmap_item_slug: String,
    pub source_feedback_id: String,    // "FB-XXXXXX"
    pub source_status: FeedbackStatus, // serializes to "duplicate"
    pub already_promoted: bool,
}

/// Structured 4xx body shape (mirrors `admin_feedback.rs`'s json-err pattern).
#[derive(Debug, Clone, Serialize)]
struct PromoteErrorBody {
    error: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<FeedbackKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<String>,
}

const TITLE_MAX_LEN_BYTES: usize = 200 * 4; // 200 chars worst case 4-byte UTF-8

pub async fn promote_handler(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
    Json(req): Json<PromoteRequest>,
) -> Result<Json<PromoteResponse>, ApiError> {
    let project_scope = sole_project_scope(&state, &session.scope).await?;
    let fb_id = FeedbackId::from(feedback_id);
    let outcome =
        perform_promote(&state, &project_scope, &fb_id, &req, session.scope.tenant_id()).await?;
    Ok(Json(outcome))
}

/// Core promote pipeline, decoupled from axum extractors so tests can
/// drive it without building an HTTP request (matches the
/// `perform_transition` pattern in `admin_feedback.rs`).
#[allow(clippy::too_many_lines)]
pub(crate) async fn perform_promote(
    state: &AppState,
    project_scope: &ProjectScope,
    feedback_id: &FeedbackId,
    req: &PromoteRequest,
    promoted_by: Uuid,
) -> Result<PromoteResponse, ApiError> {
    // ── Validation: slug + title bounds.
    let slug = req.slug.trim();
    validate_slug(slug).map_err(|e| match e {
        ApiError::BadRequest(_) => bad_request("InvalidSlug", None, Some(slug.to_string())),
        other => other,
    })?;
    if let Some(t) = &req.title {
        let trimmed_len = t.trim().len();
        if trimmed_len == 0 || trimmed_len > TITLE_MAX_LEN_BYTES {
            return Err(bad_request("InvalidSlug", None, Some(slug.to_string())));
        }
    }

    // ── Resolve source feedback (scope-checked).
    let (feedback, _history) = state
        .feedback
        .get_with_history(project_scope, feedback_id)
        .await
        .map_err(|e| match e {
            RepoError::NotFound => not_found("FeedbackNotFound"),
            other => ApiError::from(other),
        })?;

    // ── Category gate (Contract C16 Hard Invariant #3).
    if feedback.kind != FeedbackKind::Feature {
        return Err(bad_request("InvalidCategory", Some(feedback.kind), None));
    }

    // ── Idempotency probe (Contract C16 Hard Invariant #4).
    if let Some(existing) = state
        .roadmap_items
        .get_existing_promotion(project_scope, feedback.id)
        .await?
    {
        return Ok(PromoteResponse {
            roadmap_item_id: existing.id,
            roadmap_item_slug: existing.slug,
            source_feedback_id: feedback.short_code.as_str().to_string(),
            source_status: feedback.status, // may already be Duplicate from prior promote
            already_promoted: true,
        });
    }

    // ── Render title + body (Q24 byte-for-byte ports).
    let title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(|| render_roadmap_title(&feedback.body), str::to_string);
    let body = render_roadmap_body(&feedback.body);

    // ── Atomic same-txn pipeline (Contract C16 Hard Invariant #5).
    //    Order:
    //      1. roadmap_items INSERT  — UNIQUE(origin_feedback_id) is the
    //         idempotency cliff; collision → re-fetch and return idempotent
    //         response. UNIQUE(project_id, slug) → 409 SlugTaken.
    //      2. feedback.status UPDATE → Duplicate
    //      3. feedback_status_history INSERT — reason="promoted to roadmap"
    let mut tx = state.pool.begin().await?;
    let new_item = NewRoadmapItem {
        slug,
        title: &title,
        body: &body,
        status: RoadmapItemStatus::Considering,
        origin_feedback_id: Some(feedback.id),
        created_by: promoted_by,
    };

    let created = match state
        .roadmap_items
        .create_in_executor(project_scope, &mut tx, &new_item)
        .await
    {
        Ok(item) => item,
        Err(RepoError::Conflict) => {
            // UNIQUE violation. Rollback, then disambiguate: was it the
            // origin_feedback_id race (idempotent) or the slug collision
            // (SlugTaken)? The idempotency probe is the discriminator —
            // re-fetch and check.
            tx.rollback().await.ok();
            if let Some(existing) = state
                .roadmap_items
                .get_existing_promotion(project_scope, feedback.id)
                .await?
            {
                return Ok(PromoteResponse {
                    roadmap_item_id: existing.id,
                    roadmap_item_slug: existing.slug,
                    source_feedback_id: feedback.short_code.as_str().to_string(),
                    source_status: feedback.status,
                    already_promoted: true,
                });
            }
            // Not a re-promote — must be a slug collision against an
            // independently hand-created roadmap item.
            return Err(slug_conflict(slug.to_string()));
        }
        Err(other) => return Err(ApiError::from(other)),
    };

    let actual_from = state
        .feedback
        .update_status_in_executor(
            project_scope,
            &mut tx,
            feedback_id,
            FeedbackStatus::Duplicate,
        )
        .await?;

    state
        .feedback_history
        .append_in_executor(
            project_scope,
            &mut tx,
            feedback_id,
            actual_from,
            FeedbackStatus::Duplicate,
            Some("promoted to roadmap"),
            None,
            promoted_by,
        )
        .await?;

    tx.commit().await?;

    tracing::info!(
        target: "admin",
        feedback_id = %feedback_id,
        roadmap_item_id = %created.id,
        roadmap_item_slug = %created.slug,
        "feedback promoted to roadmap"
    );

    Ok(PromoteResponse {
        roadmap_item_id: created.id,
        roadmap_item_slug: created.slug,
        source_feedback_id: feedback.short_code.as_str().to_string(),
        source_status: FeedbackStatus::Duplicate,
        already_promoted: false,
    })
}

fn bad_request(code: &'static str, kind: Option<FeedbackKind>, slug: Option<String>) -> ApiError {
    ApiError::BadRequest(
        serde_json::to_string(&PromoteErrorBody { error: code, kind, slug })
            .unwrap_or_else(|_| code.to_string()),
    )
}

fn slug_conflict(slug: String) -> ApiError {
    ApiError::Conflict(
        serde_json::to_string(&PromoteErrorBody {
            error: "SlugTaken",
            kind: None,
            slug: Some(slug),
        })
        .unwrap_or_else(|_| "SlugTaken".to_string()),
    )
}

fn not_found(code: &'static str) -> ApiError {
    // ApiError::NotFound serializes a plain `"not found"` body — we want a
    // structured `{ "error": "FeedbackNotFound" }`. Routing via BadRequest
    // would change the HTTP code; instead use Conflict's structured body
    // path with the explicit code, OR keep the plain 404 + code mismatch.
    // For now, return plain NotFound; admin-ui error-handler keys on the
    // HTTP status rather than the body. If a Future contract change wants
    // a structured 404 body, ApiError needs a new variant.
    let _ = code;
    ApiError::NotFound
}

/// P0/P1 only support one project per tenant in practice; matches
/// `admin_feedback::sole_project_scope`. Duplicated rather than abstracted
/// — three short helpers across two handler files is below the
/// abstraction threshold.
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

/// Slug-validation helper used by [`promote_handler`].
///
/// kebab-case ASCII, length 1–80. Empty + length-out-of-range + non-ASCII +
/// non-`[a-z0-9-]` + leading/trailing/double `-` all reject. Surfaces
/// `BadRequest` so callers can wrap the structured body shape.
pub(crate) fn validate_slug(slug: &str) -> Result<(), ApiError> {
    if slug.is_empty() || slug.len() > 80 {
        return Err(ApiError::BadRequest(
            "slug must be 1..=80 chars".to_string(),
        ));
    }
    if slug.starts_with('-') || slug.ends_with('-') || slug.contains("--") {
        return Err(ApiError::BadRequest(
            "slug must be kebab-case (no leading/trailing/double dashes)".to_string(),
        ));
    }
    for c in slug.chars() {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(ApiError::BadRequest(
                "slug must be kebab-case ASCII [a-z0-9-]".to_string(),
            ));
        }
    }
    Ok(())
}

/// Auto-derive a slug from a title. Lowercase + ASCII + replace non-alnum
/// with `-`, collapse runs of `-`, trim leading/trailing `-`, truncate to
/// 80.
///
/// Pure function — surfaced so future callers (e.g. a `/promote/preview`
/// debug endpoint or a slug-suggestion helper if the admin-ui wants
/// server-side derivation later) can re-use the same algorithm the tests
/// pin. Currently unreferenced in the production path because the admin-ui
/// derives slugs client-side; kept and tested so the contract is one
/// chokepoint when that changes.
#[allow(dead_code)]
pub(crate) fn slug_from_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = false;
    for c in title.chars() {
        let lower = c.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.chars().count() <= 80 {
        trimmed
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[cfg(test)]
#[allow(clippy::uninlined_format_args, clippy::doc_markdown)]
// Byte-for-byte Q24 ports from gitcellar `roadmap_promote.rs` lines 340–416.
// Format-arg and doc-markdown lints are silenced module-wide to preserve the
// byte-for-byte invariant (FR-FBR-12, DEC-FBR-02). DO NOT remove this allow.
mod tests {
    use super::*;

    /// Q24 PRIVACY INVARIANT — the highest-stakes test in FR-FBR-12.
    ///
    /// The public roadmap item body MUST NOT contain:
    ///   1. The `FB-<digits>` display_id.
    ///   2. The submitter's username.
    ///
    /// If this test ever fails, a privacy regression has shipped. The
    /// plan's Testability Gate Findings calls this out as the watchpoint.
    ///
    /// **Byte-for-byte port from gitcellar `roadmap_promote.rs`
    /// lines 340–369. DO NOT modify.**
    #[test]
    fn q24_roadmap_body_excludes_fb_id_and_username() {
        let message = "Allow multi-account on one machine — I have alice and bob \
                       and switching is painful.";
        let body = render_roadmap_body(message);
        assert!(
            !body.contains("FB-"),
            "Q24 violation: roadmap body MUST NOT contain FB-N reference. Got: {}",
            body
        );
        // Username "alice" appears in the message itself — that's the user's
        // own choice to include in their submission. The Q24 invariant is
        // about NOT adding `submitter: alice` framing on top of it. We test
        // the absence of `Originally submitted by` style framing.
        let framing_strings = &[
            "Originally submitted by",
            "Submitter:",
            "Submitted by user_id",
            "From user:",
        ];
        for f in framing_strings {
            assert!(
                !body.contains(f),
                "Q24 violation: roadmap body MUST NOT contain submitter framing {:?}",
                f
            );
        }
        // Body must contain the original message content (trimmed).
        assert!(body.contains("Allow multi-account on one machine"));
    }

    /// **Byte-for-byte port from gitcellar `roadmap_promote.rs`
    /// lines 371–384. DO NOT modify.**
    #[test]
    fn q24_roadmap_title_excludes_added_fb_framing() {
        // Phrase a message the user did NOT prefix with FB-N — this verifies
        // the renderer doesn't ADD `[FB-N]` framing. We don't assert on the
        // verbatim user-supplied content (a user who wrote "FB-42" themselves
        // sees that survive into the public title — that's their choice and
        // not a Q24 violation; Q24 is about us not ADDING attribution).
        let title = render_roadmap_title("Allow toggling X in settings");
        assert!(!title.starts_with("[FB-"));
        assert!(!title.starts_with("FB-"));
        // Title must not exceed the cap (with ellipsis margin of 1 char).
        let count = title.chars().count();
        assert!(count <= TITLE_MAX_CHARS + 1, "title too long: {}", count);
    }

    /// **Byte-for-byte port from gitcellar `roadmap_promote.rs`
    /// lines 386–392. DO NOT modify.**
    #[test]
    fn render_roadmap_title_truncates_long_messages() {
        let long = "x".repeat(200);
        let title = render_roadmap_title(&long);
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= TITLE_MAX_CHARS + 1);
    }

    /// **Byte-for-byte port from gitcellar `roadmap_promote.rs`
    /// lines 394–401. DO NOT modify.**
    #[test]
    fn render_roadmap_title_collapses_newlines() {
        let multi = "Line one\n\nLine two\n  \nLine three";
        let title = render_roadmap_title(multi);
        assert!(!title.contains('\n'));
        assert!(title.contains("Line one"));
        assert!(title.contains("Line two"));
    }

    /// **Byte-for-byte port from gitcellar `roadmap_promote.rs`
    /// lines 403–408. DO NOT modify.**
    #[test]
    fn render_roadmap_body_invites_voting() {
        let body = render_roadmap_body("foo");
        assert!(body.contains("👍"));
        assert!(body.contains("foo"));
    }

    /// **Byte-for-byte port from gitcellar `roadmap_promote.rs`
    /// lines 410–416. DO NOT modify.**
    #[test]
    fn truncate_with_ellipsis_preserves_short_input() {
        assert_eq!(truncate_with_ellipsis("hi", 10), "hi");
        assert_eq!(truncate_with_ellipsis("hi", 2), "hi");
        let t = truncate_with_ellipsis("hello world", 5);
        assert_eq!(t, "hello…");
    }

    // ---- Net-new tests for feedbackmonk-specific helpers (not in
    //      gitcellar — slug derivation is feedbackmonk's contract C16
    //      surface). These do not modify the Q24 byte-for-byte invariant.

    #[test]
    fn validate_slug_accepts_well_formed() {
        assert!(validate_slug("dark-mode").is_ok());
        assert!(validate_slug("a").is_ok());
        assert!(validate_slug("issue-1234-toggle-x").is_ok());
        // 80-char boundary.
        let max = "a".repeat(80);
        assert!(validate_slug(&max).is_ok());
    }

    #[test]
    fn validate_slug_rejects_malformed() {
        assert!(validate_slug("").is_err());
        assert!(validate_slug(&"a".repeat(81)).is_err());
        assert!(validate_slug("-leading").is_err());
        assert!(validate_slug("trailing-").is_err());
        assert!(validate_slug("double--dash").is_err());
        assert!(validate_slug("Upper-Case").is_err());
        assert!(validate_slug("snake_case").is_err());
        assert!(validate_slug("spaces in").is_err());
        assert!(validate_slug("emoji-✨").is_err());
    }

    #[test]
    fn slug_from_title_lowercases_and_dashes() {
        assert_eq!(slug_from_title("Dark Mode"), "dark-mode");
        assert_eq!(
            slug_from_title("Allow toggling X in settings"),
            "allow-toggling-x-in-settings"
        );
        // Punctuation + accented chars collapse to single dashes; trim.
        assert_eq!(slug_from_title("  Hello, World!  "), "hello-world");
        assert_eq!(slug_from_title("café au lait"), "caf-au-lait");
    }

    #[test]
    fn slug_from_title_truncates_to_80() {
        let title = "a ".repeat(100);
        let slug = slug_from_title(&title);
        assert!(slug.chars().count() <= 80);
    }

    // ─── Integration tests — full promote pipeline ──────────────────────────
    //
    // sqlx::test bootstraps a fresh DB; we build a minimal AppState inline,
    // seed a Feature feedback, drive `perform_promote` directly, and assert
    // (a) roadmap_items row exists, (b) source.status=Duplicate,
    // (c) feedback_status_history row with reason="promoted to roadmap",
    // (d) second call returns already_promoted=true with the same slug.

    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use feedbackmonk_anon::{AnonGate, DEFAULT_RATE_LIMIT_PER_HOUR};
    use feedbackmonk_repository::{
        SqlxEmailVerificationRepo, SqlxFeedbackReplyRepo, SqlxFeedbackRepo,
        SqlxFeedbackStatusHistoryRepo, SqlxHealthCheck, SqlxProjectRepo, SqlxRoadmapItemRepo,
        SqlxRoadmapVoteRepo, SqlxSigningKeyRepo, SqlxTenantRepo,
    };
    use sqlx::PgPool;
    use std::num::NonZeroU32;

    use crate::email::send::RecordingEmailNotifier;
    use crate::email::Mailer;
    use crate::roadmap_voting_cache::VotingCache;

    struct StubMailer;
    #[async_trait::async_trait]
    impl Mailer for StubMailer {
        async fn send_verify_email(&self, _to: &str, _link: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn build_test_state(pool: &PgPool) -> AppState {
        let tenants = Arc::new(SqlxTenantRepo::new(pool.clone()));
        let projects = Arc::new(SqlxProjectRepo::new(pool.clone()));
        let signing_keys = Arc::new(SqlxSigningKeyRepo::new(pool.clone()));
        let feedback = Arc::new(SqlxFeedbackRepo::new(pool.clone()));
        let feedback_history = Arc::new(SqlxFeedbackStatusHistoryRepo::new(pool.clone()));
        let feedback_replies = Arc::new(SqlxFeedbackReplyRepo::new(pool.clone()));
        let email_verifications = Arc::new(SqlxEmailVerificationRepo::new(pool.clone()));
        let roadmap_items = Arc::new(SqlxRoadmapItemRepo::new(pool.clone()));
        let roadmap_votes = Arc::new(SqlxRoadmapVoteRepo::new(pool.clone()));
        let tier_quotas = Arc::new(feedbackmonk_repository::SqlxTierQuotaRepo::new(pool.clone()));
        let recorder = Arc::new(RecordingEmailNotifier::new());
        AppState {
            pool: pool.clone(),
            tenants,
            projects,
            signing_keys,
            feedback,
            feedback_history,
            feedback_replies,
            email_verifications,
            mailer: Arc::new(StubMailer),
            email_notifier: recorder as Arc<dyn crate::email::EmailNotifier>,
            session_secret: Arc::new([0u8; 32]),
            public_url: Arc::from("http://localhost:14304"),
            verify_token_ttl: Duration::hours(24),
            anon_gate: AnonGate::new(NonZeroU32::new(DEFAULT_RATE_LIMIT_PER_HOUR).unwrap()),
            login_gate: feedbackmonk_anon::LoginGate::with_default_quota(),
            jwt_iat_leeway_seconds: 5,
            roadmap_items,
            roadmap_votes,
            voting_cache: VotingCache::new(),
            started_at: Utc::now(),
            health: SqlxHealthCheck::new(pool.clone()),
            // P3 Stage 1 fixture extension — see
            // docs/test-modifications/20260514-p3-appstate-tier-quotas.md.
            tier_quotas,
        }
    }

    async fn seed_project_scope(state: &AppState, email: &str) -> ProjectScope {
        let t = state.tenants.create(email, "h").await.unwrap();
        let scope = state.tenants.scope_for(t.id).await.unwrap();
        state.tenants.mark_verified(&scope).await.unwrap();
        let p = state.projects.create(&scope, "Proj", "proj").await.unwrap();
        state.projects.open(&scope, p.id).await.unwrap()
    }

    async fn seed_feature_feedback(state: &AppState, scope: &ProjectScope) -> FeedbackId {
        state
            .feedback
            .submit_anonymous(
                scope,
                &[1u8; 32],
                Some("submitter@example.com"),
                "Allow toggling dark mode in settings.",
                FeedbackKind::Feature,
            )
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn promote_happy_path_creates_item_flips_source_and_appends_history(pool: PgPool) {
        let state = build_test_state(&pool);
        let scope = seed_project_scope(&state, "promote-happy@example.com").await;
        let fb_id = seed_feature_feedback(&state, &scope).await;
        let actor = scope.tenant_id();

        let req = PromoteRequest {
            slug: "dark-mode".to_string(),
            title: Some("Dark mode".to_string()),
        };
        let out = perform_promote(&state, &scope, &fb_id, &req, actor)
            .await
            .unwrap();

        assert!(!out.already_promoted);
        assert_eq!(out.roadmap_item_slug, "dark-mode");
        assert_eq!(out.source_feedback_id, fb_id.as_str());
        assert_eq!(out.source_status, FeedbackStatus::Duplicate);

        // Source feedback flipped to Duplicate.
        let (fb, history) = state.feedback.get_with_history(&scope, &fb_id).await.unwrap();
        assert_eq!(fb.status, FeedbackStatus::Duplicate);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].from_status, FeedbackStatus::Submitted);
        assert_eq!(history[0].to_status, FeedbackStatus::Duplicate);
        assert_eq!(
            history[0].reason_note.as_deref(),
            Some("promoted to roadmap")
        );
        assert!(
            history[0].duplicate_of_feedback_id.is_none(),
            "promote sets duplicate_of=NULL; this is a roadmap promotion, not a feedback↔feedback merge"
        );

        // Roadmap item exists with origin_feedback_id linked.
        let item = state
            .roadmap_items
            .get_existing_promotion(&scope, fb.id)
            .await
            .unwrap()
            .expect("origin_feedback_id index should resolve");
        assert_eq!(item.slug, "dark-mode");
        assert_eq!(item.origin_feedback_id, Some(fb.id));
        assert_eq!(item.status, RoadmapItemStatus::Considering);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn promote_is_idempotent_on_re_call(pool: PgPool) {
        let state = build_test_state(&pool);
        let scope = seed_project_scope(&state, "promote-idemp@example.com").await;
        let fb_id = seed_feature_feedback(&state, &scope).await;
        let actor = scope.tenant_id();

        let req = PromoteRequest {
            slug: "dark-mode".to_string(),
            title: None,
        };
        let first = perform_promote(&state, &scope, &fb_id, &req, actor)
            .await
            .unwrap();
        assert!(!first.already_promoted);

        let second = perform_promote(&state, &scope, &fb_id, &req, actor)
            .await
            .unwrap();
        assert!(second.already_promoted);
        assert_eq!(second.roadmap_item_slug, first.roadmap_item_slug);
        assert_eq!(second.roadmap_item_id, first.roadmap_item_id);
        // Source status stays Duplicate (no double-transition).
        assert_eq!(second.source_status, FeedbackStatus::Duplicate);

        // Only ONE history row — the second call short-circuits before
        // touching the audit log.
        let (_fb, history) = state.feedback.get_with_history(&scope, &fb_id).await.unwrap();
        assert_eq!(history.len(), 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn promote_rejects_non_feature_kind(pool: PgPool) {
        let state = build_test_state(&pool);
        let scope = seed_project_scope(&state, "promote-bug@example.com").await;
        let actor = scope.tenant_id();
        let fb_id = state
            .feedback
            .submit_anonymous(
                &scope,
                &[2u8; 32],
                None,
                "Login button stuck on mobile.",
                FeedbackKind::Bug,
            )
            .await
            .unwrap();

        let req = PromoteRequest {
            slug: "stuck-login".to_string(),
            title: None,
        };
        let err = perform_promote(&state, &scope, &fb_id, &req, actor)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ApiError::BadRequest(ref m) if m.contains("InvalidCategory")),
            "expected InvalidCategory, got {err:?}"
        );

        // No roadmap row written.
        let any = state.roadmap_items.list_admin(&scope, None, 50, 0).await.unwrap();
        assert_eq!(any.1, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn promote_rejects_invalid_slug(pool: PgPool) {
        let state = build_test_state(&pool);
        let scope = seed_project_scope(&state, "promote-bad-slug@example.com").await;
        let actor = scope.tenant_id();
        let fb_id = seed_feature_feedback(&state, &scope).await;

        let req = PromoteRequest {
            slug: "Invalid_Slug!".to_string(),
            title: None,
        };
        let err = perform_promote(&state, &scope, &fb_id, &req, actor)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ApiError::BadRequest(ref m) if m.contains("InvalidSlug")),
            "expected InvalidSlug, got {err:?}"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn promote_slug_collision_with_hand_created_returns_slug_taken(pool: PgPool) {
        let state = build_test_state(&pool);
        let scope = seed_project_scope(&state, "promote-slug-collide@example.com").await;
        let actor = scope.tenant_id();

        // Hand-create a roadmap item with the slug we'll try to promote into.
        state
            .roadmap_items
            .create(
                &scope,
                &NewRoadmapItem {
                    slug: "dark-mode",
                    title: "Existing item",
                    body: "Pre-existing",
                    status: RoadmapItemStatus::Considering,
                    origin_feedback_id: None,
                    created_by: actor,
                },
            )
            .await
            .unwrap();

        let fb_id = seed_feature_feedback(&state, &scope).await;
        let req = PromoteRequest {
            slug: "dark-mode".to_string(),
            title: None,
        };
        let err = perform_promote(&state, &scope, &fb_id, &req, actor)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ApiError::Conflict(ref m) if m.contains("SlugTaken")),
            "expected SlugTaken Conflict, got {err:?}"
        );

        // Source feedback unchanged.
        let (fb, history) = state.feedback.get_with_history(&scope, &fb_id).await.unwrap();
        assert_eq!(fb.status, FeedbackStatus::Submitted);
        assert!(history.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn promote_with_no_title_uses_render_roadmap_title_of_body(pool: PgPool) {
        let state = build_test_state(&pool);
        let scope = seed_project_scope(&state, "promote-default-title@example.com").await;
        let actor = scope.tenant_id();
        let fb_id = seed_feature_feedback(&state, &scope).await;

        let req = PromoteRequest {
            slug: "dark-mode".to_string(),
            title: None,
        };
        perform_promote(&state, &scope, &fb_id, &req, actor)
            .await
            .unwrap();

        let item = state
            .roadmap_items
            .get_by_slug(&scope, "dark-mode")
            .await
            .unwrap();
        // Title matches what render_roadmap_title produces from the body.
        assert_eq!(
            item.title,
            render_roadmap_title("Allow toggling dark mode in settings.")
        );
        // Body matches what render_roadmap_body produces (Q24 invariant
        // — invites voting, no FB-N framing).
        let body = item.body;
        assert!(body.contains("Allow toggling dark mode in settings."));
        assert!(body.contains("👍"));
        assert!(!body.contains("FB-"));
    }
}
