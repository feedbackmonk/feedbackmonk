import { useQuery } from "@tanstack/react-query";
import { listRunnerTokens } from "../../shared/ApiClient";
import { RunnerTokenCard } from "./RunnerTokenCard";

// The registered runner tokens for a project + their revocation state. Tokens
// are *optional bookkeeping* (issuance is client-side), so an empty list is the
// normal initial state — it does NOT mean no runner can authenticate.
export function RunnerTokensList({ projectId }: { projectId: string }) {
  const query = useQuery({
    queryKey: ["runner-tokens", projectId],
    queryFn: () => listRunnerTokens(projectId),
  });

  const items = query.data?.items ?? [];

  return (
    <section className="runner-tokens-list" aria-labelledby="runner-tokens-heading">
      <h2 id="runner-tokens-heading">Registered tokens</h2>

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          Loading tokens…
        </p>
      ) : query.isError ? (
        <div role="alert" className="error-block">
          Failed to load runner tokens.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      ) : items.length === 0 ? (
        <div className="empty-state">
          <p>
            No tokens registered yet. Registering a token here is optional — it
            only adds it to this list for visibility and revocation. A runner can
            authenticate the moment it mints a token from a registered runner key.
          </p>
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
