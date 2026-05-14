import { useMemo } from "react";
import axios from "axios";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ROADMAP_STATUS_LABELS,
  ROADMAP_STATUS_PUBLIC_ORDER,
  type RoadmapItem,
  type RoadmapItemStatus,
} from "../../shared/types.gen";
import {
  fetchPublicRoadmap,
  fetchPublicTopVoted,
  postCastVote,
  deleteVote,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { formatRelative } from "../../shared/format";

interface PublicRoadmapProps {
  projectId: string;
}

// Public-facing roadmap page (FR-FBR-11 + FR-FBR-13). NO admin chrome —
// intentionally minimal so the page can be embedded under a customer's
// docs domain or linked from the feedbackmonk widget. Renders items
// grouped by status in the canonical public order (in-progress → planned
// → considering → shipped → wontfix).
export function PublicRoadmap({ projectId }: PublicRoadmapProps) {
  const queryClient = useQueryClient();
  const { notify } = useToast();

  const listQuery = useQuery({
    queryKey: ["public-roadmap", projectId],
    queryFn: () => fetchPublicRoadmap(projectId),
  });

  // Top-voted shortlist sits at the top; same 60s cache window as the
  // main list, so we render `cached_at` from whichever is fresher.
  const topQuery = useQuery({
    queryKey: ["public-roadmap-top", projectId],
    queryFn: () => fetchPublicTopVoted(projectId, 5),
  });

  const grouped = useMemo(() => {
    const g: Record<RoadmapItemStatus, RoadmapItem[]> = {
      considering: [],
      planned: [],
      "in-progress": [],
      shipped: [],
      wontfix: [],
    };
    for (const it of listQuery.data?.items ?? []) {
      g[it.status].push(it);
    }
    return g;
  }, [listQuery.data]);

  const cachedAt = listQuery.data?.cached_at ?? topQuery.data?.cached_at ?? null;

  const voteMutation = useMutation({
    mutationFn: async (slug: string) => postCastVote(projectId, slug),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["public-roadmap", projectId] });
      queryClient.invalidateQueries({
        queryKey: ["public-roadmap-top", projectId],
      });
    },
    onError: (err) => {
      const msg =
        axios.isAxiosError(err) && err.response?.status === 409
          ? "You've already voted on this item."
          : "Vote failed — please try again.";
      notify(msg, "error");
    },
  });

  const retractMutation = useMutation({
    mutationFn: async (slug: string) => deleteVote(projectId, slug),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["public-roadmap", projectId] });
      queryClient.invalidateQueries({
        queryKey: ["public-roadmap-top", projectId],
      });
      notify("Vote retracted.", "info");
    },
    onError: (err) => {
      const msg =
        axios.isAxiosError(err) && err.response?.status === 403
          ? "The 60s retract window has closed for this vote."
          : "Retract failed — please try again.";
      notify(msg, "error");
    },
  });

  if (listQuery.isPending) {
    return (
      <main className="public-roadmap" aria-busy="true">
        <h1>Roadmap</h1>
        <p>Loading…</p>
      </main>
    );
  }

  if (listQuery.isError) {
    return (
      <main className="public-roadmap">
        <h1>Roadmap</h1>
        <div role="alert" className="error-block">
          Failed to load roadmap.{" "}
          <button type="button" onClick={() => listQuery.refetch()}>
            Retry
          </button>
        </div>
      </main>
    );
  }

  return (
    <main className="public-roadmap" aria-labelledby="public-roadmap-title">
      <header>
        <h1 id="public-roadmap-title">Roadmap</h1>
        <p className="muted">
          Vote on what we should ship next. One vote per visitor per item.
        </p>
      </header>

      {topQuery.data && topQuery.data.items.length > 0 ? (
        <section aria-labelledby="public-roadmap-top-label">
          <h2 id="public-roadmap-top-label">Most-voted</h2>
          <ol className="roadmap-top-list">
            {topQuery.data.items.map((it) => (
              <li key={`top-${it.slug}`}>
                <article
                  className="roadmap-item roadmap-item-compact"
                  aria-labelledby={`top-${it.slug}-title`}
                >
                  <h3 id={`top-${it.slug}-title`}>{it.title}</h3>
                  <p className="muted">
                    {ROADMAP_STATUS_LABELS[it.status]} · {it.vote_count} votes
                  </p>
                </article>
              </li>
            ))}
          </ol>
        </section>
      ) : null}

      {ROADMAP_STATUS_PUBLIC_ORDER.map((status) => {
        const items = grouped[status];
        if (items.length === 0) return null;
        return (
          <section
            key={status}
            aria-labelledby={`public-roadmap-${status}-label`}
            className={`roadmap-section roadmap-section-${status}`}
          >
            <h2 id={`public-roadmap-${status}-label`}>
              {ROADMAP_STATUS_LABELS[status]}
            </h2>
            <ol className="roadmap-item-list">
              {items.map((it) => (
                <li key={it.slug}>
                  <RoadmapItemRow
                    item={it}
                    onVote={() => voteMutation.mutate(it.slug)}
                    onRetract={() => retractMutation.mutate(it.slug)}
                    busy={voteMutation.isPending || retractMutation.isPending}
                  />
                </li>
              ))}
            </ol>
          </section>
        );
      })}

      <footer
        className="public-roadmap-footer"
        aria-label="Roadmap freshness"
      >
        {cachedAt ? (
          <p className="muted">
            Vote counts updated{" "}
            <time dateTime={cachedAt}>{formatRelative(cachedAt)}</time>.
          </p>
        ) : (
          <p className="muted">Vote counts will refresh shortly.</p>
        )}
      </footer>
    </main>
  );
}

interface RoadmapItemRowProps {
  item: RoadmapItem;
  onVote: () => void;
  onRetract: () => void;
  busy: boolean;
}

function RoadmapItemRow({ item, onVote, onRetract, busy }: RoadmapItemRowProps) {
  const voteCount = item.vote_count;
  return (
    <article
      className="roadmap-item"
      aria-labelledby={`item-${item.slug}-title`}
    >
      <header className="roadmap-item-header">
        <h3 id={`item-${item.slug}-title`}>{item.title}</h3>
        <span
          className={`status-badge status-${item.status}`}
          aria-label={`Status: ${ROADMAP_STATUS_LABELS[item.status]}`}
        >
          {ROADMAP_STATUS_LABELS[item.status]}
        </span>
      </header>
      <p className="roadmap-item-body">{item.body}</p>
      <div className="roadmap-item-actions">
        {item.voted_by_me ? (
          <button
            type="button"
            onClick={onRetract}
            disabled={busy}
            aria-pressed="true"
            aria-label={`Retract vote — current count ${voteCount}`}
          >
            ★ Voted ({voteCount})
          </button>
        ) : (
          <button
            type="button"
            onClick={onVote}
            disabled={busy}
            aria-pressed="false"
            aria-label={`Vote for ${item.title} — current count ${voteCount}`}
          >
            ☆ Vote ({voteCount})
          </button>
        )}
      </div>
    </article>
  );
}
