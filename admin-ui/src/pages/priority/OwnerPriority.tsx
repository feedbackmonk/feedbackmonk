import { useState, type ReactElement } from "react";
import { Trans } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  fetchPriorityView,
  type PriorityRankedItem,
  type PriorityUnrankedItem,
} from "../../shared/boardModerationApi";
import { StatusBadge } from "../../components/StatusBadge";
import { useAdminProject } from "../autopilot/useAdminProject";
import { useTranslation } from "../../i18n";
import { useLocale } from "../../i18n/useLocale";
import { BoardNoteEditor } from "./BoardNoteEditor";
import { uncertaintyBand } from "./uncertainty";

// /admin/priority — the owner's aggregate priority view (FR-FBR-55/56,
// Contract C44), built against `fetchPriorityView`.
//
// Three signals, three columns. The fitted priority order (with how many users
// placed each item and the fit's uncertainty), the vote count and the
// not-needed count are shown SIDE BY SIDE and never folded into one number
// (FR-FBR-55): a combined score would hide exactly the disagreement an owner
// needs to see — a much-voted item many users also call unneeded. Unranked
// items are listed apart, because "no one ranked it" is not "ranked last".
//
// Only aggregates reach this page (FR-FBR-56): the wire shape names no user and
// no list, and nothing here asks for one.
//
// Each row also opens the item's board-note editor (FR-FBR-58).
export function OwnerPriority() {
  const { t } = useTranslation("admin");
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="owner-priority-page" aria-busy="true">
        <header className="page-header">
          <h1>{t("admin.ownerPriority.title")}</h1>
        </header>
        <p className="muted">{t("admin.common.loading")}</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="owner-priority-page">
        <header className="page-header">
          <h1>{t("admin.ownerPriority.title")}</h1>
        </header>
        <div role="alert" className="error-block">
          {t("admin.common.noProjects")}
        </div>
      </main>
    );
  }
  return <OwnerPriorityInner projectId={project.projectId} />;
}

