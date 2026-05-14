import axios, { AxiosError, type AxiosInstance } from "axios";
import type {
  FeedbackDetail,
  FeedbackListResponse,
  FeedbackStatus,
  ReplyRequest,
  ReplyResponse,
  TransitionRequest,
  TransitionResponse,
} from "./types.gen";

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
    return Promise.reject(err);
  },
);

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

export { api };
