// admin-ui/src/shared/types.gen.ts
//
// Hand-rolled mirror of the backend response shapes. KEEP IN SYNC with
// `crates/feedbackmonk-api/src/handlers/admin_feedback.rs` (Stage 2 Worker A).
//
// Source of truth: docs/planning/handoffs/p1-stage1-to-stage2.md
// §TypeScript type mirror (Worker B starting kit). Verbatim copy.
//
// Stage 3 e2e includes a Vitest test asserting an admin-feedback fetch
// response parses against these types. Drift between Rust + TS surfaces
// here.

// Status workflow — Contract C6
export type FeedbackStatus =
  | "submitted"
  | "triaged"
  | "in-progress"
  | "shipped"
  | "wontfix"
  | "duplicate";

export const LEGAL_TRANSITIONS: Record<FeedbackStatus, FeedbackStatus[]> = {
  submitted: ["triaged", "wontfix", "duplicate"],
  triaged: ["in-progress", "wontfix", "duplicate", "submitted"],
  "in-progress": ["shipped", "wontfix", "duplicate", "triaged"],
  shipped: [], // terminal
  wontfix: ["submitted"], // re-open
  duplicate: ["submitted"], // un-merge
};

export type FeedbackKind = "bug" | "feature" | "question" | "other";

// Sentiment — first-class enum on feedback rows (backend serde lowercase).
// Nullable everywhere it surfaces: `null` means the backend has not yet
// classified the row (or sentiment is unavailable). Rendered icon+label
// (never color alone) by SentimentBadge, exactly like StatusBadge.
export type SentimentValue = "negative" | "neutral" | "positive";

// Contract C8 — list response
export interface FeedbackListItem {
  feedback_id: string; // "FB-XXXXXX"
  kind: FeedbackKind;
  status: FeedbackStatus;
  body_excerpt: string; // first 200 chars
  submitted_at: string; // RFC 3339
  submitter_label: string; // formatted server-side; never raw email-only
  reply_count: number;
  sentiment?: SentimentValue | null; // first-class sentiment; null = unclassified
}
export interface FeedbackListResponse {
  items: FeedbackListItem[];
  total: number;
  limit: number;
  offset: number;
}

// Contract C8 — get-with-history response
export interface StatusHistoryEntry {
  from_status: FeedbackStatus;
  to_status: FeedbackStatus;
  reason_note: string | null;
  duplicate_of_feedback_id: string | null; // "FB-XXXXXX" or null
  transitioned_by: string; // server formats UUID → email-label
  transitioned_at: string; // RFC 3339
}
export interface ReplyEntry {
  reply_id: string;
  body: string;
  visibility: "public" | "internal";
  author: string;
  created_at: string;
}
export interface FeedbackSubmitter {
  kind: "authenticated" | "anonymous";
  sub?: string;
  email?: string;
  name?: string;
}
export interface FeedbackDetail {
  feedback_id: string;
  kind: FeedbackKind;
  status: FeedbackStatus;
  body: string; // full body, unredacted (Contract C8 invariant). ALWAYS the verbatim original (Q24).
  // FR-FBR-30: the English translation of `body`, or null when untranslated
  // (pre-feature / disabled / skipped / failed / pending). The admin UI shows a
  // toggle to the translation only when this is present; `body` stays the original.
  body_translated?: string | null;
  source_lang?: string | null; // detected source language (e.g. "DE"), or null
  translation_status?:
    | "pending"
    | "translated"
    | "skipped"
    | "failed"
    | null;
  submitted_at: string;
  submitter: FeedbackSubmitter;
  external_metadata?: Record<string, unknown>;
  status_history: StatusHistoryEntry[];
  replies: ReplyEntry[];
  sentiment?: SentimentValue | null; // first-class sentiment; null = unclassified
}

// Contract C7 — transition request/response
export interface TransitionRequest {
  to_status: FeedbackStatus;
  reason_note?: string;
  duplicate_of?: string; // "FB-XXXXXX"
}
export interface TransitionResponse {
  feedback_id: string;
  from_status: FeedbackStatus;
  to_status: FeedbackStatus;
  transitioned_at: string;
  audit_id: string;
  email_queued: boolean;
}
export type TransitionErrorCode =
  | "IllegalTransition"
  | "DuplicateRequiresTarget"
  | "DuplicateTargetMissing"
  | "DuplicateSelfReference";
