import { useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  claimDomain,
  fetchHosting,
  putSubdomain,
  releaseDomain,
  type HostingSettings as HostingShape,
} from "../../shared/hostingApi";
import { extractTierCapExceeded } from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { UpgradePrompt } from "./UpgradePrompt";

// /admin/settings/hosting — tenant subdomain + custom domains (FR-FBR-32/33).
//
// Two sections with deliberately different weight:
//
//   • The SUBDOMAIN is the tenant's public address. Changing it moves every
//     board link they have shared, so the control is explicit (edit → save)
//     rather than a live-updating field, and the consequence is stated.
//   • CUSTOM DOMAINS are the paid upgrade. When the tier does not carry the
//     capability we show the UpgradePrompt instead of a form — but the server
//     is the gate (402 at claim, and no certificate at issuance), never this
//     flag. A user who reaches the endpoint directly is refused there.
//
// Chrome mirrors BoardSettings / TierSettings.
export function HostingSettings() {
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const queryKey = ["admin-hosting"];

  const query = useQuery({ queryKey, queryFn: fetchHosting });
  const settings = query.data;

  return (
    <main className="hosting-settings-page" aria-labelledby="hosting-title">
      <header className="page-header">
        <h1 id="hosting-title">Public address</h1>
        <p className="muted">
          Where your feedback board and widget live on the internet.
        </p>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          Failed to load hosting settings.{" "}
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
        <>
          <SubdomainCard
            settings={settings}
            onSaved={(next) => queryClient.setQueryData(queryKey, next)}
            notify={notify}
          />
          <CustomDomainCard
            settings={settings}
            onChanged={() => queryClient.invalidateQueries({ queryKey })}
            notify={notify}
          />
        </>
      ) : null}
    </main>
  );
}

type Notify = (message: string, tone: "success" | "error") => void;

function SubdomainCard({
  settings,
  onSaved,
  notify,
}: {
  settings: HostingShape;
  onSaved: (next: HostingShape) => void;
  notify: Notify;
}) {
  const inputId = useId();
  const [value, setValue] = useState(settings.subdomain ?? "");

  const mutation = useMutation({
    mutationFn: (label: string | null) => putSubdomain(label),
    onSuccess: (next) => {
      onSaved(next);
      notify("Public address updated.", "success");
    },
    onError: (err: unknown) => {
      notify(errorMessage(err, "Could not update your public address."), "error");
    },
  });

  // No root domain configured means this deployment does not offer subdomains
  // at all (a self-host install). Saying so is better than showing a field that
  // silently produces no address.
  if (settings.cname_target === null && settings.subdomain === null) {
    return (
      <section className="settings-card" aria-labelledby="subdomain-heading">
        <h2 id="subdomain-heading">Subdomain</h2>
        <p className="muted">
          This deployment doesn’t use tenant subdomains — your board and widget
          are served on whatever hostname this instance answers on.
        </p>
      </section>
    );
  }

  const dirty = value.trim() !== (settings.subdomain ?? "");

  return (
    <section className="settings-card" aria-labelledby="subdomain-heading">
      <h2 id="subdomain-heading">Subdomain</h2>
      {settings.public_host ? (
        <p className="muted">
          Your board is at <code>https://{settings.public_host}</code>
        </p>
      ) : null}

      <form
        onSubmit={(e) => {
          e.preventDefault();
          const trimmed = value.trim();
          mutation.mutate(trimmed === "" ? null : trimmed);
        }}
      >
        <label htmlFor={inputId}>Your subdomain</label>
        <input
          id={inputId}
          type="text"
          value={value}
          spellCheck={false}
          autoCapitalize="none"
          disabled={mutation.isPending}
          onChange={(e) => setValue(e.target.value)}
          aria-describedby={`${inputId}-help`}
        />
        <p id={`${inputId}-help`} className="muted">
          Lowercase letters, numbers and hyphens; 3–63 characters. Changing it
          moves your board — any link you’ve already shared will stop working.
        </p>
        <button type="submit" disabled={!dirty || mutation.isPending}>
          {mutation.isPending ? "Saving…" : "Save"}
        </button>
      </form>
    </section>
  );
}

