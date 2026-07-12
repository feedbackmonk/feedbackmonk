import { type WorkOrder } from "../../shared/types.gen";
import { Link } from "../../shared/router";
import { formatRelative } from "../../shared/format";
import { ActionTypeBadge, WorkOrderStateBadge } from "./badges";

// One work-order card on the Kanban board. Read-only: the whole card is a link
// to the detail page, where the owner transitions (approve / accept /
// request-changes / …) live behind their existing confirm dialogs. No inline
// state changes here — dragging or one-click transitions across the approval
// boundary is a fat-finger risk on a security gate (D-P6-4).
//
// `title` is untrusted-adjacent owner/analyst text; it is rendered as an escaped
// React text node, never markup.
export function BoardCard({ order }: { order: WorkOrder }) {
  const ownerAuthored = order.recommendation_id === null;
  return (
    <li className="ap-board-card">
      <Link
        to={`/admin/autopilot/work-orders/${encodeURIComponent(order.id)}`}
        className="ap-board-card-link"
      >
        <span className="ap-board-card-title">{order.title}</span>
      </Link>
      <div className="ap-board-card-tags">
        <ActionTypeBadge actionType={order.action_type} />
        <WorkOrderStateBadge state={order.state} />
        <span className="ap-board-card-rung">Rung {order.autonomy_rung}</span>
        {ownerAuthored ? (
          <span className="ap-board-card-owner" title="Owner-authored story">
            Owner-authored
          </span>
        ) : null}
      </div>
      {order.routing_label || order.claimed_by_runner ? (
        <div className="ap-board-card-runner">
          {order.routing_label ? (
            <span className="ap-board-card-routing" title="Routed to runner">
              → {order.routing_label}
            </span>
          ) : null}
          {order.claimed_by_runner ? (
            <span className="ap-board-card-claimed" title="Claimed by runner">
              claimed · {order.claimed_by_runner}
            </span>
          ) : null}
        </div>
      ) : null}
      <time className="ap-board-card-time muted" dateTime={order.updated_at}>
        {formatRelative(order.updated_at)}
      </time>
    </li>
  );
}
