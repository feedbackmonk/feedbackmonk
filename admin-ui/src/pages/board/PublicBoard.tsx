import { useId, useState } from "react";
import axios from "axios";
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { type BoardViewerItem, type FeedbackKind } from "../../shared/types.gen";
import {
  castBoardVote,
  endBoardSession,
  fetchBoardForViewer,
  retractBoardVote,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { formatRelative } from "../../shared/format";
import { Link } from "../../shared/router";
import { useTranslation } from "../../i18n";
import { useLabels } from "../../i18n/useLabels";
import { useLocale } from "../../i18n/useLocale";
import { LanguageSwitcher } from "../../i18n/LanguageSwitcher";
import {
  boardItemPath,
  isNotFound,
  needsSignIn,
  prioritiesPath,
  retryBoardQuery,
} from "./boardErrors";
import { SignInPrompt } from "./SignInPrompt";

interface PublicBoardProps {
  projectId: string;
}

const KINDS: FeedbackKind[] = ["bug", "feature", "question", "other"];

// Public-facing feedback board (Public Feedback Board + Moderation Gate,
// Contract C29 + C30 voting, extended by C43 end-user prioritization). NO admin
// chrome — intentionally minimal so the page can be embedded under a
// customer's docs domain or linked from the widget, mirroring PublicRoadmap.
// End-users see APPROVED feedback only.
//
// PRIVACY (C29 / Q24 sibling, FR-FBR-59): the payload carries NO submitter
// identity — the server never sends email/name/sub/anon-token/metadata/
// crash-id. There is nothing to anonymize here; this component must never
// reference such a field. A signed-in viewer's `viewer` block is THEIR OWN
// state (FR-FBR-46), absent for an anonymous viewer.
//
// Approved-only is a SERVER invariant (the board repo query hard-filters
// `moderation_status = 'approved'` in SQL — Worker A / C29 inv. 1). The client
// renders whatever the board endpoint returns; it does not re-filter. The kind
// filter (FR-FBR-42) is a query parameter the server applies, not a client
// filter over a page.
//
// ACCESS (FR-FBR-43): a `signed_in` board answers an anonymous read with
// `401 {"error":"SignInRequired"}`, rendered as a sign-in prompt — distinct
// from the board-disabled 404, rendered as "not available".
//
// VOTING (Contract C30, PF-BOARD-VOTING-01): `vote_count` is the real
// aggregate, and the vote button POSTs/DELETEs `.../board/items/{code}/vote`.
// The pressed state comes from the viewer's own `viewer.upvoted` (C43) when
// signed in; a signed-in voter may retract at any time (FR-FBR-47), an
// anonymous one within the C30 window (403 after it).
//
// LOCALIZATION (FR-FBR-36): the CHROME comes from `i18n/locales/<code>/
// public.json`; the feedback BODY is rendered verbatim in whatever language it
// was written in and is never translated on a public surface (Q24 /
// DEC-FBR-15). Do not wrap `item.body` in anything.
export function PublicBoard({ projectId }: PublicBoardProps) {
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const { t } = useTranslation("public");
  const labels = useLabels();
  const { locale } = useLocale();
  const filterId = useId();
  const [kind, setKind] = useState<FeedbackKind | "">("");

  const listQuery = useQuery({
    queryKey: ["public-board", projectId, kind || "all"],
    queryFn: () =>
      fetchBoardForViewer(projectId, kind ? { kind } : {}),
    // A board-disabled project legitimately 404s (C29 inv. 2), and a
    // `signed_in` board answers an anonymous read with SignInRequired — both
    // terminal states, not transient failures, so neither retries.
    retry: retryBoardQuery,
    // Changing the kind filter keeps the page (and the focused filter) on
    // screen while the filtered list loads.
    placeholderData: keepPreviousData,
  });

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["public-board", projectId] });

  const voteMutation = useMutation({
    mutationFn: async (shortCode: string) => castBoardVote(projectId, shortCode),
    onSuccess: invalidate,
    onError: (err) => {
      const msg = needsSignIn(err)
        ? t("public.signIn.expired")
        : axios.isAxiosError(err) && err.response?.status === 409
          ? t("public.vote.alreadyVoted")
          : t("public.vote.failed");
      notify(msg, "error");
    },
  });

  const retractMutation = useMutation({
    mutationFn: async (shortCode: string) =>
      retractBoardVote(projectId, shortCode),
    onSuccess: () => {
      void invalidate();
      notify(t("public.vote.retracted"), "info");
    },
    onError: (err) => {
      const msg = needsSignIn(err)
        ? t("public.signIn.expired")
        : axios.isAxiosError(err) && err.response?.status === 403
          ? t("public.vote.retractWindowClosed")
          : t("public.vote.retractFailed");
      notify(msg, "error");
    },
  });

  const signOutMutation = useMutation({
    mutationFn: async () => endBoardSession(projectId),
    onSuccess: () => {
      void invalidate();
      notify(t("public.board.signedOut"), "info");
    },
    onError: () => notify(t("public.board.signOutFailed"), "error"),
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
  if (listQuery.isError && isNotFound(listQuery.error)) {
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

  // A `signed_in` board read anonymously (FR-FBR-43).
  if (listQuery.isError && needsSignIn(listQuery.error)) {
    return <SignInPrompt projectId={projectId} heading={t("public.board.title")} />;
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

  const signedIn = listQuery.data?.signed_in === true;

  return (
    <main className="public-board" aria-labelledby="public-board-title">
      <header>
        <h1 id="public-board-title">{t("public.board.title")}</h1>
        <p className="muted">{t("public.board.intro")}</p>
        <LanguageSwitcher />
        {signedIn ? (
          <div
            className="board-viewer-bar"
            style={{ display: "flex", flexWrap: "wrap", gap: "0.75rem", alignItems: "center", margin: "0.75rem 0" }}
          >
            <span>{t("public.board.signedIn")}</span>
            <Link to={prioritiesPath(projectId)}>{t("public.board.myPriorities")}</Link>
            <button
              type="button"
              onClick={() => signOutMutation.mutate()}
              disabled={signOutMutation.isPending}
            >
              {t("public.board.signOut")}
            </button>
          </div>
        ) : null}
      </header>

      <p style={{ margin: "0.75rem 0" }}>
        <label htmlFor={filterId}>{t("public.board.kindFilter")}</label>{" "}
        <select
          id={filterId}
          value={kind}
          onChange={(e) => setKind(e.target.value as FeedbackKind | "")}
        >
          <option value="">{t("public.board.kindAll")}</option>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {labels.kind(k)}
            </option>
          ))}
        </select>
      </p>

      {items.length === 0 ? (
        <p className="muted">
          {kind ? t("public.board.filteredEmpty") : t("public.board.empty")}
        </p>
      ) : (
        <ol className="board-item-list">
          {items.map((it) => (
            <li key={it.short_code}>
              <BoardItemRow
                item={it}
                projectId={projectId}
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
  item: BoardViewerItem;
  projectId: string;
  locale: string;
  onVote: () => void;
  onRetract: () => void;
  busy: boolean;
}

function BoardItemRow({
  item,
  projectId,
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
  // The viewer's OWN upvote (C43 `viewer`, FR-FBR-46); `voted_by_me` is the
  // older reserved field, honoured if a server ever sends it.
  const upvoted = item.viewer?.upvoted ?? item.voted_by_me ?? false;
  return (
    <article
      className="board-item"
      aria-labelledby={`board-${item.short_code}-title`}
    >
      <header className="board-item-header">
        <h2 id={`board-${item.short_code}-title`} className="board-item-title">
          <Link to={boardItemPath(projectId, item.short_code)}>{title}</Link>
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
          {upvoted ? (
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