export interface TransitionErrorBody {
  error: TransitionErrorCode;
  from_status?: FeedbackStatus;
  to_status?: FeedbackStatus;
}

// Contract C7 — reply request/response
export interface ReplyRequest {
  body: string; // 1..16384 chars
  visibility: "public" | "internal";
}
export interface ReplyResponse {
  reply_id: string;
  feedback_id: string;
  visibility: "public" | "internal";
  created_at: string;
  email_queued: boolean;
}

// Display labels — kept here so UI never hardcodes status strings elsewhere.
export const STATUS_LABELS: Record<FeedbackStatus, string> = {
  submitted: "Submitted",
  triaged: "Triaged",
  "in-progress": "In Progress",
  shipped: "Shipped",
  wontfix: "Won't Fix",
  duplicate: "Duplicate",
};

export const KIND_LABELS: Record<FeedbackKind, string> = {
  bug: "Bug",
  feature: "Feature",
  question: "Question",
  other: "Other",
};

export const SENTIMENT_LABELS: Record<SentimentValue, string> = {
  negative: "Negative",
  neutral: "Neutral",
  positive: "Positive",
};

// Stable order for stacked-bar segments + summary tallies (most → least
// favourable, bottom-up the stack reads negative → neutral → positive).
export const SENTIMENT_ORDER: SentimentValue[] = [
  "negative",
  "neutral",
  "positive",
];

// ─────────────────────────────────────────────────────────────────────────
// Sentiment trend — admin "satisfaction over time".
// GET /api/v1/admin/feedback/sentiment-trend?bucket=week&days=90
// `buckets` is sparse: only periods with ≥1 sentiment-bearing feedback appear.
// ─────────────────────────────────────────────────────────────────────────

export type SentimentBucketGranularity = "day" | "week" | "month";

export interface SentimentTrendBucket {
  bucket_start: string; // RFC 3339 — period start
  negative: number;
  neutral: number;
  positive: number;
  total: number;
}

export interface SentimentTrendTotals {
  negative: number;
  neutral: number;
  positive: number;
  total: number;
}

export interface SentimentTrendResponse {
  bucket: SentimentBucketGranularity;
  since: string; // RFC 3339 — lower bound of the window
  buckets: SentimentTrendBucket[];
  totals: SentimentTrendTotals;
}

// ─────────────────────────────────────────────────────────────────────────
// P2 — Customer-facing roadmap surfaces
//
// Source of truth: `docs/planning/handoffs/p2-fanout-contracts.md` §TypeScript
// type mirror — frozen 2026-05-14T04:13:00Z at sha 7e1ea3a (canonical for
// C13/C14/C15 — roadmap backend; C16 — promote handler). The block below is
// the mirror verbatim + two optional widenings on `RoadmapItem`
// (`origin_feedback_id`, `voted_by_me`) per DEC-PODS-C-01.
//
// Re-apply this block when handler signatures change. NEVER remove or
// rename fields; additional optional fields are pre-authorized.
// ─────────────────────────────────────────────────────────────────────────

// --- C13: roadmap item -----------------------------------------------------

export type RoadmapItemStatus =
  | "considering"
  | "planned"
  | "in-progress"
  | "shipped"
  | "wontfix";

export const ROADMAP_STATUS_LABELS: Record<RoadmapItemStatus, string> = {
  considering: "Considering",
  planned: "Planned",
  "in-progress": "In Progress",
  shipped: "Shipped",
  wontfix: "Won't Do",
};

// Public order — what end-users see top-down on /public/.../roadmap.
export const ROADMAP_STATUS_PUBLIC_ORDER: RoadmapItemStatus[] = [
  "in-progress",
  "planned",
  "considering",
  "shipped",
  "wontfix",
];

