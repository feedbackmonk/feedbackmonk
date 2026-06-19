import axios from "axios";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  KIND_LABELS,
  STATUS_LABELS,
  type BoardItem,
} from "../../shared/types.gen";
import {
  castBoardVote,
  fetchPublicBoard,
  retractBoardVote,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { formatRelative } from "../../shared/format";

interface PublicBoardProps {
  projectId: string;
}

// Public-facing feedback board (Public Feedback Board + Moderation Gate,
// Contract C29 + C30 voting). NO admin chrome — intentionally minimal so the
// page can be embedded under a customer's docs domain or linked from the
// widget, mirroring PublicRoadmap. End-users (no login) see APPROVED feedback
// only.
//
// PRIVACY (C29 / Q24 sibling): the payload carries NO submitter identity — the
// server never sends email/name/sub/anon-token/metadata/crash-id. There is
// nothing to anonymize here; this component must never reference such a field.
//
// Approved-only is a SERVER invariant (the board repo query hard-filters
// `moderation_status = 'approved'` in SQL — Worker A / C29 inv. 1). The client
// renders whatever the board endpoint returns; it does not re-filter.
//
// VOTING (Contract C30, PF-BOARD-VOTING-01): `vote_count` is the real aggregate
// over `feedback_board_votes`, and the vote button POSTs/DELETEs
// `.../board/items/{short_code}/vote` (mirrors the roadmap vote button). The
// server enforces the moderation gate (D2): a vote on a non-approved /
// board-disabled item 404s, so the endpoint never confirms hidden feedback. The
// server does not currently echo a per-viewer `voted_by_me`, so the affordance
// renders as "Vote" and surfaces a friendly toast on the 409 (AlreadyVoted) —
// same shape as PublicRoadmap; the retract path is wired for when `voted_by_me`
// support lands.
export function PublicBoard({ projectId }: PublicBoardProps) {
  const queryClient = useQueryClient();
  const { notify } = useToast();

  const listQuery = useQuery({
    queryKey: ["public-board", projectId],
    queryFn: () => fetchPublicBoard(projectId),
    // A board-disabled project legitimately 404s (C29 inv. 2) — that's a
    // terminal "not available" state, not a transient failure, so don't retry.
    retry: (failureCount, err) => {
      if (axios.isAxiosError(err) && err.response?.status === 404) return false;
      return failureCount < 2;
    },
  });

  const voteMutation = useMutation({
    mutationFn: async (shortCode: string) => castBoardVote(projectId, shortCode),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["public-board", projectId] });
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
    mutationFn: async (shortCode: string) =>
      retractBoardVote(projectId, shortCode),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["public-board", projectId] });
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

  const items = listQuery.data?.items ?? [];
  const busy = voteMutation.isPending || retractMutation.isPending;

  if (listQuery.isPending) {
    return (
      <main className="public-board" aria-busy="true">
        <h1>Feedback board</h1>
        <p>Loading…</p>
      </main>
    );
  }

  // Board-disabled (or no such project) → clean unavailable state, NOT an
  // error block. C29 inv. 2: a project that hasn't opted into the public board
  // returns 404 and leaks no approved rows.
  if (
    listQuery.isError &&
    axios.isAxiosError(listQuery.error) &&
    listQuery.error.response?.status === 404
  ) {
    return (
      <main className="public-board" aria-labelledby="public-board-title">
        <header>
          <h1 id="public-board-title">Feedback board</h1>
        </header>
        <p className="muted">This feedback board isn’t available.</p>
      </main>
    );
  }

  if (listQuery.isError) {
    return (
      <main className="public-board">
        <h1>Feedback board</h1>
        <div role="alert" className="error-block">
          Failed to load the feedback board.{" "}
          <button type="button" onClick={() => listQuery.refetch()}>
            Retry
          </button>
        </div>
      </main>
    );
  }

  return (
    <main className="public-board" aria-labelledby="public-board-title">
      <header>
        <h1 id="public-board-title">Feedback board</h1>
        <p className="muted">
          Feedback we’ve published, with how many people have backed it. One vote
          per visitor per item.
        </p>
      </header>

      {items.length === 0 ? (
        <p className="muted">No feedback has been published yet.</p>
      ) : (
        <ol className="board-item-list">
          {items.map((it) => (
            <li key={it.short_code}>
              <BoardItemRow
                item={it}
                onVote={() => voteMutation.mutate(it.short_code)}
                onRetract={() => retractMutation.mutate(it.short_code)}
                busy={busy}
              />
            </li>
          ))}
        </ol>
      )}
    </main>
  );
}

interface BoardItemRowProps {
  item: BoardItem;
  onVote: () => void;
  onRetract: () => void;
  busy: boolean;
}

function BoardItemRow({ item, onVote, onRetract, busy }: BoardItemRowProps) {
  const title = `${KIND_LABELS[item.kind]} · ${item.short_code}`;
  const voteCount = item.vote_count;
  return (
    <article
      className="board-item"
      aria-labelledby={`board-${item.short_code}-title`}
    >
      <header className="board-item-header">
        <h2 id={`board-${item.short_code}-title`} className="board-item-title">
          {title}
        </h2>
        <span
          className={`status-badge status-${item.status}`}
          aria-label={`Status: ${STATUS_LABELS[item.status]}`}
        >
          {STATUS_LABELS[item.status]}
        </span>
      </header>
      <p className="board-item-body">{item.body}</p>
      <div className="board-item-meta">
        <div className="board-item-actions">
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
              aria-label={`Vote for ${item.short_code} — current count ${voteCount}`}
            >
              ☆ Vote ({voteCount})
            </button>
          )}
        </div>
        <time className="muted" dateTime={item.accepted_at}>
          {formatRelative(item.accepted_at)}
        </time>
      </div>
    </article>
  );
}
