import { useId, useState, type FormEvent } from "react";
import axios from "axios";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { postReply } from "../shared/ApiClient";
import { useToast } from "./Toast";

export const REPLY_MIN = 1;
export const REPLY_MAX = 16384;

interface ReplyComposerProps {
  feedbackId: string;
}

// Plain-text only by design (P1 plan Deferred Decisions: no rich-text
// toolbar). Body length matches the backend's 1..16384 range so the UI
// rejects locally before the request hits Contract C7's validator.
export function ReplyComposer({ feedbackId }: ReplyComposerProps) {
  const [body, setBody] = useState("");
  const [visibility, setVisibility] = useState<"public" | "internal">("public");
  const [error, setError] = useState<string | null>(null);
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const fieldId = useId();

  const length = body.length;
  const tooShort = length < REPLY_MIN;
  const tooLong = length > REPLY_MAX;

  const mutation = useMutation({
    mutationFn: async () =>
      postReply(feedbackId, { body, visibility }),
    onSuccess: () => {
      notify(
        visibility === "public" ? "Reply sent" : "Internal note saved",
        "success",
      );
      queryClient.invalidateQueries({
        queryKey: ["admin-feedback-detail", feedbackId],
      });
      queryClient.invalidateQueries({ queryKey: ["admin-feedback"] });
      setBody("");
      setError(null);
    },
    onError: (err) => {
      if (axios.isAxiosError(err) && err.response?.status === 400) {
        setError("Reply rejected by the server (validation).");
      } else {
        setError("Failed to send reply. Please try again.");
      }
    },
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (mutation.isPending) return;
    if (tooShort) {
      setError("Reply cannot be empty.");
      return;
    }
    if (tooLong) {
      setError(`Reply exceeds ${REPLY_MAX} characters.`);
      return;
    }
    setError(null);
    mutation.mutate();
  }

  return (
    <form className="reply-composer" onSubmit={onSubmit}>
      <h3>Reply</h3>

      <fieldset className="visibility-fieldset">
        <legend>Visibility</legend>
        <label>
          <input
            type="radio"
            name={`${fieldId}-visibility`}
            value="public"
            checked={visibility === "public"}
            onChange={() => setVisibility("public")}
          />
          Public (emailed to submitter)
        </label>
        <label>
          <input
            type="radio"
            name={`${fieldId}-visibility`}
            value="internal"
            checked={visibility === "internal"}
            onChange={() => setVisibility("internal")}
          />
          Internal (note for tenant admins only)
        </label>
      </fieldset>

      <label htmlFor={`${fieldId}-body`}>Reply body</label>
      <textarea
        id={`${fieldId}-body`}
        value={body}
        onChange={(e) => setBody(e.target.value)}
        rows={6}
        maxLength={REPLY_MAX + 1 /* allow over-limit so validation message fires */}
        aria-invalid={tooLong || (error !== null && tooShort) ? true : undefined}
        aria-describedby={`${fieldId}-counter ${error ? `${fieldId}-error` : ""}`}
      />
      <div
        id={`${fieldId}-counter`}
        className={`char-counter ${tooLong ? "char-counter-over" : ""}`}
      >
        {length} / {REPLY_MAX}
      </div>

      {error ? (
        <p id={`${fieldId}-error`} role="alert" className="error">
          {error}
        </p>
      ) : null}

      <button type="submit" disabled={mutation.isPending || tooShort || tooLong}>
        {mutation.isPending
          ? "Sending…"
          : visibility === "public"
            ? "Send reply"
            : "Save internal note"}
      </button>
    </form>
  );
}
