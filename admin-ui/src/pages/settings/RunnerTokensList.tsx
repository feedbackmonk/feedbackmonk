import { useQuery } from "@tanstack/react-query";
import { listRunnerTokens } from "../../shared/ApiClient";
import { RunnerTokenCard } from "./RunnerTokenCard";
import { useTranslation } from "../../i18n";

// The registered runner tokens for a project + their revocation state. Tokens
// are *optional bookkeeping* (issuance is client-side), so an empty list is the
// normal initial state — it does NOT mean no runner can authenticate.
export function RunnerTokensList({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
  const query = useQuery({
    queryKey: ["runner-tokens", projectId],
    queryFn: () => listRunnerTokens(projectId),
  });

  const items = query.data?.items ?? [];

  return (
    <section className="runner-tokens-list" aria-labelledby="runner-tokens-heading">
      <h2 id="runner-tokens-heading">{t("admin.runnerTokensList.heading")}</h2>

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          {t("admin.runnerTokensList.loading")}
        </p>
      ) : query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.runnerTokensList.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : items.length === 0 ? (
        <div className="empty-state">
          <p>{t("admin.runnerTokensList.empty")}</p>
        </div>
      ) : (
        <ul className="runner-token-cards">
          {items.map((token) => (
            <RunnerTokenCard
              key={token.jti}
              projectId={projectId}
              token={token}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
