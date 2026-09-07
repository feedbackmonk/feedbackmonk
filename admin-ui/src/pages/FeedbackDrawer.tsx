import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchFeedbackDetail } from "../shared/ApiClient";
import { type FeedbackSubmitter } from "../shared/types.gen";
import { StatusBadge } from "../components/StatusBadge";
import { SentimentBadge } from "../components/SentimentBadge";
import { StatusControls } from "../components/StatusControls";
import { ReplyComposer } from "../components/ReplyComposer";
import { PromoteButton } from "./roadmap/PromoteButton";
import { formatAbsolute, formatRelative } from "../shared/format";
import { Trans } from "react-i18next";
import { useTranslation } from "../i18n";
import { useLabels } from "../i18n/useLabels";
import { useLocale } from "../i18n/useLocale";

interface FeedbackDrawerProps {
  feedbackId: string;
  onClose: () => void;
}

type SimpleT = (key: string, options?: Record<string, unknown>) => string;

function submitterLabel(s: FeedbackSubmitter, t: SimpleT): string {
  if (s.kind === "authenticated") {
    return s.name ?? s.email ?? s.sub ?? t("admin.feedbackDrawer.submitterAuthenticatedFallback");
  }
  return s.email
    ? t("admin.feedbackDrawer.submitterAnonymousWithEmail", { email: s.email })
    : t("admin.feedbackDrawer.submitterAnonymous");
}

