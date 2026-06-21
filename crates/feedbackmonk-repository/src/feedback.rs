//! Feedback repository (FR-FBR-01, Contract C1).
//!
//! Two submission methods, mirroring the auth-mode/anonymous-mode split in
//! Contract C3. The schema enforces the XOR invariant via a CHECK constraint
//! (exactly one of `end_user_sub` / `anon_token_hash` is non-NULL).

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::{
    event_type_for_target, Feedback, FeedbackId, FeedbackKind, FeedbackStatus, ModerationStatus,
    Sentiment,
};

use crate::error::Result;
use crate::scope::ProjectScope;

/// Process-global "is translation enabled" flag (FR-FBR-30, DEC-FBR-IMPL-25 D1).
///
/// Set ONCE at startup from the same env read that builds the translation
/// provider (`main.rs::build_translation_provider`). Read by the submit methods
/// to decide whether to stamp `translation_status = 'pending'` on a new row.
///
/// Why a process global and not an `AppState` field (Deferred Decision D1):
/// the pending-stamp happens INSIDE the repository INSERT (Stream A), and the
/// repository crate cannot depend on the api crate where the provider is built.
/// A repo-crate global keeps the stamp decision co-located with the INSERT and
/// preserves the OnceLock recommendation's goal — zero submit-signature churn
/// and zero `AppState`-literal/test-fixture ripple (the ~9-fixture cost the
/// solicitation work, DEC-FBR-IMPL-24, flagged). Default `false` => today's
/// behaviour (no stamping) unless a deployment opts in. The translate-after-
/// accept worker is only spawned when the provider is `Some`, so a `pending`
/// row can only exist when a worker exists to drain it.
static TRANSLATION_ENABLED: AtomicBool = AtomicBool::new(false);

/// Process-global translation-enablement flag (FR-FBR-30, Deferred Decision D1).
///
/// A zero-sized handle over the [`TRANSLATION_ENABLED`] atomic. It is a TYPE
/// (not two free functions) so the `multi-tenant-isolation-check` oracle can
/// reason about it as an inherent-method surface: `set` carries a `bool`, not a
/// scope, and is a documented pre-auth exception in that oracle's allowlist
/// (it is process config, not tenant data access); `enabled` takes no argument
/// and is accepted by the oracle automatically.
pub struct TranslationFlag;

impl TranslationFlag {
    /// Enable (or disable) pending-translation stamping at submit. Called once at
    /// startup with `provider.is_some()`. Idempotent.
    pub fn set(enabled: bool) {
        TRANSLATION_ENABLED.store(enabled, Ordering::Relaxed);
    }

    /// Whether new feedback submissions are stamped `translation_status = 'pending'`.
    #[must_use]
    pub fn enabled() -> bool {
        TRANSLATION_ENABLED.load(Ordering::Relaxed)
    }
}

/// One claimed row in the translate-after-accept worklist (FR-FBR-30). The
/// worker translates `body` and writes the result back to `id` via
/// [`FeedbackRepo::set_translation`]. Tenant-agnostic transport: keyed on the
/// immutable `feedback.id` primary key, never returned to a request surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTranslation {
    pub id: Uuid,
    pub body: String,
}

#[async_trait]
pub trait FeedbackRepo: Send + Sync {
    async fn submit_authenticated(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        end_user_email: Option<&str>,
        end_user_name: Option<&str>,
        external_metadata: Option<&JsonValue>,
        // crash_event_id — external crash-event correlation key (parity Gap #2;
        // migration 00010). A FIRST-CLASS column, deliberately NOT smuggled
        // through `external_metadata` (collaboration decisions.md). `None` when
        // not crash-linked. Persisted atomically in the same INSERT so a
        // crash-linked submit can never land without its link.
        crash_event_id: Option<&str>,
        // `body` — the free-text body. An EMPTY `&str` is stored as SQL NULL (a
        // sentiment-only submission; FR-FBR-28). `sentiment` — optional 3-point
        // signal. At least one of (non-empty body, sentiment) must be present;
        // the API layer validates this before calling (and the DB CHECK is the
        // backstop).
        body: &str,
        sentiment: Option<Sentiment>,
        kind: FeedbackKind,
    ) -> Result<FeedbackId>;

    async fn submit_anonymous(
        &self,
        scope: &ProjectScope,
        anon_token_hash: &[u8; 32],
        optional_email: Option<&str>,
        body: &str,
        sentiment: Option<Sentiment>,
        kind: FeedbackKind,
    ) -> Result<FeedbackId>;

    async fn list_recent(&self, scope: &ProjectScope, limit: i64) -> Result<Vec<Feedback>>;

    /// Admin listing — paged + status-filtered (Contract C6 backing method).
    /// Returns `(items, total_matching_count)`. `total` reflects the row count
    /// matching the optional status filter, NOT the page slice size.
    async fn list_for_admin(
        &self,
        scope: &ProjectScope,
        status_filter: Option<FeedbackStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<FeedbackListItem>, u32)>;

    /// Admin full-text search (GitCellar parity gap #3). Tenant + project
    /// scoped FTS over `feedback.body_tsv` (migration 00011) using
    /// `websearch_to_tsquery` for forgiving Google-style query syntax.
    /// Returns `(items, total_matching_count)` exactly like `list_for_admin`
    /// so the admin UI reuses the same row shape. Results are ordered by
    /// `ts_rank` (relevance) then `accepted_at DESC` as a stable tiebreak.
    ///
    /// A blank/whitespace `query` yields zero rows (the handler short-circuits
    /// before calling this, but the SQL is defensive: an empty
    /// `websearch_to_tsquery` matches nothing).
    async fn search_for_admin(
        &self,
        scope: &ProjectScope,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<FeedbackListItem>, u32)>;

    /// Single-feedback view used by the admin drawer (Contract C8). Pairs
    /// the full feedback row with its complete status history newest-first.
    /// Cross-tenant lookups return `NotFound` rather than an error — Stage 2
    /// Worker A maps the `Result` onto HTTP 404 vs 500.
    async fn get_with_history(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<(Feedback, Vec<StatusHistoryRow>)>;

    /// Same-transaction `feedback.status` UPDATE. Companion to
    /// `FeedbackStatusHistoryRepo::append_in_executor` for Contract C6
    /// Hard Invariant #4 -- the transition handler updates the status
    /// column and inserts the audit row inside one transaction; both
    /// roll back together on any failure.
    ///
    /// Pre-authorized widening per Stage 1->2 handoff doc:
    /// `self_mediated=true; ratification_pending=true;
    ///  matches_spec_at=docs/planning/handoffs/p1-stage1-to-stage2.md#pre-authorized`.
    ///
    /// Returns the previous status so the audit row's `from_status` field
    /// reflects the actual pre-write state (defends against TOCTOU between
    /// the handler's `get_with_history` read and this UPDATE).
    async fn update_status_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        new_status: FeedbackStatus,
    ) -> Result<FeedbackStatus>;

    // ==== P5a (FR-FBR-19) — cluster membership setter (decision point D1) ====

    /// Set (or clear) `feedback.cluster_id` — the first-class "current cluster"
    /// pointer (D1). Own transaction. When `cluster_id` is `Some`, it MUST
    /// belong to `scope` (resolved within scope first; `NotFound` otherwise);
    /// `None` un-clusters the row. `NotFound` if the feedback is absent or out
    /// of scope.
    async fn set_cluster_id(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        cluster_id: Option<Uuid>,
    ) -> Result<()>;

