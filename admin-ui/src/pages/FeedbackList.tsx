import { useCallback, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  fetchFeedbackList,
  fetchSentimentTrend,
  searchFeedback,
} from "../shared/ApiClient";
import { type FeedbackStatus } from "../shared/types.gen";
import { StatusBadge } from "../components/StatusBadge";
import { SentimentBadge } from "../components/SentimentBadge";
import { SentimentTrendChart } from "../components/SentimentTrendChart";
import { SearchBox } from "../components/SearchBox";
import { useRouter, useSearchParams } from "../shared/router";
import { formatRelative } from "../shared/format";
import { highlightMatches } from "../shared/highlight";
import { useTranslation } from "../i18n";
import { useLabels } from "../i18n/useLabels";
import { useLocale } from "../i18n/useLocale";

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
  q: string;
}

function parseParams(p: URLSearchParams): ParsedParams {
  const status = p.get("status");
  const limit = Number(p.get("limit") ?? DEFAULT_LIMIT) || DEFAULT_LIMIT;
  const offset = Number(p.get("offset") ?? 0) || 0;
  const q = (p.get("q") ?? "").trim();
  const statusFilter =
    status && status !== "all" && STATUS_FILTERS.includes(status as FeedbackStatus)
      ? (status as FeedbackStatus)
      : undefined;
  return {
    status: statusFilter,
    limit,
    offset,
    statusKey: status ?? "all",
    q,
  };
}

