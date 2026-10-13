import { useTranslation } from "../../i18n";
import { LanguageSwitcher } from "../../i18n/LanguageSwitcher";
import { Link } from "../../shared/router";
import { boardPath } from "./boardErrors";

interface SignInPromptProps {
  projectId: string;
  /** The page's own heading, so the prompt sits under the page it replaces. */
  heading: string;
  /** Offer a way back to the board (pages below the board list). */
  showBoardLink?: boolean;
}

// What a board page shows for `401 {"error":"SignInRequired"}` (FR-FBR-43).
//
// The board cannot sign anyone in by itself: the only identity feedbackmonk
// ever has for an end user is the JWT the customer signs (DEC-FBR-04), and it
// reaches the board only through the widget's one-time handoff (FR-FBR-44/45).
// So the prompt says where to go, rather than offering a login form. It is a
// normal state, not an error: no `role="alert"`, no Retry.
export function SignInPrompt({
  projectId,
  heading,
  showBoardLink = false,
}: SignInPromptProps) {
  const { t } = useTranslation("public");
  return (
    <main className="public-board" aria-labelledby="board-sign-in-title">
      <header>
        <h1 id="board-sign-in-title">{heading}</h1>
        <LanguageSwitcher />
      </header>
      <section aria-labelledby="board-sign-in-prompt">
        <h2 id="board-sign-in-prompt">{t("public.signIn.title")}</h2>
        <p>{t("public.signIn.body")}</p>
        {showBoardLink ? (
          <p>
            <Link to={boardPath(projectId)}>{t("public.signIn.backToBoard")}</Link>
          </p>
        ) : null}
      </section>
    </main>
  );
}