    /// Same-transaction variant of [`set_cluster_id`]. Worker B's
    /// clustering-on-submit assigns the cluster in the SAME txn as the feedback
    /// insert (post-insert, same transaction — adds no new external surface and
    /// no latency beyond the deterministic heuristic).
    async fn set_cluster_id_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        cluster_id: Option<Uuid>,
    ) -> Result<()>;

    /// Bulk re-point cluster membership within the caller's txn (FR-FBR-19
    /// owner merge/split, Worker B). Moves feedback rows out of `from_cluster`
    /// into `to_cluster` (both MUST belong to `scope` — resolved within scope
    /// first; `NotFound` otherwise). `only = None` moves EVERY row currently in
    /// `from_cluster` (merge); `only = Some(short_codes)` moves only those of
    /// the listed rows that are CURRENTLY in `from_cluster` (split). The
    /// `cluster_id = from_cluster` predicate makes the returned count
    /// authoritative for the caller's `member_count` adjustment — a `short_code`
    /// in `only` that is not in `from_cluster` (or out of scope) is silently
    /// not moved and not counted. Returns the number of rows actually moved.
    async fn repoint_cluster_members_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        from_cluster: Uuid,
        to_cluster: Uuid,
        only: Option<&[String]>,
    ) -> Result<i64>;

    // ==== P5b (C26) — runner ClaimedOrder grounding read ====================

    /// List the verbatim `body` text of every feedback row CURRENTLY in
    /// `cluster_id`, oldest-first (`accepted_at ASC`), capped at `limit` rows.
    /// Tenant+project scoped — a cross-tenant `cluster_id` yields an empty Vec
    /// (no row matches the scope predicate), never another tenant's bodies.
    ///
    /// Backs the runner's `ClaimedOrder.recommendation.member_bodies` assembly
    /// (C26): the rawest UNTRUSTED feedback text the implementer prompt wraps
    /// (CLAUDE-A's `prompt::wrap_untrusted` envelope). Read-only; no new
    /// constructor → no `multi-tenant-isolation-check` allowlist entry.
    async fn list_member_bodies_for_cluster(
        &self,
        scope: &ProjectScope,
        cluster_id: Uuid,
        limit: i64,
    ) -> Result<Vec<String>>;

    // ==== FR-FBR-30 — translate-after-accept worklist (tenant-agnostic) ======
    // The translate-after-accept worker (DEC-FBR-IMPL-25) drains rows stamped
    // `translation_status='pending'`, translates them off the request path, and
    // writes the result back. These four methods are tenant-AGNOSTIC transport
    // keyed on the immutable `feedback.id` PK — they read a row's `body`,
    // translate it, and write the translation back to the SAME row; no data
    // crosses tenants and nothing is returned to a request surface. They are
    // documented pre-auth exceptions in the multi-tenant-isolation-check
    // allowlist (no &ProjectScope first arg by design — translation is a global
    // background queue, not a scoped read). DEC-FBR-03 is upheld: the SQL lives
    // here, in the sole query-path crate.

    /// Claim up to `limit` rows from the translation worklist, oldest-first.
    /// The worklist is `pending` rows PLUS `failed` rows whose attempt count is
    /// below `max_attempts` (bounded auto-retry; DEC-FBR-IMPL-25 D2). Only rows
    /// with a non-NULL `body` are returned (a sentiment-only row carries no text
    /// to translate). Read-only; the per-row status transition happens via
    /// `set_translation` / `mark_translation_skipped` / `mark_translation_failed`
    /// AFTER the provider call (the worker never holds a DB txn across a provider
    /// call). The worker loop is non-overlapping (awaits processing before the
    /// next tick, like the voting-cache tick), so a plain SELECT cannot
    /// double-claim.
    async fn claim_pending_translations(
        &self,
        max_attempts: i16,
        limit: i64,
    ) -> Result<Vec<PendingTranslation>>;

    /// Record a successful translation: write `body_translated` + the detected
    /// `source_lang` and flip `translation_status` to `translated`. The verbatim
    /// `body` is NEVER touched (store-both / Q24). The `body_tsv` FTS vector
    /// auto-recomputes from the new translation (generated column; migration
    /// 00019). Keyed on `id`.
    async fn set_translation(
        &self,
        id: Uuid,
        translated: &str,
        source_lang: &str,
    ) -> Result<()>;

    /// Mark a row `skipped` — the provider detected the source already equals
    /// the target language, so no translation is needed. Records the detected
    /// `source_lang`; leaves `body_translated` NULL so reads fall back to the
    /// (already-canonical-language) `body`. Keyed on `id`.
    async fn mark_translation_skipped(&self, id: Uuid, source_lang: &str) -> Result<()>;

    /// Mark a row `failed` after a provider error and increment its attempt
    /// counter. The row stays re-pollable until its attempts reach the worker's
    /// cap (then it falls out of the worklist). Reads fall back to `body`
    /// throughout, so a provider outage is never user-visible. Keyed on `id`.
    async fn mark_translation_failed(&self, id: Uuid) -> Result<()>;

    // ==== Gap #4 (DELTA) — end-user (JWT-sub-scoped) read surface ===========
    // GitCellar customer-#1 parity gap #4. No schema change. These methods
    // back the public `/me/feedback` + `/me/feedback/:fb/thread` routes. They
    // return the NARROW `EndUserFeedback` projection (never the full
    // `Feedback` model) so the end-user surface cannot leak internal columns
    // (anon_token_hash, external_metadata, other users' email) and stays
    // decoupled from sibling-worker additions to the `Feedback` struct.

    /// List the CALLER'S OWN feedback, newest-first, paged. Filtered by
    /// `(tenant, project, end_user_sub)`. Anonymous rows (`end_user_sub IS
    /// NULL`) are structurally excluded by the `end_user_sub = $sub`
    /// predicate. Returns `(page, total_matching)`; `total` counts all of the
    /// caller's rows, not the page slice.
    async fn list_for_end_user(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<EndUserFeedback>, u32)>;

    /// Fetch ONE feedback row that belongs to the caller. Scoped by
    /// `(tenant, project, short_code, end_user_sub)` — a `short_code` that
    /// exists but belongs to a different `end_user_sub` (or is anonymous, or
    /// is in another tenant/project) returns `NotFound`, never another user's
    /// data. Backs the `/thread` endpoint's status header.
    async fn get_for_end_user(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        feedback_id: &FeedbackId,
    ) -> Result<EndUserFeedback>;

    // ==== Public Feedback Board + Moderation Gate (C28/C29) ==================

    /// Read the current `moderation_status` for one feedback row (scope-bound).
    /// `NotFound` if absent/out-of-scope. The moderation handler reads this
    /// BEFORE the txn for the pre-DB legality check (C28 inv. 2), mirroring
    /// `perform_transition`'s `get_with_history` pre-read.
    async fn get_moderation_status(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<ModerationStatus>;

    /// Same-transaction moderation transition (C28 inv. 1): locks the row,
    /// UPDATEs `feedback.moderation_status`, and appends a
    /// `feedback_moderation_events` row (`actor='admin'`, `event_type` derived
    /// from the target) in ONE transaction. The caller opens the txn via
    /// `pool.begin()` and passes `&mut *tx`. Returns
    /// `(actual_from_status, audit_id)` — `actual_from` is read under the row
    /// lock so the handler can re-validate against a concurrent racer (TOCTOU),
    /// mirroring `update_status_in_executor`. `actor_id` is the acting admin's
    /// tenant UUID (stored as string form per migration 00016).
    async fn moderate_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        to_status: ModerationStatus,
        reason_note: Option<&str>,
        actor_id: Uuid,
    ) -> Result<(ModerationStatus, Uuid)>;

    /// Admin moderation-queue read (C28): rows in `status_filter` for the
    /// project, newest-first, paged. Returns `(page, total_matching)`. Behind
    /// `AdminSession` — submitter columns ARE included (admin-internal surface,
    /// out of the public-board PII scope).
    async fn list_pending_for_admin(
        &self,
        scope: &ProjectScope,
        status_filter: ModerationStatus,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<ModerationQueueItem>, u32)>;

    /// Public board read (C29 inv. 1): approved-only via a HARD SQL LITERAL
    /// filter, newest-first, paged. Returns `(page, total_approved)`. The
    /// projection carries NO submitter identity (C29 inv. 3 / Q24 class).
    async fn list_public_board(
        &self,
        scope: &ProjectScope,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<BoardItem>, u32)>;

    /// Public board single-item read (C29). Approved-only hard SQL literal — a
    /// `pending`/`rejected` or out-of-scope `short_code` returns `NotFound`
    /// (structurally unreachable through the board).
    async fn get_public_board_item(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<BoardItem>;

    /// Board-vote moderation gate (D2, PF-BOARD-VOTING-01): resolve a public
    /// board `short_code` to its internal `feedback.id` ONLY for an approved,
    /// in-scope row (approved-only hard SQL literal). A pending/rejected/
    /// out-of-scope `short_code` returns `NotFound` so the vote endpoints 404
    /// identically to the read path (no existence oracle for hidden feedback).
    /// SELECTs `id` only — carries no submitter identity (C29 inv. 3).
    async fn resolve_approved_board_feedback_id(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<Uuid>;

    /// Ledger predicate (C28 inv. 1): true iff an owner-authored `approve`
    /// event (`event_type='approve' AND actor='admin'`) exists for this
    /// feedback within scope. Mirrors `WorkOrderEventRepo::has_approved_event`;
    /// the `public-board-moderation-gate` oracle proves the same property
    /// independently from the ledger (anti-reward-hacking leg).
    async fn has_approve_event(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<bool>;

    // ==== Capability 1 (FR-FBR-28) — sentiment trend aggregation ============

    /// Admin "satisfaction trend over time" aggregation. Scoped to
    /// `(tenant, project)`, counts feedback rows that carry a sentiment grouped
    /// by a time bucket (`day` | `week` | `month`) and sentiment value,
    /// restricted to rows accepted on/after `since`. Returns ONE
    /// [`SentimentTrendBucket`] per non-empty time bucket (oldest-first), each
    /// fully populated (zero-filled per sentiment). Buckets with no
    /// sentiment-bearing feedback are omitted; the handler fills calendar gaps
    /// if it wants a dense series.
    async fn sentiment_trend(
        &self,
        scope: &ProjectScope,
        bucket: TrendBucket,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<SentimentTrendBucket>>;
}

/// Time-bucket granularity for [`FeedbackRepo::sentiment_trend`]. Maps to a
/// Postgres `date_trunc` unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendBucket {
    Day,
    Week,
    Month,
}

impl TrendBucket {
    /// The `date_trunc` field name. A fixed allowlist (never user-interpolated
    /// raw) — the handler parses the query param into this enum first.
    #[must_use]
    pub fn as_trunc_unit(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    /// Parse the `bucket` query param. Unknown/absent ⇒ `None` (the handler
    /// defaults to `Week`).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "day" => Some(Self::Day),
            "week" => Some(Self::Week),
            "month" => Some(Self::Month),
            _ => None,
        }
    }
}

/// One time bucket of the satisfaction trend. `bucket_start` is the truncated
/// period start (UTC); the three counts are zero-filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentimentTrendBucket {
    pub bucket_start: chrono::DateTime<chrono::Utc>,
    pub negative: i64,
    pub neutral: i64,
    pub positive: i64,
}

