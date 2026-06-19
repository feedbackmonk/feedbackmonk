import { useMutation, useQueryClient } from "@tanstack/react-query";
import { revokeRunnerToken } from "../../shared/ApiClient";
import { formatAbsolute, formatRelative } from "../../shared/format";
import { useToast } from "../../components/Toast";
import type { RunnerTokenView } from "../../shared/types.gen";

type TokenLifecycle = "active" | "revoked" | "expired";

// Derive the display lifecycle from the two server fields. Revocation wins over
// expiry (a revoked token is dead regardless of exp); expiry is informational
// only — the server's `verify_runner_token` enforces the real `exp` check.
export function tokenLifecycle(
  token: RunnerTokenView,
  now: Date = new Date(),
): TokenLifecycle {
  if (token.revoked_at) return "revoked";
  if (token.expires_at && new Date(token.expires_at).getTime() < now.getTime()) {
    return "expired";
  }
  return "active";
}

const LIFECYCLE_LABELS: Record<TokenLifecycle, string> = {
  active: "Active",
  revoked: "Revoked",
  expired: "Expired",
};

// One registered runner token. The `jti` is the load-bearing identity — the
// label is owner-supplied UNTRUSTED text, rendered as an escaped React text
// node only (never dangerouslySetInnerHTML). Revoke writes the jti to the
// append-only denylist; a revoked token is rejected even before its `exp`.
export function RunnerTokenCard({
  projectId,
  token,
}: {
  projectId: string;
  token: RunnerTokenView;
}) {
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const lifecycle = tokenLifecycle(token);
  const revoked = lifecycle === "revoked";

  const mutation = useMutation({
    mutationFn: () => revokeRunnerToken(projectId, token.jti),
    onSuccess: () => {
      notify(`Token “${token.label}” revoked.`, "success");
      queryClient.invalidateQueries({
        queryKey: ["runner-tokens", projectId],
      });
    },
    onError: () => notify("Revoke failed. Please try again.", "error"),
  });

  function onRevoke() {
    if (revoked || mutation.isPending) return;
    if (
      typeof window !== "undefined" &&
      !window.confirm(
        `Revoke token “${token.label}”? This cannot be undone — the runner ` +
          `using it must mint a new token to keep working.`,
      )
    ) {
      return;
    }
    mutation.mutate();
  }

  return (
    <li className={`runner-token-card runner-token-${lifecycle}`}>
      <div className="runner-token-main">
        <span className="runner-token-label">{token.label}</span>
        <span
          className={`runner-token-status runner-token-status-${lifecycle}`}
        >
          {LIFECYCLE_LABELS[lifecycle]}
        </span>
      </div>
      <dl className="runner-token-meta">
        <dt>Token ID (jti)</dt>
        <dd>
          <code>{token.jti}</code>
        </dd>
        <dt>Registered</dt>
        <dd>
          <time dateTime={token.created_at}>
            {formatRelative(token.created_at)}
          </time>
        </dd>
        <dt>Expires</dt>
        <dd>
          {token.expires_at ? (
            <time dateTime={token.expires_at}>
              {formatAbsolute(token.expires_at)}
            </time>
          ) : (
            <span className="muted">no expiry recorded</span>
          )}
        </dd>
        {token.revoked_at ? (
          <>
            <dt>Revoked</dt>
            <dd>
              <time dateTime={token.revoked_at}>
                {formatAbsolute(token.revoked_at)}
              </time>
            </dd>
          </>
        ) : null}
      </dl>
      <div className="runner-token-actions">
        <button
          type="button"
          className="ap-danger"
          onClick={onRevoke}
          disabled={revoked || mutation.isPending}
        >
          {revoked
            ? "Revoked"
            : mutation.isPending
              ? "Revoking…"
              : "Revoke"}
        </button>
      </div>
    </li>
  );
}
