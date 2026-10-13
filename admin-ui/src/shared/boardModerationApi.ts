// admin-ui/src/shared/boardModerationApi.ts
//
// The admin-UI client + type mirror for the moderation gate (Contract C28) and
// per-project board settings.
//
// WHY A SEPARATE FILE (not shared/ApiClient.ts + types.gen.ts): the *admin*
// moderation surface is kept in its own file, distinct from the *public* board
// client (C29) in ApiClient.ts/types.gen.ts. It reuses the shared axios `api`
// instance read-only, so the 401 interceptor + baseURL + credentials all still
// apply.
//
// SEAM ISOLATION (mirrors the codebase `P5A_PATHS` convention): every path and
// response shape is pinned in one place, against the C28 surface:
//   • `POST .../moderate` request+response — FROZEN by Contract C28, built verbatim.
//   • `GET .../moderation-queue` row — `{feedback_id, kind, moderation_status,
//     body_excerpt, submitted_at, submitter_label}` (no reply_count).
//   • board-settings — `/admin/projects/{id}/board-settings` GET + partial PATCH
//     → `{public_board_enabled, board_requires_moderation, board_access}`.
//   • Contract C44 (end-user prioritization): the owner priority view and the
//     per-item board note — docs/planning/handoffs/prioritization-contracts.md.
// `MODERATION_PATHS` / the interfaces below remain the single reconcile point.

import { api } from "./ApiClient";
import type { BoardAccess, FeedbackKind, FeedbackStatus } from "./types.gen";

// ─────────────────────────────────────────────────────────────────────────
// Moderation state machine — mirror of `feedbackmonk-core::moderation`
// (FROZEN Stage 0). DB/JSON form is lowercase, byte-for-byte with migration
// 00016's CHECK set + `ModerationStatus::as_db_str`.
// ─────────────────────────────────────────────────────────────────────────

export type ModerationStatus = "pending" | "approved" | "rejected";

export const MODERATION_STATUS_LABELS: Record<ModerationStatus, string> = {
  pending: "Pending",
  approved: "Approved",
  rejected: "Rejected",
};

// Mirror of `legal_moderation_transitions_from`. The UI offers ONLY these
// targets per current state — illegal transitions are never reachable from the
// queue; the backend 409 (C28) is belt-and-braces. A no-op (from==to) is NOT a
// legal target (matches the core's "no self-transition" invariant).
export const LEGAL_MODERATION_TRANSITIONS: Record<
  ModerationStatus,
  ModerationStatus[]
> = {
  pending: ["approved", "rejected"],
  approved: ["rejected", "pending"], // pull from board (decline / undecided)
  rejected: ["approved", "pending"], // reconsider / re-open
};

// ─────────────────────────────────────────────────────────────────────────
// Contract C28 — moderation queue read --------------------------------------
// ─────────────────────────────────────────────────────────────────────────

// The queue row is exactly these fields — NO `reply_count` (not meaningful for
// moderation), NO triage `status`. This is an admin-only surface, so
// `submitter_label` (server-
// formatted, never raw email-only) is fine to show — the PII invariant (C29)
// governs the *public* board, not the owner's queue.
export interface ModerationQueueItem {
  feedback_id: string; // "FB-XXXXXX"
  kind: FeedbackKind;
  moderation_status: ModerationStatus;
  body_excerpt: string;
  submitted_at: string; // RFC 3339
  submitter_label: string; // server-formatted; never raw email-only
}

export interface ModerationQueueResponse {
  items: ModerationQueueItem[];
  total: number;
  limit: number;
  offset: number;
}

// ─────────────────────────────────────────────────────────────────────────
// Contract C28 — `POST .../moderate` (FROZEN: request + response + 409) -------
// ─────────────────────────────────────────────────────────────────────────

export interface ModerateRequest {
  to_status: ModerationStatus;
  reason_note?: string;
}

export interface ModerateResponse {
  feedback_id: string; // "FB-XXXXXX"
  from_status: ModerationStatus;
  to_status: ModerationStatus;
  moderated_at: string; // RFC 3339
  audit_id: string; // UUID
}

// 409 body (C28) — mirrors C7's transition-error shape.
export interface ModerationErrorBody {
  error: "IllegalModerationTransition";
  from_status: ModerationStatus;
  to_status: ModerationStatus;
}

export function isModerationErrorBody(b: unknown): b is ModerationErrorBody {
  return (
    !!b &&
    typeof b === "object" &&
    (b as { error?: unknown }).error === "IllegalModerationTransition"
  );
}

// ─────────────────────────────────────────────────────────────────────────
// Board settings — the two `projects` columns from migration 00016.
// Partial PATCH so either toggle moves independently.
// ─────────────────────────────────────────────────────────────────────────

export interface BoardSettings {
  public_board_enabled: boolean;
  board_requires_moderation: boolean;
  // FR-FBR-43 (migration 00033): who may see an enabled board. Owner-only.
  board_access: BoardAccess;
}

