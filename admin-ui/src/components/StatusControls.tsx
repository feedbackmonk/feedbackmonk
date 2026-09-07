import {
  useId,
  useRef,
  useState,
  type FormEvent,
} from "react";
import axios from "axios";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  LEGAL_TRANSITIONS,
  type FeedbackStatus,
  type TransitionErrorBody,
} from "../shared/types.gen";
import { postTransition } from "../shared/ApiClient";
import { useToast } from "./Toast";
import { useTranslation } from "../i18n";
import { useLabels } from "../i18n/useLabels";

interface StatusControlsProps {
  feedbackId: string;
  currentStatus: FeedbackStatus;
}

// The state-machine rendering invariant lives here:
// `LEGAL_TRANSITIONS[currentStatus]` is the ONLY source of which transitions
// the UI offers. Backend 409 fallback (Contract C7 TransitionError) is
// belt-and-braces — illegal transitions are never reachable from this UI.
export function StatusControls({ feedbackId, currentStatus }: StatusControlsProps) {
  const { t } = useTranslation("admin");
  const labels = useLabels();
  // `?? []` is defence-in-depth, not dead code: a status string the backend
  // emits but this union doesn't know (the `wontfix` / `wont-fix` serde drift
  // fixed in feedbackmonk-core/src/status.rs) used to make this `undefined`
  // and white-screen the whole drawer on `.length`. An unknown status now
  // degrades to "no transitions offered".
  const choices = LEGAL_TRANSITIONS[currentStatus] ?? [];
  const [pendingTarget, setPendingTarget] = useState<FeedbackStatus | null>(null);
  const [reasonNote, setReasonNote] = useState("");
  const [duplicateOf, setDuplicateOf] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const dialogId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);

  const mutation = useMutation({
    mutationFn: async (target: FeedbackStatus) => {
      return postTransition(feedbackId, {
        to_status: target,
        reason_note: reasonNote.trim() || undefined,
        duplicate_of: target === "duplicate" ? duplicateOf.trim() : undefined,
      });
    },
    onSuccess: (_res, target) => {
      notify(
        t("admin.statusControls.transitionedTo", {
          status: labels.status(target),
        }),
        "success",
      );
      queryClient.invalidateQueries({
        queryKey: ["admin-feedback-detail", feedbackId],
      });
      queryClient.invalidateQueries({ queryKey: ["admin-feedback"] });
      closeDialog();
    },
    onError: (err) => {
      if (axios.isAxiosError(err) && err.response?.status === 409) {
        const body = err.response.data as TransitionErrorBody | undefined;
        setInlineError(messageForError(body?.error, t));
      } else {
        setInlineError(t("admin.statusControls.errors.generic"));
      }
    },
  });

  function openDialog(target: FeedbackStatus) {
    setPendingTarget(target);
    setReasonNote("");
    setDuplicateOf("");
    setInlineError(null);
    queueMicrotask(() => dialogRef.current?.focus());
  }

  function closeDialog() {
    setPendingTarget(null);
    setReasonNote("");
    setDuplicateOf("");
    setInlineError(null);
  }

  function onConfirm(e: FormEvent) {
    e.preventDefault();
    if (!pendingTarget) return;
    if (pendingTarget === "duplicate" && !duplicateOf.trim()) {
      setInlineError(t("admin.statusControls.errors.duplicateRequired"));
      return;
    }
    mutation.mutate(pendingTarget);
  }

  if (choices.length === 0) {
    return (
      <div className="status-controls status-controls-terminal">
        <p className="muted">{t("admin.statusControls.terminal")}</p>
      </div>
    );
  }

  return (
    <div className="status-controls">
      <h3>{t("admin.statusControls.heading")}</h3>
      <p className="muted">
        {t("admin.statusControls.fromTo", {
          status: labels.status(currentStatus),
        })}
      </p>
      <div
        className="status-choices"
        role="group"
        aria-label={t("admin.statusControls.heading")}
      >
        {choices.map((target) => (
          <button
            key={target}
            type="button"
            onClick={() => openDialog(target)}
            disabled={mutation.isPending}
          >
            {labels.status(target)}
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
              {t("admin.statusControls.transitionTo", {
                status: labels.status(pendingTarget),
              })}
            </h4>

            {pendingTarget === "duplicate" ? (
              <>
                <label htmlFor={`${dialogId}-dup`}>
                  {t("admin.statusControls.duplicateOfLabel")}
                </label>
                <input
                  id={`${dialogId}-dup`}
                  type="text"
                  value={duplicateOf}
                  onChange={(e) => setDuplicateOf(e.target.value)}
                  required
                  aria-required="true"
                  autoFocus
                />
              </>
            ) : null}

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
                  : t("admin.common.confirm")}
              </button>
            </div>
          </form>
        </div>
      ) : null}
    </div>
  );
}

function messageForError(
  code: TransitionErrorBody["error"] | undefined,
  t: (key: string) => string,
): string {
  switch (code) {
    case "IllegalTransition":
      return t("admin.statusControls.errors.illegalTransition");
    case "DuplicateRequiresTarget":
      return t("admin.statusControls.errors.duplicateRequired");
    case "DuplicateTargetMissing":
      return t("admin.statusControls.errors.duplicateTargetMissing");
    case "DuplicateSelfReference":
      return t("admin.statusControls.errors.duplicateSelfReference");
    default:
      return t("admin.statusControls.errors.transitionRejected");
  }
}
