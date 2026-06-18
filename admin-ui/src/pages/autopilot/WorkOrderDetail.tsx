import { useId, useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ACTION_TYPE_LABELS,
  AUTONOMY_RUNG_LABELS,
  WORK_ORDER_EVENT_LABELS,
  WORK_ORDER_OWNER_TRANSITIONS,
  WORK_ORDER_STATE_LABELS,
  WORK_ORDER_TERMINAL_STATES,
  type AutonomyRung,
  type OwnerOverrides,
  type WorkOrderDetail as WorkOrderDetailShape,
  type WorkOrderOwnerEventType,
} from "../../shared/types.gen";
import { fetchWorkOrderDetail, transitionWorkOrder } from "../../shared/ApiClient";
import { Link } from "../../shared/router";
import { formatAbsolute, formatRelative } from "../../shared/format";
import { useToast } from "../../components/Toast";
import { useAdminProject } from "./useAdminProject";
import { WorkOrderStateBadge } from "./badges";

// Owner-facing label + tone for each owner-authored transition (C22 authz
// table, owner-only rows). `approve` is NOT here — it routes through the
// dedicated /approve security gate on the recommendation card, never folded in.
const EVENT_META: Record<
  WorkOrderOwnerEventType,
  { label: string; danger?: boolean; needsOverrides?: boolean }
> = {
  accept: { label: "Accept result" },
  "request-changes": { label: "Request changes", needsOverrides: true },
  reject: { label: "Reject", danger: true },
  cancel: { label: "Cancel order", danger: true },
  retry: { label: "Retry" },
};

// FR-FBR-21 — work-order detail. The order's current state, the full append-only
// event ledger (the audit trail), and the owner transitions legal from the
// current state. On a `reported` order the owner can accept / request-changes /
// reject; request-changes carries a new authoritative owner_overrides delta
// (Q17). The ledger is the trust record — it is shown verbatim, untrusted
// `detail` rendered as escaped data.
export function WorkOrderDetail({ workOrderId }: { workOrderId: string }) {
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
  return (
    <WorkOrderDetailInner projectId={project.projectId} workOrderId={workOrderId} />
  );
}

function WorkOrderDetailInner({
  projectId,
  workOrderId,
}: {
  projectId: string;
  workOrderId: string;
}) {
  const query = useQuery({
    queryKey: ["autopilot-work-order", projectId, workOrderId],
    queryFn: () => fetchWorkOrderDetail(projectId, workOrderId),
  });
  const [pending, setPending] = useState<WorkOrderOwnerEventType | null>(null);

  if (query.isPending) {
    return (
      <main className="ap-page" aria-busy="true">
        <BackLink />
        <p className="muted">Loading work order…</p>
      </main>
    );
  }
  if (query.isError || !query.data) {
    return (
      <main className="ap-page">
        <BackLink />
        <div role="alert" className="error-block">
          Failed to load this work order.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      </main>
    );
  }

  const wo: WorkOrderDetailShape = query.data;
  const transitions = WORK_ORDER_OWNER_TRANSITIONS[wo.state] ?? [];
  const terminal = WORK_ORDER_TERMINAL_STATES.includes(wo.state);

  return (
    <main className="ap-page" aria-labelledby="ap-wo-title">
      <BackLink />
      <header className="ap-wo-header">
        <h1 id="ap-wo-title">{wo.title}</h1>
        <WorkOrderStateBadge state={wo.state} />
      </header>

      <section className="drawer-meta" aria-label="Work-order metadata">
        <dl>
          <dt>Action</dt>
          <dd>{ACTION_TYPE_LABELS[wo.action_type]}</dd>
          <dt>Autonomy rung</dt>
          <dd>{rungLabel(wo.autonomy_rung)}</dd>
          <dt>State</dt>
          <dd>{WORK_ORDER_STATE_LABELS[wo.state]}</dd>
          {wo.approved_at ? (
            <>
              <dt>Approved</dt>
              <dd>
                <time dateTime={wo.approved_at}>
                  {formatAbsolute(wo.approved_at)}
                </time>
                {wo.approved_by ? ` by ${wo.approved_by}` : null}
              </dd>
            </>
          ) : null}
          {wo.failure_reason ? (
            <>
              <dt>Failure</dt>
              <dd className="ap-wo-failure">{wo.failure_reason}</dd>
            </>
          ) : null}
        </dl>
      </section>

      <section aria-labelledby="ap-wo-instructions-label">
        <h2 id="ap-wo-instructions-label">Instructions</h2>
        {/* Authoritative order text — the owner_overrides already merged in by
            the server. Rendered as escaped data. */}
        <p className="ap-rec-text">{wo.instructions}</p>
      </section>

      <section aria-labelledby="ap-wo-ledger-label">
        <h2 id="ap-wo-ledger-label">Event ledger</h2>
        <p className="muted">
          Append-only audit trail. Every state change is recorded here in the
          same transaction it happens.
        </p>
        {wo.events.length === 0 ? (
          <p className="muted">No events yet.</p>
        ) : (
          <ol className="status-history ap-wo-ledger">
            {wo.events.map((ev) => (
              <li key={ev.id}>
                <span className="status-history-arrow">
                  {ev.from_state
                    ? `${WORK_ORDER_STATE_LABELS[ev.from_state]} → `
                    : ""}
                  {WORK_ORDER_STATE_LABELS[ev.to_state]}
                </span>
                <span className="muted">
                  {" "}
                  · {WORK_ORDER_EVENT_LABELS[ev.event_type] ?? ev.event_type} ·{" "}
                  {ev.actor}
                  {ev.actor_id ? ` (${ev.actor_id})` : ""} ·{" "}
                  <time dateTime={ev.at}>{formatRelative(ev.at)}</time>
                </span>
                {ev.detail ? (
                  <p className="reason-note ap-wo-event-detail">
                    {summarizeDetail(ev.detail)}
                  </p>
                ) : null}
              </li>
            ))}
          </ol>
        )}
      </section>

      <section aria-labelledby="ap-wo-actions-label" className="ap-wo-actions">
        <h2 id="ap-wo-actions-label">Owner actions</h2>
        {terminal ? (
          <p className="muted">
            This order is {WORK_ORDER_STATE_LABELS[wo.state].toLowerCase()} —
            terminal, no further actions.
          </p>
        ) : transitions.length === 0 ? (
          <p className="muted">
            Waiting on the agent — no owner action is available in this state.
          </p>
        ) : (
          <div role="group" aria-label="Owner actions" className="status-choices">
            {transitions.map((ev) => (
              <button
                key={ev}
                type="button"
                className={EVENT_META[ev].danger ? "ap-danger" : undefined}
                onClick={() => setPending(ev)}
              >
                {EVENT_META[ev].label}
              </button>
            ))}
          </div>
        )}
      </section>

      {pending ? (
        <TransitionDialog
          projectId={projectId}
          workOrderId={workOrderId}
          event={pending}
          wo={wo}
          onClose={() => setPending(null)}
        />
      ) : null}
    </main>
  );
}

