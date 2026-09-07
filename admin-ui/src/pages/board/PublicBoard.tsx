import axios from "axios";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type BoardItem } from "../../shared/types.gen";
import {
  castBoardVote,
  fetchPublicBoard,
  retractBoardVote,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { formatRelative } from "../../shared/format";
import { useTranslation } from "../../i18n";
import { useLabels } from "../../i18n/useLabels";
import { useLocale } from "../../i18n/useLocale";
import { LanguageSwitcher } from "../../i18n/LanguageSwitcher";

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
//
// LOCALIZATION (FR-FBR-36): the CHROME comes from `i18n/locales/<code>/
// public.json`; the feedback BODY is rendered verbatim in whatever language it
// was written in and is never translated on a public surface (Q24 /
// DEC-FBR-15). Do not wrap `item.body` in anything.
export function PublicBoard({ projectId }: PublicBoardProps) {
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const { t } = useTranslation("public");
  const { locale } = useLocale();

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
          ? t("public.vote.alreadyVoted")
          : t("public.vote.failed");
      notify(msg, "error");
    },
  });

  const retractMutation = useMutation({
    mutationFn: async (shortCode: string) =>
      retractBoardVote(projectId, shortCode),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["public-board", projectId] });
      notify(t("public.vote.retracted"), "info");
    },
    onError: (err) => {
      const msg =
        axios.isAxiosError(err) && err.response?.status === 403
          ? t("public.vote.retractWindowClosed")
          : t("public.vote.retractFailed");
      notify(msg, "error");
    },
  });

  const items = listQuery.data?.items ?? [];
  const busy = voteMutation.isPending || retractMutation.isPending;

  if (listQuery.isPending) {
    return (
      <main className="public-board" aria-busy="true">
        <h1>{t("public.board.title")}</h1>
        <p>{t("public.common.loading")}</p>
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
          <h1 id="public-board-title">{t("public.board.title")}</h1>
          <LanguageSwitcher />
        </header>
        <p className="muted">{t("public.board.unavailable")}</p>
      </main>
    );
  }

  if (listQuery.isError) {
    return (
      <main className="public-board">
        <h1>{t("public.board.title")}</h1>
        <div role="alert" className="error-block">
          {t("public.board.loadError")}{" "}
          <button type="button" onClick={() => listQuery.refetch()}>
            {t("public.common.retry")}
          </button>
        </div>
      </main>
    );
  }

  return (
    <main className="public-board" aria-labelledby="public-board-title">
      <header>
        <h1 id="public-board-title">{t("public.board.title")}</h1>
        <p className="muted">{t("public.board.intro")}</p>
        <LanguageSwitcher />
      </header>

      {items.length === 0 ? (
        <p className="muted">{t("public.board.empty")}</p>
      ) : (
        <ol className="board-item-list">
          {items.map((it) => (
            <li key={it.short_code}>
              <BoardItemRow
                item={it}
                locale={locale}
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
  locale: string;
  onVote: () => void;
  onRetract: () => void;
  busy: boolean;
}

function BoardItemRow({
  item,
  locale,
  onVote,
  onRetract,
  busy,
}: BoardItemRowProps) {
  const { t } = useTranslation("public");
  const labels = useLabels();
  const statusLabel = labels.status(item.status);
  const title = t("public.board.itemTitle", {
    kind: labels.kind(item.kind),
    code: item.short_code,
  });
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
          aria-label={t("public.common.statusLabel", { status: statusLabel })}
        >
          {statusLabel}
        </span>
      </header>
      <p className="board-item-body" dir="auto">{item.body}</p>
      <div className="board-item-meta">
        <div className="board-item-actions">
          {item.voted_by_me ? (
            <button
              type="button"
              onClick={onRetract}
              disabled={busy}
              aria-pressed="true"
              aria-label={t("public.common.retractVoteAria", {
                votes: voteCount,
              })}
            >
              {t("public.common.voted", { votes: voteCount })}
            </button>
          ) : (
            <button
              type="button"
              onClick={onVote}
              disabled={busy}
              aria-pressed="false"
              aria-label={t("public.board.voteAria", {
                code: item.short_code,
                votes: voteCount,
              })}
            >
              {t("public.common.vote", { votes: voteCount })}
            </button>
          )}
        </div>
        <time className="muted" dateTime={item.accepted_at}>
          {formatRelative(item.accepted_at, locale)}
        </time>
      </div>
    </article>
  );
}