export function FeedbackList() {
  const { t } = useTranslation("admin");
  const labels = useLabels();
  const { locale } = useLocale();
  const [params, setParams] = useSearchParams();
  const { navigate } = useRouter();
  const parsed = useMemo(() => parseParams(params), [params]);
  const [trendOpen, setTrendOpen] = useState(false);

  const searching = parsed.q.length > 0;

  // Satisfaction trend — lazily fetched only when the panel is expanded, so
  // the list page pays nothing for it by default. Sparse series; renders its
  // own empty state when there's no sentiment data.
  const trendQuery = useQuery({
    queryKey: ["admin-sentiment-trend"],
    queryFn: () => fetchSentimentTrend(),
    enabled: trendOpen,
  });

  const query = useQuery({
    queryKey: [
      "admin-feedback",
      {
        status: parsed.status,
        limit: parsed.limit,
        offset: parsed.offset,
        q: parsed.q,
      },
    ],
    // When a search query is present, hit the FTS endpoint (gap #3); otherwise
    // the status-filtered list. Both return the identical response shape and
    // both honour the status pill, so search + filter compose.
    queryFn: () =>
      searching
        ? searchFeedback({
            q: parsed.q,
            status: parsed.status,
            limit: parsed.limit,
            offset: parsed.offset,
          })
        : fetchFeedbackList({
            status: parsed.status,
            limit: parsed.limit,
            offset: parsed.offset,
          }),
    placeholderData: (prev) => prev,
  });

  const setQuery = useCallback(
    (next: string) => {
      const p = new URLSearchParams(params);
      if (next) p.set("q", next);
      else p.delete("q");
      p.delete("offset");
      setParams(p);
    },
    [params, setParams],
  );

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

  // Drops the query AND the status pill in one navigation (one history entry,
  // one refetch) — the escape hatch when both are narrowing the list.
  function resetAll() {
    const p = new URLSearchParams(params);
    p.delete("q");
    p.delete("status");
    p.delete("offset");
    setParams(p);
  }

  const items = query.data?.items ?? [];
  const total = query.data?.total ?? 0;
  const limit = parsed.limit;
  const offset = parsed.offset;
  const rangeStart = items.length === 0 ? 0 : offset + 1;
  const rangeEnd = offset + items.length;
  const statusLabel = parsed.status ? labels.status(parsed.status) : null;
  const narrowed = searching || parsed.status !== undefined;

  return (
    <main className="feedback-list-page">
      <header className="page-header">
        <h1>{t("admin.feedbackList.title")}</h1>
        <SearchBox value={parsed.q} onSearch={setQuery} />
      </header>

      <section className="satisfaction-panel">
        <button
          type="button"
          className="satisfaction-toggle"
          aria-expanded={trendOpen}
          aria-controls={trendOpen ? "satisfaction-trend" : undefined}
          onClick={() => setTrendOpen((v) => !v)}
        >
          <span aria-hidden="true">{trendOpen ? "▾" : "▸"}</span>{" "}
          {t("admin.feedbackList.satisfactionTrend")}
        </button>
        {trendOpen ? (
          <div id="satisfaction-trend" className="satisfaction-body">
            {trendQuery.isPending ? (
              <p className="muted">{t("admin.common.loading")}</p>
            ) : trendQuery.isError ? (
              <div role="alert" className="error-block">
                {t("admin.feedbackList.trendLoadError")}{" "}
                <button type="button" onClick={() => trendQuery.refetch()}>
                  {t("admin.common.retry")}
                </button>
              </div>
            ) : (
              <SentimentTrendChart data={trendQuery.data} />
            )}
          </div>
        ) : null}
      </section>

      <nav
        className="status-filters"
        aria-label={t("admin.feedbackList.filterByStatusAria")}
      >
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
              {key === "all" ? t("admin.feedbackList.allStatuses") : labels.status(key)}
            </button>
          );
        })}
      </nav>

      {narrowed && !query.isPending && !query.isError ? (
        <p
          className={`results-summary ${query.isPlaceholderData ? "muted" : ""}`}
        >
          <span>
            {searching && statusLabel
              ? t("admin.feedbackList.resultsSummaryBoth", {
                  count: total,
                  query: parsed.q,
                  status: statusLabel,
                })
              : searching
                ? t("admin.feedbackList.resultsSummarySearch", {
                    count: total,
                    query: parsed.q,
                  })
                : t("admin.feedbackList.resultsSummaryStatus", {
                    count: total,
                    status: statusLabel,
                  })}
          </span>
          {searching && statusLabel ? (
            <button type="button" className="link-button" onClick={resetAll}>
              {t("admin.feedbackList.resetAllAction")}
            </button>
          ) : null}
        </p>
      ) : null}

      {query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.feedbackList.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted">{t("admin.common.loading")}</p>
      ) : items.length === 0 ? (
        <div className="empty-state">
          <p>
            {searching && statusLabel
              ? t("admin.feedbackList.emptyStatusQuery", {
                  status: statusLabel.toLowerCase(),
                  query: parsed.q,
                })
              : searching
                ? t("admin.feedbackList.emptyQuery", { query: parsed.q })
                : t("admin.feedbackList.emptyFilter")}
          </p>
          <div className="empty-actions">
            {searching ? (
              <button type="button" onClick={() => setQuery("")}>
                {t("admin.feedbackList.clearSearchAction")}
              </button>
            ) : null}
            {parsed.status ? (
              <button type="button" onClick={() => setStatus("all")}>
                {t("admin.feedbackList.clearFilterAction")}
              </button>
            ) : null}
            {searching && parsed.status ? (
              <button type="button" onClick={resetAll}>
                {t("admin.feedbackList.resetAllAction")}
              </button>
            ) : null}
          </div>
        </div>
      ) : (
        <table className="feedback-table">
          <caption className="visually-hidden">
            {t("admin.feedbackList.tableCaption")}
          </caption>
          <thead>
            <tr>
              <th scope="col">{t("admin.feedbackList.columns.id")}</th>
              <th scope="col">{t("admin.feedbackList.columns.kind")}</th>
              <th scope="col">{t("admin.feedbackList.columns.status")}</th>
              <th scope="col">{t("admin.feedbackList.columns.sentiment")}</th>
              <th scope="col">{t("admin.feedbackList.columns.excerpt")}</th>
              <th scope="col">{t("admin.feedbackList.columns.submitted")}</th>
              <th scope="col">{t("admin.feedbackList.columns.from")}</th>
              <th scope="col">{t("admin.feedbackList.columns.replies")}</th>
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
                aria-label={t("admin.feedbackList.openRow", {
                  id: row.feedback_id,
                })}
              >
                <td className="mono">{row.feedback_id}</td>
                <td>
                  <span className={`kind-badge kind-${row.kind}`}>
                    {labels.kind(row.kind)}
                  </span>
                </td>
                <td>
                  <StatusBadge status={row.status} />
                </td>
                <td>
                  {row.sentiment ? (
                    <SentimentBadge sentiment={row.sentiment} />
                  ) : (
                    <span
                      className="muted"
                      aria-label={t("admin.feedbackList.noSentiment")}
                    >
                      —
                    </span>
                  )}
                </td>
                <td className="excerpt" dir="auto">
                  {searching
                    ? highlightMatches(row.body_excerpt, parsed.q)
                    : row.body_excerpt}
                </td>
                <td>
                  <time dateTime={row.submitted_at}>
                    {formatRelative(row.submitted_at, locale)}
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
          {t("admin.feedbackList.paginationRange", {
            start: rangeStart,
            end: rangeEnd,
            total,
          })}
        </span>
        <button
          type="button"
          onClick={() => setOffset(Math.max(0, offset - limit))}
          disabled={offset === 0 || query.isPending}
        >
          {t("admin.common.previous")}
        </button>
        <button
          type="button"
          onClick={() => setOffset(offset + limit)}
          disabled={offset + items.length >= total || query.isPending}
        >
          {t("admin.common.next")}
        </button>
      </footer>
    </main>
  );
}
