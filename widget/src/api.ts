import type {
  ApiError,
  MountOptions,
  SubmitFeedbackRequest,
  SubmitFeedbackResponse,
  WidgetConfig,
} from "./types.js";

// HTTP helpers for the feedbackmonk widget. CSP-safe — uses native fetch, no
// eval, no Function constructor, no dynamic import. Calls home ONLY to the
// configured feedbackmonk backend (DEC-FBR-02 brand promise).

const DEFAULT_API_BASE = "https://api.feedbackmonk.com";

export function resolveApiBase(opts: MountOptions): string {
  if (opts.apiBase) {
    return opts.apiBase.replace(/\/$/, "");
  }
  return DEFAULT_API_BASE;
}

function authHeaders(opts: MountOptions): Record<string, string> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    Accept: "application/json",
  };
  if (opts.jwt) {
    headers["Authorization"] = "Bearer " + opts.jwt;
  }
  return headers;
}

async function readError(response: Response): Promise<ApiError> {
  try {
    const data = (await response.json()) as Partial<ApiError>;
    if (data && typeof data.code === "string" && typeof data.message === "string") {
      return { code: data.code, message: data.message };
    }
  } catch {
    // fall through
  }
  return {
    code: "http_" + response.status,
    message: "Request failed (" + response.status + ")",
  };
}

export async function fetchWidgetConfig(
  projectId: string,
  opts: MountOptions,
): Promise<WidgetConfig> {
  const url =
    resolveApiBase(opts) +
    "/api/v1/projects/" +
    encodeURIComponent(projectId) +
    "/widget-config";
  const response = await fetch(url, {
    method: "GET",
    headers: { Accept: "application/json" },
    credentials: "omit",
  });
  if (!response.ok) {
    throw await readError(response);
  }
  return (await response.json()) as WidgetConfig;
}

export async function submitFeedback(
  projectId: string,
  payload: SubmitFeedbackRequest,
  opts: MountOptions,
): Promise<SubmitFeedbackResponse> {
  const url =
    resolveApiBase(opts) +
    "/api/v1/projects/" +
    encodeURIComponent(projectId) +
    "/feedback";
  const response = await fetch(url, {
    method: "POST",
    headers: authHeaders(opts),
    body: JSON.stringify(payload),
    credentials: opts.jwt ? "omit" : "include",
  });
  if (!response.ok) {
    throw await readError(response);
  }
  return (await response.json()) as SubmitFeedbackResponse;
}
