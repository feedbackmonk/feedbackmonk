import axios, { AxiosError, type AxiosInstance } from "axios";
import type {
  AdminRoadmapCreateRequest,
  AdminRoadmapPatchRequest,
  FeedbackDetail,
  FeedbackListResponse,
  FeedbackStatus,
  PromoteRequest,
  PromoteResponse,
  RetractResponse,
  ReplyRequest,
  ReplyResponse,
  RoadmapItem,
  RoadmapListResponse,
  TierCapExceededBody,
  TierStatus,
  TopVotedResponse,
  TransitionRequest,
  TransitionResponse,
  VoteResponse,
} from "./types.gen";
import { isTierCapExceeded } from "./types.gen";

const api: AxiosInstance = axios.create({
  baseURL: "/api/v1",
  withCredentials: true,
  headers: { "Content-Type": "application/json" },
});

api.interceptors.response.use(
  (r) => r,
  (err: AxiosError) => {
    if (
      err.response?.status === 401 &&
      typeof location !== "undefined" &&
      location.pathname !== "/login"
    ) {
      const next = encodeURIComponent(location.pathname + location.search);
      location.replace(`/login?next=${next}`);
    }
    // Parse Contract C18 TierCapExceeded once and tag the error so any
    // mutation `onError` callback can surface the UpgradePrompt toast
    // without re-parsing. Status mapping (Contract C18): 409 for `project`,
    // 402 for `feedback_in_rolling_month`. Tag without short-circuiting —
    // callers still receive the AxiosError for normal rejection plumbing.
    const status = err.response?.status;
    if (status === 402 || status === 409) {
      const body = err.response?.data;
      if (isTierCapExceeded(body)) {
        (err as AxiosError & { tierCapExceeded?: TierCapExceededBody }).tierCapExceeded =
          body;
      }
    }
    return Promise.reject(err);
  },
);

/**
 * Extract a Contract C18 `TierCapExceededBody` from a thrown error, or
 * `null` if the error is not a tier-cap rejection. Designed to be called
 * from a mutation `onError(err)` callback:
 *
 * ```ts
 * onError: (err) => {
 *   const body = extractTierCapExceeded(err);
 *   if (body) notify(body.upgrade_hint, "error");
 * }
 * ```
 */
export function extractTierCapExceeded(err: unknown): TierCapExceededBody | null {
  if (
    err &&
    typeof err === "object" &&
    "tierCapExceeded" in err &&
    isTierCapExceeded((err as { tierCapExceeded: unknown }).tierCapExceeded)
  ) {
    return (err as { tierCapExceeded: TierCapExceededBody }).tierCapExceeded;
  }
  // Belt-and-braces: parse on the fly if the interceptor didn't tag (e.g.,
  // a non-axios error wrapping the body, or a fixture that bypassed the
  // interceptor).
  if (axios.isAxiosError(err) && isTierCapExceeded(err.response?.data)) {
    return err.response.data as TierCapExceededBody;
  }
  return null;
}

export interface ListParams {
  status?: FeedbackStatus;
  limit?: number;
  offset?: number;
}

export async function fetchFeedbackList(
  params: ListParams = {},
): Promise<FeedbackListResponse> {
  const r = await api.get<FeedbackListResponse>("/admin/feedback", {
    params: {
      status: params.status,
      limit: params.limit ?? 20,
      offset: params.offset ?? 0,
    },
  });
  return r.data;
}

export interface SearchParams {
  q: string;
  limit?: number;
  offset?: number;
}

// Gap #3 — admin full-text search. Shares the Contract C8 list response
// shape, so the feedback table renders search hits with the same rows.
export async function searchFeedback(
  params: SearchParams,
): Promise<FeedbackListResponse> {
  const r = await api.get<FeedbackListResponse>("/admin/feedback/search", {
    params: {
      q: params.q,
      limit: params.limit ?? 20,
      offset: params.offset ?? 0,
    },
  });
  return r.data;
}

export async function fetchFeedbackDetail(
  feedbackId: string,
): Promise<FeedbackDetail> {
  const r = await api.get<FeedbackDetail>(
    `/admin/feedback/${encodeURIComponent(feedbackId)}`,
  );
  return r.data;
}

export async function postTransition(
  feedbackId: string,
  body: TransitionRequest,
): Promise<TransitionResponse> {
  const r = await api.post<TransitionResponse>(
    `/admin/feedback/${encodeURIComponent(feedbackId)}/transition`,
    body,
  );
  return r.data;
}

