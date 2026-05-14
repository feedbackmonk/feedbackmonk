import { useId, useMemo, useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ROADMAP_STATUS_LABELS,
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
  const projectQuery = useQuery({
    queryKey: ["admin-projects"],
    queryFn: fetchAdminProjects,
    staleTime: 60_000,
  });

  if (projectQuery.isPending) {
    return (
      <main className="admin-roadmap" aria-busy="true">
        <h1>Roadmap (admin)</h1>
        <p>Loading…</p>
      </main>
    );
  }
  if (projectQuery.isError || !projectQuery.data?.projects.length) {
    return (
      <main className="admin-roadmap">
        <h1>Roadmap (admin)</h1>
        <div role="alert" className="error-block">
          No projects configured. Create one before managing the roadmap.
        </div>
      </main>
    );
  }
  const projectId = projectQuery.data.projects[0].project_id;
  return <AdminRoadmapInner projectId={projectId} />;
}

function AdminRoadmapInner({ projectId }: { projectId: string }) {
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
        <h1 id="admin-roadmap-title">Roadmap (admin)</h1>
        <button
          type="button"
          onClick={() => setCreateOpen(true)}
          className="primary"
        >
          New item
        </button>
      </header>

      {listQuery.isPending ? (
        <p aria-busy="true">Loading…</p>
      ) : listQuery.isError ? (
        <div role="alert" className="error-block">
          Failed to load roadmap.{" "}
          <button type="button" onClick={() => listQuery.refetch()}>
            Retry
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
                {ROADMAP_STATUS_LABELS[status]}{" "}
                <span className="muted">({items.length})</span>
              </h2>
              {items.length === 0 ? (
                <p className="muted">No items.</p>
              ) : (
                <ul className="roadmap-admin-list">
                  {items.map((it) => (
                    <li key={it.slug} className="roadmap-admin-row">
                      <div>
                        <strong>{it.title}</strong>{" "}
                        <code className="muted">/{it.slug}</code>
                        {it.origin_feedback_id ? (
                          <span className="muted">
                            {" "}· promoted from {it.origin_feedback_id}
                          </span>
                        ) : null}
                        <div className="muted">{it.vote_count} votes</div>
                      </div>
                      <button type="button" onClick={() => setEditing(it)}>
                        Edit
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
            notify("Roadmap item created.", "success");
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
            notify("Roadmap item updated.", "success");
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
    onError: () => setInlineError("Create failed. Please try again."),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (!slug.trim() || !title.trim() || !body.trim()) {
      setInlineError("Slug, title, and body are required.");
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
        <h2 id={`${dialogId}-title`}>New roadmap item</h2>

        <label htmlFor={`${dialogId}-slug`}>Slug (kebab-case, 1–80 chars)</label>
        <input
          id={`${dialogId}-slug`}
          type="text"
          value={slug}
          onChange={(e) => setSlug(e.target.value)}
          required
          maxLength={80}
          autoFocus
        />

        <label htmlFor={`${dialogId}-title-input`}>Title</label>
        <input
          id={`${dialogId}-title-input`}
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          required
          maxLength={200}
        />

        <label htmlFor={`${dialogId}-body`}>Body</label>
        <textarea
          id={`${dialogId}-body`}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          required
          maxLength={16384}
          rows={6}
        />

        <label htmlFor={`${dialogId}-status`}>Initial status</label>
        <select
          id={`${dialogId}-status`}
          value={status}
          onChange={(e) => setStatus(e.target.value as RoadmapItemStatus)}
        >
          {ALL_STATUSES.map((s) => (
            <option key={s} value={s}>
              {ROADMAP_STATUS_LABELS[s]}
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
            Cancel
          </button>
          <button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Creating…" : "Create"}
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
    onError: () => setInlineError("Update failed. Please try again."),
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
          Edit <code>{item.slug}</code>
        </h2>

        <label htmlFor={`${dialogId}-title-input`}>Title</label>
        <input
          id={`${dialogId}-title-input`}
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          required
          maxLength={200}
        />

        <label htmlFor={`${dialogId}-body`}>Body</label>
        <textarea
          id={`${dialogId}-body`}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          required
          maxLength={16384}
          rows={6}
        />

        <label htmlFor={`${dialogId}-status`}>Status</label>
        <select
          id={`${dialogId}-status`}
          value={status}
          onChange={(e) => setStatus(e.target.value as RoadmapItemStatus)}
        >
          {ALL_STATUSES.map((s) => (
            <option key={s} value={s}>
              {ROADMAP_STATUS_LABELS[s]}
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
            Cancel
          </button>
          <button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </div>
  );
}
