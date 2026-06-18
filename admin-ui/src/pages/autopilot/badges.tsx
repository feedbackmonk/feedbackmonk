import {
  ACTION_TYPE_LABELS,
  CLUSTER_PRIORITY_LABELS,
  WORK_ORDER_EXECUTION_STATES,
  WORK_ORDER_STATE_LABELS,
  type ActionType,
  type ClusterPriority,
  type WorkOrderState,
} from "../../shared/types.gen";

// Small presentational badges shared across the autopilot surface. Each pairs
// its color with a text label so meaning is never carried by color alone
// (WCAG 1.4.1) — the same discipline as StatusBadge / the usage meter.

export function PriorityBadge({ priority }: { priority: ClusterPriority }) {
  return (
    <span
      className={`ap-priority ap-priority-${priority}`}
      data-priority={priority}
    >
      {CLUSTER_PRIORITY_LABELS[priority]}
    </span>
  );
}

export function ActionTypeBadge({ actionType }: { actionType: ActionType }) {
  return (
    <span className={`ap-action ap-action-${actionType}`}>
      {ACTION_TYPE_LABELS[actionType]}
    </span>
  );
}

export function WorkOrderStateBadge({ state }: { state: WorkOrderState }) {
  const executing = WORK_ORDER_EXECUTION_STATES.includes(state);
  return (
    <span
      className={`ap-wo-state ap-wo-state-${state}`}
      // Surface "the agent is acting on code right now" to assistive tech, not
      // just sighted users. Display-only — never an authorization signal.
      aria-label={
        executing
          ? `${WORK_ORDER_STATE_LABELS[state]} (agent executing)`
          : WORK_ORDER_STATE_LABELS[state]
      }
    >
      {WORK_ORDER_STATE_LABELS[state]}
    </span>
  );
}

// Confidence is advisory grounding metadata, not an authorization input. Render
// it as a labelled percentage with a non-color-only meter (text + bar).
export function ConfidenceMeter({ confidence }: { confidence: number }) {
  const pct = Math.round(clamp01(confidence) * 100);
  return (
    <span className="ap-confidence" title={`Model confidence ${pct}%`}>
      <span className="ap-confidence-label">Confidence</span>
      <span
        className="ap-confidence-bar"
        role="meter"
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={`Confidence ${pct} percent`}
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
