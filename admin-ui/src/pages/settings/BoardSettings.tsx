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
import { useTranslation } from "../../i18n";

// /admin/settings/board — per-project public-board settings (migration 00016
// columns `public_board_enabled` + `board_requires_moderation`). Mirrors the
// RunnerTokens project-resolution wrapper + TierSettings card chrome.
export function BoardSettings() {
  const { t } = useTranslation("admin");
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="board-settings-page" aria-busy="true">
        <header className="page-header">
          <h1>{t("admin.boardSettings.title")}</h1>
        </header>
        <p className="muted">{t("admin.common.loading")}</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="board-settings-page">
        <header className="page-header">
          <h1>{t("admin.boardSettings.title")}</h1>
        </header>
        <div role="alert" className="error-block">
          {t("admin.common.noProjects")}
        </div>
      </main>
    );
  }
  return <BoardSettingsInner projectId={project.projectId} />;
}

function BoardSettingsInner({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
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
      notify(t("admin.boardSettings.saved"), "success");
    },
    onError: () => notify(t("admin.boardSettings.saveFailed"), "error"),
  });

  const settings = query.data;
  const saving = mutation.isPending;

  return (
    <main className="board-settings-page" aria-labelledby="board-settings-title">
      <header className="page-header">
        <h1 id="board-settings-title">{t("admin.boardSettings.title")}</h1>
        <p className="muted">{t("admin.boardSettings.intro")}</p>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.boardSettings.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          {t("admin.common.loading")}
        </p>
      ) : settings ? (
        <section
          className="board-settings-card"
          aria-label={t("admin.boardSettings.cardAria")}
        >
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
              <label htmlFor={enabledId}>
                {t("admin.boardSettings.enableLabel")}
              </label>
              <p className="muted">{t("admin.boardSettings.enableNote")}</p>
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
              <label htmlFor={moderationId}>
                {t("admin.boardSettings.requireModerationLabel")}
              </label>
              <p id={`${moderationId}-note`} className="muted">
                {t("admin.boardSettings.requireModerationNote")}
              </p>
            </div>
          </div>
        </section>
      ) : null}
    </main>
  );
}
