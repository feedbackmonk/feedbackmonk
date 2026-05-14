import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchFeedbackList } from "../shared/ApiClient";
import {
  KIND_LABELS,
  STATUS_LABELS,
  type FeedbackStatus,
} from "../shared/types.gen";
import { StatusBadge } from "../components/StatusBadge";
import { useRouter, useSearchParams } from "../shared/router";
import { formatRelative } from "../shared/format";

const STATUS_FILTERS: (FeedbackStatus | "all")[] = [
  "all",
  "submitted",
  "triaged",
  "in-progress",
  "shipped",
  "wontfix",
  "duplicate",
];

const DEFAULT_LIMIT = 20;

interface ParsedParams {
  status: FeedbackStatus | undefined;
  limit: number;
  offset: number;
  statusKey: string;
}

function parseParams(p: URLSearchParams): ParsedParams {
  const status = p.get("status");
  const limit = Number(p.get("limit") ?? DEFAULT_LIMIT) || DEFAULT_LIMIT;
  const offset = Number(p.get("offset") ?? 0) || 0;
  const statusFilter =
    status && status !== "all" && STATUS_FILTERS.includes(status as FeedbackStatus)
      ? (status as FeedbackStatus)
      : undefined;
  return {
    status: statusFilter,
    limit,
    offset,
    statusKey: status ?? "all",
  };
}

export function FeedbackList() {
  const [params, setParams] = useSearchParams();
  const { navigate } = useRouter();
  const parsed = useMemo(() => parseParams(params), [params]);

  const query = useQuery({
    queryKey: [
      "admin-feedback",
      { status: parsed.status, limit: parsed.limit, offset: parsed.offset },
    ],
    queryFn: () =>
      fetchFeedbackList({
        status: parsed.status,
        limit: parsed.limit,
        offset: parsed.offset,
      }),
    placeholderData: (prev) => prev,
  });

  function setStatus(next: string) {
    const p = new URLSearchParams(params);
    if (next === "all") p.delete("status");
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
  const limit = parsed.limit;
  const offset = parsed.offset;
  const rangeStart = items.length === 0 ? 0 : offset + 1;
  const rangeEnd = offset + items.length;

  return (
    <main className="feedback-list-page">
      <header className="page-header">
        <h1>Feedback</h1>
      </header>

      <nav className="status-filters" aria-label="Filter by status">
        {STATUS_FILTERS.map((key) => {
          const active = parsed.statusKey === key;
          return (
            <button
              key={key}
              type="button"
              className={`pill ${active ? "pill-active" : ""}`}
              aria-pressed={active}
              onClick={() => setStatus(key)}
            >
              {key === "all" ? "All" : STATUS_LABELS[key]}
            </button>
          );
        })}
      </nav>

      {query.isError ? (
        <div role="alert" className="error-block">
          Failed to load feedback.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted">Loading…</p>
      ) : items.length === 0 ? (
        <div className="empty-state">
          <p>No feedback matches this filter.</p>
          {parsed.status ? (
            <button type="button" onClick={() => setStatus("all")}>
              Clear filter
            </button>
          ) : null}
        </div>
      ) : (
        <table className="feedback-table">
          <caption className="visually-hidden">
            Feedback items, sorted newest first.
          </caption>
          <thead>
            <tr>
              <th scope="col">ID</th>
              <th scope="col">Kind</th>
              <th scope="col">Status</th>
              <th scope="col">Excerpt</th>
              <th scope="col">Submitted</th>
              <th scope="col">From</th>
              <th scope="col">Replies</th>
            </tr>
          </thead>
          <tbody>
            {items.map((row) => (
              <tr
                key={row.feedback_id}
                tabIndex={0}
                onClick={() =>
                  navigate(`/feedback/${encodeURIComponent(row.feedback_id)}`)
                }
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    navigate(
                      `/feedback/${encodeURIComponent(row.feedback_id)}`,
                    );
                  }
                }}
                aria-label={`Open ${row.feedback_id}`}
              >
                <td className="mono">{row.feedback_id}</td>
                <td>
                  <span className={`kind-badge kind-${row.kind}`}>
                    {KIND_LABELS[row.kind]}
                  </span>
                </td>
                <td>
                  <StatusBadge status={row.status} />
                </td>
                <td className="excerpt">{row.body_excerpt}</td>
                <td>
                  <time dateTime={row.submitted_at}>
                    {formatRelative(row.submitted_at)}
                  </time>
                </td>
                <td>{row.submitter_label}</td>
                <td>{row.reply_count}</td>
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