export async function postReply(
  feedbackId: string,
  body: ReplyRequest,
): Promise<ReplyResponse> {
  const r = await api.post<ReplyResponse>(
    `/admin/feedback/${encodeURIComponent(feedbackId)}/reply`,
    body,
  );
  return r.data;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export async function postLogin(body: LoginRequest): Promise<void> {
  await api.post("/auth/login", body);
}

// Admin's `GET /api/v1/projects` — used to resolve sole-project-id for
// admin URLs that don't carry one in the path (e.g. /admin/roadmap). The
// public roadmap page is project-segmented (`/public/projects/:projectId`)
// so it never calls this.
export interface AdminProjectListItem {
  project_id: string;
  name: string;
  slug: string;
  created_at: string;
}
export interface AdminProjectListResponse {
  projects: AdminProjectListItem[];
}
export async function fetchAdminProjects(): Promise<AdminProjectListResponse> {
  const r = await api.get<AdminProjectListResponse>("/projects");
  return r.data;
}

// ─────────────────────────────────────────────────────────────────────────
// P2 — Roadmap endpoints (Contracts C15 + C16). Wired against Worker B's
// public + admin roadmap routers and Worker C's promote handler. Paths
// match the plan §Contract C15/C16 specs verbatim.
// ─────────────────────────────────────────────────────────────────────────

export interface RoadmapListParams {
  status?: string;
  limit?: number;
  offset?: number;
}

export async function fetchPublicRoadmap(
  projectId: string,
  params: RoadmapListParams = {},
): Promise<RoadmapListResponse> {
  const r = await api.get<RoadmapListResponse>(
    `/projects/${encodeURIComponent(projectId)}/roadmap`,
    {
      params: {
        status: params.status,
        limit: params.limit ?? 50,
        offset: params.offset ?? 0,
      },
    },
  );
  return r.data;
}

export async function fetchPublicTopVoted(
  projectId: string,
  limit = 10,
): Promise<TopVotedResponse> {
  const r = await api.get<TopVotedResponse>(
    `/projects/${encodeURIComponent(projectId)}/roadmap/top-voted`,
    { params: { limit } },
  );
  return r.data;
}

export async function fetchAdminRoadmap(
  projectId: string,
  params: RoadmapListParams = {},
): Promise<RoadmapListResponse> {
  const r = await api.get<RoadmapListResponse>(
    `/admin/projects/${encodeURIComponent(projectId)}/roadmap`,
    {
      params: {
        status: params.status,
        limit: params.limit ?? 100,
        offset: params.offset ?? 0,
      },
    },
  );
  return r.data;
}

export async function postCastVote(
  projectId: string,
  slug: string,
): Promise<VoteResponse> {
  const r = await api.post<VoteResponse>(
    `/projects/${encodeURIComponent(projectId)}/roadmap/items/${encodeURIComponent(slug)}/vote`,
  );
  return r.data;
}

export async function deleteVote(
  projectId: string,
  slug: string,
): Promise<RetractResponse> {
  const r = await api.delete<RetractResponse>(
    `/projects/${encodeURIComponent(projectId)}/roadmap/items/${encodeURIComponent(slug)}/vote`,
  );
  return r.data;
}

export async function postCreateRoadmapItem(
  projectId: string,
  body: AdminRoadmapCreateRequest,
): Promise<RoadmapItem> {
  const r = await api.post<RoadmapItem>(
    `/admin/projects/${encodeURIComponent(projectId)}/roadmap/items`,
    body,
  );
  return r.data;
}

export async function patchRoadmapItem(
  projectId: string,
  slug: string,
  body: AdminRoadmapPatchRequest,
): Promise<RoadmapItem> {
  const r = await api.patch<RoadmapItem>(
    `/admin/projects/${encodeURIComponent(projectId)}/roadmap/items/${encodeURIComponent(slug)}`,
    body,
  );
  return r.data;
}

export async function postPromoteFeedback(
  feedbackId: string,
  body: PromoteRequest,
): Promise<PromoteResponse> {
  const r = await api.post<PromoteResponse>(
    `/admin/feedback/${encodeURIComponent(feedbackId)}/promote`,
    body,
  );
  return r.data;
}

// ─────────────────────────────────────────────────────────────────────────
// P3 — Tier status (Contract C17). Read-only; consumed by TierSettings page.
// ─────────────────────────────────────────────────────────────────────────

export async function fetchTierStatus(): Promise<TierStatus> {
  const r = await api.get<TierStatus>("/admin/tier");
  return r.data;
}

export { api };
