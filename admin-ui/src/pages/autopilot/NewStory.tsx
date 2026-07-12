import { useId, useState, type FormEvent } from "react";
import axios from "axios";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  ACTION_TYPE_LABELS,
  type ActionType,
  type AutonomyRung,
  type CreateOwnerWorkOrderRequest,
} from "../../shared/types.gen";
import { createWorkOrder } from "../../shared/ApiClient";
import { Link, useRouter } from "../../shared/router";
import { useToast } from "../../components/Toast";
import { useAdminProject } from "./useAdminProject";
import { AutonomyRungDial } from "./AutonomyRungDial";

// Action types offered when an owner authors a story from scratch. `no_action`
// is excluded on purpose — like Rung 0, it means "no work order", which is not
// a thing you pick when you are deliberately creating one. The backend accepts
// any valid ActionType; the exclusion is a UX guardrail, not an authz control.
const SELECTABLE_ACTION_TYPES: ActionType[] = [
  "feature_implementation",
  "bug_fix",
  "enhancement",
  "investigation",
];

// /admin/autopilot/work-orders/new — owner-authored "New story" (Contract C31
// §3, owner-authored variant). Gives the owner a first-class path to author a
// work order NOT derived from a feedback recommendation.
//
// SECURITY BOUNDARY (FR-FBR-25a): this form only ever CREATES a `draft`. It
// NEVER approves — approval stays a separate, deliberate act on the work-order
// detail page. Nothing here is default-focused or default-checked on an
// approval-adjacent control (there is no approval control here at all).
//
// The owner-authored `title`/`instructions` are TRUSTED-layer input (owner
// authored, like `owner_overrides`) — but they are still rendered as escaped
// React text everywhere they surface downstream.
export function NewStory() {
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
  return <NewStoryInner projectId={project.projectId} />;
}

function NewStoryInner({ projectId }: { projectId: string }) {
  const fieldId = useId();
  const queryClient = useQueryClient();
  const { navigate } = useRouter();
  const { notify } = useToast();

  const [title, setTitle] = useState("");
  const [instructions, setInstructions] = useState("");
  const [actionType, setActionType] = useState<ActionType>(
    "feature_implementation",
  );
  // Rung 1 is the default — the safest rung that still produces a work order.
  // The owner signs every order at approval regardless of rung.
  const [rung, setRung] = useState<AutonomyRung>(1);
  const [routingLabel, setRoutingLabel] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () => {
      // Owner-authored variant of the C31 create union: the body has NO
      // `recommendation_id` key at all (the server discriminates structurally
      // — sending `recommendation_id: null` would be a mixed/incomplete body).
      const body: CreateOwnerWorkOrderRequest = {
        title: title.trim(),
        instructions: instructions.trim(),
        action_type: actionType,
        autonomy_rung: rung,
      };
      const trimmedLabel = routingLabel.trim();
      if (trimmedLabel) body.routing_label = trimmedLabel;
      return createWorkOrder(projectId, body);
    },
    onSuccess: (draft) => {
      notify("Work order drafted.", "success");
      queryClient.invalidateQueries({
        queryKey: ["autopilot-work-orders", projectId],
      });
      // Land on the created draft's detail page. Approval is a separate,
      // deliberate act there — this form never approves.
      navigate(`/admin/autopilot/work-orders/${encodeURIComponent(draft.id)}`);
    },
    onError: (err) => setInlineError(renderCreateError(err)),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (!title.trim()) {
      setInlineError("A title is required.");
      return;
    }
    if (!instructions.trim()) {
      setInlineError("Instructions are required.");
      return;
    }
    mutation.mutate();
  }

  return (
    <main className="ap-page" aria-labelledby="ap-new-story-title">
      <BackLink />
      <header className="page-header">
        <h1 id="ap-new-story-title">New story</h1>
        <p className="muted">
          Author a work order directly — not derived from feedback. It is
          created as a draft; nothing runs until you approve it on the next
          screen.
        </p>
      </header>

      <form onSubmit={onSubmit} aria-labelledby="ap-new-story-title">
        <label htmlFor={`${fieldId}-title`}>Title</label>
        <input
          id={`${fieldId}-title`}
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          required
          maxLength={512}
          autoFocus
        />

        <label htmlFor={`${fieldId}-instructions`}>Instructions</label>
        <textarea
          id={`${fieldId}-instructions`}
          value={instructions}
          onChange={(e) => setInstructions(e.target.value)}
          required
          maxLength={16384}
          rows={8}
        />

        <label htmlFor={`${fieldId}-action-type`}>Action type</label>
        <select
          id={`${fieldId}-action-type`}
          value={actionType}
          onChange={(e) => setActionType(e.target.value as ActionType)}
        >
          {SELECTABLE_ACTION_TYPES.map((at) => (
            <option key={at} value={at}>
              {ACTION_TYPE_LABELS[at]}
            </option>
          ))}
        </select>

        <AutonomyRungDial
          value={rung}
          onChange={setRung}
          disabled={mutation.isPending}
          idPrefix={fieldId}
        />

        <label htmlFor={`${fieldId}-routing-label`}>
          Routing label (optional)
        </label>
        <input
          id={`${fieldId}-routing-label`}
          type="text"
          value={routingLabel}
          onChange={(e) => setRoutingLabel(e.target.value)}
          maxLength={128}
          aria-describedby={`${fieldId}-routing-help`}
        />
        <p id={`${fieldId}-routing-help`} className="muted">
          Runner identity (token <code>sub</code>) that must execute this order.
          Leave empty for any runner.
        </p>

        {inlineError ? (
          <p role="alert" className="error">
            {inlineError}
          </p>
        ) : null}

        <div className="dialog-actions">
          <Link to="/admin/autopilot/work-orders" className="ap-back-link">
            Cancel
          </Link>
          <button type="submit" className="primary" disabled={mutation.isPending}>
            {mutation.isPending ? "Creating…" : "Create draft"}
          </button>
        </div>
      </form>
    </main>
  );
}

// Map a create failure to an inline message. The backend rejects a
// mixed/incomplete body with 400 (C31 §3 XOR-validation); a routing-label
// conflict surfaces as 409. Exact server shapes are announced by CLAUDE-A —
// until then, render defensively: prefer the server's `error` string if
// present, else a generic message keyed on status.
function renderCreateError(err: unknown): string {
  if (axios.isAxiosError(err)) {
    const status = err.response?.status;
    const data = err.response?.data;
    const serverError =
      data && typeof data === "object" && typeof (data as { error?: unknown }).error === "string"
        ? (data as { error: string }).error
        : null;
    if (status === 400) {
      return serverError
        ? `The work order was rejected: ${serverError}`
        : "The work order was rejected — check the title, instructions, and action type.";
    }
    if (status === 409) {
      return serverError
        ? `Routing conflict: ${serverError}`
        : "Routing conflict — that runner label could not be assigned.";
    }
  }
  return "Could not create the work order. Please try again.";
}

function BackLink() {
  return (
    <Link to="/admin/autopilot/work-orders" className="ap-back-link">
      ← Back to work orders
    </Link>
  );
}