/// Admin moderation-queue row (C28). Behind `AdminSession`, so it MAY carry
/// submitter columns — distinct from the public [`BoardItem`], which never does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModerationQueueItem {
    pub feedback_id: FeedbackId,
    pub kind: FeedbackKind,
    pub moderation_status: ModerationStatus,
    /// First 200 chars of the body.
    pub body_excerpt: String,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
    pub submitter_email: Option<String>,
    pub is_anonymous: bool,
}

/// Narrow PUBLIC-board projection (C29). Mirrors [`EndUserFeedback`]: it carries
/// ONLY public-facing fields and NEVER submitter identity (privacy invariant,
/// sibling to Q24). `vote_count` is the real aggregate over
/// `feedback_board_votes` (D1 — direct SQL `LEFT JOIN` count, not a cache;
/// PF-BOARD-VOTING-01). It is a public aggregate count, never voter identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardItem {
    pub feedback_id: FeedbackId,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body: String,
    pub vote_count: i64,
    pub accepted_at: chrono::DateTime<chrono::Utc>,
}

/// Narrow projection of a feedback row for the end-user (JWT) read surface
/// (Gap #4). Deliberately omits every internal/other-party column — the
/// end-user only ever sees their own submission's public-facing fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndUserFeedback {
    pub feedback_id: FeedbackId,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    /// Empty string when the submission was sentiment-only (no body).
    pub body: String,
    /// The submitter's own sentiment, if they gave one (FR-FBR-28).
    pub sentiment: Option<Sentiment>,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
}

/// Trimmed list item — the columns the admin list page renders, plus the
/// `reply_count` that Stage 2 Worker A's `/admin/feedback` endpoint exposes
/// in its JSON shape (Contract C8). `reply_count` is hard-zero in Stage 1
/// because the `feedback_replies` table doesn't exist yet (Stage 2 Worker A
/// migration 00004 adds it). Worker A widens this SQL when the table lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackListItem {
    pub feedback_id: FeedbackId,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    /// Optional 3-point satisfaction signal (FR-FBR-28). `None` when the
    /// submission carried no sentiment.
    pub sentiment: Option<Sentiment>,
    /// First 200 chars of the body. The admin UI fetches the full body via
    /// `get_with_history` when the user opens the drawer.
    pub body_excerpt: String,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
    /// Hash of (auth-mode email | anon mode marker). Worker A's HTTP layer
    /// turns this into a display string like `"alice@example.com"` or
    /// `"anonymous"`.
    pub submitter_email: Option<String>,
    pub is_anonymous: bool,
    /// Stage 1 always zero. Stage 2 Worker A wires this to
    /// `feedback_replies` once that table exists.
    pub reply_count: i64,
}

/// One row of `feedback_status_history`. Stage 2 Worker A's HTTP layer
/// joins `transitioned_by` against the future `tenant_users` table to
/// derive a human-readable label; Stage 1 returns the raw UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusHistoryRow {
    pub id: uuid::Uuid,
    pub feedback_id: uuid::Uuid,
    pub from_status: FeedbackStatus,
    pub to_status: FeedbackStatus,
    pub reason_note: Option<String>,
    pub duplicate_of_feedback_id: Option<uuid::Uuid>,
    pub transitioned_by: uuid::Uuid,
    pub transitioned_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct SqlxFeedbackRepo {
    pool: PgPool,
}