// ─── Transition confirm dialog ──────────────────────────────────────────────

function TransitionDialog({
  projectId,
  workOrderId,
  event,
  wo,
  onClose,
}: {
  projectId: string;
  workOrderId: string;
  event: WorkOrderOwnerEventType;
  wo: WorkOrderDetailShape;
  onClose: () => void;
}) {
  const dialogId = useId();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const meta = EVENT_META[event];
  const [note, setNote] = useState("");
  const [title, setTitle] = useState(wo.title);
  const [instructions, setInstructions] = useState(wo.instructions);
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () => {
      const detail: Record<string, unknown> = {};
      if (note.trim()) detail.note = note.trim();
      if (meta.needsOverrides) {
        // Q17 — request-changes carries a NEW authoritative overrides delta.
        const overrides: OwnerOverrides = {};
        if (title.trim() && title !== wo.title) overrides.title = title.trim();
        if (instructions.trim() && instructions !== wo.instructions) {
          overrides.instructions = instructions.trim();
        }
        if (Object.keys(overrides).length > 0) detail.owner_overrides = overrides;
      }
      return transitionWorkOrder(projectId, workOrderId, {
        event_type: event,
        detail: Object.keys(detail).length > 0 ? detail : undefined,
      });
    },
    onSuccess: () => {
      notify(`${meta.label} recorded.`, "success");
      queryClient.invalidateQueries({
        queryKey: ["autopilot-work-order", projectId, workOrderId],
      });
      queryClient.invalidateQueries({
        queryKey: ["autopilot-work-orders", projectId],
      });
      onClose();
    },
    onError: () => setInlineError("Action failed. Please try again."),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    mutation.mutate();
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={`${dialogId}-title`}
      className="dialog dialog-overlay"
    >
      <form onSubmit={onSubmit} className="dialog-body">
        <h3 id={`${dialogId}-title`}>{meta.label}</h3>

        {meta.needsOverrides ? (
          <>
            <p className="muted">
              Requesting changes re-opens the order for another pass. Your edits
              are authoritative — they override the prior instructions.
            </p>
            <label htmlFor={`${dialogId}-title-input`}>Title</label>
            <input
              id={`${dialogId}-title-input`}
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              maxLength={200}
              autoFocus
            />
            <label htmlFor={`${dialogId}-instructions`}>Instructions</label>
            <textarea
              id={`${dialogId}-instructions`}
              value={instructions}
              onChange={(e) => setInstructions(e.target.value)}
              maxLength={16384}
              rows={6}
            />
          </>
        ) : null}

        <label htmlFor={`${dialogId}-note`}>Note (optional)</label>
        <textarea
          id={`${dialogId}-note`}
          value={note}
          onChange={(e) => setNote(e.target.value)}
          maxLength={2048}
          rows={3}
          autoFocus={!meta.needsOverrides}
        />

        {inlineError ? (
          <p role="alert" className="error">
            {inlineError}
          </p>
        ) : null}

        <div className="dialog-actions">
          <button type="button" onClick={onClose} disabled={mutation.isPending}>
            Cancel
          </button>
          <button
            type="submit"
            className={meta.danger ? "ap-danger" : "primary"}
            disabled={mutation.isPending}
          >
            {mutation.isPending ? "Working…" : meta.label}
          </button>
        </div>
      </form>
    </div>
  );
}

// Render an event's untrusted jsonb `detail` as a short, escaped summary —
// never a raw dump. Surfaces a note + whether authoritative overrides were
// attached, without echoing arbitrary nested content.
function summarizeDetail(detail: Record<string, unknown>): string {
  const parts: string[] = [];
  if (typeof detail.note === "string" && detail.note) parts.push(detail.note);
  if (detail.owner_overrides && typeof detail.owner_overrides === "object") {
    const keys = Object.keys(detail.owner_overrides as object);
    if (keys.length > 0) {
      parts.push(`(authoritative overrides: ${keys.join(", ")})`);
    }
  }
  return parts.length > 0 ? parts.join(" ") : "—";
}

function rungLabel(rung: number): string {
  if (rung >= 0 && rung <= 3) return AUTONOMY_RUNG_LABELS[rung as AutonomyRung];
  return `Rung ${rung}`;
}

function BackLink() {
  return (
    <Link to="/admin/autopilot/work-orders" className="ap-back-link">
      ← Back to work orders
    </Link>
  );
}