export interface RoadmapItem {
  slug: string;
  title: string;
  body: string;
  status: RoadmapItemStatus;
  vote_count: number;
  created_at: string; // RFC 3339
  updated_at: string;
  // Pre-authorized widenings per GUIDE.md §8 — `origin_feedback_id` is
  // already in B's migration 00006 schema and only surfaces on admin
  // endpoints (server omits it on public). `voted_by_me` is a UI-ergonomic
  // surface so the vote button can render its toggled state without a
  // separate roundtrip. See DEC-PODS-C-01.
  origin_feedback_id?: string; // FB-XXXXXX of the promoted source feedback (admin-only)
  voted_by_me?: boolean; // pre-cached on the response if known
}

// --- C14: voting ------------------------------------------------------------

export type RoadmapVoterMode = "jwt" | "anon";

export interface VoteResponse {
  item_slug: string;
  voter_mode: RoadmapVoterMode;
  cast_at: string;
}

export interface VoteErrorBody {
  error:
    | "AlreadyVoted"
    | "RateLimitExceeded"
    | "VoteNotFound"
    | "RetractionWindowExpired";
  retry_after_seconds?: number; // only on RateLimitExceeded
}

export interface RetractResponse {
  item_slug: string;
  retracted_at: string;
}

// --- C15: list + admin ------------------------------------------------------

export interface RoadmapListResponse {
  items: RoadmapItem[];
  total: number;
  limit: number;
  offset: number;
  cached_at: string | null;
}

export interface TopVotedItem {
  slug: string;
  title: string;
  status: RoadmapItemStatus;
  vote_count: number;
}

export interface TopVotedResponse {
  items: TopVotedItem[];
  cached_at: string | null;
}

export interface AdminCreateRoadmapItemRequest {
  slug: string;
  title: string;
  body: string;
  status?: RoadmapItemStatus; // defaults to "considering"
}

export interface AdminPatchRoadmapItemRequest {
  title?: string;
  body?: string;
  status?: RoadmapItemStatus;
}

// Legacy alias names used in admin-ui (kept for stylistic consistency with
// `TransitionRequest` / `ReplyRequest`); identical shapes.
export type AdminRoadmapCreateRequest = AdminCreateRoadmapItemRequest;
export type AdminRoadmapPatchRequest = AdminPatchRoadmapItemRequest;

// --- C16: promote -----------------------------------------------------------

export interface PromoteRequest {
  slug: string; // 1..=80 chars; kebab-case ASCII
  title?: string; // defaults to render_roadmap_title(feedback.body)
}

export interface PromoteResponse {
  roadmap_item_id: string;
  roadmap_item_slug: string;
  source_feedback_id: string; // "FB-XXXXXX"
  source_status: "duplicate"; // always "duplicate" after a successful promote
  already_promoted: boolean;
}

export interface PromoteErrorBody {
  error:
    | "InvalidCategory"
    | "InvalidSlug"
    | "FeedbackNotFound"
    | "SlugTaken"
    | "InternalError";
  kind?: "bug" | "feature" | "question" | "other"; // on InvalidCategory
  slug?: string; // on InvalidSlug / SlugTaken
}

// ─────────────────────────────────────────────────────────────────────────
// C29 — Public feedback board read + privacy shape (Public Feedback Board +
// Moderation Gate, Stage 1). Owner of the wire shape: Worker A (backend);
// consumer: Worker D (this file's mirror) + the PublicBoard page.
//
// PRIVACY INVARIANT (load-bearing, sibling to Q24): the board item NEVER
// carries submitter identity — no `end_user_email` / `end_user_name` /
// `end_user_sub` / `anon_token_hash`, no `external_metadata`, no
// `crash_event_id`, no admin/internal reply content. The server simply never
// sends those fields, so there is nothing to anonymize client-side. Do NOT
// widen this interface with any submitter-identity field.
//
// Source of truth: docs/planning/handoffs/board-moderation-contracts.md §C29.
// Frozen at Stage 0 (GATE 0 = e8ef874). Drift here is a Stage 1 impl bug.
// ─────────────────────────────────────────────────────────────────────────