impl SqlxFeedbackRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FeedbackRepo for SqlxFeedbackRepo {
    async fn submit_authenticated(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        end_user_email: Option<&str>,
        end_user_name: Option<&str>,
        external_metadata: Option<&JsonValue>,
        crash_event_id: Option<&str>,
        body: &str,
        sentiment: Option<Sentiment>,
        kind: FeedbackKind,
    ) -> Result<FeedbackId> {
        let short_code = FeedbackId::generate();
        let kind_str = kind.as_str();
        // Empty body => SQL NULL (sentiment-only submission, FR-FBR-28). The
        // DB CHECK `feedback_body_or_sentiment_check` is the backstop.
        let body_opt: Option<&str> = (!body.is_empty()).then_some(body);
        let sentiment_str: Option<&str> = sentiment.map(Sentiment::as_db_str);
        // FR-FBR-30: stamp `pending` for the translate-after-accept worker ONLY
        // when translation is enabled AND the row carries body text. Else NULL
        // (lazy backfill: untouched). Computed here, off the public submit path's
        // critical work — no provider call ever happens synchronously.
        let translation_status: Option<&str> =
            (TranslationFlag::enabled() && body_opt.is_some()).then_some("pending");
        sqlx::query!(
            r#"
            INSERT INTO feedback (
                short_code, project_id, tenant_id,
                end_user_sub, end_user_email, end_user_name,
                external_metadata, crash_event_id, body, sentiment, kind,
                translation_status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
            short_code.as_str(),
            scope.project_id(),
            scope.tenant_id(),
            end_user_sub,
            end_user_email,
            end_user_name,
            external_metadata,
            crash_event_id,
            body_opt,
            sentiment_str,
            kind_str,
            translation_status,
        )
        .execute(&self.pool)
        .await?;
        Ok(short_code)
    }

    async fn submit_anonymous(
        &self,
        scope: &ProjectScope,
        anon_token_hash: &[u8; 32],
        optional_email: Option<&str>,
        body: &str,
        sentiment: Option<Sentiment>,
        kind: FeedbackKind,
    ) -> Result<FeedbackId> {
        let short_code = FeedbackId::generate();
        let kind_str = kind.as_str();
        let token: &[u8] = anon_token_hash.as_slice();
        let body_opt: Option<&str> = (!body.is_empty()).then_some(body);
        let sentiment_str: Option<&str> = sentiment.map(Sentiment::as_db_str);
        // FR-FBR-30: see submit_authenticated — stamp `pending` only when
        // translation is enabled AND the row carries body text.
        let translation_status: Option<&str> =
            (TranslationFlag::enabled() && body_opt.is_some()).then_some("pending");
        sqlx::query!(
            r#"
            INSERT INTO feedback (
                short_code, project_id, tenant_id,
                end_user_email, anon_token_hash, body, sentiment, kind,
                translation_status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            short_code.as_str(),
            scope.project_id(),
            scope.tenant_id(),
            optional_email,
            token,
            body_opt,
            sentiment_str,
            kind_str,
            translation_status,
        )
        .execute(&self.pool)
        .await?;

        // Upsert the anon_submissions counter (dedup tracking; FR-FBR-06).
        sqlx::query!(
            r#"
            INSERT INTO anon_submissions (anon_token_hash, project_id)
            VALUES ($1, $2)
            ON CONFLICT (anon_token_hash, project_id) DO UPDATE
              SET last_submission_at = now(),
                  submission_count = anon_submissions.submission_count + 1
            "#,
            token,
            scope.project_id(),
        )
        .execute(&self.pool)
        .await?;

        Ok(short_code)
    }

    async fn list_recent(&self, scope: &ProjectScope, limit: i64) -> Result<Vec<Feedback>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, short_code, project_id, tenant_id,
                   end_user_sub, end_user_email, end_user_name,
                   external_metadata, crash_event_id, anon_token_hash, body, sentiment, kind, accepted_at, status
            FROM feedback
            WHERE project_id = $1 AND tenant_id = $2
            ORDER BY accepted_at DESC
            LIMIT $3
            "#,
            scope.project_id(),
            scope.tenant_id(),
            limit,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Feedback {
                id: r.id,
                short_code: FeedbackId::from(r.short_code),
                project_id: r.project_id,
                tenant_id: r.tenant_id,
                end_user_sub: r.end_user_sub,
                end_user_email: r.end_user_email,
                end_user_name: r.end_user_name,
                external_metadata: r.external_metadata,
                crash_event_id: r.crash_event_id,
                anon_token_hash: r.anon_token_hash,
                // Nullable body (sentiment-only rows) maps to "" (FR-FBR-28).
                body: r.body.unwrap_or_default(),
                sentiment: r.sentiment.as_deref().and_then(Sentiment::parse),
                kind: FeedbackKind::from_db_str(&r.kind),
                accepted_at: r.accepted_at,
                status: FeedbackStatus::from_db_str(&r.status),
            })
            .collect())
    }

    async fn list_for_admin(
        &self,
        scope: &ProjectScope,
        status_filter: Option<FeedbackStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<FeedbackListItem>, u32)> {
        // `Option<&str>` lets sqlx bind a nullable parameter; the WHERE
        // clause's `$3::text IS NULL OR status = $3` collapses to "no filter"
        // when the caller passes `None`.
        let status_str: Option<&'static str> = status_filter.map(FeedbackStatus::as_db_str);

        let items = sqlx::query!(
            r#"
            SELECT short_code,
                   kind,
                   status,
                   sentiment,
                   left(body, 200) AS body_excerpt,
                   end_user_email,
                   anon_token_hash IS NOT NULL AS is_anonymous,
                   accepted_at
            FROM feedback
            WHERE tenant_id = $1
              AND project_id = $2
              AND ($3::text IS NULL OR status = $3)
            ORDER BY accepted_at DESC
            LIMIT $4
            OFFSET $5
            "#,
            scope.tenant_id(),
            scope.project_id(),
            status_str,
            i64::from(limit),
            i64::from(offset),
        )
        .fetch_all(&self.pool)
        .await?;

        let total_row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback
            WHERE tenant_id = $1
              AND project_id = $2
              AND ($3::text IS NULL OR status = $3)
            "#,
            scope.tenant_id(),
            scope.project_id(),
            status_str,
        )
        .fetch_one(&self.pool)
        .await?;
        let total: u32 = total_row.count.try_into().unwrap_or(u32::MAX);

        let list = items
            .into_iter()
            .map(|r| FeedbackListItem {
                feedback_id: FeedbackId::from(r.short_code),
                kind: FeedbackKind::from_db_str(&r.kind),
                status: FeedbackStatus::from_db_str(&r.status),
                sentiment: r.sentiment.as_deref().and_then(Sentiment::parse),
                body_excerpt: r.body_excerpt.unwrap_or_default(),
                submitted_at: r.accepted_at,
                submitter_email: r.end_user_email,
                is_anonymous: r.is_anonymous.unwrap_or(false),
                // Stage 2 Worker A's migration 00004 (feedback_replies) and
                // their handler widening surface the real count. Stage 1
                // returns hard-zero per the brief's scope discipline.
                reply_count: 0,
            })
            .collect();

        Ok((list, total))
    }

    async fn search_for_admin(
        &self,
        scope: &ProjectScope,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<FeedbackListItem>, u32)> {
        // Same `(tenant_id, project_id)` scope clause as every other feedback
        // read (DEC-FBR-03 sole-query-path). `websearch_to_tsquery` parses the
        // raw admin query forgivingly (quoted phrases, `-exclude`, `or`) and
        // never raises a parse error, so a malformed/blank query simply matches
        // nothing. Ordering: relevance first, then newest-first as a stable
        // tiebreak so equal-rank rows page deterministically.
        let items = sqlx::query!(
            r#"
            SELECT short_code,
                   kind,
                   status,
                   sentiment,
                   left(body, 200) AS body_excerpt,
                   end_user_email,
                   anon_token_hash IS NOT NULL AS is_anonymous,
                   accepted_at
            FROM feedback
            WHERE tenant_id = $1
              AND project_id = $2
              AND body_tsv @@ websearch_to_tsquery('english', $3)
            ORDER BY ts_rank(body_tsv, websearch_to_tsquery('english', $3)) DESC,
                     accepted_at DESC
            LIMIT $4
            OFFSET $5
            "#,
            scope.tenant_id(),
            scope.project_id(),
            query,
            i64::from(limit),
            i64::from(offset),
        )
        .fetch_all(&self.pool)
        .await?;

        let total_row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback
            WHERE tenant_id = $1
              AND project_id = $2
              AND body_tsv @@ websearch_to_tsquery('english', $3)
            "#,
            scope.tenant_id(),
            scope.project_id(),
            query,
        )
        .fetch_one(&self.pool)
        .await?;
        let total: u32 = total_row.count.try_into().unwrap_or(u32::MAX);

        let list = items
            .into_iter()
            .map(|r| FeedbackListItem {
                feedback_id: FeedbackId::from(r.short_code),
                kind: FeedbackKind::from_db_str(&r.kind),
                status: FeedbackStatus::from_db_str(&r.status),
                sentiment: r.sentiment.as_deref().and_then(Sentiment::parse),
                body_excerpt: r.body_excerpt.unwrap_or_default(),
                submitted_at: r.accepted_at,
                submitter_email: r.end_user_email,
                is_anonymous: r.is_anonymous.unwrap_or(false),
                // Mirrors list_for_admin: the HTTP layer enriches the real
                // reply_count per row (the repository's search method stays a
                // pure feedback read).
                reply_count: 0,
            })
            .collect();

        Ok((list, total))
    }

    async fn get_with_history(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<(Feedback, Vec<StatusHistoryRow>)> {
        let row = sqlx::query!(
            r#"
            SELECT id, short_code, project_id, tenant_id,
                   end_user_sub, end_user_email, end_user_name,
                   external_metadata, crash_event_id, anon_token_hash, body, sentiment, kind, accepted_at, status
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;

        let feedback = Feedback {
            id: row.id,
            short_code: FeedbackId::from(row.short_code),
            project_id: row.project_id,
            tenant_id: row.tenant_id,
            end_user_sub: row.end_user_sub,
            end_user_email: row.end_user_email,
            end_user_name: row.end_user_name,
            external_metadata: row.external_metadata,
            crash_event_id: row.crash_event_id,
            anon_token_hash: row.anon_token_hash,
            body: row.body.unwrap_or_default(),
            sentiment: row.sentiment.as_deref().and_then(Sentiment::parse),
            kind: FeedbackKind::from_db_str(&row.kind),
            accepted_at: row.accepted_at,
            status: FeedbackStatus::from_db_str(&row.status),
        };

        let history_rows = sqlx::query!(
            r#"
            SELECT id, feedback_id, from_status, to_status, reason_note,
                   duplicate_of_feedback_id, transitioned_by, transitioned_at
            FROM feedback_status_history
            WHERE feedback_id = $1
            ORDER BY transitioned_at DESC
            "#,
            feedback.id,
        )
        .fetch_all(&self.pool)
        .await?;

        let history = history_rows
            .into_iter()
            .map(|r| StatusHistoryRow {
                id: r.id,
                feedback_id: r.feedback_id,
                from_status: FeedbackStatus::from_db_str(&r.from_status),
                to_status: FeedbackStatus::from_db_str(&r.to_status),
                reason_note: r.reason_note,
                duplicate_of_feedback_id: r.duplicate_of_feedback_id,
                transitioned_by: r.transitioned_by,
                transitioned_at: r.transitioned_at,
            })
            .collect();

        Ok((feedback, history))
    }

    async fn update_status_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        new_status: FeedbackStatus,
    ) -> Result<FeedbackStatus> {
        // The UPDATE...RETURNING gives us back the row we just updated; we
        // need the PRE-update status, so we read it inside the same txn
        // BEFORE the write. Scope filter on both reads/writes ensures a
        // cross-tenant feedback_id cannot be touched.
        let pre = sqlx::query!(
            r#"
            SELECT status
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            FOR UPDATE
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;
        let from_status = FeedbackStatus::from_db_str(&pre.status);

        sqlx::query!(
            r#"
            UPDATE feedback
            SET status = $1
            WHERE tenant_id = $2 AND project_id = $3 AND short_code = $4
            "#,
            new_status.as_db_str(),
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .execute(&mut *conn)
        .await?;

        Ok(from_status)
    }

    // ==== P5a (FR-FBR-19) — cluster membership setter impl ==================

    async fn set_cluster_id(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        cluster_id: Option<Uuid>,
    ) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        self.set_cluster_id_in_executor(scope, &mut conn, feedback_id, cluster_id)
            .await
    }

    async fn set_cluster_id_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        cluster_id: Option<Uuid>,
    ) -> Result<()> {
        // When assigning (not clearing), the target cluster MUST belong to the
        // same scope — a cross-tenant cluster_id cannot be stamped onto this
        // feedback. The DB FK guarantees the cluster exists; this guarantees it
        // is OURS.
        if let Some(cid) = cluster_id {
            sqlx::query!(
                r#"
                SELECT id FROM feedback_clusters
                WHERE tenant_id = $1 AND project_id = $2 AND id = $3
                "#,
                scope.tenant_id(),
                scope.project_id(),
                cid,
            )
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(crate::error::RepoError::NotFound)?;
        }

        let result = sqlx::query!(
            r#"
            UPDATE feedback
            SET cluster_id = $1
            WHERE tenant_id = $2 AND project_id = $3 AND short_code = $4
            "#,
            cluster_id,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(crate::error::RepoError::NotFound);
        }
        Ok(())
    }

    async fn repoint_cluster_members_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        from_cluster: Uuid,
        to_cluster: Uuid,
        only: Option<&[String]>,
    ) -> Result<i64> {
        // Both clusters MUST be ours — resolve each within scope first so a
        // cross-tenant cluster id can neither source nor sink members.
        for cid in [from_cluster, to_cluster] {
            sqlx::query!(
                r#"
                SELECT id FROM feedback_clusters
                WHERE tenant_id = $1 AND project_id = $2 AND id = $3
                "#,
                scope.tenant_id(),
                scope.project_id(),
                cid,
            )
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(crate::error::RepoError::NotFound)?;
        }

        // `only IS NULL` → move every member of from_cluster (merge);
        // otherwise restrict to the listed short_codes (split). The
        // `cluster_id = from_cluster` predicate keeps the count authoritative.
        let only_owned: Option<Vec<String>> = only.map(<[String]>::to_vec);
        let result = sqlx::query!(
            r#"
            UPDATE feedback
            SET cluster_id = $4
            WHERE tenant_id = $1 AND project_id = $2
              AND cluster_id = $3
              AND ($5::text[] IS NULL OR short_code = ANY($5::text[]))
            "#,
            scope.tenant_id(),
            scope.project_id(),
            from_cluster,
            to_cluster,
            only_owned.as_deref(),
        )
        .execute(&mut *conn)
        .await?;

        Ok(i64::try_from(result.rows_affected()).unwrap_or(i64::MAX))
    }

    // ==== P5b (C26) — runner ClaimedOrder grounding read impl ================

    async fn list_member_bodies_for_cluster(
        &self,
        scope: &ProjectScope,
        cluster_id: Uuid,
        limit: i64,
    ) -> Result<Vec<String>> {
        // Scope predicate (tenant + project) is the isolation guarantee: a
        // cross-tenant cluster_id matches no rows. Oldest-first + short_code
        // tiebreak for a deterministic, stable order (the runner prompt is
        // order-insensitive, but tests assert on it).
        // FR-FBR-30 (Stream D): the analyst/clustering machine consumer reads the
        // English translation when present, falling back to the verbatim original
        // when absent (NULL/disabled/skipped/failed) or empty. `NULLIF(...,'')`
        // guards against an empty-string translation collapsing to the original.
        // This is the ONLY consumer read-site that switches to the translation;
        // every public / end-user / board / admin-display read keeps the verbatim
        // `body` (Q24 + display authenticity). The `body` column is selected as
        // `text` (the coalesce of two text columns) — shape unchanged (Vec<String>).
        let rows = sqlx::query!(
            r#"
            SELECT coalesce(NULLIF(body_translated, ''), body) AS body
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND cluster_id = $3
            ORDER BY accepted_at ASC, short_code ASC
            LIMIT $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            cluster_id,
            limit,
        )
        .fetch_all(&self.pool)
        .await?;
        // Sentiment-only rows (NULL body AND NULL translation) carry no text to
        // ground a recommendation — drop them from the member-bodies read.
        Ok(rows.into_iter().filter_map(|r| r.body).collect())
    }

    // ==== FR-FBR-30 — translate-after-accept worklist impl ===================

    async fn claim_pending_translations(
        &self,
        max_attempts: i16,
        limit: i64,
    ) -> Result<Vec<PendingTranslation>> {
        // Worklist = pending rows + bounded-retry failed rows. `body IS NOT NULL`
        // excludes sentiment-only rows (which are never stamped pending anyway,
        // but the predicate makes the SELECT total). Oldest-first for fair drain.
        let rows = sqlx::query!(
            r#"
            SELECT id, body
            FROM feedback
            WHERE body IS NOT NULL
              AND (translation_status = 'pending'
                   OR (translation_status = 'failed' AND translation_attempts < $1))
            ORDER BY accepted_at ASC
            LIMIT $2
            "#,
            max_attempts,
            limit,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.body.map(|body| PendingTranslation { id: r.id, body }))
            .collect())
    }

    async fn set_translation(
        &self,
        id: Uuid,
        translated: &str,
        source_lang: &str,
    ) -> Result<()> {
        // Store-both: writes body_translated + source_lang and flips status.
        // NEVER touches `body` (Q24). body_tsv auto-recomputes (generated col).
        sqlx::query!(
            r#"
            UPDATE feedback
            SET body_translated = $2,
                source_lang = $3,
                translation_status = 'translated'
            WHERE id = $1
            "#,
            id,
            translated,
            source_lang,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_translation_skipped(&self, id: Uuid, source_lang: &str) -> Result<()> {
        // Already-canonical-language row: record the detected language, leave
        // body_translated NULL so reads coalesce to the original `body`.
        sqlx::query!(
            r#"
            UPDATE feedback
            SET source_lang = $2,
                translation_status = 'skipped'
            WHERE id = $1
            "#,
            id,
            source_lang,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_translation_failed(&self, id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE feedback
            SET translation_status = 'failed',
                translation_attempts = translation_attempts + 1
            WHERE id = $1
            "#,
            id,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ==== Gap #4 (DELTA) — end-user read surface impl =======================

    async fn list_for_end_user(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<EndUserFeedback>, u32)> {
        // `end_user_sub = $3` is the isolation predicate: a caller sees ONLY
        // their own rows, and anonymous rows (end_user_sub IS NULL) never
        // match. No internal columns are selected.
        let rows = sqlx::query!(
            r#"
            SELECT short_code, kind, status, body, sentiment, accepted_at
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND end_user_sub = $3
            ORDER BY accepted_at DESC
            LIMIT $4 OFFSET $5
            "#,
            scope.tenant_id(),
            scope.project_id(),
            end_user_sub,
            i64::from(limit),
            i64::from(offset),
        )
        .fetch_all(&self.pool)
        .await?;

        let total_row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND end_user_sub = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            end_user_sub,
        )
        .fetch_one(&self.pool)
        .await?;
        let total: u32 = total_row.count.try_into().unwrap_or(u32::MAX);

        let items = rows
            .into_iter()
            .map(|r| EndUserFeedback {
                feedback_id: FeedbackId::from(r.short_code),
                kind: FeedbackKind::from_db_str(&r.kind),
                status: FeedbackStatus::from_db_str(&r.status),
                body: r.body.unwrap_or_default(),
                sentiment: r.sentiment.as_deref().and_then(Sentiment::parse),
                submitted_at: r.accepted_at,
            })
            .collect();

        Ok((items, total))
    }

    async fn get_for_end_user(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        feedback_id: &FeedbackId,
    ) -> Result<EndUserFeedback> {
        // The `AND end_user_sub = $4` clause is the load-bearing isolation
        // check: a short_code belonging to a DIFFERENT sub (or anonymous, or
        // another tenant/project) returns NotFound, never another user's row.
        let row = sqlx::query!(
            r#"
            SELECT short_code, kind, status, body, sentiment, accepted_at
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2
              AND short_code = $3 AND end_user_sub = $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
            end_user_sub,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;

        Ok(EndUserFeedback {
            feedback_id: FeedbackId::from(row.short_code),
            kind: FeedbackKind::from_db_str(&row.kind),
            status: FeedbackStatus::from_db_str(&row.status),
            body: row.body.unwrap_or_default(),
            sentiment: row.sentiment.as_deref().and_then(Sentiment::parse),
            submitted_at: row.accepted_at,
        })
    }

    // ==== Public Feedback Board + Moderation Gate (C28/C29) impl =============

    async fn get_moderation_status(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<ModerationStatus> {
        let row = sqlx::query!(
            r#"
            SELECT moderation_status
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;
        Ok(ModerationStatus::from_db_str(&row.moderation_status))
    }

    async fn moderate_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        to_status: ModerationStatus,
        reason_note: Option<&str>,
        actor_id: Uuid,
    ) -> Result<(ModerationStatus, Uuid)> {
        // Lock the row + recover the pre-update status AND the UUID PK (the
        // events ledger keys on feedback.id, not short_code). Scope filter on
        // both read/write rejects a cross-tenant short_code before any write.
        let pre = sqlx::query!(
            r#"
            SELECT id, moderation_status
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            FOR UPDATE
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;
        let from_status = ModerationStatus::from_db_str(&pre.moderation_status);

        sqlx::query!(
            r#"
            UPDATE feedback
            SET moderation_status = $1
            WHERE tenant_id = $2 AND project_id = $3 AND short_code = $4
            "#,
            to_status.as_db_str(),
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .execute(&mut *conn)
        .await?;

        // Append the immutable ledger row in the SAME txn (C28 inv. 1). actor
        // is always 'admin' in v1 (only the owner moderates); event_type is
        // derived from the target (approve|reject|reset).
        let event_type = event_type_for_target(to_status);
        let actor_id_str = actor_id.to_string();
        let inserted = sqlx::query!(
            r#"
            INSERT INTO feedback_moderation_events (
                tenant_id, project_id, feedback_id,
                from_status, to_status, event_type, actor, actor_id, reason_note
            )
            VALUES ($1, $2, $3, $4, $5, $6, 'admin', $7, $8)
            RETURNING id
            "#,
            scope.tenant_id(),
            scope.project_id(),
            pre.id,
            from_status.as_db_str(),
            to_status.as_db_str(),
            event_type,
            actor_id_str,
            reason_note,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok((from_status, inserted.id))
    }

    async fn list_pending_for_admin(
        &self,
        scope: &ProjectScope,
        status_filter: ModerationStatus,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<ModerationQueueItem>, u32)> {
        let status_str = status_filter.as_db_str();
        let items = sqlx::query!(
            r#"
            SELECT short_code,
                   kind,
                   moderation_status,
                   left(body, 200) AS body_excerpt,
                   end_user_email,
                   anon_token_hash IS NOT NULL AS is_anonymous,
                   accepted_at
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND moderation_status = $3
            ORDER BY accepted_at DESC
            LIMIT $4 OFFSET $5
            "#,
            scope.tenant_id(),
            scope.project_id(),
            status_str,
            i64::from(limit),
            i64::from(offset),
        )
        .fetch_all(&self.pool)
        .await?;

        let total_row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND moderation_status = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            status_str,
        )
        .fetch_one(&self.pool)
        .await?;
        let total: u32 = total_row.count.try_into().unwrap_or(u32::MAX);

        let list = items
            .into_iter()
            .map(|r| ModerationQueueItem {
                feedback_id: FeedbackId::from(r.short_code),
                kind: FeedbackKind::from_db_str(&r.kind),
                moderation_status: ModerationStatus::from_db_str(&r.moderation_status),
                body_excerpt: r.body_excerpt.unwrap_or_default(),
                submitted_at: r.accepted_at,
                submitter_email: r.end_user_email,
                is_anonymous: r.is_anonymous.unwrap_or(false),
            })
            .collect();

        Ok((list, total))
    }

    async fn list_public_board(
        &self,
        scope: &ProjectScope,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<BoardItem>, u32)> {
        // C29 inv. 1: approved-only is a HARD SQL LITERAL filter — never a
        // bound param, never handler-side, never optional. The projection is
        // exactly the public-facing columns (no submitter identity; C29 inv. 3
        // / Q24 class), mirroring `list_for_end_user`. `vote_count` is a
        // correlated count over `feedback_board_votes` (D1, PF-BOARD-VOTING-01)
        // — a public aggregate, never voter identity.
        let items = sqlx::query!(
            r#"
            SELECT
                f.short_code,
                f.kind,
                f.status,
                f.body,
                f.accepted_at,
                (SELECT count(*) FROM feedback_board_votes v WHERE v.feedback_id = f.id) AS "vote_count!"
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2
              AND f.moderation_status = 'approved'
            ORDER BY f.accepted_at DESC
            LIMIT $3 OFFSET $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            i64::from(limit),
            i64::from(offset),
        )
        .fetch_all(&self.pool)
        .await?;

        let total_row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2
              AND moderation_status = 'approved'
            "#,
            scope.tenant_id(),
            scope.project_id(),
        )
        .fetch_one(&self.pool)
        .await?;
        let total: u32 = total_row.count.try_into().unwrap_or(u32::MAX);

        let list = items
            .into_iter()
            .map(|r| BoardItem {
                feedback_id: FeedbackId::from(r.short_code),
                kind: FeedbackKind::from_db_str(&r.kind),
                status: FeedbackStatus::from_db_str(&r.status),
                body: r.body.unwrap_or_default(),
                vote_count: r.vote_count,
                accepted_at: r.accepted_at,
            })
            .collect();

        Ok((list, total))
    }

    async fn get_public_board_item(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<BoardItem> {
        // Approved-only hard SQL literal (C29 inv. 1). A non-approved or
        // out-of-scope short_code returns NotFound — unreachable through the
        // board. Same public-only projection as `list_public_board`, plus the
        // `feedback_board_votes` aggregate (D1, PF-BOARD-VOTING-01).
        let row = sqlx::query!(
            r#"
            SELECT
                f.short_code,
                f.kind,
                f.status,
                f.body,
                f.accepted_at,
                (SELECT count(*) FROM feedback_board_votes v WHERE v.feedback_id = f.id) AS "vote_count!"
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2 AND f.short_code = $3
              AND f.moderation_status = 'approved'
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;

        Ok(BoardItem {
            feedback_id: FeedbackId::from(row.short_code),
            kind: FeedbackKind::from_db_str(&row.kind),
            status: FeedbackStatus::from_db_str(&row.status),
            body: row.body.unwrap_or_default(),
            vote_count: row.vote_count,
            accepted_at: row.accepted_at,
        })
    }

    async fn resolve_approved_board_feedback_id(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<Uuid> {
        // Board-vote MODERATION GATE (plan D2). Resolves a public board
        // short_code to its internal `feedback.id` ONLY when the row is
        // approved + in scope — approved-only is a HARD SQL LITERAL (C29 inv. 1
        // posture, same as the board reads). A pending/rejected/out-of-scope
        // short_code returns NotFound, so the vote endpoints 404 identically to
        // the read path and never become an existence oracle for hidden
        // feedback. SELECTs `id` ONLY — no submitter identity (C29 inv. 3).
        let row = sqlx::query!(
            r#"
            SELECT id
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
              AND moderation_status = 'approved'
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;

        Ok(row.id)
    }

    async fn has_approve_event(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<bool> {
        // The ledger predicate (C28 inv. 1). Scoped on the events table
        // directly; the scalar subquery resolves short_code -> id within the
        // same scope so a cross-tenant short_code yields no match.
        let row = sqlx::query!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM feedback_moderation_events e
                WHERE e.tenant_id = $1 AND e.project_id = $2
                  AND e.event_type = 'approve' AND e.actor = 'admin'
                  AND e.feedback_id = (
                      SELECT id FROM feedback
                      WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
                  )
            ) AS "exists!"
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.exists)
    }

    async fn sentiment_trend(
        &self,
        scope: &ProjectScope,
        bucket: TrendBucket,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<SentimentTrendBucket>> {
        // `date_trunc($1, accepted_at)` takes the unit as a BOUND text param —
        // `bucket.as_trunc_unit()` is a fixed allowlist (day|week|month), never
        // raw user input. Same `(tenant_id, project_id)` scope clause as every
        // other feedback read (DEC-FBR-03). FILTER aggregates zero-fill each
        // sentiment per bucket in one pass.
        let unit = bucket.as_trunc_unit();
        let rows = sqlx::query!(
            r#"
            SELECT date_trunc($1, accepted_at) AS "bucket_start!",
                   count(*) FILTER (WHERE sentiment = 'negative') AS "negative!",
                   count(*) FILTER (WHERE sentiment = 'neutral')  AS "neutral!",
                   count(*) FILTER (WHERE sentiment = 'positive') AS "positive!"
            FROM feedback
            WHERE tenant_id = $2
              AND project_id = $3
              AND sentiment IS NOT NULL
              AND accepted_at >= $4
            GROUP BY date_trunc($1, accepted_at)
            ORDER BY date_trunc($1, accepted_at) ASC
            "#,
            unit,
            scope.tenant_id(),
            scope.project_id(),
            since,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| SentimentTrendBucket {
                bucket_start: r.bucket_start,
                negative: r.negative,
                neutral: r.neutral,
                positive: r.positive,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use serde_json::json;
    use sqlx::PgPool;

    async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn submit_authenticated_round_trips(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "auth@example.com").await;

        let meta = json!({"user_id": "u-1", "plan": "pro"});
        let id = repo
            .submit_authenticated(
                &scope,
                "auth0|sub-123",
                Some("u@example.com"),
                Some("Alice"),
                Some(&meta),
                None, // crash_event_id — not crash-linked here
                "It crashed when I clicked save",
                None,
                FeedbackKind::Bug,
            )
            .await
            .unwrap();
        assert!(id.as_str().starts_with("FB-"));

        let recent = repo.list_recent(&scope, 10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].short_code, id);
        assert_eq!(recent[0].kind, FeedbackKind::Bug);
        assert_eq!(recent[0].end_user_sub.as_deref(), Some("auth0|sub-123"));
        assert!(recent[0].anon_token_hash.is_none());
        // Not crash-linked → crash_event_id is NULL.
        assert_eq!(recent[0].crash_event_id, None);
    }

    // ---- Gap #2 crash-event correlation (BRAVO) ----

    #[sqlx::test(migrations = "../../migrations")]
    async fn submit_authenticated_persists_crash_event_id(pool: PgPool) {
        // A crash-linked auth-mode submission stores crash_event_id as a
        // first-class column (NOT inside external_metadata) and round-trips
        // through both read paths (list_recent + get_with_history).
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "crash-link@example.com").await;

        let crash_id = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
        let id = repo
            .submit_authenticated(
                &scope,
                "auth0|sub-crash",
                Some("dev@example.com"),
                Some("Dev"),
                None,
                Some(crash_id),
                "App panicked on save",
                None,
                FeedbackKind::Bug,
            )
            .await
            .unwrap();

        // list_recent read path.
        let recent = repo.list_recent(&scope, 10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].crash_event_id.as_deref(), Some(crash_id));
        // First-class column — must NOT have been smuggled into metadata.
        assert!(recent[0].external_metadata.is_none());

        // get_with_history read path.
        let (fb, _hist) = repo.get_with_history(&scope, &id).await.unwrap();
        assert_eq!(fb.crash_event_id.as_deref(), Some(crash_id));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn submit_anonymous_round_trips_and_tracks_dedup(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "anon@example.com").await;

        let token = [9u8; 32];
        let id1 = repo
            .submit_anonymous(&scope, &token, None, "First note", None, FeedbackKind::Other)
            .await
            .unwrap();
        let id2 = repo
            .submit_anonymous(&scope, &token, Some("opt@in.com"), "Second", None, FeedbackKind::Feature)
            .await
            .unwrap();
        assert_ne!(id1.as_str(), id2.as_str());

        let recent = repo.list_recent(&scope, 10).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert!(recent.iter().all(|f| f.end_user_sub.is_none()));
        assert!(recent.iter().all(|f| f.anon_token_hash.as_deref() == Some(token.as_slice())));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_for_admin_returns_paged_results_with_total(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "admin@example.com").await;

        // Seed three submissions.
        for body in ["one", "two", "three"] {
            repo.submit_anonymous(&scope, &[7u8; 32], None, body, None, FeedbackKind::Other)
                .await
                .unwrap();
        }

        let (page, total) = repo.list_for_admin(&scope, None, 2, 0).await.unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(total, 3);

        let (page2, total2) = repo.list_for_admin(&scope, None, 2, 2).await.unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(total2, 3);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_for_admin_cross_tenant_negative(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-admin@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-admin@example.com").await;
        repo.submit_anonymous(&s1, &[1u8; 32], None, "from s1", None, FeedbackKind::Other)
            .await
            .unwrap();

        // Querying from s2's scope must return 0 rows for s1's feedback,
        // NOT an error. This is the multi-tenant-isolation invariant.
        let (page, total) = repo.list_for_admin(&s2, None, 10, 0).await.unwrap();
        assert!(page.is_empty());
        assert_eq!(total, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_for_admin_status_filter_returns_matching_rows(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "status-filter@example.com").await;
        repo.submit_anonymous(&scope, &[5u8; 32], None, "row", None, FeedbackKind::Other)
            .await
            .unwrap();

        // New rows are 'submitted' by default; filtering by Triaged returns 0.
        let (page, total) = repo
            .list_for_admin(&scope, Some(FeedbackStatus::Triaged), 10, 0)
            .await
            .unwrap();
        assert_eq!(page.len(), 0);
        assert_eq!(total, 0);

        // Filtering by Submitted returns the row.
        let (page, total) = repo
            .list_for_admin(&scope, Some(FeedbackStatus::Submitted), 10, 0)
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(total, 1);
        assert_eq!(page[0].status, FeedbackStatus::Submitted);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_with_history_returns_feedback_and_empty_history_initially(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "history@example.com").await;
        let id = repo
            .submit_anonymous(&scope, &[3u8; 32], None, "row body", None, FeedbackKind::Bug)
            .await
            .unwrap();

        let (fb, history) = repo.get_with_history(&scope, &id).await.unwrap();
        assert_eq!(fb.short_code, id);
        assert_eq!(fb.status, FeedbackStatus::Submitted);
        // No transitions yet (Stage 2 Worker A's handler writes these).
        assert!(history.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_with_history_cross_tenant_negative(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-history@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-history@example.com").await;
        let id = repo
            .submit_anonymous(&s1, &[6u8; 32], None, "cross-tenant target", None, FeedbackKind::Other)
            .await
            .unwrap();

        // Reading s1's feedback through s2's scope must NotFound, NOT error.
        let err = repo.get_with_history(&s2, &id).await.unwrap_err();
        assert!(matches!(err, crate::error::RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_recent_only_returns_scope_owner_rows(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2@example.com").await;

        repo.submit_anonymous(&s1, &[1u8; 32], None, "from s1", None, FeedbackKind::Other).await.unwrap();
        repo.submit_anonymous(&s2, &[2u8; 32], None, "from s2-a", None, FeedbackKind::Other).await.unwrap();
        repo.submit_anonymous(&s2, &[3u8; 32], None, "from s2-b", None, FeedbackKind::Other).await.unwrap();

        let s1_rows = repo.list_recent(&s1, 10).await.unwrap();
        let s2_rows = repo.list_recent(&s2, 10).await.unwrap();
        assert_eq!(s1_rows.len(), 1);
        assert_eq!(s2_rows.len(), 2);

        // Cross-tenant invariant: s1's rows do not appear in s2's list and vice versa.
        let s1_bodies: Vec<&str> = s1_rows.iter().map(|f| f.body.as_str()).collect();
        let s2_bodies: Vec<&str> = s2_rows.iter().map(|f| f.body.as_str()).collect();
        assert!(s1_bodies.contains(&"from s1"));
        assert!(!s1_bodies.iter().any(|b| b.starts_with("from s2")));
        assert!(s2_bodies.iter().all(|b| b.starts_with("from s2")));
    }

    // ---- Gap #3 full-text search (Task Zero: isolation-first) ----

    #[sqlx::test(migrations = "../../migrations")]
    async fn search_for_admin_matches_body_terms(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "fts-match@example.com").await;

        repo.submit_anonymous(&scope, &[1u8; 32], None, "the checkout button is broken", None, FeedbackKind::Bug)
            .await
            .unwrap();
        repo.submit_anonymous(&scope, &[2u8; 32], None, "please add a dark theme", None, FeedbackKind::Feature)
            .await
            .unwrap();

        // Multi-term query: both lexemes present in the first row's body.
        let (hits, total) = repo.search_for_admin(&scope, "broken checkout", 20, 0).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].body_excerpt.contains("checkout"));

        // Non-matching term returns nothing (not an error).
        let (none, none_total) = repo.search_for_admin(&scope, "nonexistentterm", 20, 0).await.unwrap();
        assert!(none.is_empty());
        assert_eq!(none_total, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn search_for_admin_cross_tenant_negative(pool: PgPool) {
        // THE load-bearing invariant for gap #3: search must never leak
        // another tenant's feedback. Mirrors list_for_admin_cross_tenant_negative.
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "fts-owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "fts-owner2@example.com").await;
        repo.submit_anonymous(&s1, &[1u8; 32], None, "secret roadmap leak details", None, FeedbackKind::Other)
            .await
            .unwrap();

        // s2 searches for s1's distinctive term — must return 0 rows, not error.
        let (page, total) = repo.search_for_admin(&s2, "secret roadmap", 20, 0).await.unwrap();
        assert!(page.is_empty(), "cross-tenant FTS must not leak rows");
        assert_eq!(total, 0);

        // s1 (the owner) finds its own row.
        let (own, own_total) = repo.search_for_admin(&s1, "secret roadmap", 20, 0).await.unwrap();
        assert_eq!(own.len(), 1);
        assert_eq!(own_total, 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn search_for_admin_paginates_with_total(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "fts-page@example.com").await;
        for i in 0..3 {
            repo.submit_anonymous(
                &scope,
                &[u8::try_from(i).unwrap(); 32],
                None,
                "shared keyword in every row",
                None,
                FeedbackKind::Other,
            )
            .await
            .unwrap();
        }

        let (page1, total) = repo.search_for_admin(&scope, "keyword", 2, 0).await.unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(total, 3);

        let (page2, total2) = repo.search_for_admin(&scope, "keyword", 2, 2).await.unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(total2, 3);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn search_for_admin_blank_query_matches_nothing(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "fts-blank@example.com").await;
        repo.submit_anonymous(&scope, &[1u8; 32], None, "some body text", None, FeedbackKind::Other)
            .await
            .unwrap();

        // websearch_to_tsquery('') yields an empty query that matches nothing.
        let (page, total) = repo.search_for_admin(&scope, "   ", 20, 0).await.unwrap();
        assert!(page.is_empty());
        assert_eq!(total, 0);
    }

    // ---- P5a (CLAUDE-B) cluster re-pointing (merge/split primitive) ----

    async fn cluster_id_of(pool: &PgPool, short_code: &str) -> Option<uuid::Uuid> {
        sqlx::query!(
            "SELECT cluster_id FROM feedback WHERE short_code = $1",
            short_code
        )
        .fetch_one(pool)
        .await
        .unwrap()
        .cluster_id
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn repoint_cluster_members_moves_all_then_subset(pool: PgPool) {
        use crate::clusters::{ClusterRepo, SqlxClusterRepo};
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let crepo = SqlxClusterRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "repoint@example.com").await;

        let a = crepo.create(&scope, "A", None, FeedbackKind::Bug, "agent").await.unwrap();
        let b = crepo.create(&scope, "B", None, FeedbackKind::Bug, "agent").await.unwrap();

        // Three feedback, all initially in cluster A.
        let mut ids = Vec::new();
        for i in 0..3 {
            let id = repo
                .submit_anonymous(&scope, &[u8::try_from(i).unwrap(); 32], None, "x", None, FeedbackKind::Bug)
                .await
                .unwrap();
            repo.set_cluster_id(&scope, &id, Some(a.id)).await.unwrap();
            ids.push(id);
        }

        // Merge: move EVERY member of A into B (only = None).
        let mut tx = pool.begin().await.unwrap();
        let moved = repo
            .repoint_cluster_members_in_executor(&scope, &mut tx, a.id, b.id, None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(moved, 3);
        for id in &ids {
            assert_eq!(cluster_id_of(&pool, id.as_str()).await, Some(b.id));
        }
        // A is now empty — a second merge moves nothing.
        let mut tx = pool.begin().await.unwrap();
        let again = repo
            .repoint_cluster_members_in_executor(&scope, &mut tx, a.id, b.id, None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(again, 0);

        // Split: peel ONLY ids[0] back into A (only = Some).
        let only = vec![ids[0].as_str().to_string()];
        let mut tx = pool.begin().await.unwrap();
        let split = repo
            .repoint_cluster_members_in_executor(&scope, &mut tx, b.id, a.id, Some(&only))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(split, 1);
        assert_eq!(cluster_id_of(&pool, ids[0].as_str()).await, Some(a.id));
        assert_eq!(cluster_id_of(&pool, ids[1].as_str()).await, Some(b.id));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn repoint_rejects_cross_tenant_cluster(pool: PgPool) {
        use crate::clusters::{ClusterRepo, SqlxClusterRepo};
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let crepo = SqlxClusterRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "repoint-ct1@example.com").await;
        let s2 = seed_project_scope(&pool, "repoint-ct2@example.com").await;
        let a1 = crepo.create(&s1, "A1", None, FeedbackKind::Bug, "agent").await.unwrap();
        let b2 = crepo.create(&s2, "B2", None, FeedbackKind::Bug, "agent").await.unwrap();

        // s1 cannot sink members into s2's cluster.
        let mut tx = pool.begin().await.unwrap();
        let err = repo
            .repoint_cluster_members_in_executor(&s1, &mut tx, a1.id, b2.id, None)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::RepoError::NotFound));
        tx.rollback().await.ok();
    }

    // ---- FR-FBR-28 sentiment: storage round-trip + trend aggregation ----

    #[sqlx::test(migrations = "../../migrations")]
    async fn submit_persists_sentiment(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "sentiment-rt@example.com").await;

        // Authenticated submit with a sentiment.
        let id = repo
            .submit_authenticated(
                &scope, "auth0|sub-s", Some("u@x.com"), None, None, None,
                "loved it", Some(Sentiment::Positive), FeedbackKind::Other,
            )
            .await
            .unwrap();
        // Anonymous submit with a different sentiment.
        repo.submit_anonymous(&scope, &[4u8; 32], None, "hated it", Some(Sentiment::Negative), FeedbackKind::Bug)
            .await
            .unwrap();

        // list_recent read path carries sentiment.
        let recent = repo.list_recent(&scope, 10).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert!(recent.iter().any(|f| f.sentiment == Some(Sentiment::Positive)));
        assert!(recent.iter().any(|f| f.sentiment == Some(Sentiment::Negative)));

        // get_with_history read path carries sentiment.
        let (fb, _h) = repo.get_with_history(&scope, &id).await.unwrap();
        assert_eq!(fb.sentiment, Some(Sentiment::Positive));
        assert_eq!(fb.body, "loved it");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn sentiment_only_submission_stores_empty_body(pool: PgPool) {
        // A sentiment-only submission (no body) is valid; the empty body maps
        // to "" on read (DB stores NULL). FR-FBR-28 + DEC-FBR-IMPL-23.
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "sentiment-only@example.com").await;

        let id = repo
            .submit_anonymous(&scope, &[5u8; 32], None, "", Some(Sentiment::Neutral), FeedbackKind::Other)
            .await
            .unwrap();

        let (fb, _h) = repo.get_with_history(&scope, &id).await.unwrap();
        assert_eq!(fb.body, "", "sentiment-only submission has empty body");
        assert_eq!(fb.sentiment, Some(Sentiment::Neutral));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn sentiment_trend_aggregates_and_excludes_bodyless_nulls(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "sentiment-trend@example.com").await;

        // Seed 2 positive, 1 neutral, 1 negative (all "now") + 1 row with NO
        // sentiment (must be excluded from the trend).
        for s in [Sentiment::Positive, Sentiment::Positive, Sentiment::Neutral, Sentiment::Negative] {
            repo.submit_anonymous(&scope, &[1u8; 32], None, "x", Some(s), FeedbackKind::Other)
                .await
                .unwrap();
        }
        repo.submit_anonymous(&scope, &[2u8; 32], None, "no sentiment here", None, FeedbackKind::Other)
            .await
            .unwrap();

        let since = chrono::Utc::now() - chrono::Duration::days(1);
        let buckets = repo.sentiment_trend(&scope, TrendBucket::Day, since).await.unwrap();

        // All four sentiment-bearing rows land in one day bucket.
        assert_eq!(buckets.len(), 1, "all rows in a single day bucket");
        let b = &buckets[0];
        assert_eq!(b.positive, 2);
        assert_eq!(b.neutral, 1);
        assert_eq!(b.negative, 1);
        // The body-only (NULL sentiment) row is excluded: total == 4, not 5.
        assert_eq!(b.positive + b.neutral + b.negative, 4);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn sentiment_trend_is_scope_isolated(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "trend-ct1@example.com").await;
        let s2 = seed_project_scope(&pool, "trend-ct2@example.com").await;
        repo.submit_anonymous(&s1, &[1u8; 32], None, "x", Some(Sentiment::Positive), FeedbackKind::Other)
            .await
            .unwrap();

        let since = chrono::Utc::now() - chrono::Duration::days(1);
        // s2 sees none of s1's sentiment.
        let buckets = repo.sentiment_trend(&s2, TrendBucket::Day, since).await.unwrap();
        assert!(buckets.is_empty(), "cross-tenant trend must not leak rows");
    }
}
