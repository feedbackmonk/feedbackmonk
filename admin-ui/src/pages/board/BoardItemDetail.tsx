import axios from "axios";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  castBoardVote,
  clearNotNeeded,
  fetchBoardItemDetail,
  fetchMyPriorities,
  markNotNeeded,
  retractBoardVote,
  saveMyPriorities,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { formatAbsolute } from "../../shared/format";
import { Link } from "../../shared/router";
import { useTranslation } from "../../i18n";
import { useLabels } from "../../i18n/useLabels";
import { useLocale } from "../../i18n/useLocale";
import { LanguageSwitcher } from "../../i18n/LanguageSwitcher";
import {
  boardPath,
  isNotFound,
  needsSignIn,
  prioritiesPath,
  retryBoardQuery,
} from "./boardErrors";
import { SignInPrompt } from "./SignInPrompt";

// Board detail view (FR-FBR-57/58/59, Contract C43).
//
// Renders ONLY the C43 detail shape: the body verbatim in its submitted
// language (Q24 / DEC-FBR-15 — never translated, never wrapped), kind, status,
// vote count, the date it was accepted onto the board, the owner's board note
// (FR-FBR-58) and the status timeline — each entry a status and a date, never
// its note or who made it (FR-FBR-57). FR-FBR-59: no submitter identity, reply,
// internal note, attachment, translated body or locale is ever on the wire, and
// this page reads none.
//
// SIGNED-IN ACTIONS: a `viewer` block is present only for a signed-in viewer
// (FR-FBR-46). "I don't need this" (FR-FBR-48) and "add to my priorities"
// (FR-FBR-50) are offered only for an item in the ranking pool
// (`in_ranking_pool`, FR-FBR-51). Setting not-needed clears the viewer's
// upvote server-side in the same transaction; the page simply refetches.
export interface BoardItemDetailProps {
  projectId: string;
  shortCode: string;
}