export interface BoardItem {
  short_code: string; // "FB-XXXXXX" — the public board item identifier
  body: string; // verbatim feedback body
  kind: FeedbackKind; // bug | feature | question | other
  status: FeedbackStatus; // submitted | triaged | in-progress | shipped | wontfix | duplicate
  vote_count: number; // real aggregate over feedback_board_votes (C30, D1)
  accepted_at: string; // RFC 3339 — when the item was approved onto the board
  // The *current viewer's* vote state. The server does not yet echo this on the
  // board read (reserved, wire-absent — mirrors DEC-PODS-C-01's `voted_by_me` on
  // RoadmapItem); the vote button renders "Vote" until it does. NOT a
  // submitter-identity field.
  voted_by_me?: boolean;
}

export interface BoardListResponse {
  items: BoardItem[];
  total: number;
  limit: number;
  offset: number;
}

// --- C30: board voting (PF-BOARD-VOTING-01) ---------------------------------
// The board sibling of the roadmap VoteResponse/RetractResponse, keyed on
// `short_code` (the board item id) instead of `item_slug`. Voter resolution +
// error bodies (AlreadyVoted/RateLimitExceeded/VoteNotFound/
// RetractionWindowExpired) are identical to roadmap voting — reuse VoteErrorBody.
export interface BoardVoteResponse {
  short_code: string;
  voter_mode: RoadmapVoterMode;
  cast_at: string;
}

export interface BoardRetractResponse {
  short_code: string;
  retracted_at: string;
}

// ─────────────────────────────────────────────────────────────────────────
// P3 — Tier model + cap-aware error rendering (Contracts C17/C18/C19).
//
// Source of truth: docs/planning/handoffs/p3-stage1-to-stage2.md
// (frozen verbatim at Stage 1 exit, commit d2266ae). Drift here is a Stage 2
// implementation bug; semantic shape changes require a DEC-FBR-* + Stage 1
// re-engagement.
// ─────────────────────────────────────────────────────────────────────────

export type Tier = "free" | "starter" | "pro" | "self_host";

export type ResourceKind = "project" | "feedback_in_rolling_month";

export interface TierQuotas {
  projects_per_org: number | null;        // null = unlimited
  monthly_feedback_volume: number | null; // null = unlimited
  custom_branding: boolean;
  custom_domain: boolean;
  eu_residency: boolean;
  footer_text: string | null;             // "powered by feedbackmonk" (Free) or null (paid)
}

export interface TierUsage {
  projects: number;
  feedback_monthly: number;
  period_start: string;                   // ISO-8601
}

export interface TierStatus {
  tier: Tier;
  quotas: TierQuotas;
  usage: TierUsage;
}

export interface TierCapExceededBody {
  error: "tier_cap_exceeded";
  tier: Tier;
  resource: ResourceKind;
  current: number;
  limit: number;
  upgrade_hint: string;
}

// Type-guard for narrow error handling — used by ApiClient response interceptor
// and UpgradePrompt toast. Mirrors handoff §TypeScript starter kit verbatim.
export function isTierCapExceeded(body: unknown): body is TierCapExceededBody {
  return (
    typeof body === "object" &&
    body !== null &&
    (body as { error?: unknown }).error === "tier_cap_exceeded"
  );
}

// Display labels — kept here so UI never hardcodes tier strings elsewhere.
export const TIER_LABELS: Record<Tier, string> = {
  free: "Free",
  starter: "Starter",
  pro: "Pro",
  self_host: "Self-host",
};

export const RESOURCE_LABELS: Record<ResourceKind, string> = {
  project: "projects",
  feedback_in_rolling_month: "monthly feedback",
};

// ─────────────────────────────────────────────────────────────────────────
// P5a — Agentic feedback-resolution loop (recommend-only). Contracts C22/C23.
//
// Hand-rolled mirror of the FROZEN Stage-0 enums + the C23 row shapes + the
// C22 work-order API request/response surface. Casings are pinned by the
// Stage-0 core crate and MUST match byte-for-byte:
//   - WorkOrderState  → kebab-case  (crates/feedbackmonk-core/src/work_order.rs)
//   - ActionType      → snake_case  (crates/feedbackmonk-core/src/action_type.rs)
// Cluster/recommendation/sweep READ shapes are mirrored from the C23 column
// lists (plan §Contract C23); reconcile to Worker A's (work_orders +
// recommendations) and Worker B's (clusters + sweeps) actual response structs
// when they announce in the PODS messages channel (Interface Contract 1).
//
// SECURITY: every text field below (cluster label/summary/priority_rationale,
// recommendation title/body/rationale, feedback excerpts, source_refs) is
// UNTRUSTED public input. Render it as escaped React text nodes only — NEVER
// `dangerouslySetInnerHTML` (FR-FBR-25 data-envelope; the C24 corpus exists
// because of this).
// ─────────────────────────────────────────────────────────────────────────

