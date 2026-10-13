import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { BoardItem, PriorityListResponse } from "../../shared/types.gen";
import { fetchMyPriorities, saveMyPriorities } from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { Link } from "../../shared/router";
import { useTranslation } from "../../i18n";
import { useLabels } from "../../i18n/useLabels";
import { LanguageSwitcher } from "../../i18n/LanguageSwitcher";
import {
  boardItemPath,
  boardPath,
  isNotFound,
  needsSignIn,
  retryBoardQuery,
} from "./boardErrors";
import { SignInPrompt } from "./SignInPrompt";

// A signed-in viewer's priority list (FR-FBR-50/51, Contract C43).
//
// ONE ORDERED LIST PER PROJECT, SAVED WHOLE. Every change — move, add, remove —
// sends the viewer's whole current order (`saveMyPriorities`), top first. The
// page shows the new order at once and reconciles with the server's answer;
// only the newest save's answer is applied, so two quick moves never let an
// older response overwrite a newer order.
//
// REORDER BY KEYBOARD AND BY POINTER (FR-FBR-50). Each entry has Move up / Move
// down / Remove buttons — ordinary buttons, so Tab and Enter/Space are the whole
// keyboard interface — and entries can also be dragged. After a keyboard move
// the focus follows the moved item, so pressing Enter again keeps moving it;
// a move that lands an item at either end hands focus to the button that can
// still act. A polite live region announces each change.
//
// FEWER THAN TWO ITEMS: accepted, and the page says the list does not yet
// influence priority (FR-FBR-50, FR-FBR-52: one item states no preference).
//
// LEAVING THE POOL (FR-FBR-51): an item that stops being an open feature
// request is reported by the GET once (`removed` / `removed_hidden`) and
// dropped by the server. The page keeps that notice for the visit, since the
// next answer no longer carries it.
//
// PRIVACY (FR-FBR-56): this list is the viewer's own; nothing here is public,
// and the page never shows any aggregate priority.
export interface MyPrioritiesProps {
  projectId: string;
}

type FocusIntent = { code: string; action: "up" | "down" | "add" } | { action: "removed" };

