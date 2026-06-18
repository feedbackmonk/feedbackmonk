import axios, { AxiosError, type AxiosInstance } from "axios";
import type {
  AdminRoadmapCreateRequest,
  AdminRoadmapPatchRequest,
  AnalysisSweep,
  ApproveWorkOrderRequest,
  ClusterDetail,
  ClusterListResponse,
  ClusterPriority,
  ClusterStatus,
  CreateWorkOrderRequest,
  FeedbackDetail,
  FeedbackListResponse,
  FeedbackStatus,
  PromoteRequest,
  PromoteResponse,
  Recommendation,
  RecommendationListResponse,
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
  WorkOrder,
  WorkOrderDetail,
  WorkOrderListResponse,
  WorkOrderState,
  WorkOrderTransitionRequest,
  WorkOrderTransitionResponse,
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
  // Route is flat under /api/v1 (POST /api/v1/login) — matches the API's
  // LoginGate (DEC-FBR-IMPL-10) and the signup/verify-email convention. The
  // earlier "/auth/login" 404'd in-browser (the catch-all "Login failed");
  // curl-only verification against /login masked it.
  await api.post("/login", body);
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

// ─────────────────────────────────────────────────────────────────────────
// P5a — Agentic loop: autopilot review & approval surface (FR-FBR-21).
//
// Two seams converge here:
//   • Work-order endpoints (create / approve / transition / list / detail) are
//     Worker A's and are FROZEN by Contract C22 — paths + request bodies are
//     pinned by the C22 HTTP-surface table; only the RESPONSE struct fields
//     reconcile to A's compiled structs (Interface Contract 1).
//   • Cluster / recommendation / sweep READS are Worker B's admin routers.
//     Their paths are NOT enumerated in any frozen contract (MSG-001 to LEAD).
//     PROCEED-ON-ASSUMPTION, flagged UNACKED: mirror the C22 project-scoped,
//     AdminSession-guarded, no-CORS convention. ALL path strings live in the
//     `P5A_PATHS` map below so reconciling to B's actual paths is a one-line
//     change per endpoint, never a hunt through call sites.
// ─────────────────────────────────────────────────────────────────────────

const P5A_PATHS = {
  // Worker B — read seam (assumed; reconcile on B's announce, MSG-001).
  clusters: (pid: string) => `/projects/${encodeURIComponent(pid)}/clusters`,
  cluster: (pid: string, cid: string) =>
    `/projects/${encodeURIComponent(pid)}/clusters/${encodeURIComponent(cid)}`,
  clusterRecommendations: (pid: string, cid: string) =>
    `/projects/${encodeURIComponent(pid)}/clusters/${encodeURIComponent(cid)}/recommendations`,
  sweeps: (pid: string) => `/projects/${encodeURIComponent(pid)}/sweeps`,
  sweepLatest: (pid: string) =>
    `/projects/${encodeURIComponent(pid)}/sweeps/latest`,
  // Worker B confirmed (2026-06-18 19:45): project-scoped, rec_id is unique so
  // no cluster_id segment. Sets proposed → rejected, no work order.
  recommendationReject: (pid: string, rid: string) =>
    `/projects/${encodeURIComponent(pid)}/recommendations/${encodeURIComponent(rid)}/reject`,
  // Worker A — Contract C22 (paths FROZEN).
  workOrders: (pid: string) =>
    `/projects/${encodeURIComponent(pid)}/work-orders`,
  workOrder: (pid: string, wid: string) =>
    `/projects/${encodeURIComponent(pid)}/work-orders/${encodeURIComponent(wid)}`,
  workOrderApprove: (pid: string, wid: string) =>
    `/projects/${encodeURIComponent(pid)}/work-orders/${encodeURIComponent(wid)}/approve`,
  workOrderTransition: (pid: string, wid: string) =>
    `/projects/${encodeURIComponent(pid)}/work-orders/${encodeURIComponent(wid)}/transition`,
} as const;

// --- Cluster / sweep reads (Worker B seam) ---------------------------------

export interface ClusterListParams {
  status?: ClusterStatus;
  priority?: ClusterPriority;
  limit?: number;
  offset?: number;
}

export async function fetchClusters(
  projectId: string,
  params: ClusterListParams = {},
): Promise<ClusterListResponse> {
  const r = await api.get<ClusterListResponse>(P5A_PATHS.clusters(projectId), {
    params: {
      status: params.status,
      priority: params.priority,
      limit: params.limit ?? 100,
      offset: params.offset ?? 0,
    },
  });
  return r.data;
}

export async function fetchClusterDetail(
  projectId: string,
  clusterId: string,
): Promise<ClusterDetail> {
  const r = await api.get<ClusterDetail>(
    P5A_PATHS.cluster(projectId, clusterId),
  );
  return r.data;
}

export async function fetchClusterRecommendations(
  projectId: string,
  clusterId: string,
): Promise<RecommendationListResponse> {
  const r = await api.get<RecommendationListResponse>(
    P5A_PATHS.clusterRecommendations(projectId, clusterId),
  );
  return r.data;
}

// Reject a recommendation outright (no work order created) — sets the
// recommendation's status to `rejected`. This is a RECOMMENDATION-status write,
// which is Worker B's `recommendations.rs` surface (not a C22 work-order
// transition; the C22 `reject` transition is `reported → failed` on an existing
// work order, a different thing). Path confirmed by Worker B (MSG 19:45);
// `reason` is accepted-but-ignored in P5a (no rejection-reason column yet).
export async function rejectRecommendation(
  projectId: string,
  recommendationId: string,
  reason?: string,
): Promise<Recommendation> {
  const r = await api.post<Recommendation>(
    P5A_PATHS.recommendationReject(projectId, recommendationId),
    reason ? { reason } : {},
  );
  return r.data;
}

export async function fetchLatestSweep(
  projectId: string,
): Promise<AnalysisSweep | null> {
  // `/sweeps/latest` may legitimately 404 before the first sweep runs — treat
  // that as "no digest yet" rather than an error surface.
  try {
    const r = await api.get<AnalysisSweep>(P5A_PATHS.sweepLatest(projectId));
    return r.data;
  } catch (err) {
    if (axios.isAxiosError(err) && err.response?.status === 404) return null;
    throw err;
  }
}

// --- Work-order endpoints (Worker A seam — Contract C22) --------------------

export interface WorkOrderListParams {
  state?: WorkOrderState;
  cluster_id?: string;
  limit?: number;
  offset?: number;
}

export async function fetchWorkOrders(
  projectId: string,
  params: WorkOrderListParams = {},
): Promise<WorkOrderListResponse> {
  const r = await api.get<WorkOrderListResponse>(
    P5A_PATHS.workOrders(projectId),
    {
      params: {
        state: params.state,
        cluster_id: params.cluster_id,
        limit: params.limit ?? 50,
        offset: params.offset ?? 0,
      },
    },
  );
  return r.data;
}

export async function fetchWorkOrderDetail(
  projectId: string,
  workOrderId: string,
): Promise<WorkOrderDetail> {
  const r = await api.get<WorkOrderDetail>(
    P5A_PATHS.workOrder(projectId, workOrderId),
  );
  return r.data;
}

// Create returns A's `WorkOrderView` (the created draft) — we use its `id` to
// chain the approval gate.
export async function createWorkOrder(
  projectId: string,
  body: CreateWorkOrderRequest,
): Promise<WorkOrder> {
  const r = await api.post<WorkOrder>(P5A_PATHS.workOrders(projectId), body);
  return r.data;
}

// The owner-approval gate — the security boundary (FR-FBR-25a). Distinct
// endpoint from the generic transition so the deliberate "I approve this"
// action is never folded into a convenience path. Returns A's TransitionResponse.
export async function approveWorkOrder(
  projectId: string,
  workOrderId: string,
  body: ApproveWorkOrderRequest = {},
): Promise<WorkOrderTransitionResponse> {
  const r = await api.post<WorkOrderTransitionResponse>(
    P5A_PATHS.workOrderApprove(projectId, workOrderId),
    body,
  );
  return r.data;
}

export async function transitionWorkOrder(
  projectId: string,
  workOrderId: string,
  body: WorkOrderTransitionRequest,
): Promise<WorkOrderTransitionResponse> {
  const r = await api.post<WorkOrderTransitionResponse>(
    P5A_PATHS.workOrderTransition(projectId, workOrderId),
    body,
  );
  return r.data;
}

export { api };