// --- C22: work-order state machine (mirror of WorkOrderState, kebab-case) ---

export type WorkOrderState =
  | "draft"
  | "approved"
  | "dispatched"
  | "claimed"
  | "building"
  | "verifying"
  | "reported"
  | "completed"
  | "failed"
  | "cancelled";

export const WORK_ORDER_STATE_LABELS: Record<WorkOrderState, string> = {
  draft: "Draft",
  approved: "Approved",
  dispatched: "Dispatched",
  claimed: "Claimed",
  building: "Building",
  verifying: "Verifying",
  reported: "Reported",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};

// States for which `is_execution_state` is true in the core crate — i.e. the
// agent is acting on the customer's code. Surfaced so the UI can visually
// distinguish "the agent is executing this" from "awaiting a human". Mirrors
// WorkOrderState::is_execution_state (everything from `dispatched` onward that
// isn't a terminal/awaiting-owner state). Display-only; never an authz source.
export const WORK_ORDER_EXECUTION_STATES: WorkOrderState[] = [
  "dispatched",
  "claimed",
  "building",
  "verifying",
];

export const WORK_ORDER_TERMINAL_STATES: WorkOrderState[] = [
  "completed",
  "cancelled",
];

// --- C23: ActionType (snake_case) -------------------------------------------

export type ActionType =
  | "bug_fix"
  | "feature_implementation"
  | "enhancement"
  | "investigation"
  | "no_action";

export const ACTION_TYPE_LABELS: Record<ActionType, string> = {
  bug_fix: "Bug fix",
  feature_implementation: "Feature",
  enhancement: "Enhancement",
  investigation: "Investigation",
  no_action: "No action",
};

// --- C23: cluster + recommendation + sweep enums ----------------------------

export type ClusterPriority = "high" | "medium" | "low" | "none";

export const CLUSTER_PRIORITY_LABELS: Record<ClusterPriority, string> = {
  high: "High",
  medium: "Medium",
  low: "Low",
  none: "None",
};

// Digest ordering — highest-priority clusters first.
export const CLUSTER_PRIORITY_ORDER: ClusterPriority[] = [
  "high",
  "medium",
  "low",
  "none",
];

export type ClusterStatus = "open" | "actioned" | "dismissed" | "merged";

export const CLUSTER_STATUS_LABELS: Record<ClusterStatus, string> = {
  open: "Open",
  actioned: "Actioned",
  dismissed: "Dismissed",
  merged: "Merged",
};

export type RecommendationStatus =
  | "proposed"
  | "approved"
  | "tweaked_approved"
  | "rejected"
  | "superseded";

export const RECOMMENDATION_STATUS_LABELS: Record<RecommendationStatus, string> =
  {
    proposed: "Proposed",
    approved: "Approved",
    tweaked_approved: "Approved (tweaked)",
    rejected: "Rejected",
    superseded: "Superseded",
  };

export type SweepTrigger = "schedule" | "on_demand";
export type SweepStatus = "running" | "completed" | "failed";
export type ClusterCreatedBy = "agent" | "admin";

// --- C23: row shapes (mirrored from the column lists) -----------------------

// `source_refs` is jsonb in C23: "file/line/doc REFERENCES the analyst
// inspected (grounding evidence, *never dumps*)". Modeled as a list of
// reference descriptors; rendered as citations, not content. Shape is
// best-effort until Worker B announces — kept permissive so unexpected jsonb
// never crashes the render (it falls back to a stringified citation).
export interface SourceRef {
  label?: string; // human label, e.g. "src/auth/mod.rs:42"
  ref?: string; // path / locator
  kind?: string; // "file" | "doc" | "line" | ...
  detail?: string; // short note (NOT a content dump)
}

