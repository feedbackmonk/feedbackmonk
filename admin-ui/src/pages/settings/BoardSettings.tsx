import { useId } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  fetchBoardSettings,
  patchBoardSettings,
  type BoardSettings as BoardSettingsShape,
  type BoardSettingsPatch,
} from "../../shared/boardModerationApi";
import { useToast } from "../../components/Toast";
import { useAdminProject } from "../autopilot/useAdminProject";

// /admin/settings/board — per-project public-board settings (migration 00016
// columns `public_board_enabled` + `board_requires_moderation`). Mirrors the
// RunnerTokens project-resolution wrapper + TierSettings card chrome.
export function BoardSettings() {
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="board-settings-page" aria-busy="true">
        <header className="page-header">
          <h1>Public board</h1>
        </header>
        <p className="muted">Loading…</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="board-settings-page">
        <header className="page-header">
          <h1>Public board</h1>
        </header>
        <div role="alert" className="error-block">
          No projects configured.
        </div>
      </main>
    );
  }
  return <BoardSettingsInner projectId={project.projectId} />;
}

function BoardSettingsInner({ projectId }: { projectId: string }) {
  const enabledId = useId();
  const moderationId = useId();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const queryKey = ["admin-board-settings", projectId];

  const query = useQuery({
    queryKey,
    queryFn: () => fetchBoardSettings(projectId),
  });

  const mutation = useMutation({
    mutationFn: (patch: BoardSettingsPatch) =>
      patchBoardSettings(projectId, patch),
    onSuccess: (data) => {
      // Reflect the server's authoritative settings immediately.
      queryClient.setQueryData<BoardSettingsShape>(queryKey, data);
      notify("Board settings saved.", "success");
    },
    onError: () => notify("Could not save board settings.", "error"),
  });

  const settings = query.data;
  const saving = mutation.isPending;

  return (
    <main className="board-settings-page" aria-labelledby="board-settings-title">
      <header className="page-header">
        <h1 id="board-settings-title">Public board</h1>
        <p className="muted">
          The public board lets end-users see and vote on feedback you’ve
          approved. It’s off by default — no feedback is ever public until you
          enable the board and approve items in the moderation queue.
        </p>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          Failed to load board settings.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          Loading…
        </p>
      ) : settings ? (
        <section className="board-settings-card" aria-label="Board settings">
          <div className="settings-toggle">
            <input
              id={enabledId}
              type="checkbox"
              checked={settings.public_board_enabled}
              disabled={saving}
              onChange={(e) =>
                mutation.mutate({ public_board_enabled: e.target.checked })
              }
            />
            <div className="settings-toggle-text">
              <label htmlFor={enabledId}>Enable public board</label>
              <p className="muted">
                When on, approved feedback is visible at your public board URL.
                Turning it off hides the board entirely (existing approvals are
                kept).
              </p>
            </div>
          </div>

          <div className="settings-toggle">
            <input
              id={moderationId}
              type="checkbox"
              checked={settings.board_requires_moderation}
              // v1 always requires moderation: the board read hard-filters to
              // approved rows in SQL (Contract C29 inv. 1) regardless of this
              // flag. The column is reserved for a future auto-approve
              // relaxation, so the control is shown read-only rather than
              // implying an effect it doesn't yet have (mirrors TierSettings'
              // `notImplemented` footnote discipline).
              disabled
              aria-describedby={`${moderationId}-note`}
            />
            <div className="settings-toggle-text">
              <label htmlFor={moderationId}>Require moderation</label>
              <p id={`${moderationId}-note`} className="muted">
                Always on in v1 — every item is reviewed before it can appear on
                the board. Auto-approve is reserved for a future release.
              </p>
            </div>
          </div>
        </section>
      ) : null}
    </main>
  );
}
