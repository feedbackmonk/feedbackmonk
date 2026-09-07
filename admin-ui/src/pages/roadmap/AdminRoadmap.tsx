import { useId, useMemo, useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  type AdminRoadmapCreateRequest,
  type AdminRoadmapPatchRequest,
  type RoadmapItem,
  type RoadmapItemStatus,
} from "../../shared/types.gen";
import {
  fetchAdminProjects,
  fetchAdminRoadmap,
  patchRoadmapItem,
  postCreateRoadmapItem,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { useTranslation } from "../../i18n";
import { useLabels } from "../../i18n/useLabels";

const ALL_STATUSES: RoadmapItemStatus[] = [
  "considering",
  "planned",
  "in-progress",
  "shipped",
  "wontfix",
];

// Resolves the admin's sole project id (P0/P1 invariant: one project per
// tenant in practice). Multi-project URL routing is deferred to P3.
export function AdminRoadmap() {
  const { t } = useTranslation("admin");
  const projectQuery = useQuery({
    queryKey: ["admin-projects"],
    queryFn: fetchAdminProjects,
    staleTime: 60_000,
  });

  if (projectQuery.isPending) {
    return (
      <main className="admin-roadmap" aria-busy="true">
        <h1>{t("admin.adminRoadmap.title")}</h1>
        <p>{t("admin.common.loading")}</p>
      </main>
    );
  }
  if (projectQuery.isError || !projectQuery.data?.projects.length) {
    return (
      <main className="admin-roadmap">
        <h1>{t("admin.adminRoadmap.title")}</h1>
        <div role="alert" className="error-block">
          {t("admin.adminRoadmap.noProjects")}
        </div>
      </main>
    );
  }
  const projectId = projectQuery.data.projects[0].project_id;
  return <AdminRoadmapInner projectId={projectId} />;
}

function AdminRoadmapInner({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
  const labels = useLabels();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<RoadmapItem | null>(null);

  const listQuery = useQuery({
    queryKey: ["admin-roadmap", projectId],
    queryFn: () => fetchAdminRoadmap(projectId),
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

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["admin-roadmap", projectId] });

  return (
    <main className="admin-roadmap" aria-labelledby="admin-roadmap-title">
      <header className="admin-roadmap-header">
        <h1 id="admin-roadmap-title">{t("admin.adminRoadmap.title")}</h1>
        <button
          type="button"
          onClick={() => setCreateOpen(true)}
          className="primary"
        >
          {t("admin.adminRoadmap.newItem")}
        </button>
      </header>

      {listQuery.isPending ? (
        <p aria-busy="true">{t("admin.common.loading")}</p>
      ) : listQuery.isError ? (
        <div role="alert" className="error-block">
          {t("admin.adminRoadmap.loadError")}{" "}
          <button type="button" onClick={() => listQuery.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : (
        ALL_STATUSES.map((status) => {
          const items = grouped[status];
          return (
            <section
              key={status}
              aria-labelledby={`admin-roadmap-${status}-label`}
              className={`roadmap-section roadmap-section-${status}`}
            >
              <h2 id={`admin-roadmap-${status}-label`}>
                {labels.roadmapStatus(status)}{" "}
                <span className="muted">({items.length})</span>
              </h2>
              {items.length === 0 ? (
                <p className="muted">{t("admin.adminRoadmap.noItems")}</p>
              ) : (
                <ul className="roadmap-admin-list">
                  {items.map((it) => (
                    <li key={it.slug} className="roadmap-admin-row">
                      <div>
                        <strong>{it.title}</strong>{" "}
                        <code className="muted">/{it.slug}</code>
                        {it.origin_feedback_id ? (
                          <span className="muted">
                            {" "}
                            ·{" "}
                            {t("admin.adminRoadmap.promotedFrom", {
                              id: it.origin_feedback_id,
                            })}
                          </span>
                        ) : null}
                        <div className="muted">
                          {t("admin.adminRoadmap.voteCount", {
                            count: it.vote_count,
                          })}
                        </div>
                      </div>
                      <button type="button" onClick={() => setEditing(it)}>
                        {t("admin.common.edit")}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          );
        })
      )}

      {createOpen ? (
        <CreateModal
          projectId={projectId}
          onClose={() => setCreateOpen(false)}
          onSuccess={() => {
            notify(t("admin.adminRoadmap.created"), "success");
            invalidate();
            setCreateOpen(false);
          }}
        />
      ) : null}

      {editing ? (
        <EditModal
          projectId={projectId}
          item={editing}
          onClose={() => setEditing(null)}
          onSuccess={() => {
            notify(t("admin.adminRoadmap.updated"), "success");
            invalidate();
            setEditing(null);
          }}
        />
      ) : null}
    </main>
  );
}

// ─── Create modal ─────────────────────────────────────────────────────────

function CreateModal({
  projectId,
  onClose,
  onSuccess,
}: {
  projectId: string;
  onClose: () => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation("admin");
  const labels = useLabels();
  const dialogId = useId();
  const [slug, setSlug] = useState("");
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [status, setStatus] = useState<RoadmapItemStatus>("considering");
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: async () => {
      const body_: AdminRoadmapCreateRequest = {
        slug: slug.trim(),
        title: title.trim(),
        body: body.trim(),
        status,
      };
      return postCreateRoadmapItem(projectId, body_);
    },
    onSuccess,
    onError: () => setInlineError(t("admin.adminRoadmap.createFailed")),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (!slug.trim() || !title.trim() || !body.trim()) {
      setInlineError(t("admin.adminRoadmap.requiredFields"));
      return;
    }
    mutation.mutate();
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={`${dialogId}-title`}
      className="dialog dialog-overlay"
    >
      <form onSubmit={onSubmit} className="dialog-body">
        <h2 id={`${dialogId}-title`}>{t("admin.adminRoadmap.newItemTitle")}</h2>

        <label htmlFor={`${dialogId}-slug`}>
          {t("admin.adminRoadmap.slugLabel")}
        </label>
        <input
          id={`${dialogId}-slug`}
          type="text"
          value={slug}
          onChange={(e) => setSlug(e.target.value)}
          required
          maxLength={80}
          autoFocus
        />

        <label htmlFor={`${dialogId}-title-input`}>
          {t("admin.adminRoadmap.titleLabel")}
        </label>
        <input
          id={`${dialogId}-title-input`}
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          required
          maxLength={200}
        />

        <label htmlFor={`${dialogId}-body`}>{t("admin.adminRoadmap.bodyLabel")}</label>
        <textarea
          id={`${dialogId}-body`}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          required
          maxLength={16384}
          rows={6}
        />

        <label htmlFor={`${dialogId}-status`}>
          {t("admin.adminRoadmap.initialStatusLabel")}
        </label>
        <select
          id={`${dialogId}-status`}
          value={status}
          onChange={(e) => setStatus(e.target.value as RoadmapItemStatus)}
        >
          {ALL_STATUSES.map((s) => (
            <option key={s} value={s}>
              {labels.roadmapStatus(s)}
            </option>
          ))}
        </select>

        {inlineError ? (
          <p role="alert" className="error">
            {inlineError}
          </p>
        ) : null}

        <div className="dialog-actions">
          <button type="button" onClick={onClose} disabled={mutation.isPending}>
            {t("admin.common.cancel")}
          </button>
          <button type="submit" disabled={mutation.isPending}>
            {mutation.isPending
              ? t("admin.adminRoadmap.creating")
              : t("admin.common.create")}
          </button>
        </div>
      </form>
    </div>
  );
}

// ─── Edit modal ───────────────────────────────────────────────────────────

function EditModal({
  projectId,
  item,
  onClose,
  onSuccess,
}: {
  projectId: string;
  item: RoadmapItem;
  onClose: () => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation("admin");
  const labels = useLabels();
  const dialogId = useId();
  const [title, setTitle] = useState(item.title);
  const [body, setBody] = useState(item.body);
  const [status, setStatus] = useState<RoadmapItemStatus>(item.status);
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: async () => {
      const patch: AdminRoadmapPatchRequest = {};
      if (title !== item.title) patch.title = title.trim();
      if (body !== item.body) patch.body = body.trim();
      if (status !== item.status) patch.status = status;
      return patchRoadmapItem(projectId, item.slug, patch);
    },
    onSuccess,
    onError: () => setInlineError(t("admin.adminRoadmap.updateFailed")),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    mutation.mutate();
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={`${dialogId}-title`}
      className="dialog dialog-overlay"
    >
      <form onSubmit={onSubmit} className="dialog-body">
        <h2 id={`${dialogId}-title`}>
          {t("admin.adminRoadmap.editHeading")} <code>{item.slug}</code>
        </h2>

        <label htmlFor={`${dialogId}-title-input`}>
          {t("admin.adminRoadmap.titleLabel")}
        </label>
        <input
          id={`${dialogId}-title-input`}
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          required
          maxLength={200}
        />

        <label htmlFor={`${dialogId}-body`}>{t("admin.adminRoadmap.bodyLabel")}</label>
        <textarea
          id={`${dialogId}-body`}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          required
          maxLength={16384}
          rows={6}
        />

        <label htmlFor={`${dialogId}-status`}>
          {t("admin.adminRoadmap.statusLabel")}
        </label>
        <select
          id={`${dialogId}-status`}
          value={status}
          onChange={(e) => setStatus(e.target.value as RoadmapItemStatus)}
        >
          {ALL_STATUSES.map((s) => (
            <option key={s} value={s}>
              {labels.roadmapStatus(s)}
            </option>
          ))}
        </select>

        {inlineError ? (
          <p role="alert" className="error">
            {inlineError}
          </p>
        ) : null}

        <div className="dialog-actions">
          <button type="button" onClick={onClose} disabled={mutation.isPending}>
            {t("admin.common.cancel")}
          </button>
          <button type="submit" disabled={mutation.isPending}>
            {mutation.isPending
              ? t("admin.adminRoadmap.saving")
              : t("admin.common.save")}
          </button>
        </div>
      </form>
    </div>
  );
}
