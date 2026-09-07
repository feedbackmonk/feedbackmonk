import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  CLUSTER_PRIORITY_ORDER,
  type ClusterPriority,
  type ClusterSummary,
} from "../../shared/types.gen";
import { fetchClusters, fetchLatestSweep } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { formatRelative } from "../../shared/format";
import { useAdminProject } from "./useAdminProject";
import { PriorityBadge } from "./badges";
import { Trans } from "react-i18next";
import { useTranslation } from "../../i18n";
import { useLabels } from "../../i18n/useLabels";
import { useAdminLabels } from "../../i18n/useAdminLabels";
import { useLocale } from "../../i18n/useLocale";

// FR-FBR-21 review surface — the digest. "What changed since the last sweep"
// (latest sweep summary) above the owner-visible cluster list, ordered by
// priority. Every cluster shows its `priority_rationale` (load-bearing for
// explainability — the owner must see WHY a cluster is prioritized). All
// cluster text is untrusted public-derived data, rendered as escaped text.
export function AutopilotDigest() {
  const { t } = useTranslation("admin");
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="ap-page" aria-busy="true">
        <h1>{t("admin.autopilotDigest.title")}</h1>
        <p className="muted">{t("admin.common.loading")}</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="ap-page">
        <h1>{t("admin.autopilotDigest.title")}</h1>
        <div role="alert" className="error-block">
          {t("admin.autopilotDigest.noProjects")}
        </div>
      </main>
    );
  }
  return <AutopilotDigestInner projectId={project.projectId} />;
}

function AutopilotDigestInner({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
  const adminLabels = useAdminLabels();
  const { locale } = useLocale();
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
        <h1 id="ap-digest-title">{t("admin.autopilotDigest.title")}</h1>
        <Link to="/admin/autopilot/work-orders/new" className="ap-nav-link">
          {t("admin.autopilotDigest.newStory")}
        </Link>
        <Link to="/admin/autopilot/board" className="ap-nav-link">
          {t("admin.autopilotDigest.boardView")}
        </Link>
        <Link to="/admin/autopilot/work-orders" className="ap-nav-link">
          {t("admin.autopilotDigest.workOrders")}
        </Link>
      </header>

      <section className="ap-digest-summary" aria-labelledby="ap-digest-changes">
        <h2 id="ap-digest-changes">{t("admin.autopilotDigest.changesHeading")}</h2>
        {sweepQuery.isPending ? (
          <p className="muted">{t("admin.autopilotDigest.loadingDigest")}</p>
        ) : sweep ? (
          <div className="ap-sweep-card">
            <p className="ap-sweep-meta muted">
              <Trans
                i18nKey="admin.autopilotDigest.sweepMeta"
                t={t}
                values={{
                  when: formatRelative(sweep.started_at, locale),
                  trigger:
                    sweep.triggered_by === "schedule"
                      ? t("admin.autopilotDigest.triggerScheduled")
                      : t("admin.autopilotDigest.triggerOnDemand"),
                  status: sweep.status,
                  touched: sweep.clusters_touched,
                  emitted: sweep.recommendations_emitted,
                }}
                components={{ time: <time dateTime={sweep.started_at} /> }}
              />
            </p>
            {sweep.digest_summary ? (
              <p className="ap-sweep-summary">{sweep.digest_summary}</p>
            ) : (
              <p className="muted">
                {t("admin.autopilotDigest.noDigestSummary")}
              </p>
            )}
          </div>
        ) : (
          <p className="muted">{t("admin.autopilotDigest.noSweepYet")}</p>
        )}
      </section>

      <section aria-labelledby="ap-clusters-title">
        <h2 id="ap-clusters-title">
          {t("admin.autopilotDigest.clustersHeading")}{" "}
          <span className="muted">({totalClusters})</span>
        </h2>

        {clustersQuery.isPending ? (
          <p className="muted" aria-busy="true">
            {t("admin.autopilotDigest.loadingClusters")}
          </p>
        ) : clustersQuery.isError ? (
          <div role="alert" className="error-block">
            {t("admin.autopilotDigest.clustersLoadError")}{" "}
            <button type="button" onClick={() => clustersQuery.refetch()}>
              {t("admin.common.retry")}
            </button>
          </div>
        ) : totalClusters === 0 ? (
          <div className="empty-state">
            <p>{t("admin.autopilotDigest.noClusters")}</p>
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
                  {t("admin.autopilotDigest.priorityHeading", {
                    priority: adminLabels.clusterPriority(priority),
                  })}{" "}
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
  const { t } = useTranslation("admin");
  const labels = useLabels();
  const adminLabels = useAdminLabels();
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
            {labels.kind(cluster.kind)}
          </span>
          <span className="muted">
            {t("admin.autopilotDigest.clusterMeta", {
              status: adminLabels.clusterStatus(cluster.status),
              count: cluster.member_count,
            })}
          </span>
        </div>
      </div>
      {/* priority_rationale is load-bearing for explainability — shown as
          quoted data whenever the analyst supplied one. */}
      {cluster.priority_rationale ? (
        <p className="ap-cluster-rationale">
          <span className="muted">
            {t("admin.autopilotDigest.whyThisPriority")}
          </span>
          {cluster.priority_rationale}
        </p>
      ) : null}
    </li>
  );
}
