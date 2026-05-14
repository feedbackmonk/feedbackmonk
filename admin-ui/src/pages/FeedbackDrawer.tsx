import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchFeedbackDetail } from "../shared/ApiClient";
import {
  KIND_LABELS,
  STATUS_LABELS,
  type FeedbackSubmitter,
} from "../shared/types.gen";
import { StatusBadge } from "../components/StatusBadge";
import { StatusControls } from "../components/StatusControls";
import { ReplyComposer } from "../components/ReplyComposer";
import { PromoteButton } from "./roadmap/PromoteButton";
import { formatAbsolute, formatRelative } from "../shared/format";

interface FeedbackDrawerProps {
  feedbackId: string;
  onClose: () => void;
}

function submitterLabel(s: FeedbackSubmitter): string {
  if (s.kind === "authenticated") {
    return s.name ?? s.email ?? s.sub ?? "(authenticated user)";
  }
  return s.email ? `anonymous (${s.email})` : "anonymous";
}

export function FeedbackDrawer({ feedbackId, onClose }: FeedbackDrawerProps) {
  const [tab, setTab] = useState<"public" | "internal">("public");
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
            aria-label="Close drawer"
          >
            ×
          </button>
        </header>

        {query.isPending ? (
          <p className="muted">Loading…</p>
        ) : query.isError || !detail ? (
          <div role="alert" className="error-block">
            Failed to load feedback.{" "}
            <button type="button" onClick={() => query.refetch()}>
              Retry
            </button>
          </div>
        ) : (
          <>
            <section className="drawer-meta">
              <dl>
                <dt>Status</dt>
                <dd>
                  <StatusBadge status={detail.status} />
                </dd>
                <dt>Kind</dt>
                <dd>{KIND_LABELS[detail.kind]}</dd>
                <dt>Submitted</dt>
                <dd>
                  <time dateTime={detail.submitted_at}>
                    {formatAbsolute(detail.submitted_at)} (
                    {formatRelative(detail.submitted_at)})
                  </time>
                </dd>
                <dt>From</dt>
                <dd>{submitterLabel(detail.submitter)}</dd>
              </dl>
            </section>

            <section aria-labelledby="drawer-body-label">
              <h3 id="drawer-body-label">Body</h3>
              {/*
                  Body is rendered as plain text. Submitter-provided content
                  MUST NOT pass through dangerouslySetInnerHTML — stored-XSS
                  defense per handoff doc Contract C8 invariant.
                */}
              <p className="feedback-body">{detail.body}</p>
            </section>

            <section aria-labelledby="drawer-history-label">
              <h3 id="drawer-history-label">Status history</h3>
              {detail.status_history.length === 0 ? (
                <p className="muted">No status changes yet.</p>
              ) : (
                <ol className="status-history">
                  {detail.status_history.map((entry, i) => (
                    <li key={`${entry.transitioned_at}-${i}`}>
                      <span className="status-history-arrow">
                        {STATUS_LABELS[entry.from_status]} →{" "}
                        {STATUS_LABELS[entry.to_status]}
                      </span>
                      <span className="muted">
                        {" "}
                        by {entry.transitioned_by} ·{" "}
                        <time dateTime={entry.transitioned_at}>
                          {formatRelative(entry.transitioned_at)}
                        </time>
                      </span>
                      {entry.reason_note ? (
                        <p className="reason-note">{entry.reason_note}</p>
                      ) : null}
                      {entry.duplicate_of_feedback_id ? (
                        <p className="muted mono">
                          duplicate of {entry.duplicate_of_feedback_id}
                        </p>
                      ) : null}
                    </li>
                  ))}
                </ol>
              )}
            </section>

            <section aria-labelledby="drawer-replies-label">
              <h3 id="drawer-replies-label">Replies</h3>
              <div role="tablist" aria-label="Reply visibility" className="tabs">
                <button
                  type="button"
                  role="tab"
                  aria-selected={tab === "public"}
                  onClick={() => setTab("public")}
                  className={tab === "public" ? "tab tab-active" : "tab"}
                >
                  Public ({replies.filter((r) => r.visibility === "public").length})
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={tab === "internal"}
                  onClick={() => setTab("internal")}
                  className={tab === "internal" ? "tab tab-active" : "tab"}
                >
                  Internal (
                  {replies.filter((r) => r.visibility === "internal").length})
                </button>
              </div>
              {visibleReplies.length === 0 ? (
                <p className="muted">No {tab} replies yet.</p>
              ) : (
                <ol className="reply-list">
                  {visibleReplies.map((r) => (
                    <li key={r.reply_id}>
                      <header className="reply-header">
                        <strong>{r.author}</strong>{" "}
                        <span className="muted">
                          <time dateTime={r.created_at}>
                            {formatRelative(r.created_at)}
                          </time>
                        </span>
                      </header>
                      <p className="reply-body">{r.body}</p>
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
