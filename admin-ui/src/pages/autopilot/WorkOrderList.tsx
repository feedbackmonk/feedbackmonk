import { useQuery } from "@tanstack/react-query";
import {
  ACTION_TYPE_LABELS,
  AUTONOMY_RUNG_LABELS,
  type AutonomyRung,
} from "../../shared/types.gen";
import { fetchWorkOrders } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { formatRelative } from "../../shared/format";
import { useAdminProject } from "./useAdminProject";
import { WorkOrderStateBadge } from "./badges";

// FR-FBR-21 — the work-order list. Every approved decision becomes a work
// order; this is the owner's audit-forward view of them. Read-only here;
// per-order owner actions live in the detail page.
export function WorkOrderList() {
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
  return <WorkOrderListInner projectId={project.projectId} />;
}

function WorkOrderListInner({ projectId }: { projectId: string }) {
  const query = useQuery({
    queryKey: ["autopilot-work-orders", projectId],
    queryFn: () => fetchWorkOrders(projectId),
  });

  const items = query.data?.items ?? [];

  return (
    <main className="ap-page" aria-labelledby="ap-wo-list-title">
      <BackLink />
      <header className="page-header">
        <h1 id="ap-wo-list-title">Work orders</h1>
      </header>

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          Loading…
        </p>
      ) : query.isError ? (
        <div role="alert" className="error-block">
          Failed to load work orders.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      ) : items.length === 0 ? (
        <div className="empty-state">
          <p>
            No work orders yet. Approve a recommendation to create the first one.
          </p>
        </div>
      ) : (
        <table className="feedback-table">
          <caption className="visually-hidden">
            Work orders, newest first.
          </caption>
          <thead>
            <tr>
              <th scope="col">Title</th>
              <th scope="col">Action</th>
              <th scope="col">State</th>
              <th scope="col">Rung</th>
              <th scope="col">Updated</th>
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
                <td>{ACTION_TYPE_LABELS[wo.action_type]}</td>
                <td>
                  <WorkOrderStateBadge state={wo.state} />
                </td>
                <td>{rungLabel(wo.autonomy_rung)}</td>
                <td>
                  <time dateTime={wo.updated_at}>
                    {formatRelative(wo.updated_at)}
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

function rungLabel(rung: number): string {
  if (rung >= 0 && rung <= 3) return AUTONOMY_RUNG_LABELS[rung as AutonomyRung];
  return `Rung ${rung}`;
}

function BackLink() {
  return (
    <Link to="/admin/autopilot" className="ap-back-link">
      ← Back to digest
    </Link>
  );
}
