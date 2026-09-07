import { useQuery } from "@tanstack/react-query";
import {
  type ClusterDetail as ClusterDetailShape,
} from "../../shared/types.gen";
import { fetchClusterDetail } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { formatRelative } from "../../shared/format";
import { useAdminProject } from "./useAdminProject";
import { PriorityBadge } from "./badges";
import { RecommendationCard } from "./RecommendationCard";
import { Trans } from "react-i18next";
import { useTranslation } from "../../i18n";
import { useLabels } from "../../i18n/useLabels";
import { useAdminLabels } from "../../i18n/useAdminLabels";
import { useLocale } from "../../i18n/useLocale";

// FR-FBR-21 cluster detail — the cluster's members (the feedback grouped into
// it, rendered as quoted data) and its recommendations (newest first) with the
// owner's approve / tweak / reject controls. The priority_rationale is shown
// prominently (explainability). All member/cluster text is untrusted public
// input, rendered as escaped React text nodes.
export function ClusterDetail({ clusterId }: { clusterId: string }) {
  const { t } = useTranslation("admin");
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="ap-page" aria-busy="true">
        <BackLink />
        <p className="muted">{t("admin.common.loading")}</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="ap-page">
        <BackLink />
        <div role="alert" className="error-block">
          {t("admin.common.noProjects")}
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
  const { t } = useTranslation("admin");
  const labels = useLabels();
  const adminLabels = useAdminLabels();
  const { locale } = useLocale();
  const query = useQuery({
    queryKey: ["autopilot-cluster", projectId, clusterId],
    queryFn: () => fetchClusterDetail(projectId, clusterId),
  });

  if (query.isPending) {
    return (
      <main className="ap-page" aria-busy="true">
        <BackLink />
        <p className="muted">{t("admin.clusterDetail.loadingCluster")}</p>
      </main>
    );
  }
  if (query.isError || !query.data) {
    return (
      <main className="ap-page">
        <BackLink />
        <div role="alert" className="error-block">
          {t("admin.clusterDetail.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
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
            {labels.kind(cluster.kind)}
          </span>
          <span className="muted">
            {adminLabels.clusterStatus(cluster.status)}
          </span>
        </div>
      </header>

      <section aria-labelledby="ap-cluster-summary-label">
        <h2 id="ap-cluster-summary-label" className="visually-hidden">
          {t("admin.clusterDetail.summaryHeading")}
        </h2>
        {cluster.summary ? (
          <p className="ap-cluster-summary">{cluster.summary}</p>
        ) : null}
        {cluster.priority_rationale ? (
          <p className="ap-cluster-rationale">
            <span className="muted">
              {t("admin.autopilotDigest.whyThisPriority")}
            </span>
            {cluster.priority_rationale}
          </p>
        ) : null}
        {cluster.last_swept_at ? (
          <p className="muted">
            <Trans
              i18nKey="admin.clusterDetail.lastSwept"
              t={t}
              values={{ when: formatRelative(cluster.last_swept_at, locale) }}
              components={{ time: <time dateTime={cluster.last_swept_at} /> }}
            />
          </p>
        ) : null}
      </section>

      <section aria-labelledby="ap-cluster-recs-label">
        <h2 id="ap-cluster-recs-label">
          {t("admin.clusterDetail.recommendationsHeading")}{" "}
          <span className="muted">({cluster.recommendations.length})</span>
        </h2>
        {cluster.recommendations.length === 0 ? (
          <p className="muted">
            {t("admin.clusterDetail.noRecommendations")}
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
            {t("admin.clusterDetail.groupedFeedbackHeading")}{" "}
            <span className="muted">({cluster.members.length})</span>
          </h2>
          {cluster.members.length === 0 ? (
            <p className="muted">{t("admin.clusterDetail.noMembers")}</p>
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
                  {labels.kind(m.kind)}
                </span>
                <span className="muted">{labels.status(m.status)}</span>
                {/* Untrusted submitter text — quoted data only. */}
                <span className="ap-member-excerpt" dir="auto">{m.body_excerpt}</span>
                <time className="muted" dateTime={m.submitted_at}>
                  {formatRelative(m.submitted_at, locale)}
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
  const { t } = useTranslation("admin");
  return (
    <Link to="/admin/autopilot" className="ap-back-link">
      {t("admin.common.backToDigest")}
    </Link>
  );
}
