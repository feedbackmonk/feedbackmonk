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

// Contract C8 — list response
export interface FeedbackListItem {
  feedback_id: string; // "FB-XXXXXX"
  kind: FeedbackKind;
  status: FeedbackStatus;
  body_excerpt: string; // first 200 chars
  submitted_at: string; // RFC 3339
  submitter_label: string; // formatted server-side; never raw email-only
  reply_count: number;
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
  body: string; // full body, unredacted (Contract C8 invariant)
  submitted_at: string;
  submitter: FeedbackSubmitter;
  external_metadata?: Record<string, unknown>;
  status_history: StatusHistoryEntry[];
  replies: ReplyEntry[];
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
