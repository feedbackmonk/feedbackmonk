import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  fetchModerationQueue,
  MODERATION_STATUS_LABELS,
  type ModerationStatus,
} from "../../shared/boardModerationApi";
import { KIND_LABELS } from "../../shared/types.gen";
import { useSearchParams } from "../../shared/router";
import { formatRelative } from "../../shared/format";
import { ModerationActions } from "./ModerationActions";

// The three moderation states (migration 00016 / `feedbackmonk-core::moderation`).
// `pending` is the default queue view; `approved`/`rejected` are review filters.
const MODERATION_FILTERS: ModerationStatus[] = [
  "pending",
  "approved",
  "rejected",
];

const DEFAULT_LIMIT = 20;
const QUEUE_KEY = "admin-moderation-queue";

interface ParsedParams {
  status: ModerationStatus;
  limit: number;
  offset: number;
}

function parseParams(p: URLSearchParams): ParsedParams {
  const raw = p.get("status");
  const status: ModerationStatus =
    raw && (MODERATION_FILTERS as string[]).includes(raw)
      ? (raw as ModerationStatus)
      : "pending";
  const limit = Number(p.get("limit") ?? DEFAULT_LIMIT) || DEFAULT_LIMIT;
  const offset = Number(p.get("offset") ?? 0) || 0;
  return { status, limit, offset };
}

// /admin/moderation — the owner's moderation queue (Contract C28). Pending
// feedback awaiting an approve/reject decision; approving publishes the row to
// the public board. Mirrors the FeedbackList table + WorkOrderList chrome.
export function ModerationQueue() {
  const [params, setParams] = useSearchParams();
  const parsed = useMemo(() => parseParams(params), [params]);

  const query = useQuery({
    queryKey: [
      QUEUE_KEY,
      { status: parsed.status, limit: parsed.limit, offset: parsed.offset },
    ],
    queryFn: () =>
      fetchModerationQueue({
        status: parsed.status,
        limit: parsed.limit,
        offset: parsed.offset,
      }),
    placeholderData: (prev) => prev,
  });

  function setStatus(next: ModerationStatus) {
    const p = new URLSearchParams(params);
    if (next === "pending") p.delete("status");
    else p.set("status", next);
    p.delete("offset");
    setParams(p);
  }

  function setOffset(next: number) {
    const p = new URLSearchParams(params);
    if (next === 0) p.delete("offset");
    else p.set("offset", String(next));
    setParams(p);
  }

  const items = query.data?.items ?? [];
  const total = query.data?.total ?? 0;
  const { limit, offset } = parsed;
  const rangeStart = items.length === 0 ? 0 : offset + 1;
  const rangeEnd = offset + items.length;

  return (
    <main className="moderation-queue-page" aria-labelledby="moderation-title">
      <header className="page-header">
        <h1 id="moderation-title">Moderation</h1>
        <p className="muted">
          Approve feedback to publish it to your public board; reject to keep it
          private.
        </p>
      </header>

      <nav className="status-filters" aria-label="Filter by moderation status">
        {MODERATION_FILTERS.map((key) => {
          const active = parsed.status === key;
          return (
            <button
              key={key}
              type="button"
              className={`pill ${active ? "pill-active" : ""}`}
              aria-pressed={active}
              onClick={() => setStatus(key)}
            >
              {MODERATION_STATUS_LABELS[key]}
            </button>
          );
        })}
      </nav>

      {query.isError ? (
        <div role="alert" className="error-block">
          Failed to load the moderation queue.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          Loading…
        </p>
      ) : items.length === 0 ? (
        <div className="empty-state">
          <p>
            {parsed.status === "pending"
              ? "Nothing awaiting moderation. New feedback appears here for review."
              : `No ${MODERATION_STATUS_LABELS[parsed.status].toLowerCase()} feedback.`}
          </p>
        </div>
      ) : (
        <table className="feedback-table moderation-table">
          <caption className="visually-hidden">
            Feedback {MODERATION_STATUS_LABELS[parsed.status].toLowerCase()} for
            moderation, newest first.
          </caption>
          <thead>
            <tr>
              <th scope="col">ID</th>
              <th scope="col">Kind</th>
              <th scope="col">Excerpt</th>
              <th scope="col">From</th>
              <th scope="col">Submitted</th>
              <th scope="col">Actions</th>
            </tr>
          </thead>
          <tbody>
            {items.map((row) => (
              <tr key={row.feedback_id}>
                <td className="mono">{row.feedback_id}</td>
                <td>
                  <span className={`kind-badge kind-${row.kind}`}>
                    {KIND_LABELS[row.kind]}
                  </span>
                </td>
                {/*
                  Submitter-provided body rendered as plain text — never via
                  dangerouslySetInnerHTML (stored-XSS defense, same invariant as
                  FeedbackDrawer / Contract C8).
                */}
                <td className="excerpt">{row.body_excerpt}</td>
                <td>{row.submitter_label}</td>
                <td>
                  <time dateTime={row.submitted_at}>
                    {formatRelative(row.submitted_at)}
                  </time>
                </td>
                <td>
                  <ModerationActions
                    feedbackId={row.feedback_id}
                    currentStatus={row.moderation_status}
                    invalidateKey={[QUEUE_KEY]}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <footer className="pagination">
        <span aria-live="polite">
          {rangeStart}&ndash;{rangeEnd} of {total}
        </span>
        <button
          type="button"
          onClick={() => setOffset(Math.max(0, offset - limit))}
          disabled={offset === 0 || query.isPending}
        >
          Previous
        </button>
        <button
          type="button"
          onClick={() => setOffset(offset + limit)}
          disabled={offset + items.length >= total || query.isPending}
        >
          Next
        </button>
      </footer>
    </main>
  );
}
