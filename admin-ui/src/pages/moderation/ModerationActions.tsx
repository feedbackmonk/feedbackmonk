import { useId, useRef, useState, type FormEvent } from "react";
import axios from "axios";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  LEGAL_MODERATION_TRANSITIONS,
  isModerationErrorBody,
  postModerate,
  type ModerationStatus,
} from "../../shared/boardModerationApi";
import { useToast } from "../../components/Toast";
import { useTranslation } from "../../i18n";
import { useAdminLabels } from "../../i18n/useAdminLabels";

interface ModerationActionsProps {
  feedbackId: string;
  currentStatus: ModerationStatus;
  /** Query key to invalidate on success (the queue list this row belongs to). */
  invalidateKey: unknown[];
}

// Per-row moderation controls. Same shape as `StatusControls` (the proven,
// axe-clean state-machine UI): `LEGAL_MODERATION_TRANSITIONS[currentStatus]` is
// the ONLY source of offered actions, so an illegal moderation transition is
// never reachable from the UI; the backend 409 (Contract C28) is belt-and-braces.
//
// The trust-boundary framing (FR-FBR-25a sibling): approving here is the
// deliberate owner action that makes a feedback row publicly visible. Reaching
// `approved` writes a `feedback_moderation_events` 'approve' row server-side
// (C28 inv. 1) — the ledger the `public-board-moderation-gate` oracle trusts.
export function ModerationActions({
  feedbackId,
  currentStatus,
  invalidateKey,
}: ModerationActionsProps) {
  const { t } = useTranslation("admin");
  const adminLabels = useAdminLabels();
  const choices = LEGAL_MODERATION_TRANSITIONS[currentStatus];
  const [pendingTarget, setPendingTarget] = useState<ModerationStatus | null>(
    null,
  );
  const [reasonNote, setReasonNote] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const dialogId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);

  const mutation = useMutation({
    mutationFn: (target: ModerationStatus) =>
      postModerate(feedbackId, {
        to_status: target,
        reason_note: reasonNote.trim() || undefined,
      }),
    onSuccess: (_res, target) => {
      notify(
        t("admin.moderationActions.movedTo", {
          feedbackId,
          status: adminLabels.moderationStatus(target),
        }),
        "success",
      );
      queryClient.invalidateQueries({ queryKey: invalidateKey });
      closeDialog();
    },
    onError: (err) => {
      if (
        axios.isAxiosError(err) &&
        err.response?.status === 409 &&
        isModerationErrorBody(err.response.data)
      ) {
        setInlineError(t("admin.moderationActions.errors.illegalTransition"));
      } else {
        setInlineError(t("admin.moderationActions.errors.generic"));
      }
    },
  });

  function openDialog(target: ModerationStatus) {
    setPendingTarget(target);
    setReasonNote("");
    setInlineError(null);
    queueMicrotask(() => dialogRef.current?.focus());
  }

  function closeDialog() {
    setPendingTarget(null);
    setReasonNote("");
    setInlineError(null);
  }

  function onConfirm(e: FormEvent) {
    e.preventDefault();
    if (!pendingTarget) return;
    mutation.mutate(pendingTarget);
  }

  if (choices.length === 0) return null;

  return (
    <div className="moderation-actions">
      <div
        className="moderation-choices"
        role="group"
        aria-label={t("admin.moderationActions.moderateAria", { feedbackId })}
      >
        {choices.map((target) => (
          <button
            key={target}
            type="button"
            className={`moderation-action moderation-action-${target}`}
            onClick={() => openDialog(target)}
            disabled={mutation.isPending}
          >
            {adminLabels.moderationStatus(target)}
          </button>
        ))}
      </div>

      {pendingTarget ? (
        <div
          ref={dialogRef}
          role="dialog"
          aria-modal="true"
          aria-labelledby={`${dialogId}-title`}
          tabIndex={-1}
          className="dialog"
        >
          <form onSubmit={onConfirm}>
            <h4 id={`${dialogId}-title`}>
              {t("admin.moderationActions.setTo", {
                feedbackId,
                status: adminLabels.moderationStatus(pendingTarget),
              })}
            </h4>
            <p className="muted">
              {pendingTarget === "approved"
                ? t("admin.moderationActions.explainApprove")
                : pendingTarget === "rejected"
                  ? t("admin.moderationActions.explainReject")
                  : t("admin.moderationActions.explainReset")}
            </p>

            <label htmlFor={`${dialogId}-reason`}>
              {t("admin.statusControls.reasonNoteLabel")}
            </label>
            <textarea
              id={`${dialogId}-reason`}
              value={reasonNote}
              onChange={(e) => setReasonNote(e.target.value)}
              maxLength={2048}
              rows={3}
            />

            {inlineError ? (
              <p role="alert" className="error">
                {inlineError}
              </p>
            ) : null}

            <div className="dialog-actions">
              <button
                type="button"
                onClick={closeDialog}
                disabled={mutation.isPending}
              >
                {t("admin.common.cancel")}
              </button>
              <button type="submit" disabled={mutation.isPending}>
                {mutation.isPending
                  ? t("admin.statusControls.submitting")
                  : t("admin.moderationActions.confirm", {
                      status: adminLabels.moderationStatus(pendingTarget),
                    })}
              </button>
            </div>
          </form>
        </div>
      ) : null}
    </div>
  );
}