export function FeedbackDrawer({ feedbackId, onClose }: FeedbackDrawerProps) {
  const { t } = useTranslation("admin");
  const labels = useLabels();
  const { locale } = useLocale();
  const [tab, setTab] = useState<"public" | "internal">("public");
  // FR-FBR-30 (#3): show the English translation in place of the verbatim
  // original. Off by default — the admin sees the original first (Q24/authenticity).
  const [showTranslated, setShowTranslated] = useState(false);
  const drawerRef = useRef<HTMLDivElement>(null);

  const query = useQuery({
    queryKey: ["admin-feedback-detail", feedbackId],
    queryFn: () => fetchFeedbackDetail(feedbackId),
  });

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    drawerRef.current?.focus();
    // Reset the translation toggle when switching to a different feedback.
    setShowTranslated(false);
  }, [feedbackId]);

  const detail = query.data;
  const replies = detail?.replies ?? [];
  const visibleReplies = replies.filter((r) => r.visibility === tab);

  return (
    <>
      <div
        className="drawer-scrim"
        onClick={onClose}
        aria-hidden="true"
      />
      <aside
        ref={drawerRef}
        className="drawer"
        role="dialog"
        aria-modal="true"
        aria-labelledby="drawer-title"
        tabIndex={-1}
      >
        <header className="drawer-header">
          <h2 id="drawer-title" className="mono">
            {feedbackId}
          </h2>
          <button
            type="button"
            className="drawer-close"
            onClick={onClose}
            aria-label={t("admin.feedbackDrawer.closeAria")}
          >
            ×
          </button>
        </header>

        {query.isPending ? (
          <p className="muted">{t("admin.common.loading")}</p>
        ) : query.isError || !detail ? (
          <div role="alert" className="error-block">
            {t("admin.feedbackDrawer.loadError")}{" "}
            <button type="button" onClick={() => query.refetch()}>
              {t("admin.common.retry")}
            </button>
          </div>
        ) : (
          <>
            <section className="drawer-meta">
              <dl>
                <dt>{t("admin.feedbackDrawer.fields.status")}</dt>
                <dd>
                  <StatusBadge status={detail.status} />
                </dd>
                <dt>{t("admin.feedbackDrawer.fields.kind")}</dt>
                <dd>{labels.kind(detail.kind)}</dd>
                <dt>{t("admin.feedbackDrawer.fields.sentiment")}</dt>
                <dd>
                  {detail.sentiment ? (
                    <SentimentBadge sentiment={detail.sentiment} />
                  ) : (
                    <span className="muted">—</span>
                  )}
                </dd>
                <dt>{t("admin.feedbackDrawer.fields.submitted")}</dt>
                <dd>
                  <time dateTime={detail.submitted_at}>
                    {formatAbsolute(detail.submitted_at, locale)} (
                    {formatRelative(detail.submitted_at, locale)})
                  </time>
                </dd>
                <dt>{t("admin.feedbackDrawer.fields.from")}</dt>
                <dd>{submitterLabel(detail.submitter, t)}</dd>
              </dl>
            </section>

            <section aria-labelledby="drawer-body-label">
              <h3 id="drawer-body-label">{t("admin.feedbackDrawer.bodyHeading")}</h3>
              {/*
                  Body is rendered as plain text. Submitter-provided content
                  MUST NOT pass through dangerouslySetInnerHTML — stored-XSS
                  defense per handoff doc Contract C8 invariant.

                  FR-FBR-30 (#3): when an English translation exists, offer a
                  toggle. The verbatim original is shown by default; the
                  translation is machine-generated and never replaces the stored
                  original (Q24). Untranslated rows show no toggle.
                */}
              {detail.body_translated ? (
                <div className="translation-toggle">
                  <button
                    type="button"
                    className="tab"
                    aria-pressed={showTranslated}
                    onClick={() => setShowTranslated((v) => !v)}
                  >
                    {showTranslated
                      ? t("admin.feedbackDrawer.showOriginal")
                      : detail.source_lang
                        ? t("admin.feedbackDrawer.showTranslationFrom", {
                            lang: detail.source_lang,
                          })
                        : t("admin.feedbackDrawer.showTranslation")}
                  </button>{" "}
                  {showTranslated ? (
                    <span className="muted">
                      {detail.source_lang
                        ? t("admin.feedbackDrawer.machineTranslationFrom", {
                            lang: detail.source_lang,
                          })
                        : t("admin.feedbackDrawer.machineTranslation")}
                    </span>
                  ) : null}
                </div>
              ) : null}
              <p
                className="feedback-body"
                lang={showTranslated ? "en" : undefined}
                dir="auto"
              >
                {showTranslated && detail.body_translated
                  ? detail.body_translated
                  : detail.body}
              </p>
            </section>

            <section aria-labelledby="drawer-history-label">
              <h3 id="drawer-history-label">{t("admin.feedbackDrawer.historyHeading")}</h3>
              {detail.status_history.length === 0 ? (
                <p className="muted">{t("admin.feedbackDrawer.noHistory")}</p>
              ) : (
                <ol className="status-history">
                  {detail.status_history.map((entry, i) => (
                    <li key={`${entry.transitioned_at}-${i}`}>
                      <span className="status-history-arrow">
                        {labels.status(entry.from_status)} →{" "}
                        {labels.status(entry.to_status)}
                      </span>
                      <span className="muted">
                        {" "}
                        <Trans
                          i18nKey="admin.feedbackDrawer.historyByAt"
                          t={t}
                          values={{
                            actor: entry.transitioned_by,
                            when: formatRelative(entry.transitioned_at, locale),
                          }}
                          components={{
                            time: <time dateTime={entry.transitioned_at} />,
                          }}
                        />
                      </span>
                      {entry.reason_note ? (
                        <p className="reason-note" dir="auto">{entry.reason_note}</p>
                      ) : null}
                      {entry.duplicate_of_feedback_id ? (
                        <p className="muted mono">
                          {t("admin.feedbackDrawer.duplicateOf", {
                            id: entry.duplicate_of_feedback_id,
                          })}
                        </p>
                      ) : null}
                    </li>
                  ))}
                </ol>
              )}
            </section>

            <section aria-labelledby="drawer-replies-label">
              <h3 id="drawer-replies-label">{t("admin.feedbackDrawer.repliesHeading")}</h3>
              <div
                role="tablist"
                aria-label={t("admin.feedbackDrawer.replyVisibilityAria")}
                className="tabs"
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={tab === "public"}
                  onClick={() => setTab("public")}
                  className={tab === "public" ? "tab tab-active" : "tab"}
                >
                  {t("admin.feedbackDrawer.publicTab", {
                    count: replies.filter((r) => r.visibility === "public").length,
                  })}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={tab === "internal"}
                  onClick={() => setTab("internal")}
                  className={tab === "internal" ? "tab tab-active" : "tab"}
                >
                  {t("admin.feedbackDrawer.internalTab", {
                    count: replies.filter((r) => r.visibility === "internal").length,
                  })}
                </button>
              </div>
              {visibleReplies.length === 0 ? (
                <p className="muted">
                  {tab === "public"
                    ? t("admin.feedbackDrawer.noPublicReplies")
                    : t("admin.feedbackDrawer.noInternalReplies")}
                </p>
              ) : (
                <ol className="reply-list">
                  {visibleReplies.map((r) => (
                    <li key={r.reply_id}>
                      <header className="reply-header">
                        <strong>{r.author}</strong>{" "}
                        <span className="muted">
                          <time dateTime={r.created_at}>
                            {formatRelative(r.created_at, locale)}
                          </time>
                        </span>
                      </header>
                      <p className="reply-body" dir="auto">{r.body}</p>
                    </li>
                  ))}
                </ol>
              )}
            </section>

            <ReplyComposer feedbackId={feedbackId} />
            <StatusControls
              feedbackId={feedbackId}
              currentStatus={detail.status}
            />
            <PromoteButton
              feedbackId={feedbackId}
              kind={detail.kind}
              status={detail.status}
              bodyPreview={detail.body}
            />
          </>
        )}
      </aside>
    </>
  );
}
