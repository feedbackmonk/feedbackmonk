import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { type WorkOrder } from "../../shared/types.gen";
import { fetchWorkOrders } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { useAdminProject } from "./useAdminProject";
import { BoardCard } from "./BoardCard";
import { BOARD_COLUMNS, groupByColumn, type BoardColumnId } from "./boardColumns";

// C31 §7 — read-only Kanban view of work orders, grouped into 6 lifecycle
// columns (see boardColumns.ts). No drag-to-transition (D-P6-4): every state
// change stays behind the explicit dialogs on the detail page. Depends only on
// Stage 0 (nullable provenance + routing_label already served by WorkOrderView).

// Fetch a generous page so the board is a faithful overview; if the tenant has
// more orders than this, we surface it explicitly (no silent drop — see below).
const FETCH_LIMIT = 200;
// Per-column render cap — a runaway column is truncated with an explicit
// "+N more" link to the filtered list, never silently cut.
const COLUMN_CARD_CAP = 25;

export function Board() {
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
  return <BoardInner projectId={project.projectId} />;
}

function BoardInner({ projectId }: { projectId: string }) {
  const query = useQuery({
    queryKey: ["autopilot-board", projectId],
    queryFn: () => fetchWorkOrders(projectId, { limit: FETCH_LIMIT }),
  });

  const items = useMemo<WorkOrder[]>(() => query.data?.items ?? [], [query.data]);
  const groups = useMemo(() => groupByColumn(items), [items]);

  // If the server reports more orders than we fetched (or we filled the page
  // exactly), the board is not showing everything — say so, don't pretend.
  const total = query.data?.total;
  const overFetchWindow =
    (typeof total === "number" && total > items.length) ||
    items.length >= FETCH_LIMIT;

  return (
    <main className="ap-page ap-board-page" aria-labelledby="ap-board-title">
      <BackLink />
      <header className="page-header">
        <h1 id="ap-board-title">Board</h1>
        <Link to="/admin/autopilot/work-orders" className="ap-nav-link">
          List view →
        </Link>
      </header>

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          Loading board…
        </p>
      ) : query.isError ? (
        <div role="alert" className="error-block">
          Failed to load the board.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      ) : (
        <>
          {overFetchWindow ? (
            <p className="muted ap-board-truncation-note">
              Showing the {items.length} most recent work orders
              {typeof total === "number" ? ` of ${total}` : ""}. Older orders are
              in the{" "}
              <Link to="/admin/autopilot/work-orders">full list</Link>.
            </p>
          ) : null}
          <div className="ap-board-scroll">
            <ol className="ap-board-columns" aria-label="Work orders by state">
              {BOARD_COLUMNS.map((col) => (
                <BoardColumn
                  key={col.id}
                  columnId={col.id}
                  label={col.label}
                  orders={groups[col.id]}
                />
              ))}
            </ol>
          </div>
        </>
      )}
    </main>
  );
}

function BoardColumn({
  columnId,
  label,
  orders,
}: {
  columnId: BoardColumnId;
  label: string;
  orders: WorkOrder[];
}) {
  const headingId = `ap-board-col-${columnId}`;
  const shown = orders.slice(0, COLUMN_CARD_CAP);
  const overflow = orders.length - shown.length;
  return (
    <li className="ap-board-column">
      <section aria-labelledby={headingId} className="ap-board-column-inner">
        <h2 id={headingId} className="ap-board-column-head">
          {label} <span className="muted">({orders.length})</span>
        </h2>
        {orders.length === 0 ? (
          <p className="muted ap-board-column-empty">Nothing here.</p>
        ) : (
          <ol className="ap-board-card-list" aria-label={`${label} work orders`}>
            {shown.map((order) => (
              <BoardCard key={order.id} order={order} />
            ))}
          </ol>
        )}
        {overflow > 0 ? (
          <Link
            to="/admin/autopilot/work-orders"
            className="ap-board-column-more"
          >
            +{overflow} more →
          </Link>
        ) : null}
      </section>
    </li>
  );
}

function BackLink() {
  return (
    <Link to="/admin/autopilot" className="ap-back-link">
      ← Back to digest
    </Link>
  );
}
