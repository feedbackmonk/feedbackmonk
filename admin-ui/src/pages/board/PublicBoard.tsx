import axios from "axios";
import { useQuery } from "@tanstack/react-query";
import {
  KIND_LABELS,
  STATUS_LABELS,
  type BoardItem,
} from "../../shared/types.gen";
import { fetchPublicBoard } from "../../shared/ApiClient";
import { formatRelative } from "../../shared/format";

interface PublicBoardProps {
  projectId: string;
}

// Public-facing feedback board (Public Feedback Board + Moderation Gate,
// Contract C29). NO admin chrome — intentionally minimal so the page can be
// embedded under a customer's docs domain or linked from the widget, mirroring
// PublicRoadmap. End-users (no login) see APPROVED feedback only.
//
// PRIVACY (C29 / Q24 sibling): the payload carries NO submitter identity — the
// server never sends email/name/sub/anon-token/metadata/crash-id. There is
// nothing to anonymize here; this component must never reference such a field.
//
// Approved-only is a SERVER invariant (the board repo query hard-filters
// `moderation_status = 'approved'` in SQL — Worker A / C29 inv. 1). The client
// renders whatever the board endpoint returns; it does not re-filter.
//
// VOTING: deferred to a follow-up (Worker A Task Zero — new
// `feedback_board_votes` table). Stage 1 ships `vote_count` READ-ONLY (the
// server returns a hard `0`, mirroring how `reply_count` shipped in C8 Stage
// 1). No vote button is wired this stage. TODO(board-voting follow-up): add the
// vote/retract affordance once A exposes `POST/DELETE .../board/items/{code}/vote`.
export function PublicBoard({ projectId }: PublicBoardProps) {
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

  const items = listQuery.data?.items ?? [];

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
          Feedback we’ve published, with how many people have backed it.
        </p>
      </header>

      {items.length === 0 ? (
        <p className="muted">No feedback has been published yet.</p>
      ) : (
        <ol className="board-item-list">
          {items.map((it) => (
            <li key={it.short_code}>
              <BoardItemRow item={it} />
            </li>
          ))}
        </ol>
      )}
    </main>
  );
}

interface BoardItemRowProps {
  item: BoardItem;
}

function BoardItemRow({ item }: BoardItemRowProps) {
  const title = `${KIND_LABELS[item.kind]} · ${item.short_code}`;
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
        {/* Read-only vote count this stage (voting deferred — see file header). */}
        <span className="board-item-votes">
          {item.vote_count} {item.vote_count === 1 ? "vote" : "votes"}
        </span>
        <time className="muted" dateTime={item.accepted_at}>
          {formatRelative(item.accepted_at)}
        </time>
      </div>
    </article>
  );
}