function OwnerPriorityInner({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
  const { locale } = useLocale();
  const [editing, setEditing] = useState<string | null>(null);

  const query = useQuery({
    queryKey: ["admin-priority", projectId],
    queryFn: () => fetchPriorityView(projectId),
  });
  const data = query.data;

  const noteButton = (shortCode: string) => (
    <button
      type="button"
      className="link-button"
      aria-label={t("admin.ownerPriority.editNoteAria", { item: shortCode })}
      aria-expanded={editing === shortCode}
      onClick={() => setEditing(shortCode)}
    >
      {t("admin.ownerPriority.editNote")}
    </button>
  );

  const count = (n: number) => n.toLocaleString(locale);

  const uncertainty = (item: PriorityRankedItem) =>
    t("admin.ownerPriority.uncertaintyValue", {
      band: t(`admin.ownerPriority.uncertaintyBand.${uncertaintyBand(item.uncertainty)}`),
      value: Number.isFinite(item.uncertainty)
        ? item.uncertainty.toLocaleString(locale, {
            minimumFractionDigits: 2,
            maximumFractionDigits: 2,
          })
        : "∞",
    });

  return (
    <main className="owner-priority-page" aria-labelledby="owner-priority-title">
      <header className="page-header">
        <h1 id="owner-priority-title">{t("admin.ownerPriority.title")}</h1>
        <p className="muted">{t("admin.ownerPriority.intro")}</p>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.ownerPriority.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          {t("admin.common.loading")}
        </p>
      ) : data ? (
        <>
          <p className="muted owner-priority-meta">
            <span>
              <Trans
                i18nKey="admin.ownerPriority.computedAt"
                t={t}
                values={{
                  when: new Date(data.computed_at).toLocaleString(locale),
                }}
                components={{ time: <time dateTime={data.computed_at} /> }}
              />
            </span>{" "}
            <span>
              {t("admin.ownerPriority.contributingRankers", {
                count: data.contributing_rankers,
              })}
            </span>
          </p>

          <section aria-labelledby="owner-priority-ranked">
            <h2 id="owner-priority-ranked">
              {t("admin.ownerPriority.rankedHeading")}
            </h2>
            {data.ranked.length === 0 ? (
              <div className="empty-state">
                <p>{t("admin.ownerPriority.rankedEmpty")}</p>
              </div>
            ) : (
              <div className="table-scroll">
                <table className="feedback-table owner-priority-table">
                  <caption className="visually-hidden">
                    {t("admin.ownerPriority.rankedCaption")}
                  </caption>
                  <thead>
                    <tr>
                      <th scope="col">{t("admin.ownerPriority.columns.rank")}</th>
                      <th scope="col">{t("admin.ownerPriority.columns.item")}</th>
                      <th scope="col">{t("admin.ownerPriority.columns.excerpt")}</th>
                      <th scope="col">{t("admin.ownerPriority.columns.status")}</th>
                      <th scope="col">{t("admin.ownerPriority.columns.rankers")}</th>
                      <th scope="col">{t("admin.ownerPriority.columns.uncertainty")}</th>
                      <th scope="col">{t("admin.ownerPriority.columns.votes")}</th>
                      <th scope="col">{t("admin.ownerPriority.columns.notNeeded")}</th>
                      <th scope="col">{t("admin.ownerPriority.columns.boardNote")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.ranked.map((item) => (
                      <tr key={item.short_code}>
                        <th scope="row">{count(item.rank)}</th>
                        <td className="mono">{item.short_code}</td>
                        <td className="excerpt" dir="auto">
                          {item.body_excerpt}
                        </td>
                        <td>
                          <StatusBadge status={item.status} />
                        </td>
                        <td>{count(item.rankers)}</td>
                        <td>{uncertainty(item)}</td>
                        <td>{count(item.vote_count)}</td>
                        <td>{count(item.not_needed_count)}</td>
                        <td>{noteButton(item.short_code)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>

          <section aria-labelledby="owner-priority-unranked">
            <h2 id="owner-priority-unranked">
              {t("admin.ownerPriority.unrankedHeading")}
            </h2>
            {data.unranked.length === 0 ? (
              <div className="empty-state">
                <p>{t("admin.ownerPriority.unrankedEmpty")}</p>
              </div>
            ) : (
              <UnrankedTable
                items={data.unranked}
                count={count}
                noteButton={noteButton}
              />
            )}
          </section>

          {editing ? (
            <BoardNoteEditor
              key={editing}
              shortCode={editing}
              onClose={() => setEditing(null)}
            />
          ) : null}
        </>
      ) : null}
    </main>
  );
}

function UnrankedTable({
  items,
  count,
  noteButton,
}: {
  items: PriorityUnrankedItem[];
  count: (n: number) => string;
  noteButton: (shortCode: string) => ReactElement;
}) {
  const { t } = useTranslation("admin");
  return (
    <div className="table-scroll">
      <table className="feedback-table owner-priority-table">
        <caption className="visually-hidden">
          {t("admin.ownerPriority.unrankedCaption")}
        </caption>
        <thead>
          <tr>
            <th scope="col">{t("admin.ownerPriority.columns.item")}</th>
            <th scope="col">{t("admin.ownerPriority.columns.excerpt")}</th>
            <th scope="col">{t("admin.ownerPriority.columns.status")}</th>
            <th scope="col">{t("admin.ownerPriority.columns.votes")}</th>
            <th scope="col">{t("admin.ownerPriority.columns.notNeeded")}</th>
            <th scope="col">{t("admin.ownerPriority.columns.boardNote")}</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <tr key={item.short_code}>
              <th scope="row" className="mono">
                {item.short_code}
              </th>
              <td className="excerpt" dir="auto">
                {item.body_excerpt}
              </td>
              <td>
                <StatusBadge status={item.status} />
              </td>
              <td>{count(item.vote_count)}</td>
              <td>{count(item.not_needed_count)}</td>
              <td>{noteButton(item.short_code)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
