import { useQuery } from "@tanstack/react-query";
import {
  CLUSTER_STATUS_LABELS,
  KIND_LABELS,
  STATUS_LABELS,
  type ClusterDetail as ClusterDetailShape,
} from "../../shared/types.gen";
import { fetchClusterDetail } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { formatRelative } from "../../shared/format";
import { useAdminProject } from "./useAdminProject";
import { PriorityBadge } from "./badges";
import { RecommendationCard } from "./RecommendationCard";

// FR-FBR-21 cluster detail — the cluster's members (the feedback grouped into
// it, rendered as quoted data) and its recommendations (newest first) with the
// owner's approve / tweak / reject controls. The priority_rationale is shown
// prominently (explainability). All member/cluster text is untrusted public
// input, rendered as escaped React text nodes.
export function ClusterDetail({ clusterId }: { clusterId: string }) {
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="ap-page" aria-busy="true">
        <BackLink />
        <p className="muted">Loading…</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="ap-page">
        <BackLink />
        <div role="alert" className="error-block">
          No projects configured.
        </div>
      </main>
    );
  }
  return <ClusterDetailInner projectId={project.projectId} clusterId={clusterId} />;
}

function ClusterDetailInner({
  projectId,
  clusterId,
}: {
  projectId: string;
  clusterId: string;
}) {
  const query = useQuery({
    queryKey: ["autopilot-cluster", projectId, clusterId],
    queryFn: () => fetchClusterDetail(projectId, clusterId),
  });

  if (query.isPending) {
    return (
      <main className="ap-page" aria-busy="true">
        <BackLink />
        <p className="muted">Loading cluster…</p>
      </main>
    );
  }
  if (query.isError || !query.data) {
    return (
      <main className="ap-page">
        <BackLink />
        <div role="alert" className="error-block">
          Failed to load this cluster.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      </main>
    );
  }

  const cluster: ClusterDetailShape = query.data;

  return (
    <main className="ap-page" aria-labelledby="ap-cluster-title">
      <BackLink />
      <header className="ap-cluster-header">
        <h1 id="ap-cluster-title">{cluster.label}</h1>
        <div className="ap-cluster-tags">
          <PriorityBadge priority={cluster.priority} />
          <span className={`kind-badge kind-${cluster.kind}`}>
            {KIND_LABELS[cluster.kind]}
          </span>
          <span className="muted">{CLUSTER_STATUS_LABELS[cluster.status]}</span>
        </div>
      </header>

      <section aria-labelledby="ap-cluster-summary-label">
        <h2 id="ap-cluster-summary-label" className="visually-hidden">
          Summary
        </h2>
        {cluster.summary ? (
          <p className="ap-cluster-summary">{cluster.summary}</p>
        ) : null}
        {cluster.priority_rationale ? (
          <p className="ap-cluster-rationale">
            <span className="muted">Why this priority: </span>
            {cluster.priority_rationale}
          </p>
        ) : null}
        {cluster.last_swept_at ? (
          <p className="muted">
            Last swept{" "}
            <time dateTime={cluster.last_swept_at}>
              {formatRelative(cluster.last_swept_at)}
            </time>
          </p>
        ) : null}
      </section>

      <section aria-labelledby="ap-cluster-recs-label">
        <h2 id="ap-cluster-recs-label">
          Recommendations{" "}
          <span className="muted">({cluster.recommendations.length})</span>
        </h2>
        {cluster.recommendations.length === 0 ? (
          <p className="muted">
            No recommendations yet. They are emitted by analysis sweeps.
          </p>
        ) : (
          <div className="ap-rec-cards">
            {cluster.recommendations.map((rec) => (
              <RecommendationCard key={rec.id} rec={rec} projectId={projectId} />
            ))}
          </div>
        )}
      </section>

      {/* Worker B's ClusterDetailView does not serve individual members in
          P5a (member_count is on the summary). Render this section only if a
          backend later includes the members array. */}
      {cluster.members ? (
        <section aria-labelledby="ap-cluster-members-label">
          <h2 id="ap-cluster-members-label">
            Grouped feedback{" "}
            <span className="muted">({cluster.members.length})</span>
          </h2>
          {cluster.members.length === 0 ? (
            <p className="muted">No feedback in this cluster.</p>
          ) : (
            <ul className="ap-member-list">
              {cluster.members.map((m) => (
              <li key={m.feedback_id} className="ap-member-row">
                <Link
                  to={`/feedback/${encodeURIComponent(m.feedback_id)}`}
                  className="mono"
                >
                  {m.feedback_id}
                </Link>
                <span className={`kind-badge kind-${m.kind}`}>
                  {KIND_LABELS[m.kind]}
                </span>
                <span className="muted">{STATUS_LABELS[m.status]}</span>
                {/* Untrusted submitter text — quoted data only. */}
                <span className="ap-member-excerpt">{m.body_excerpt}</span>
                <time className="muted" dateTime={m.submitted_at}>
                  {formatRelative(m.submitted_at)}
                </time>
                </li>
              ))}
            </ul>
          )}
        </section>
      ) : null}
    </main>
  );
}

function BackLink() {
  return (
    <Link to="/admin/autopilot" className="ap-back-link">
      ← Back to digest
    </Link>
  );
}