export function MyPriorities({ projectId }: MyPrioritiesProps) {
  const { t } = useTranslation("public");
  const { notify } = useToast();
  const queryClient = useQueryClient();
  const queryKey = useMemo(
    () => ["public-board", projectId, "priorities"],
    [projectId],
  );

  const query = useQuery({
    queryKey,
    queryFn: () => fetchMyPriorities(projectId),
    retry: retryBoardQuery,
  });

  // The order on screen while a save is in flight; null = the server's order.
  const [pendingOrder, setPendingOrder] = useState<string[] | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [leftPool, setLeftPool] = useState<{
    items: BoardItem[];
    hidden: number;
  } | null>(null);
  const saveSeq = useRef(0);
  const focusIntent = useRef<FocusIntent | null>(null);
  const buttons = useRef(new Map<string, HTMLButtonElement>());
  const listHeading = useRef<HTMLHeadingElement>(null);
  const dragged = useRef<string | null>(null);

  const data = query.data;

  // Keep the one-time "left the pool" report for the whole visit.
  useEffect(() => {
    if (!data || leftPool) return;
    if (data.removed.length > 0 || data.removed_hidden > 0) {
      setLeftPool({ items: data.removed, hidden: data.removed_hidden });
    }
  }, [data, leftPool]);

  const byCode = useMemo(() => {
    const m = new Map<string, BoardItem>();
    for (const it of data?.entries ?? []) m.set(it.short_code, it);
    for (const it of data?.unranked ?? []) m.set(it.short_code, it);
    return m;
  }, [data]);

  const order = useMemo(
    () =>
      (pendingOrder ?? data?.entries.map((e) => e.short_code) ?? []).filter((c) =>
        byCode.has(c),
      ),
    [pendingOrder, data, byCode],
  );

  const unranked = useMemo(() => {
    const inList = new Set(order);
    const seen = new Set<string>();
    const out: BoardItem[] = [];
    for (const it of [...(data?.unranked ?? []), ...(data?.entries ?? [])]) {
      if (inList.has(it.short_code) || seen.has(it.short_code)) continue;
      seen.add(it.short_code);
      out.push(it);
    }
    return out;
  }, [order, data]);

  // Restore focus after a keyboard change (see the header comment).
  useLayoutEffect(() => {
    const intent = focusIntent.current;
    if (!intent) return;
    focusIntent.current = null;
    if (intent.action === "removed") {
      listHeading.current?.focus();
      return;
    }
    const idx = order.indexOf(intent.code);
    if (idx < 0) return;
    const last = order.length - 1;
    let target: "up" | "down" | "remove" = intent.action === "add" ? "up" : intent.action;
    if (target === "up" && idx === 0) target = last > 0 ? "down" : "remove";
    if (target === "down" && idx === last) target = idx > 0 ? "up" : "remove";
    buttons.current.get(`${intent.code}:${target}`)?.focus();
  }, [order]);

  async function save(
    next: string[],
    message: string,
    intent: FocusIntent | null,
  ) {
    focusIntent.current = intent;
    setPendingOrder(next);
    setAnnouncement(message);
    const mine = ++saveSeq.current;
    try {
      const res: PriorityListResponse = await saveMyPriorities(projectId, next);
      if (mine !== saveSeq.current) return;
      queryClient.setQueryData(queryKey, res);
      setPendingOrder(null);
    } catch (err) {
      if (mine !== saveSeq.current) return;
      setPendingOrder(null);
      notify(
        needsSignIn(err) ? t("public.signIn.expired") : t("public.priorities.saveFailed"),
        "error",
      );
      void queryClient.invalidateQueries({ queryKey });
    }
  }

  function move(code: string, to: number, intent: FocusIntent | null) {
    const from = order.indexOf(code);
    if (from < 0 || to < 0 || to >= order.length || to === from) return;
    const next = [...order];
    next.splice(from, 1);
    next.splice(to, 0, code);
    void save(
      next,
      t("public.priorities.moved", {
        code,
        position: to + 1,
        total: next.length,
      }),
      intent,
    );
  }

  function add(code: string) {
    const next = [...order, code];
    void save(
      next,
      t("public.priorities.added", { code, position: next.length }),
      { code, action: "add" },
    );
  }

  function remove(code: string) {
    void save(
      order.filter((c) => c !== code),
      t("public.priorities.removed", { code }),
      { action: "removed" },
    );
  }

  const pageTitle = t("public.priorities.title");

  if (query.isPending) {
    return (
      <main className="public-board" aria-busy="true">
        <h1>{pageTitle}</h1>
        <p>{t("public.common.loading")}</p>
      </main>
    );
  }

  if (query.isError && needsSignIn(query.error)) {
    return <SignInPrompt projectId={projectId} heading={pageTitle} showBoardLink />;
  }

  if (query.isError) {
    return (
      <main className="public-board" aria-labelledby="board-priorities-title">
        <header>
          <h1 id="board-priorities-title">{pageTitle}</h1>
          <LanguageSwitcher />
        </header>
        {isNotFound(query.error) ? (
          <p className="muted">{t("public.board.unavailable")}</p>
        ) : (
          <div role="alert" className="error-block">
            {t("public.priorities.loadError")}{" "}
            <button type="button" onClick={() => query.refetch()}>
              {t("public.common.retry")}
            </button>
          </div>
        )}
      </main>
    );
  }

  const setButton = (key: string) => (el: HTMLButtonElement | null) => {
    if (el) buttons.current.set(key, el);
    else buttons.current.delete(key);
  };

  return (
    <main className="public-board" aria-labelledby="board-priorities-title">
      <p>
        <Link to={boardPath(projectId)}>{t("public.priorities.backToBoard")}</Link>
      </p>
      <header>
        <h1 id="board-priorities-title">{pageTitle}</h1>
        <p className="muted">{t("public.priorities.intro")}</p>
        <LanguageSwitcher />
      </header>

      <p role="status" className="visually-hidden">
        {announcement}
      </p>

      {leftPool ? (
        <section aria-labelledby="board-priorities-left">
          <h2 id="board-priorities-left">{t("public.priorities.leftPoolTitle")}</h2>
          {leftPool.items.length > 0 ? (
            <>
              <p>
                {t("public.priorities.leftPoolBody", {
                  count: leftPool.items.length,
                })}
              </p>
              <ul>
                {leftPool.items.map((it) => (
                  <li key={it.short_code}>
                    <Link to={boardItemPath(projectId, it.short_code)}>
                      {it.short_code}
                    </Link>{" "}
                    <span dir="auto">{it.body}</span>
                  </li>
                ))}
              </ul>
            </>
          ) : null}
          {leftPool.hidden > 0 ? (
            <p>
              {t("public.priorities.leftPoolHidden", { count: leftPool.hidden })}
            </p>
          ) : null}
        </section>
      ) : null}

      <section aria-labelledby="board-priorities-list">
        <h2 id="board-priorities-list" ref={listHeading} tabIndex={-1}>
          {t("public.priorities.listTitle")}
        </h2>
        {order.length < 2 ? (
          <p className="board-priorities-note">{t("public.priorities.tooFewNote")}</p>
        ) : null}
        {order.length === 0 ? (
          <p className="muted">{t("public.priorities.listEmpty")}</p>
        ) : (
          <>
            <p className="muted">{t("public.priorities.reorderHelp")}</p>
            <ol className="board-priority-list">
              {order.map((code, i) => {
                const it = byCode.get(code)!;
                return (
                  <li
                    key={code}
                    draggable
                    onDragStart={(e) => {
                      dragged.current = code;
                      e.dataTransfer.effectAllowed = "move";
                      e.dataTransfer.setData("text/plain", code);
                    }}
                    onDragOver={(e) => {
                      if (dragged.current) e.preventDefault();
                    }}
                    onDrop={(e) => {
                      e.preventDefault();
                      const from = dragged.current;
                      dragged.current = null;
                      // A pointer move leaves focus where the pointer put it.
                      if (from && from !== code) move(from, i, null);
                    }}
                    onDragEnd={() => {
                      dragged.current = null;
                    }}
                    style={{ margin: "0.5rem 0" }}
                  >
                    <PriorityItem item={it} projectId={projectId} />
                    <div
                      className="board-item-actions"
                      style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem" }}
                    >
                      <button
                        type="button"
                        ref={setButton(`${code}:up`)}
                        disabled={i === 0}
                        aria-label={t("public.priorities.moveUp", { code })}
                        onClick={() => move(code, i - 1, { code, action: "up" })}
                      >
                        {t("public.priorities.moveUpLabel")}
                      </button>
                      <button
                        type="button"
                        ref={setButton(`${code}:down`)}
                        disabled={i === order.length - 1}
                        aria-label={t("public.priorities.moveDown", { code })}
                        onClick={() => move(code, i + 1, { code, action: "down" })}
                      >
                        {t("public.priorities.moveDownLabel")}
                      </button>
                      <button
                        type="button"
                        ref={setButton(`${code}:remove`)}
                        aria-label={t("public.priorities.remove", { code })}
                        onClick={() => remove(code)}
                      >
                        {t("public.priorities.removeLabel")}
                      </button>
                    </div>
                  </li>
                );
              })}
            </ol>
          </>
        )}
      </section>

      <section aria-labelledby="board-priorities-unranked">
        <h2 id="board-priorities-unranked">{t("public.priorities.unrankedTitle")}</h2>
        {unranked.length === 0 ? (
          <p className="muted">{t("public.priorities.unrankedEmpty")}</p>
        ) : (
          <ul className="board-unranked-list">
            {unranked.map((it) => (
              <li key={it.short_code} style={{ margin: "0.5rem 0" }}>
                <PriorityItem item={it} projectId={projectId} />
                <button
                  type="button"
                  aria-label={t("public.priorities.add", { code: it.short_code })}
                  onClick={() => add(it.short_code)}
                >
                  {t("public.priorities.addLabel")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}

function PriorityItem({ item, projectId }: { item: BoardItem; projectId: string }) {
  const { t } = useTranslation("public");
  const labels = useLabels();
  const statusLabel = labels.status(item.status);
  return (
    <div className="board-item">
      <div className="board-item-header">
        <Link to={boardItemPath(projectId, item.short_code)}>{item.short_code}</Link>{" "}
        <span
          className={`status-badge status-${item.status}`}
          aria-label={t("public.common.statusLabel", { status: statusLabel })}
        >
          {statusLabel}
        </span>
      </div>
      <p className="board-item-body" dir="auto">{item.body}</p>
    </div>
  );
}
