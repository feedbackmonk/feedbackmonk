import { useId, useState, type FormEvent } from "react";
import axios from "axios";
import { useMutation } from "@tanstack/react-query";
import {
  type FeedbackKind,
  type FeedbackStatus,
  type PromoteErrorBody,
} from "../../shared/types.gen";
import { postPromoteFeedback } from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { useRouter } from "../../shared/router";
import { useTranslation } from "../../i18n";

interface PromoteButtonProps {
  feedbackId: string;
  kind: FeedbackKind;
  status: FeedbackStatus;
  bodyPreview: string;
}

// Conditional render — promote is offered ONLY for feature requests that
// haven't already been merged out (status !== "duplicate"). Hide entirely
// rather than disable: we don't want noise on bug/question feedback.
export function PromoteButton({
  feedbackId,
  kind,
  status,
  bodyPreview,
}: PromoteButtonProps) {
  if (kind !== "feature" || status === "duplicate") {
    return null;
  }
  return <PromoteButtonInner feedbackId={feedbackId} bodyPreview={bodyPreview} />;
}

function PromoteButtonInner({
  feedbackId,
  bodyPreview,
}: {
  feedbackId: string;
  bodyPreview: string;
}) {
  const { t } = useTranslation("admin");
  const dialogId = useId();
  const { notify } = useToast();
  const { navigate } = useRouter();
  const [open, setOpen] = useState(false);
  // Auto-suggested slug from a coarse client-side mirror of the server's
  // `slug_from_title` helper. The server re-validates so any UI/server
  // drift surfaces as a 400 InvalidSlug — not silent corruption.
  const [slug, setSlug] = useState(() => slugSuggest(bodyPreview));
  const [titleOverride, setTitleOverride] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: async () =>
      postPromoteFeedback(feedbackId, {
        slug: slug.trim(),
        title: titleOverride.trim() || undefined,
      }),
    onSuccess: (res) => {
      if (res.already_promoted) {
        notify(t("admin.promoteButton.alreadyPromoted"), "info");
      } else {
        notify(t("admin.promoteButton.promoted"), "success");
      }
      setOpen(false);
      // Admin route does NOT include the project segment (server resolves
      // sole-project-per-session per the existing /feedback pattern). The
      // public route at /public/projects/:projectId/roadmap is the only
      // project-segmented one. Documented in decisions.md as a self-
      // mediated UI-URL adaptation.
      navigate(
        `/admin/roadmap?highlight=${encodeURIComponent(res.roadmap_item_slug)}`,
      );
    },
    onError: (err) => {
      if (axios.isAxiosError(err) && err.response?.status === 400) {
        const body = err.response.data as PromoteErrorBody | undefined;
        setInlineError(messageForError(body?.error, t));
      } else {
        setInlineError(t("admin.promoteButton.errors.generic"));
      }
    },
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (!slug.trim()) {
      setInlineError(t("admin.promoteButton.errors.slugRequired"));
      return;
    }
    mutation.mutate();
  }

  return (
    <div className="promote-button-wrap">
      <button
        type="button"
        onClick={() => {
          setSlug(slugSuggest(bodyPreview));
          setTitleOverride("");
          setInlineError(null);
          setOpen(true);
        }}
        aria-haspopup="dialog"
        aria-expanded={open}
      >
        {t("admin.promoteButton.cta")}
      </button>

      {open ? (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby={`${dialogId}-title`}
          className="dialog dialog-overlay"
        >
          <form onSubmit={onSubmit} className="dialog-body">
            <h2 id={`${dialogId}-title`}>{t("admin.promoteButton.cta")}</h2>
            <p className="muted">
              {t("admin.promoteButton.explain", { feedbackId })}
            </p>

            <label htmlFor={`${dialogId}-slug`}>
              {t("admin.promoteButton.slugLabel")}
            </label>
            <input
              id={`${dialogId}-slug`}
              type="text"
              value={slug}
              onChange={(e) => setSlug(e.target.value)}
              required
              maxLength={80}
              autoFocus
            />

            <label htmlFor={`${dialogId}-title-input`}>
              {t("admin.promoteButton.titleLabel")}
            </label>
            <input
              id={`${dialogId}-title-input`}
              type="text"
              value={titleOverride}
              onChange={(e) => setTitleOverride(e.target.value)}
              maxLength={200}
              placeholder={t("admin.promoteButton.titlePlaceholder")}
            />

            {inlineError ? (
              <p role="alert" className="error">
                {inlineError}
              </p>
            ) : null}

            <div className="dialog-actions">
              <button
                type="button"
                onClick={() => setOpen(false)}
                disabled={mutation.isPending}
              >
                {t("admin.common.cancel")}
              </button>
              <button type="submit" disabled={mutation.isPending}>
                {mutation.isPending
                  ? t("admin.promoteButton.promoting")
                  : t("admin.promoteButton.promote")}
              </button>
            </div>
          </form>
        </div>
      ) : null}
    </div>
  );
}

// Coarse client-side mirror of the server's slug-from-title helper. Server
// is authoritative; this exists only to pre-fill the input. Replaces any
// non-alnum run with a single dash, lowercases, trims dashes, truncates 80.
export function slugSuggest(text: string): string {
  const lowered = text.toLowerCase();
  const dashed = lowered.replace(/[^a-z0-9]+/g, "-");
  const trimmed = dashed.replace(/^-+|-+$/g, "");
  return trimmed.slice(0, 80);
}

function messageForError(
  code: PromoteErrorBody["error"] | undefined,
  t: (key: string) => string,
): string {
  switch (code) {
    case "InvalidCategory":
      return t("admin.promoteButton.errors.invalidCategory");
    case "InvalidSlug":
      return t("admin.promoteButton.errors.invalidSlug");
    case "FeedbackNotFound":
      return t("admin.promoteButton.errors.notFound");
    case "SlugTaken":
      return t("admin.promoteButton.errors.slugTaken");
    case "InternalError":
      return t("admin.promoteButton.errors.internal");
    default:
      return t("admin.promoteButton.errors.rejected");
  }
}
