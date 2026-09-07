import { type FeedbackStatus } from "../shared/types.gen";
import { useLabels } from "../i18n/useLabels";

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
  const labels = useLabels();
  // A status outside the union (backend/UI drift) must still render something
  // legible rather than an empty pill — fall back to the raw wire value and a
  // neutral icon. Previously an unrecognised status produced a badge with no
  // text at all, which read as "no status" instead of "unknown status".
  const label = labels.status(status);
  const icon = STATUS_ICONS[status] ?? "•";
  return (
    <span className={`status-badge status-${status}`}>
      <span aria-hidden="true" className="status-badge-icon">
        {icon}
      </span>
      <span className="status-badge-label">{label}</span>
    </span>
  );
}
