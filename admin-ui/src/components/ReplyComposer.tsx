import { useId, useState, type FormEvent } from "react";
import axios from "axios";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { postReply } from "../shared/ApiClient";
import { useToast } from "./Toast";
import { useTranslation } from "../i18n";

export const REPLY_MIN = 1;
export const REPLY_MAX = 16384;

interface ReplyComposerProps {
  feedbackId: string;
}

// Plain-text only by design (P1 plan Deferred Decisions: no rich-text
// toolbar). Body length matches the backend's 1..16384 range so the UI
// rejects locally before the request hits Contract C7's validator.
export function ReplyComposer({ feedbackId }: ReplyComposerProps) {
  const { t } = useTranslation("admin");
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
        visibility === "public"
          ? t("admin.replyComposer.replySent")
          : t("admin.replyComposer.noteSaved"),
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
        setError(t("admin.replyComposer.errors.rejected"));
      } else {
        setError(t("admin.replyComposer.errors.generic"));
      }
    },
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (mutation.isPending) return;
    if (tooShort) {
      setError(t("admin.replyComposer.errors.empty"));
      return;
    }
    if (tooLong) {
      setError(t("admin.replyComposer.errors.tooLong", { max: REPLY_MAX }));
      return;
    }
    setError(null);
    mutation.mutate();
  }

  return (
    <form className="reply-composer" onSubmit={onSubmit}>
      <h3>{t("admin.replyComposer.heading")}</h3>

      <fieldset className="visibility-fieldset">
        <legend>{t("admin.replyComposer.visibilityLegend")}</legend>
        <label>
          <input
            type="radio"
            name={`${fieldId}-visibility`}
            value="public"
            checked={visibility === "public"}
            onChange={() => setVisibility("public")}
          />
          {t("admin.replyComposer.visibilityPublic")}
        </label>
        <label>
          <input
            type="radio"
            name={`${fieldId}-visibility`}
            value="internal"
            checked={visibility === "internal"}
            onChange={() => setVisibility("internal")}
          />
          {t("admin.replyComposer.visibilityInternal")}
        </label>
      </fieldset>

      <label htmlFor={`${fieldId}-body`}>{t("admin.replyComposer.bodyLabel")}</label>
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
          ? t("admin.replyComposer.sending")
          : visibility === "public"
            ? t("admin.replyComposer.sendReply")
            : t("admin.replyComposer.saveNote")}
      </button>
    </form>
  );
}
