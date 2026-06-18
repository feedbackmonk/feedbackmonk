import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  CLUSTER_PRIORITY_LABELS,
  CLUSTER_PRIORITY_ORDER,
  CLUSTER_STATUS_LABELS,
  KIND_LABELS,
  type ClusterPriority,
  type ClusterSummary,
} from "../../shared/types.gen";
import { fetchClusters, fetchLatestSweep } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { formatRelative } from "../../shared/format";
import { useAdminProject } from "./useAdminProject";
import { PriorityBadge } from "./badges";

// FR-FBR-21 review surface — the digest. "What changed since the last sweep"
// (latest sweep summary) above the owner-visible cluster list, ordered by
// priority. Every cluster shows its `priority_rationale` (load-bearing for
// explainability — the owner must see WHY a cluster is prioritized). All
// cluster text is untrusted public-derived data, rendered as escaped text.
export function AutopilotDigest() {
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="ap-page" aria-busy="true">
        <h1>Autopilot</h1>
        <p className="muted">Loading…</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="ap-page">
        <h1>Autopilot</h1>
        <div role="alert" className="error-block">
          No projects configured. Create one before reviewing the autopilot
          digest.
        </div>
      </main>
    );
  }
  return <AutopilotDigestInner projectId={project.projectId} />;
}

function AutopilotDigestInner({ projectId }: { projectId: string }) {
  const sweepQuery = useQuery({
    queryKey: ["autopilot-latest-sweep", projectId],
    queryFn: () => fetchLatestSweep(projectId),
  });
  const clustersQuery = useQuery({
    queryKey: ["autopilot-clusters", projectId],
    queryFn: () => fetchClusters(projectId),
  });

  const grouped = useMemo(() => {
    const g: Record<ClusterPriority, ClusterSummary[]> = {
      high: [],
      medium: [],
      low: [],
      none: [],
    };
    for (const c of clustersQuery.data?.items ?? []) {
      // A cluster that has been merged into a survivor is not its own row in
      // the digest — its members live under the survivor.
      if (c.status === "merged") continue;
      g[c.priority].push(c);
    }
    return g;
  }, [clustersQuery.data]);

  const sweep = sweepQuery.data;
  const totalClusters = clustersQuery.data?.items.length ?? 0;

  return (
    <main className="ap-page" aria-labelledby="ap-digest-title">
      <header className="page-header">
        <h1 id="ap-digest-title">Autopilot</h1>
        <Link to="/admin/autopilot/work-orders" className="ap-nav-link">
          Work orders →
        </Link>
      </header>

      <section className="ap-digest-summary" aria-labelledby="ap-digest-changes">
        <h2 id="ap-digest-changes">What changed since the last sweep</h2>
        {sweepQuery.isPending ? (
          <p className="muted">Loading digest…</p>
        ) : sweep ? (
          <div className="ap-sweep-card">
            <p className="ap-sweep-meta muted">
              Last sweep{" "}
              <time dateTime={sweep.started_at}>
                {formatRelative(sweep.started_at)}
              </time>{" "}
              ({sweep.triggered_by === "schedule" ? "scheduled" : "on demand"}) ·{" "}
              {sweep.status} · {sweep.clusters_touched} clusters touched ·{" "}
              {sweep.recommendations_emitted} recommendations
            </p>
            {sweep.digest_summary ? (
              <p className="ap-sweep-summary">{sweep.digest_summary}</p>
            ) : (
              <p className="muted">No digest summary recorded for this sweep.</p>
            )}
          </div>
        ) : (
          <p className="muted">
            No analysis sweep has run yet. Clusters below are formed
            continuously as feedback arrives.
          </p>
        )}
      </section>

      <section aria-labelledby="ap-clusters-title">
        <h2 id="ap-clusters-title">
          Clusters <span className="muted">({totalClusters})</span>
        </h2>

        {clustersQuery.isPending ? (
          <p className="muted" aria-busy="true">
            Loading clusters…
          </p>
        ) : clustersQuery.isError ? (
          <div role="alert" className="error-block">
            Failed to load clusters.{" "}
            <button type="button" onClick={() => clustersQuery.refetch()}>
              Retry
            </button>
          </div>
        ) : totalClusters === 0 ? (
          <div className="empty-state">
            <p>No clusters yet. They form automatically as feedback arrives.</p>
          </div>
        ) : (
          CLUSTER_PRIORITY_ORDER.map((priority) => {
            const items = grouped[priority];
            if (items.length === 0) return null;
            return (
              <section
                key={priority}
                aria-labelledby={`ap-priority-${priority}`}
                className={`ap-priority-section ap-priority-section-${priority}`}
              >
                <h3 id={`ap-priority-${priority}`}>
                  {CLUSTER_PRIORITY_LABELS[priority]} priority{" "}
                  <span className="muted">({items.length})</span>
                </h3>
                <ul className="ap-cluster-list">
                  {items.map((c) => (
                    <ClusterRow key={c.id} cluster={c} />
                  ))}
                </ul>
              </section>
            );
          })
        )}
      </section>
    </main>
  );
}

function ClusterRow({ cluster }: { cluster: ClusterSummary }) {
  return (
    <li className="ap-cluster-row">
      <div className="ap-cluster-row-main">
        <Link
          to={`/admin/autopilot/clusters/${encodeURIComponent(cluster.id)}`}
          className="ap-cluster-link"
        >
          {cluster.label}
        </Link>
        <div className="ap-cluster-tags">
          <PriorityBadge priority={cluster.priority} />
          <span className={`kind-badge kind-${cluster.kind}`}>
            {KIND_LABELS[cluster.kind]}
          </span>
          <span className="muted">
            {CLUSTER_STATUS_LABELS[cluster.status]} · {cluster.member_count}{" "}
            {cluster.member_count === 1 ? "item" : "items"}
          </span>
        </div>
      </div>
      {/* priority_rationale is load-bearing for explainability — shown as
          quoted data whenever the analyst supplied one. */}
      {cluster.priority_rationale ? (
        <p className="ap-cluster-rationale">
          <span className="muted">Why this priority: </span>
          {cluster.priority_rationale}
        </p>
      ) : null}
    </li>
  );
}