export function BoardItemDetail({ projectId, shortCode }: BoardItemDetailProps) {
  const { t } = useTranslation("public");
  const labels = useLabels();
  const { locale } = useLocale();
  const { notify } = useToast();
  const queryClient = useQueryClient();

  const detailQuery = useQuery({
    queryKey: ["public-board", projectId, "item", shortCode],
    queryFn: () => fetchBoardItemDetail(projectId, shortCode),
    retry: retryBoardQuery,
  });

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["public-board", projectId] });

  const failed = (fallback: string) => (err: unknown) =>
    notify(needsSignIn(err) ? t("public.signIn.expired") : fallback, "error");

  const voteMutation = useMutation({
    mutationFn: (upvoted: boolean) =>
      upvoted
        ? retractBoardVote(projectId, shortCode)
        : castBoardVote(projectId, shortCode),
    onSuccess: () => void invalidate(),
    onError: (err) => {
      if (axios.isAxiosError(err) && err.response?.status === 409) {
        notify(t("public.vote.alreadyVoted"), "error");
      } else if (axios.isAxiosError(err) && err.response?.status === 403) {
        notify(t("public.vote.retractWindowClosed"), "error");
      } else {
        failed(t("public.vote.failed"))(err);
      }
    },
  });

  const notNeededMutation = useMutation({
    mutationFn: (set: boolean) =>
      set
        ? markNotNeeded(projectId, shortCode)
        : clearNotNeeded(projectId, shortCode),
    onSuccess: (res) => {
      void invalidate();
      notify(
        res.not_needed
          ? t("public.detail.notNeededSet")
          : t("public.detail.notNeededCleared"),
        "info",
      );
    },
    onError: failed(t("public.detail.saveFailed")),
  });

  // The list is saved whole (FR-FBR-50), so adding or removing one item reads
  // the current order first and writes it back with the one change.
  const priorityMutation = useMutation({
    mutationFn: async (add: boolean) => {
      const current = await fetchMyPriorities(projectId);
      const order = current.entries.map((e) => e.short_code);
      const next = add
        ? order.includes(shortCode)
          ? order
          : [...order, shortCode]
        : order.filter((c) => c !== shortCode);
      return saveMyPriorities(projectId, next);
    },
    onSuccess: (_res, add) => {
      void invalidate();
      notify(
        add
          ? t("public.detail.addedToPriorities")
          : t("public.detail.removedFromPriorities"),
        "info",
      );
    },
    onError: failed(t("public.detail.saveFailed")),
  });

  const pageTitle = t("public.board.title");

  if (detailQuery.isPending) {
    return (
      <main className="public-board" aria-busy="true">
        <h1>{pageTitle}</h1>
        <p>{t("public.common.loading")}</p>
      </main>
    );
  }

  if (detailQuery.isError && needsSignIn(detailQuery.error)) {
    return <SignInPrompt projectId={projectId} heading={pageTitle} showBoardLink />;
  }

  if (detailQuery.isError) {
    const notFound = isNotFound(detailQuery.error);
    return (
      <main className="public-board" aria-labelledby="board-detail-title">
        <header>
          <h1 id="board-detail-title">{pageTitle}</h1>
          <LanguageSwitcher />
        </header>
        {notFound ? (
          <p className="muted">{t("public.detail.notFound")}</p>
        ) : (
          <div role="alert" className="error-block">
            {t("public.detail.loadError")}{" "}
            <button type="button" onClick={() => detailQuery.refetch()}>
              {t("public.common.retry")}
            </button>
          </div>
        )}
        <p>
          <Link to={boardPath(projectId)}>{t("public.detail.backToBoard")}</Link>
        </p>
      </main>
    );
  }

  const item = detailQuery.data;
  const statusLabel = labels.status(item.status);
  const viewer = item.viewer;
  const upvoted = viewer?.upvoted ?? false;
  const inPriorities =
    viewer?.priority_position !== null && viewer?.priority_position !== undefined;
  const busy =
    voteMutation.isPending ||
    notNeededMutation.isPending ||
    priorityMutation.isPending;

  return (
    <main className="public-board" aria-labelledby="board-detail-title">
      <p>
        <Link to={boardPath(projectId)}>{t("public.detail.backToBoard")}</Link>
      </p>
      <header>
        <h1 id="board-detail-title">
          {t("public.board.itemTitle", {
            kind: labels.kind(item.kind),
            code: item.short_code,
          })}
        </h1>
        <LanguageSwitcher />
      </header>

      <article className="board-item" aria-labelledby="board-detail-title">
        <p className="board-item-body" dir="auto">{item.body}</p>

        <dl className="board-item-facts">
          <dt>{t("public.detail.kind")}</dt>
          <dd>{labels.kind(item.kind)}</dd>
          <dt>{t("public.detail.status")}</dt>
          <dd>
            <span
              className={`status-badge status-${item.status}`}
              aria-label={t("public.common.statusLabel", { status: statusLabel })}
            >
              {statusLabel}
            </span>
          </dd>
          <dt>{t("public.detail.votes")}</dt>
          <dd>{item.vote_count}</dd>
          <dt>{t("public.detail.accepted")}</dt>
          <dd>
            <time dateTime={item.accepted_at}>
              {formatAbsolute(item.accepted_at, locale)}
            </time>
          </dd>
        </dl>

        <div className="board-item-actions">
          <button
            type="button"
            onClick={() => voteMutation.mutate(upvoted)}
            disabled={busy}
            aria-pressed={upvoted ? "true" : "false"}
            aria-label={
              upvoted
                ? t("public.common.retractVoteAria", { votes: item.vote_count })
                : t("public.board.voteAria", {
                    code: item.short_code,
                    votes: item.vote_count,
                  })
            }
          >
            {upvoted
              ? t("public.common.voted", { votes: item.vote_count })
              : t("public.common.vote", { votes: item.vote_count })}
          </button>
        </div>
      </article>

      {item.board_note ? (
        <section aria-labelledby="board-detail-note">
          <h2 id="board-detail-note">{t("public.detail.noteTitle")}</h2>
          <p dir="auto" style={{ whiteSpace: "pre-wrap" }}>
            {item.board_note.body}
          </p>
          <p className="muted">
            {t("public.detail.noteUpdated", {
              when: formatAbsolute(item.board_note.updated_at, locale),
            })}
          </p>
        </section>
      ) : null}

      <section aria-labelledby="board-detail-timeline">
        <h2 id="board-detail-timeline">{t("public.detail.timelineTitle")}</h2>
        {item.timeline.length === 0 ? (
          <p className="muted">{t("public.detail.timelineEmpty")}</p>
        ) : (
          <ol className="board-item-timeline">
            {item.timeline.map((entry, i) => (
              <li key={`${entry.status}-${entry.at}-${i}`}>
                {labels.status(entry.status)}{" "}
                <time className="muted" dateTime={entry.at}>
                  {formatAbsolute(entry.at, locale)}
                </time>
              </li>
            ))}
          </ol>
        )}
      </section>

      {viewer && item.in_ranking_pool ? (
        <section aria-labelledby="board-detail-actions">
          <h2 id="board-detail-actions">{t("public.detail.actionsTitle")}</h2>
          <div
            className="board-item-actions"
            style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem" }}
          >
            <button
              type="button"
              onClick={() => notNeededMutation.mutate(!viewer.not_needed)}
              disabled={busy}
              aria-pressed={viewer.not_needed ? "true" : "false"}
            >
              {t("public.detail.notNeeded")}
            </button>
            <button
              type="button"
              onClick={() => priorityMutation.mutate(!inPriorities)}
              disabled={busy}
            >
              {inPriorities
                ? t("public.detail.removeFromPriorities")
                : t("public.detail.addToPriorities")}
            </button>
          </div>
          {inPriorities ? (
            <p className="muted">
              {t("public.detail.inPriorities", {
                position: (viewer.priority_position ?? 0) + 1,
              })}
            </p>
          ) : null}
          <p>
            <Link to={prioritiesPath(projectId)}>{t("public.board.myPriorities")}</Link>
          </p>
        </section>
      ) : null}
    </main>
  );
}
