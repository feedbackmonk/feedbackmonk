import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { exchangeBoardSession } from "../../shared/ApiClient";
import { Link, useRouter } from "../../shared/router";
import { useTranslation } from "../../i18n";
import { boardPath } from "./boardErrors";

// The one-time sign-in handoff to the hosted board (FR-FBR-44, Contract C43).
//
// The widget opens `/public/projects/{id}/board/sign-in#token=<fresh JWT>`
// (FR-FBR-45). This page:
//   1. reads `token` from the URL FRAGMENT — never the query string or path,
//      which reach server logs and Referer headers; the fragment does not;
//   2. removes the fragment from the address bar AND the history entry
//      (`history.replaceState`) at once, before the exchange resolves, so the
//      token is gone even if the exchange fails or the visitor leaves
//      mid-request — the contract's "after the exchange" is a floor, not a
//      requirement to keep the token visible while the POST is in flight;
//   3. POSTs it once to `/board/session` (`exchangeBoardSession`), which sets
//      the HttpOnly board-session cookie, and drops its own reference to it;
//   4. replaces this history entry with the board, so Back never returns here.
//
// ONCE ONLY: a ref guards the exchange, because StrictMode (and any remount)
// runs the effect twice on the same instance, and the second run must not read
// an already-cleared fragment as "no token".
export interface SignInHandoffProps {
  projectId: string;
}

type HandoffState = "working" | "missing" | "failed";

/** The `token` parameter of a `#token=…` fragment, or null. */
export function readFragmentToken(hash: string): string | null {
  const raw = hash.startsWith("#") ? hash.slice(1) : hash;
  if (!raw) return null;
  const token = new URLSearchParams(raw).get("token");
  return token && token.trim() ? token.trim() : null;
}

export function SignInHandoff({ projectId }: SignInHandoffProps) {
  const { t } = useTranslation("public");
  const { navigate } = useRouter();
  const queryClient = useQueryClient();
  const [state, setState] = useState<HandoffState>("working");
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;

    const token = readFragmentToken(window.location.hash);
    // Clear the fragment from the address bar and this history entry first,
    // whatever it held (FR-FBR-44).
    if (window.location.hash) {
      window.history.replaceState(
        window.history.state,
        "",
        window.location.pathname + window.location.search,
      );
    }
    if (!token) {
      setState("missing");
      return;
    }

    // No cleanup cancels this: StrictMode's simulated unmount would cancel the
    // only exchange there is, and the guarded second run starts none.
    exchangeBoardSession(projectId, token)
      .then(() => {
        // The viewer's state on every board query changed.
        void queryClient.invalidateQueries({ queryKey: ["public-board", projectId] });
        navigate(boardPath(projectId), { replace: true });
      })
      .catch(() => setState("failed"));
    // Empty deps: the exchange runs once per page load, guarded by `started`.
  }, []);

  return (
    <main
      className="public-board"
      aria-labelledby="board-handoff-title"
      aria-busy={state === "working" ? "true" : undefined}
    >
      <h1 id="board-handoff-title">{t("public.handoff.title")}</h1>
      {state === "working" ? (
        <p role="status">{t("public.handoff.working")}</p>
      ) : (
        <>
          <p role="status">
            {state === "missing"
              ? t("public.handoff.missing")
              : t("public.handoff.failed")}
          </p>
          <p>
            <Link to={boardPath(projectId)}>
              {t("public.handoff.continueAnonymous")}
            </Link>
          </p>
        </>
      )}
    </main>
  );
}
