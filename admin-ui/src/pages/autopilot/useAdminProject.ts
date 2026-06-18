import { useQuery } from "@tanstack/react-query";
import { fetchAdminProjects } from "../../shared/ApiClient";

// Resolves the admin's sole project id — the same P0/P1 invariant the roadmap
// admin page relies on (one project per tenant in practice; multi-project URL
// routing is deferred). Centralized here so every autopilot page shares one
// resolution + cache key (`admin-projects`, also used by AdminRoadmap).
export interface AdminProjectState {
  status: "pending" | "error" | "ready";
  projectId: string | null;
}

export function useAdminProject(): AdminProjectState {
  const query = useQuery({
    queryKey: ["admin-projects"],
    queryFn: fetchAdminProjects,
    staleTime: 60_000,
  });

  if (query.isPending) return { status: "pending", projectId: null };
  if (query.isError || !query.data?.projects.length) {
    return { status: "error", projectId: null };
  }
  return { status: "ready", projectId: query.data.projects[0].project_id };
}