function CustomDomainCard({
  settings,
  onChanged,
  notify,
}: {
  settings: HostingShape;
  onChanged: () => void;
  notify: Notify;
}) {
  const inputId = useId();
  const [value, setValue] = useState("");

  const claim = useMutation({
    mutationFn: (domain: string) => claimDomain(domain),
    onSuccess: () => {
      setValue("");
      onChanged();
      notify("Domain added. Point your DNS at us to finish.", "success");
    },
    onError: (err: unknown) =>
      notify(errorMessage(err, "Could not add that domain."), "error"),
  });

  const release = useMutation({
    mutationFn: (id: string) => releaseDomain(id),
    onSuccess: () => {
      onChanged();
      notify("Domain removed.", "success");
    },
    onError: () => notify("Could not remove that domain.", "error"),
  });

  // Custom domains need a CNAME target, which only exists once this deployment
  // offers subdomains. Hide the whole section otherwise rather than offering a
  // feature with no instructions.
  if (settings.cname_target === null) {
    return null;
  }

  return (
    <section className="settings-card" aria-labelledby="custom-domain-heading">
      <h2 id="custom-domain-heading">Your own domain</h2>
      <p className="muted">
        Serve your board <em>and</em> your widget from a hostname you own, like{" "}
        <code>feedback.yourcompany.com</code>. A first-party endpoint isn’t
        blocked by tracker blocklists and passes a strict <code>connect-src</code>{" "}
        policy.
      </p>

      {settings.custom_domain_available ? (
        <>
          <ol className="muted">
            <li>Add your hostname below.</li>
            <li>
              Create a CNAME record pointing it at{" "}
              <code>{settings.cname_target}</code>.
            </li>
            <li>
              We issue the HTTPS certificate automatically once DNS resolves.
            </li>
          </ol>

          <form
            onSubmit={(e) => {
              e.preventDefault();
              const trimmed = value.trim();
              if (trimmed) claim.mutate(trimmed);
            }}
          >
            <label htmlFor={inputId}>Hostname</label>
            <input
              id={inputId}
              type="text"
              value={value}
              placeholder="feedback.yourcompany.com"
              spellCheck={false}
              autoCapitalize="none"
              disabled={claim.isPending}
              onChange={(e) => setValue(e.target.value)}
            />
            <button type="submit" disabled={!value.trim() || claim.isPending}>
              {claim.isPending ? "Adding…" : "Add domain"}
            </button>
          </form>
        </>
      ) : (
        <UpgradePrompt
          currentTier={settings.tier}
          message="Custom domains are available on the Pro plan and above."
        />
      )}

      {settings.domains.length > 0 ? (
        <table className="domain-table">
          <caption className="visually-hidden">Your custom domains</caption>
          <thead>
            <tr>
              <th scope="col">Domain</th>
              <th scope="col">Status</th>
              <th scope="col">
                <span className="visually-hidden">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {settings.domains.map((d) => (
              <tr key={d.id}>
                <td>
                  <code>{d.domain}</code>
                </td>
                <td>
                  {/* WCAG 1.4.1: status is conveyed by the word, not by colour
                      alone — the same dual-encoding rule UsageMeter follows. */}
                  {d.status === "active" ? "Live" : "Waiting for DNS"}
                </td>
                <td>
                  <button
                    type="button"
                    disabled={release.isPending}
                    onClick={() => release.mutate(d.id)}
                  >
                    Remove
                    <span className="visually-hidden"> {d.domain}</span>
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </section>
  );
}

/**
 * Prefer the server's own message.
 *
 * A 402 here is the FR-FBR-33 tier gate firing; the shared
 * `extractTierCapExceeded` helper already understands that envelope, so the
 * upgrade hint the server sends is what the user sees rather than a generic
 * failure.
 */
function errorMessage(err: unknown, fallback: string): string {
  const capped = extractTierCapExceeded(err);
  if (capped) return capped.upgrade_hint;
  const detail = (err as { response?: { data?: { error?: unknown } } })?.response
    ?.data?.error;
  return typeof detail === "string" && detail.length > 0 ? detail : fallback;
}