// = Worker B's `ClusterView` (reconciled to B's announced struct, 2026-06-18
// 19:45). `summary` + `priority_rationale` are nullable per B's wire shape.
export interface ClusterSummary {
  id: string;
  label: string;
  summary: string | null;
  kind: FeedbackKind; // lowercase enum bug|feature|question|other
  priority: ClusterPriority;
  priority_rationale: string | null; // explainability — shown when present
  status: ClusterStatus;
  merged_into_id: string | null;
  member_count: number;
  last_swept_at: string | null; // RFC 3339
  created_by: ClusterCreatedBy;
  created_at: string;
  updated_at: string;
}

export interface ClusterMember {
  feedback_id: string; // "FB-XXXXXX"
  kind: FeedbackKind;
  status: FeedbackStatus;
  body_excerpt: string; // server-truncated; untrusted — render as data
  submitted_at: string;
}

// = Worker B's `RecommendationView` (reconciled 2026-06-18 19:45). `rationale`
// is nullable per B's wire shape; all keys snake_case.
export interface Recommendation {
  id: string;
  cluster_id: string;
  sweep_id: string | null;
  action_type: ActionType;
  title: string;
  body: string;
  rationale: string | null;
  source_refs: SourceRef[]; // json array of refs
  confidence: number; // 0..1
  status: RecommendationStatus;
  generated_at: string;
  created_at: string;
}

// = Worker B's `ClusterDetailView`: ClusterView fields flattened at top level
// PLUS `recommendations` (newest-first). B does NOT serve cluster `members` in
// P5a, so `members` is optional — rendered only when a backend later provides it.
export interface ClusterDetail extends ClusterSummary {
  recommendations: Recommendation[]; // newest first
  members?: ClusterMember[];
}

// = Worker B's `SweepView` (reconciled 2026-06-18 19:45). P5a sweeps return
// status "completed", agent_version "p5a-deterministic-floor",
// recommendations_emitted 0, digest_summary = deterministic tallies + quoted
// cluster labels (untrusted — render as data).
export interface AnalysisSweep {
  id: string;
  triggered_by: SweepTrigger;
  started_at: string;
  completed_at: string | null;
  status: SweepStatus;
  clusters_touched: number;
  recommendations_emitted: number;
  runner_id: string | null;
  agent_version: string | null;
  digest_summary: string | null;
}

// Worker B merge/split admin responses (snake_case). The autopilot review
// surface does not trigger merge/split itself in P5a, but the response shapes
// are mirrored here so any future control reconciles against B's wire shape.
export interface ClusterMergeResponse {
  merged_cluster_id: string;
  survivor_id: string;
  members_moved: number;
}

export interface ClusterSplitResponse {
  source_cluster_id: string;
  new_cluster_id: string;
  members_moved: number;
}

// --- C22: work-order row + event ledger shapes ------------------------------

export type WorkOrderActor = "admin" | "runner" | "system";

export interface WorkOrderEvent {
  id: string;
  from_state: WorkOrderState | null; // null on the creating event
  to_state: WorkOrderState;
  event_type: string; // approve | cancel | dispatch | claim | … (C22 authz table)
  actor: WorkOrderActor;
  actor_id: string | null; // server formats to a label where it can
  detail: Record<string, unknown> | null; // jsonb — overrides delta, reason, …
  at: string; // RFC 3339
}

// = Worker A's `WorkOrderView` (Interface Contract 1, reconciled to A's compiled
// structs 2026-06-18). A's list endpoint returns the FULL view per row (not a
// trimmed list item), and the detail response is this view flattened + `events`.
export interface WorkOrder {
  id: string;
  recommendation_id: string;
  cluster_id: string;
  action_type: ActionType;
  title: string;
  instructions: string;
  owner_overrides: Record<string, unknown> | null;
  autonomy_rung: number; // 0..3
  state: WorkOrderState;
  approved_by: string | null;
  approved_at: string | null;
  dispatched_at: string | null;
  claimed_by_runner: string | null;
  result_ref: Record<string, unknown> | null;
  failure_reason: string | null;
  created_at: string;
  updated_at: string;
}

// = A's `WorkOrderDetailResponse` (WorkOrderView flattened + the event ledger).
export interface WorkOrderDetail extends WorkOrder {
  events: WorkOrderEvent[]; // append-only ledger, chronological
}

