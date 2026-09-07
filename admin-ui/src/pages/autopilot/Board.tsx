import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { type WorkOrder } from "../../shared/types.gen";
import { fetchWorkOrders } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { useAdminProject } from "./useAdminProject";
import { BoardCard } from "./BoardCard";
import { BOARD_COLUMNS, groupByColumn, type BoardColumnId } from "./boardColumns";
import { Trans } from "react-i18next";
import { useTranslation } from "../../i18n";

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
  return <BoardInner projectId={project.projectId} />;
}

function BoardInner({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
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
        <h1 id="ap-board-title">{t("admin.autopilotBoard.title")}</h1>
        <Link to="/admin/autopilot/work-orders" className="ap-nav-link">
          {t("admin.autopilotBoard.listView")}
        </Link>
      </header>

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          {t("admin.autopilotBoard.loadingBoard")}
        </p>
      ) : query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.autopilotBoard.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : (
        <>
          {overFetchWindow ? (
            <p className="muted ap-board-truncation-note">
              <Trans
                i18nKey={
                  typeof total === "number"
                    ? "admin.autopilotBoard.truncationNoteWithTotal"
                    : "admin.autopilotBoard.truncationNote"
                }
                t={t}
                values={{ shown: items.length, total }}
                components={{
                  link: <Link to="/admin/autopilot/work-orders">{null}</Link>,
                }}
              />
            </p>
          ) : null}
          <div className="ap-board-scroll">
            <ol
              className="ap-board-columns"
              aria-label={t("admin.autopilotBoard.columnsAria")}
            >
              {BOARD_COLUMNS.map((col) => (
                <BoardColumn
                  key={col.id}
                  columnId={col.id}
                  label={t(col.labelKey)}
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
  const { t } = useTranslation("admin");
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
          <p className="muted ap-board-column-empty">
            {t("admin.autopilotBoard.columnEmpty")}
          </p>
        ) : (
          <ol
            className="ap-board-card-list"
            aria-label={t("admin.autopilotBoard.columnOrdersAria", { label })}
          >
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
            {t("admin.autopilotBoard.moreLink", { count: overflow })}
          </Link>
        ) : null}
      </section>
    </li>
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