export type BoardSettingsPatch = Partial<BoardSettings>;

// ─────────────────────────────────────────────────────────────────────────
// Path map — the single reconcile point. One-line change each if a route moves.
// ─────────────────────────────────────────────────────────────────────────

const MODERATION_PATHS = {
  // C28 — AdminSession, tenant-scoped (DEC-FBR-03). CONFIRMED by A (final C28 surface).
  moderationQueue: () => `/admin/feedback/moderation-queue`,
  moderate: (feedbackId: string) =>
    `/admin/feedback/${encodeURIComponent(feedbackId)}/moderate`,
  // CONFIRMED by A (answers Q-C2): admin board-settings GET+PATCH, AdminSession, no CORS.
  boardSettings: (projectId: string) =>
    `/admin/projects/${encodeURIComponent(projectId)}/board-settings`,
  // C44 — owner priority view + board note (AdminSession, no CORS).
  priority: (projectId: string) =>
    `/admin/projects/${encodeURIComponent(projectId)}/priority`,
  boardNote: (feedbackId: string) =>
    `/admin/feedback/${encodeURIComponent(feedbackId)}/board-note`,
} as const;

// ─────────────────────────────────────────────────────────────────────────
// Client functions
// ─────────────────────────────────────────────────────────────────────────

export interface ModerationQueueParams {
  status?: ModerationStatus; // defaults to "pending" (the queue) server-side
  limit?: number;
  offset?: number;
}

export async function fetchModerationQueue(
  params: ModerationQueueParams = {},
): Promise<ModerationQueueResponse> {
  const r = await api.get<ModerationQueueResponse>(
    MODERATION_PATHS.moderationQueue(),
    {
      params: {
        status: params.status ?? "pending",
        limit: params.limit ?? 20,
        offset: params.offset ?? 0,
      },
    },
  );
  return r.data;
}

export async function postModerate(
  feedbackId: string,
  body: ModerateRequest,
): Promise<ModerateResponse> {
  const r = await api.post<ModerateResponse>(
    MODERATION_PATHS.moderate(feedbackId),
    body,
  );
  return r.data;
}

export async function fetchBoardSettings(
  projectId: string,
): Promise<BoardSettings> {
  const r = await api.get<BoardSettings>(
    MODERATION_PATHS.boardSettings(projectId),
  );
  return r.data;
}

export async function patchBoardSettings(
  projectId: string,
  body: BoardSettingsPatch,
): Promise<BoardSettings> {
  const r = await api.patch<BoardSettings>(
    MODERATION_PATHS.boardSettings(projectId),
    body,
  );
  return r.data;
}

// ─────────────────────────────────────────────────────────────────────────
// Contract C44 — owner priority view (FR-FBR-55/56). Votes, not-needed and the
// fitted priority are SEPARATE columns; nothing here combines them, and nothing
// names a user or a user's list.
// ─────────────────────────────────────────────────────────────────────────

export interface PriorityRankedItem {
  short_code: string;
  body_excerpt: string;
  status: FeedbackStatus;
  rank: number; // 1-based position in the fitted order
  rankers: number; // users who placed it
  uncertainty: number; // standard error of the fitted strength; the UI bands it
  vote_count: number;
  not_needed_count: number; // owner-only (FR-FBR-49)
}

export interface PriorityUnrankedItem {
  short_code: string;
  body_excerpt: string;
  status: FeedbackStatus;
  rankers: number;
  vote_count: number;
  not_needed_count: number;
}

export interface PriorityView {
  computed_at: string;
  contributing_rankers: number;
  ranked: PriorityRankedItem[];
  unranked: PriorityUnrankedItem[];
}

export async function fetchPriorityView(projectId: string): Promise<PriorityView> {
  const r = await api.get<PriorityView>(MODERATION_PATHS.priority(projectId));
  return r.data;
}

// ─────────────────────────────────────────────────────────────────────────
// Contract C44 — board note (FR-FBR-58): one plain-text note per board item,
// written for every board viewer. 404 when the item is not on the board.
// ─────────────────────────────────────────────────────────────────────────

export interface OwnerBoardNote {
  body: string;
  updated_at: string;
}

export interface BoardNoteResponse {
  short_code: string;
  note: OwnerBoardNote | null;
}

export async function fetchBoardNote(feedbackId: string): Promise<BoardNoteResponse> {
  const r = await api.get<BoardNoteResponse>(MODERATION_PATHS.boardNote(feedbackId));
  return r.data;
}

export async function putBoardNote(
  feedbackId: string,
  body: string,
): Promise<BoardNoteResponse> {
  const r = await api.put<BoardNoteResponse>(MODERATION_PATHS.boardNote(feedbackId), {
    body,
  });
  return r.data;
}

export async function deleteBoardNote(feedbackId: string): Promise<BoardNoteResponse> {
  const r = await api.delete<BoardNoteResponse>(MODERATION_PATHS.boardNote(feedbackId));
  return r.data;
}
