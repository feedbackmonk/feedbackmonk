import {
  WORK_ORDER_EXECUTION_STATES,
  type ActionType,
  type ClusterPriority,
  type WorkOrderState,
} from "../../shared/types.gen";
import { useTranslation } from "../../i18n";
import { useAdminLabels } from "../../i18n/useAdminLabels";

// Small presentational badges shared across the autopilot surface. Each pairs
// its color with a text label so meaning is never carried by color alone
// (WCAG 1.4.1) — the same discipline as StatusBadge / the usage meter.

export function PriorityBadge({ priority }: { priority: ClusterPriority }) {
  const adminLabels = useAdminLabels();
  return (
    <span
      className={`ap-priority ap-priority-${priority}`}
      data-priority={priority}
    >
      {adminLabels.clusterPriority(priority)}
    </span>
  );
}

export function ActionTypeBadge({ actionType }: { actionType: ActionType }) {
  const adminLabels = useAdminLabels();
  return (
    <span className={`ap-action ap-action-${actionType}`}>
      {adminLabels.actionType(actionType)}
    </span>
  );
}

export function WorkOrderStateBadge({ state }: { state: WorkOrderState }) {
  const { t } = useTranslation("admin");
  const adminLabels = useAdminLabels();
  const executing = WORK_ORDER_EXECUTION_STATES.includes(state);
  const label = adminLabels.workOrderState(state);
  return (
    <span
      className={`ap-wo-state ap-wo-state-${state}`}
      // Surface "the agent is acting on code right now" to assistive tech, not
      // just sighted users. Display-only — never an authorization signal.
      aria-label={
        executing
          ? t("admin.badges.executingAria", { state: label })
          : label
      }
    >
      {label}
    </span>
  );
}

// Confidence is advisory grounding metadata, not an authorization input. Render
// it as a labelled percentage with a non-color-only meter (text + bar).
export function ConfidenceMeter({ confidence }: { confidence: number }) {
  const { t } = useTranslation("admin");
  const pct = Math.round(clamp01(confidence) * 100);
  return (
    <span
      className="ap-confidence"
      title={t("admin.badges.confidenceTitle", { pct })}
    >
      <span className="ap-confidence-label">
        {t("admin.badges.confidenceLabel")}
      </span>
      <span
        className="ap-confidence-bar"
        role="meter"
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={t("admin.badges.confidencePercentAria", { pct })}
      >
        <span className="ap-confidence-fill" style={{ width: `${pct}%` }} />
      </span>
      <span className="ap-confidence-value">{pct}%</span>
    </span>
  );
}

function clamp01(n: number): number {
  if (Number.isNaN(n)) return 0;
  return Math.min(1, Math.max(0, n));
}