// --- Paginated list envelopes (mirror the existing C8 list shape) -----------

// Pagination fields are optional: Worker A's work-order list returns just
// `{ items }` (Interface Contract 1); B's cluster/sweep envelopes may or may not
// page. `items` is the only field every consumer relies on.
export interface ClusterListResponse {
  items: ClusterSummary[];
  total?: number;
  limit?: number;
  offset?: number;
}

export interface RecommendationListResponse {
  items: Recommendation[];
  total?: number;
  limit?: number;
  offset?: number;
}

export interface WorkOrderListResponse {
  items: WorkOrder[];
  total?: number;
  limit?: number;
  offset?: number;
}

export interface SweepListResponse {
  items: AnalysisSweep[];
  total?: number;
  limit?: number;
  offset?: number;
}

// = Worker A's `TransitionResponse` — returned by /approve and /transition (NOT
// the full work order). `auto_accepted` is present only when a rung≥2 `reported`
// order auto-accepts. Named distinctly from the feedback-status `TransitionResponse`
// (C7) above to avoid a collision.
export interface WorkOrderTransitionResponse {
  work_order_id: string;
  from_state: WorkOrderState;
  to_state: WorkOrderState;
  event_type: string;
  audit_id: string;
  auto_accepted?: boolean;
}

// --- C22: request bodies ----------------------------------------------------

// Q17 — "tweak before approve" = AUTHORITATIVE overrides (not appended
// instructions). The owner's edit is the trust signal; at dispatch these win
// over the recommendation's fields. Captured as a typed delta over the two
// editable fields (title + instructions); extra keys allowed for forward-compat.
export interface OwnerOverrides {
  title?: string;
  instructions?: string;
  [key: string]: unknown;
}

// Autonomy rung — how far the agent walks before it needs a signature.
// Sent at work-order CREATE (C22). Rung 0 never produces a work order, so the
// create dial offers 1..3 only; 0 is surfaced for explanation, not selection.
export type AutonomyRung = 0 | 1 | 2 | 3;

export const AUTONOMY_RUNG_LABELS: Record<AutonomyRung, string> = {
  0: "Rung 0 — Organize",
  1: "Rung 1 — Draft",
  2: "Rung 2 — Auto-execute low-stakes",
  3: "Rung 3 — Act & report",
};

// What each rung AUTHORIZES — surfaced in the dial so the owner sees the
// blast radius of the rung they pick (this is a security control, not UX sugar).
export const AUTONOMY_RUNG_DESCRIPTIONS: Record<AutonomyRung, string> = {
  0: "Cluster & prioritize only. No work order is ever created. (Not selectable here — Rung 0 means no order.)",
  1: "The agent drafts a work order for your review. Nothing runs until you approve. You sign every order.",
  2: "Low-stakes actions auto-execute after approval; anything riskier is escalated back to you for a signature.",
  3: "The agent acts on approved orders and reports back. You review results, not every step. Highest autonomy.",
};

// Owner-authored transition event types (the `/transition` endpoint body).
// Excludes `approve` (its own `/approve` endpoint) and all runner/system
// events (claim/dispatch/building/…). Mirrors the C22 authz table's
// owner-only rows.
export type WorkOrderOwnerEventType =
  | "cancel"
  | "accept"
  | "request-changes"
  | "reject"
  | "retry";

export const WORK_ORDER_EVENT_LABELS: Record<string, string> = {
  approve: "Approved",
  cancel: "Cancelled",
  dispatch: "Dispatched",
  claim: "Claimed",
  building: "Building",
  verifying: "Verifying",
  reported: "Reported",
  accept: "Accepted",
  "request-changes": "Requested changes",
  reject: "Rejected",
  retry: "Retried",
};

export interface CreateWorkOrderRequest {
  recommendation_id: string;
  autonomy_rung: AutonomyRung;
  owner_overrides?: OwnerOverrides;
}

export interface ApproveWorkOrderRequest {
  owner_overrides?: OwnerOverrides;
}

export interface WorkOrderTransitionRequest {
  event_type: WorkOrderOwnerEventType;
  detail?: Record<string, unknown>;
}

