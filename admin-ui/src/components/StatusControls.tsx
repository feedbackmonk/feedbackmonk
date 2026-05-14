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
  STATUS_LABELS,
  type FeedbackStatus,
  type TransitionErrorBody,
} from "../shared/types.gen";
import { postTransition } from "../shared/ApiClient";
import { useToast } from "./Toast";

interface StatusControlsProps {
  feedbackId: string;
  currentStatus: FeedbackStatus;
}

// The state-machine rendering invariant lives here:
// `LEGAL_TRANSITIONS[currentStatus]` is the ONLY source of which transitions
// the UI offers. Backend 409 fallback (Contract C7 TransitionError) is
// belt-and-braces — illegal transitions are never reachable from this UI.
export function StatusControls({ feedbackId, currentStatus }: StatusControlsProps) {
  const choices = LEGAL_TRANSITIONS[currentStatus];
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
      notify(`Transitioned to ${STATUS_LABELS[target]}.`, "success");
      queryClient.invalidateQueries({
        queryKey: ["admin-feedback-detail", feedbackId],
      });
      queryClient.invalidateQueries({ queryKey: ["admin-feedback"] });
      closeDialog();
    },
    onError: (err) => {
      if (axios.isAxiosError(err) && err.response?.status === 409) {
        const body = err.response.data as TransitionErrorBody | undefined;
        setInlineError(messageForError(body?.error));
      } else {
        setInlineError("Transition failed. Please try again.");
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
      setInlineError("A duplicate target (FB-XXXXXX) is required.");
      return;
    }
    mutation.mutate(pendingTarget);
  }

  if (choices.length === 0) {
    return (
      <div className="status-controls status-controls-terminal">
        <p className="muted">Terminal — no further transitions.</p>
      </div>
    );
  }

  return (
    <div className="status-controls">
      <h3>Transition status</h3>
      <p className="muted">From {STATUS_LABELS[currentStatus]} to:</p>
      <div className="status-choices" role="group" aria-label="Transition status">
        {choices.map((target) => (
          <button
            key={target}
            type="button"
            onClick={() => openDialog(target)}
            disabled={mutation.isPending}
          >
            {STATUS_LABELS[target]}
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
              Transition to {STATUS_LABELS[pendingTarget]}
            </h4>

            {pendingTarget === "duplicate" ? (
              <>
                <label htmlFor={`${dialogId}-dup`}>
                  Duplicate of (required, e.g. FB-XXXXXX)
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
              Reason note (optional)
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
                Cancel
              </button>
              <button type="submit" disabled={mutation.isPending}>
                {mutation.isPending ? "Submitting…" : "Confirm"}
              </button>
            </div>
          </form>
        </div>
      ) : null}
    </div>
  );
}

function messageForError(code?: TransitionErrorBody["error"]): string {
  switch (code) {
    case "IllegalTransition":
      return "That transition is not allowed from the current status.";
    case "DuplicateRequiresTarget":
      return "A duplicate target is required.";
    case "DuplicateTargetMissing":
      return "The duplicate target was not found in this project.";
    case "DuplicateSelfReference":
      return "A feedback item cannot be a duplicate of itself.";
    default:
      return "Transition rejected. Please try again.";
  }
}
