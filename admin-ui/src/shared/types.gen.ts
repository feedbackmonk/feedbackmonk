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
