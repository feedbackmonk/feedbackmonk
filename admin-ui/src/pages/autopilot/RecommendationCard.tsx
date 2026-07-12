import { useId, useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  RECOMMENDATION_STATUS_LABELS,
  type AutonomyRung,
  type OwnerOverrides,
  type Recommendation,
} from "../../shared/types.gen";
import {
  approveWorkOrder,
  createWorkOrder,
  rejectRecommendation,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { ActionTypeBadge, ConfidenceMeter } from "./badges";
import { SourceRefList } from "./SourceRefList";
import { AutonomyRungDial } from "./AutonomyRungDial";

type Mode = null | "approve" | "tweak" | "reject";

// A recommendation card with the owner's approve / tweak / reject controls.
//
// THE security boundary (FR-FBR-25a): approval is an explicit, deliberate,
// non-default action. There is NO inline one-click approve and NO pre-checked
// box — every approval opens a confirm dialog the owner must actively submit.
// Approve and Tweak both follow the C22 create→approve flow:
//   POST /work-orders  (create draft from this recommendation, at a rung)
//   POST /work-orders/:id/approve  (the owner-authored `approved` event — the
//                                   ledger row C22 inv. 1 requires)
// Tweak additionally carries authoritative `owner_overrides` (Q17): the owner's
// edits WIN over the recommendation's fields at dispatch, and the recommendation
// is stamped `tweaked_approved`. Reject sets the recommendation to `rejected`
// without ever creating a work order.
//
// All recommendation text (title/body/rationale) is untrusted public-derived
// data and is rendered as escaped React text nodes — never as markup.
export function RecommendationCard({
  rec,
  projectId,
}: {
  rec: Recommendation;
  projectId: string;
}) {
  const [mode, setMode] = useState<Mode>(null);
  const actionable = rec.status === "proposed";

  return (
    <article className="ap-rec-card" aria-labelledby={`rec-${rec.id}-title`}>
      <header className="ap-rec-head">
        <h4 id={`rec-${rec.id}-title`} className="ap-rec-title">
          {rec.title}
        </h4>
        <div className="ap-rec-tags">
          <ActionTypeBadge actionType={rec.action_type} />
          <span className={`ap-rec-status ap-rec-status-${rec.status}`}>
            {RECOMMENDATION_STATUS_LABELS[rec.status]}
          </span>
        </div>
      </header>

      <ConfidenceMeter confidence={rec.confidence} />

      <section aria-label="Recommendation detail" className="ap-rec-body">
        {/* Untrusted analyst-derived text — plain escaped text only. */}
        <p className="ap-rec-text">{rec.body}</p>
        {rec.rationale ? (
          <>
            <h5 className="ap-rec-subhead">Rationale</h5>
            <p className="ap-rec-text muted">{rec.rationale}</p>
          </>
        ) : null}
      </section>

      <section aria-label="Source references" className="ap-rec-sources">
        <h5 className="ap-rec-subhead">Grounding evidence</h5>
        <SourceRefList refs={rec.source_refs} />
      </section>

      {actionable ? (
        <div
          className="ap-rec-actions"
          role="group"
          aria-label="Recommendation decision"
        >
          <button
            type="button"
            className="primary"
            onClick={() => setMode("approve")}
          >
            Approve…
          </button>
          <button type="button" onClick={() => setMode("tweak")}>
            Tweak…
          </button>
          <button
            type="button"
            className="ap-danger"
            onClick={() => setMode("reject")}
          >
            Reject…
          </button>
        </div>
      ) : (
        <p className="muted ap-rec-decided">
          Decision recorded: {RECOMMENDATION_STATUS_LABELS[rec.status]}.
        </p>
      )}

      {mode === "approve" || mode === "tweak" ? (
        <ApproveDialog
          rec={rec}
          projectId={projectId}
          tweak={mode === "tweak"}
          onClose={() => setMode(null)}
        />
      ) : null}
      {mode === "reject" ? (
        <RejectDialog
          rec={rec}
          projectId={projectId}
          onClose={() => setMode(null)}
        />
      ) : null}
    </article>
  );
}

// ─── Approve / Tweak dialog — the deliberate approval gate ──────────────────

function ApproveDialog({
  rec,
  projectId,
  tweak,
  onClose,
}: {
  rec: Recommendation;
  projectId: string;
  tweak: boolean;
  onClose: () => void;
}) {
  const dialogId = useId();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  // Rung 1 is the default — the safest rung that still produces a work order
  // (the owner signs every order; the rung only governs post-approval reach).
  const [rung, setRung] = useState<AutonomyRung>(1);
  const [title, setTitle] = useState(rec.title);
  const [instructions, setInstructions] = useState(rec.body);
  // C31 §4 — optional named-runner routing. Set/overridden at approve (the Q17
  // tweak surface); empty means first-claim-wins (any runner). Coordination
  // metadata, not a trust boundary — the approval signature is the security gate.
  const [routingLabel, setRoutingLabel] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: async () => {
      // Q17 — overrides are authoritative edits, sent only when the owner
      // actually changed a field (a no-op tweak stays a plain approve).
      let overrides: OwnerOverrides | undefined;
      if (tweak) {
        overrides = {};
        if (title.trim() && title !== rec.title) overrides.title = title.trim();
        if (instructions.trim() && instructions !== rec.body) {
          overrides.instructions = instructions.trim();
        }
        if (Object.keys(overrides).length === 0) overrides = undefined;
      }
      // Step 1: create the draft work order at the chosen rung.
      const draft = await createWorkOrder(projectId, {
        recommendation_id: rec.id,
        autonomy_rung: rung,
        owner_overrides: overrides,
      });
      // Step 2: the owner-authored approval — the security gate. This is the
      // `approved` ledger event C22 inv. 1 requires before any execution state.
      // C31 §4: the routing target is set at approve, sent only when the owner
      // actually named one (empty stays first-claim-wins, body unchanged).
      const label = routingLabel.trim();
      return approveWorkOrder(projectId, draft.id, {
        owner_overrides: overrides,
        ...(label ? { routing_label: label } : {}),
      });
    },
    onSuccess: () => {
      notify(
        tweak ? "Tweaked work order approved." : "Work order approved.",
        "success",
      );
      queryClient.invalidateQueries({
        queryKey: ["autopilot-cluster", projectId, rec.cluster_id],
      });
      queryClient.invalidateQueries({
        queryKey: ["autopilot-work-orders", projectId],
      });
      onClose();
    },
    onError: () =>
      setInlineError(
        "Could not create and approve the work order. Please try again.",
      ),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (tweak && !title.trim()) {
      setInlineError("A title is required.");
      return;
    }
    mutation.mutate();
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={`${dialogId}-title`}
      className="dialog dialog-overlay ap-approve-dialog"
    >
      <form onSubmit={onSubmit} className="dialog-body">
        <h3 id={`${dialogId}-title`}>
          {tweak ? "Tweak & approve" : "Approve work order"}
        </h3>
        <p className="muted">
          {tweak
            ? "Your edits are authoritative — they override the recommendation when the work order is dispatched."
            : "Approving creates a work order and records your owner approval. Nothing the agent does to your code can happen without this signature."}
        </p>

        {tweak ? (
          <>
            <label htmlFor={`${dialogId}-title-input`}>Title (authoritative)</label>
            <input
              id={`${dialogId}-title-input`}
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              required
              maxLength={200}
              autoFocus
            />
            <label htmlFor={`${dialogId}-instructions`}>
              Instructions (authoritative)
            </label>
            <textarea
              id={`${dialogId}-instructions`}
              value={instructions}
              onChange={(e) => setInstructions(e.target.value)}
              maxLength={16384}
              rows={6}
            />
          </>
        ) : (
          <section className="ap-approve-preview" aria-label="Recommendation">
            <strong>{rec.title}</strong>
            <p className="ap-rec-text">{rec.body}</p>
          </section>
        )}

        <AutonomyRungDial
          value={rung}
          onChange={setRung}
          disabled={mutation.isPending}
          idPrefix={dialogId}
        />

        <label htmlFor={`${dialogId}-routing`}>Routing label (optional)</label>
        <input
          id={`${dialogId}-routing`}
          type="text"
          value={routingLabel}
          onChange={(e) => setRoutingLabel(e.target.value)}
          maxLength={128}
          disabled={mutation.isPending}
          aria-describedby={`${dialogId}-routing-help`}
        />
        <p id={`${dialogId}-routing-help`} className="muted">
          Runner identity (token <code>sub</code>) that must execute this order.
          Leave empty for any runner.
        </p>

        {inlineError ? (
          <p role="alert" className="error">
            {inlineError}
          </p>
        ) : null}

        <div className="dialog-actions">
          <button type="button" onClick={onClose} disabled={mutation.isPending}>
            Cancel
          </button>
          {/* The deliberate approval submit — never auto-focused, never a
              default-checked convenience. */}
          <button type="submit" className="primary" disabled={mutation.isPending}>
            {mutation.isPending
              ? "Approving…"
              : tweak
                ? "Approve tweaked order"
                : "Approve & create work order"}
          </button>
        </div>
      </form>
    </div>
  );
}

// ─── Reject dialog ──────────────────────────────────────────────────────────

function RejectDialog({
  rec,
  projectId,
  onClose,
}: {
  rec: Recommendation;
  projectId: string;
  onClose: () => void;
}) {
  const dialogId = useId();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const [reason, setReason] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () =>
      rejectRecommendation(projectId, rec.id, reason.trim() || undefined),
    onSuccess: () => {
      notify("Recommendation rejected.", "success");
      queryClient.invalidateQueries({
        queryKey: ["autopilot-cluster", projectId, rec.cluster_id],
      });
      onClose();
    },
    onError: () => setInlineError("Could not reject. Please try again."),
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
        <h3 id={`${dialogId}-title`}>Reject recommendation</h3>
        <p className="muted">
          This marks the recommendation rejected. No work order is created and
          nothing runs.
        </p>
        <label htmlFor={`${dialogId}-reason`}>Reason (optional)</label>
        <textarea
          id={`${dialogId}-reason`}
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          maxLength={2048}
          rows={3}
          autoFocus
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
            className="ap-danger"
            disabled={mutation.isPending}
          >
            {mutation.isPending ? "Rejecting…" : "Reject recommendation"}
          </button>
        </div>
      </form>
    </div>
  );
}
