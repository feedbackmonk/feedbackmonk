import { useId, useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
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
import { Trans } from "react-i18next";
import { useTranslation } from "../../i18n";
import { useAdminLabels } from "../../i18n/useAdminLabels";

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
  const { t } = useTranslation("admin");
  const adminLabels = useAdminLabels();
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
            {adminLabels.recommendationStatus(rec.status)}
          </span>
        </div>
      </header>

      <ConfidenceMeter confidence={rec.confidence} />

      <section
        aria-label={t("admin.recommendationCard.detailAria")}
        className="ap-rec-body"
      >
        {/* Untrusted analyst-derived text — plain escaped text only. */}
        <p className="ap-rec-text" dir="auto">{rec.body}</p>
        {rec.rationale ? (
          <>
            <h5 className="ap-rec-subhead">
              {t("admin.recommendationCard.rationaleHeading")}
            </h5>
            <p className="ap-rec-text muted">{rec.rationale}</p>
          </>
        ) : null}
      </section>

      <section
        aria-label={t("admin.recommendationCard.sourcesAria")}
        className="ap-rec-sources"
      >
        <h5 className="ap-rec-subhead">
          {t("admin.recommendationCard.groundingHeading")}
        </h5>
        <SourceRefList refs={rec.source_refs} />
      </section>

      {actionable ? (
        <div
          className="ap-rec-actions"
          role="group"
          aria-label={t("admin.recommendationCard.decisionAria")}
        >
          <button
            type="button"
            className="primary"
            onClick={() => setMode("approve")}
          >
            {t("admin.recommendationCard.approveAction")}
          </button>
          <button type="button" onClick={() => setMode("tweak")}>
            {t("admin.recommendationCard.tweakAction")}
          </button>
          <button
            type="button"
            className="ap-danger"
            onClick={() => setMode("reject")}
          >
            {t("admin.recommendationCard.rejectAction")}
          </button>
        </div>
      ) : (
        <p className="muted ap-rec-decided">
          {t("admin.recommendationCard.decisionRecorded", {
            status: adminLabels.recommendationStatus(rec.status),
          })}
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
  const { t } = useTranslation("admin");
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
        tweak
          ? t("admin.recommendationCard.approveDialog.tweakedApproved")
          : t("admin.recommendationCard.approveDialog.approved"),
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
      setInlineError(t("admin.recommendationCard.approveDialog.errorGeneric")),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (tweak && !title.trim()) {
      setInlineError(t("admin.newStory.errors.titleRequired"));
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
          {tweak
            ? t("admin.recommendationCard.approveDialog.tweakHeading")
            : t("admin.recommendationCard.approveDialog.approveHeading")}
        </h3>
        <p className="muted">
          {tweak
            ? t("admin.recommendationCard.approveDialog.tweakExplain")
            : t("admin.recommendationCard.approveDialog.approveExplain")}
        </p>

        {tweak ? (
          <>
            <label htmlFor={`${dialogId}-title-input`}>
              {t("admin.recommendationCard.approveDialog.titleLabel")}
            </label>
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
              {t("admin.recommendationCard.approveDialog.instructionsLabel")}
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
          <section
            className="ap-approve-preview"
            aria-label={t("admin.recommendationCard.approveDialog.previewAria")}
          >
            <strong>{rec.title}</strong>
            <p className="ap-rec-text" dir="auto">{rec.body}</p>
          </section>
        )}

        <AutonomyRungDial
          value={rung}
          onChange={setRung}
          disabled={mutation.isPending}
          idPrefix={dialogId}
        />

        <label htmlFor={`${dialogId}-routing`}>
          {t("admin.newStory.routingLabelLabel")}
        </label>
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
          <Trans
            i18nKey="admin.common.routingHelp"
            t={t}
            components={{ code: <code /> }}
          />
        </p>

        {inlineError ? (
          <p role="alert" className="error">
            {inlineError}
          </p>
        ) : null}

        <div className="dialog-actions">
          <button type="button" onClick={onClose} disabled={mutation.isPending}>
            {t("admin.common.cancel")}
          </button>
          {/* The deliberate approval submit — never auto-focused, never a
              default-checked convenience. */}
          <button type="submit" className="primary" disabled={mutation.isPending}>
            {mutation.isPending
              ? t("admin.recommendationCard.approveDialog.approving")
              : tweak
                ? t("admin.recommendationCard.approveDialog.submitTweak")
                : t("admin.recommendationCard.approveDialog.submitApprove")}
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
  const { t } = useTranslation("admin");
  const dialogId = useId();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const [reason, setReason] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () =>
      rejectRecommendation(projectId, rec.id, reason.trim() || undefined),
    onSuccess: () => {
      notify(t("admin.recommendationCard.rejectDialog.rejected"), "success");
      queryClient.invalidateQueries({
        queryKey: ["autopilot-cluster", projectId, rec.cluster_id],
      });
      onClose();
    },
    onError: () =>
      setInlineError(t("admin.recommendationCard.rejectDialog.errorGeneric")),
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
        <h3 id={`${dialogId}-title`}>
          {t("admin.recommendationCard.rejectDialog.heading")}
        </h3>
        <p className="muted">
          {t("admin.recommendationCard.rejectDialog.explain")}
        </p>
        <label htmlFor={`${dialogId}-reason`}>
          {t("admin.recommendationCard.rejectDialog.reasonLabel")}
        </label>
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
            {t("admin.common.cancel")}
          </button>
          <button
            type="submit"
            className="ap-danger"
            disabled={mutation.isPending}
          >
            {mutation.isPending
              ? t("admin.recommendationCard.rejectDialog.rejecting")
              : t("admin.recommendationCard.rejectDialog.submit")}
          </button>
        </div>
      </form>
    </div>
  );
}
