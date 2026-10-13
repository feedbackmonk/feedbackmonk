import { useEffect, useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  deleteBoardNote,
  fetchBoardNote,
  putBoardNote,
  type BoardNoteResponse,
} from "../../shared/boardModerationApi";
import { useToast } from "../../components/Toast";
import { useTranslation } from "../../i18n";
import { useLocale } from "../../i18n/useLocale";

// The owner's board-note editor (FR-FBR-58, Contract C44): set, edit and clear
// the one plain-text note a board item carries for every board viewer. It is
// NOT a reply to the submitter — replies never reach a board surface — so it
// lives beside the board's own signals, not in the feedback drawer.
//
// The server trims and refuses an empty or over-long body (1..=4000 chars); the
// editor mirrors that so Save is never offered for a body it would refuse. An
// item that is not on the board answers 404, which the editor says in words.

export const BOARD_NOTE_MAX_CHARS = 4000;

function isNotFound(err: unknown): boolean {
  return (
    !!err &&
    typeof err === "object" &&
    (err as { response?: { status?: number } }).response?.status === 404
  );
}

export function BoardNoteEditor({
  shortCode,
  onClose,
}: {
  shortCode: string;
  onClose: () => void;
}) {
  const { t } = useTranslation("admin");
  const { locale } = useLocale();
  const { notify } = useToast();
  const queryClient = useQueryClient();
  const headingId = useId();
  const fieldId = useId();
  const counterId = useId();
  const queryKey = ["admin-board-note", shortCode];

  const query = useQuery({
    queryKey,
    queryFn: () => fetchBoardNote(shortCode),
    retry: (count, err) => !isNotFound(err) && count < 2,
  });

  const stored = query.data?.note ?? null;
  const [draft, setDraft] = useState("");
  useEffect(() => {
    setDraft(query.data?.note?.body ?? "");
  }, [query.data]);

  const onStored = (data: BoardNoteResponse, message: string) => {
    queryClient.setQueryData<BoardNoteResponse>(queryKey, data);
    setDraft(data.note?.body ?? "");
    notify(message, "success");
  };

  const save = useMutation({
    mutationFn: (body: string) => putBoardNote(shortCode, body),
    onSuccess: (data) => onStored(data, t("admin.boardNoteEditor.saved")),
    onError: () => notify(t("admin.boardNoteEditor.saveFailed"), "error"),
  });
  const clear = useMutation({
    mutationFn: () => deleteBoardNote(shortCode),
    onSuccess: (data) => onStored(data, t("admin.boardNoteEditor.cleared")),
    onError: () => notify(t("admin.boardNoteEditor.saveFailed"), "error"),
  });

  const trimmed = draft.trim();
  const busy = save.isPending || clear.isPending;
  const canSave =
    !busy &&
    trimmed.length > 0 &&
    trimmed.length <= BOARD_NOTE_MAX_CHARS &&
    trimmed !== (stored?.body ?? "");

  return (
    <section className="board-note-editor" aria-labelledby={headingId}>
      <h2 id={headingId}>
        {t("admin.boardNoteEditor.heading", { item: shortCode })}
      </h2>
      <p className="muted">{t("admin.boardNoteEditor.intro")}</p>

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          {t("admin.boardNoteEditor.loading")}
        </p>
      ) : query.isError ? (
        <div role="alert" className="error-block">
          {isNotFound(query.error) ? (
            t("admin.boardNoteEditor.notOnBoard")
          ) : (
            <>
              {t("admin.boardNoteEditor.loadError")}{" "}
              <button type="button" onClick={() => query.refetch()}>
                {t("admin.common.retry")}
              </button>
            </>
          )}
        </div>
      ) : (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (canSave) save.mutate(trimmed);
          }}
        >
          <p className="muted">
            {stored ? (
              t("admin.boardNoteEditor.lastUpdated", {
                when: new Date(stored.updated_at).toLocaleString(locale),
              })
            ) : (
              t("admin.boardNoteEditor.none")
            )}
          </p>
          <label htmlFor={fieldId}>{t("admin.boardNoteEditor.label")}</label>
          <textarea
            id={fieldId}
            value={draft}
            rows={4}
            maxLength={BOARD_NOTE_MAX_CHARS}
            disabled={busy}
            aria-describedby={counterId}
            onChange={(e) => setDraft(e.target.value)}
          />
          <p id={counterId} className="char-counter">
            {t("admin.boardNoteEditor.counter", {
              length: draft.length,
              max: BOARD_NOTE_MAX_CHARS,
            })}
          </p>
          <div className="board-note-actions">
            <button type="submit" disabled={!canSave}>
              {t("admin.boardNoteEditor.save")}
            </button>
            {stored ? (
              <button
                type="button"
                disabled={busy}
                onClick={() => clear.mutate()}
              >
                {t("admin.boardNoteEditor.clear")}
              </button>
            ) : null}
            <button type="button" onClick={onClose}>
              {t("admin.boardNoteEditor.close")}
            </button>
          </div>
        </form>
      )}
    </section>
  );
}
