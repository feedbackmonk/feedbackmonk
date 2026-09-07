import { useQuery } from "@tanstack/react-query";
import { type AutonomyRung } from "../../shared/types.gen";
import { fetchWorkOrders } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { formatRelative } from "../../shared/format";
import { useAdminProject } from "./useAdminProject";
import { WorkOrderStateBadge } from "./badges";
import { useTranslation } from "../../i18n";
import { useAdminLabels } from "../../i18n/useAdminLabels";
import { useLocale } from "../../i18n/useLocale";

// FR-FBR-21 — the work-order list. Every approved decision becomes a work
// order; this is the owner's audit-forward view of them. Read-only here;
// per-order owner actions live in the detail page.
export function WorkOrderList() {
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
  return <WorkOrderListInner projectId={project.projectId} />;
}

function WorkOrderListInner({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
  const adminLabels = useAdminLabels();
  const { locale } = useLocale();
  const query = useQuery({
    queryKey: ["autopilot-work-orders", projectId],
    queryFn: () => fetchWorkOrders(projectId),
  });

  const items = query.data?.items ?? [];

  function rungLabel(rung: number): string {
    if (rung >= 0 && rung <= 3) {
      return adminLabels.autonomyRungLabel(rung as AutonomyRung);
    }
    return t("admin.boardCard.rung", { rung });
  }

  return (
    <main className="ap-page" aria-labelledby="ap-wo-list-title">
      <BackLink />
      <header className="page-header">
        <h1 id="ap-wo-list-title">{t("admin.workOrderList.title")}</h1>
        <Link to="/admin/autopilot/work-orders/new" className="ap-nav-link">
          {t("admin.autopilotDigest.newStory")}
        </Link>
        <Link to="/admin/autopilot/board" className="ap-nav-link">
          {t("admin.autopilotDigest.boardView")}
        </Link>
      </header>

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          {t("admin.common.loading")}
        </p>
      ) : query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.workOrderList.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : items.length === 0 ? (
        <div className="empty-state">
          <p>{t("admin.workOrderList.empty")}</p>
        </div>
      ) : (
        <table className="feedback-table">
          <caption className="visually-hidden">
            {t("admin.workOrderList.tableCaption")}
          </caption>
          <thead>
            <tr>
              <th scope="col">{t("admin.workOrderList.columns.title")}</th>
              <th scope="col">{t("admin.workOrderList.columns.action")}</th>
              <th scope="col">{t("admin.workOrderList.columns.state")}</th>
              <th scope="col">{t("admin.workOrderList.columns.rung")}</th>
              <th scope="col">{t("admin.workOrderList.columns.updated")}</th>
            </tr>
          </thead>
          <tbody>
            {items.map((wo) => (
              <tr key={wo.id}>
                <td>
                  <Link
                    to={`/admin/autopilot/work-orders/${encodeURIComponent(wo.id)}`}
                  >
                    {wo.title}
                  </Link>
                </td>
                <td>{adminLabels.actionType(wo.action_type)}</td>
                <td>
                  <WorkOrderStateBadge state={wo.state} />
                </td>
                <td>{rungLabel(wo.autonomy_rung)}</td>
                <td>
                  <time dateTime={wo.updated_at}>
                    {formatRelative(wo.updated_at, locale)}
                  </time>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
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