// The owner-facing transitions available from a given state, derived from the
// C22 legal-transition table + authz matrix (owner-authored rows only).
// `approve` is excluded here — it routes through the dedicated `/approve`
// security gate, surfaced separately in the UI as a deliberate confirm.
export const WORK_ORDER_OWNER_TRANSITIONS: Record<
  WorkOrderState,
  WorkOrderOwnerEventType[]
> = {
  draft: ["cancel"], // approve handled via the /approve gate, not here
  approved: ["cancel"], // dispatch is system-authored
  dispatched: ["cancel"],
  claimed: ["cancel"],
  building: ["cancel"],
  verifying: ["cancel"],
  reported: ["accept", "request-changes", "reject"],
  completed: [], // terminal
  failed: ["retry", "cancel"],
  cancelled: [], // terminal
};

// ─────────────────────────────────────────────────────────────────────────
// P5b — Runner-token lifecycle admin surface + key-class registration
// (Contract C25, FR-FBR-24).
//
// Hand-rolled mirror — NO ts-rs/typeshare in this repo. KEEP IN SYNC with the
// Rust source of truth (field names + types verbatim):
//   - Runner-token shapes  → crates/feedbackmonk-api/src/handlers/runner_tokens.rs
//                            (RegisterRunnerTokenRequest, RunnerTokenView,
//                             RunnerTokenListResponse)
//   - key_class + key register → crates/feedbackmonk-api/src/handlers/signing_keys.rs
//                            (RegisterKeyRequest.key_class, RegisterKeyResponse)
//   - KeyClass enum (lowercase serde) → crates/feedbackmonk-core/src/models.rs
//
// All `/runner-tokens` + `/signing-keys` endpoints are AdminSession-only and
// merged WITHOUT `.layer(cors)` — same admin-session calls as TierSettings.
//
// SECURITY note to surface in UI copy: a runner token authorizes ONLY runner
// transitions and can NEVER author `approved` (C22 inv. 2). Even full token
// compromise cannot bypass the owner-approval gate — which is why issuance is
// safe to automate. Drift between Rust + TS surfaces in the Stage 3 e2e parse
// test.
// ─────────────────────────────────────────────────────────────────────────

// Privilege class of a registered signing key (mirror of KeyClass, lowercase).
// `identity` (default) verifies end-user submission JWTs; `runner` verifies the
// runner's `scope:"runner:write"` tokens. Registering a `runner`-class key is
// how an owner enables runner-token minting.
export type KeyClass = "identity" | "runner";

export const KEY_CLASS_LABELS: Record<KeyClass, string> = {
  identity: "Identity (end-user submissions)",
  runner: "Runner (autonomous agent)",
};

// --- C25: signing-key registration (key_class field, P5b) -------------------

// POST /api/v1/projects/:project_id/signing-keys request. The contract-of-record
// field name is `public_key_base64` (the handler also accepts the `public_key_b64`
// alias, but the spec name is what we send). `key_class` omitted ⇒ "identity".
export interface RegisterKeyRequest {
  public_key_base64: string; // standard base64 of the 32-byte raw Ed25519 public key
  label: string; // 1..=100 chars after trim
  key_class?: KeyClass; // omit ⇒ "identity"
}

export interface RegisterKeyResponse {
  key_id: string; // UUID
  label: string;
  registered_at: string; // RFC 3339
}

// --- C25: runner-token lifecycle --------------------------------------------

// POST /api/v1/projects/:project_id/runner-tokens — register an issued token
// for visibility (optional bookkeeping; idempotent upsert on (project, jti)).
export interface RegisterRunnerTokenRequest {
  jti: string; // the token's jti claim (client-minted UUID); 1..=200 chars
  label: string; // human label, e.g. "ci-runner"; 1..=100 chars
  expires_at?: string | null; // the token's exp as RFC3339 (visibility only)
}

// One runner-token row: registry fields + the joined revocation state.
export interface RunnerTokenView {
  jti: string;
  label: string;
  expires_at: string | null; // RFC 3339 or null
  created_at: string; // RFC 3339
  revoked_at: string | null; // Some ⇒ revoked (dead regardless of exp)
}

export interface RunnerTokenListResponse {
  items: RunnerTokenView[];
}
