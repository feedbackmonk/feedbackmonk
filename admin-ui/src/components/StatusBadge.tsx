import { STATUS_LABELS, type FeedbackStatus } from "../shared/types.gen";

const STATUS_ICONS: Record<FeedbackStatus, string> = {
  submitted: "•",
  triaged: "◐",
  "in-progress": "▶",
  shipped: "✓",
  wontfix: "✕",
  duplicate: "⇉",
};

// Color alone never carries meaning (WCAG 1.4.1); the icon + label pair is
// load-bearing for non-sighted and color-deficient users alike.
export function StatusBadge({ status }: { status: FeedbackStatus }) {
  return (
    <span className={`status-badge status-${status}`}>
      <span aria-hidden="true" className="status-badge-icon">
        {STATUS_ICONS[status]}
      </span>
      <span className="status-badge-label">{STATUS_LABELS[status]}</span>
    </span>
  );
}
