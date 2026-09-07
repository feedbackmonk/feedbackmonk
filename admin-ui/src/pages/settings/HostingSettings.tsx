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
import { Trans } from "react-i18next";
import { useTranslation } from "../../i18n";

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
  const { t } = useTranslation("admin");
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const queryKey = ["admin-hosting"];

  const query = useQuery({ queryKey, queryFn: fetchHosting });
  const settings = query.data;

  return (
    <main className="hosting-settings-page" aria-labelledby="hosting-title">
      <header className="page-header">
        <h1 id="hosting-title">{t("admin.hostingSettings.title")}</h1>
        <p className="muted">{t("admin.hostingSettings.intro")}</p>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.hostingSettings.loadError")}{" "}
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
  const { t } = useTranslation("admin");
  const inputId = useId();
  const [value, setValue] = useState(settings.subdomain ?? "");

  const mutation = useMutation({
    mutationFn: (label: string | null) => putSubdomain(label),
    onSuccess: (next) => {
      onSaved(next);
      notify(t("admin.hostingSettings.subdomain.saved"), "success");
    },
    onError: (err: unknown) => {
      notify(
        errorMessage(err, t("admin.hostingSettings.subdomain.saveFailed")),
        "error",
      );
    },
  });

  // No root domain configured means this deployment does not offer subdomains
  // at all (a self-host install). Saying so is better than showing a field that
  // silently produces no address.
  if (settings.cname_target === null && settings.subdomain === null) {
    return (
      <section className="settings-card" aria-labelledby="subdomain-heading">
        <h2 id="subdomain-heading">{t("admin.hostingSettings.subdomain.heading")}</h2>
        <p className="muted">{t("admin.hostingSettings.subdomain.noSubdomains")}</p>
      </section>
    );
  }

  const dirty = value.trim() !== (settings.subdomain ?? "");

  return (
    <section className="settings-card" aria-labelledby="subdomain-heading">
      <h2 id="subdomain-heading">{t("admin.hostingSettings.subdomain.heading")}</h2>
      {settings.public_host ? (
        <p className="muted">
          <Trans
            i18nKey="admin.hostingSettings.subdomain.boardAt"
            t={t}
            values={{ host: settings.public_host }}
            components={{ code: <code /> }}
          />
        </p>
      ) : null}

      <form
        onSubmit={(e) => {
          e.preventDefault();
          const trimmed = value.trim();
          mutation.mutate(trimmed === "" ? null : trimmed);
        }}
      >
        <label htmlFor={inputId}>{t("admin.hostingSettings.subdomain.fieldLabel")}</label>
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
          {t("admin.hostingSettings.subdomain.help")}
        </p>
        <button type="submit" disabled={!dirty || mutation.isPending}>
          {mutation.isPending
            ? t("admin.hostingSettings.saving")
            : t("admin.common.save")}
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
  const { t } = useTranslation("admin");
  const inputId = useId();
  const [value, setValue] = useState("");

  const claim = useMutation({
    mutationFn: (domain: string) => claimDomain(domain),
    onSuccess: () => {
      setValue("");
      onChanged();
      notify(t("admin.hostingSettings.customDomain.added"), "success");
    },
    onError: (err: unknown) =>
      notify(
        errorMessage(err, t("admin.hostingSettings.customDomain.addFailed")),
        "error",
      ),
  });

  const release = useMutation({
    mutationFn: (id: string) => releaseDomain(id),
    onSuccess: () => {
      onChanged();
      notify(t("admin.hostingSettings.customDomain.removed"), "success");
    },
    onError: () =>
      notify(t("admin.hostingSettings.customDomain.removeFailed"), "error"),
  });

  // Custom domains need a CNAME target, which only exists once this deployment
  // offers subdomains. Hide the whole section otherwise rather than offering a
  // feature with no instructions.
  if (settings.cname_target === null) {
    return null;
  }

  return (
    <section className="settings-card" aria-labelledby="custom-domain-heading">
      <h2 id="custom-domain-heading">
        {t("admin.hostingSettings.customDomain.heading")}
      </h2>
      <p className="muted">
        <Trans
          i18nKey="admin.hostingSettings.customDomain.intro"
          t={t}
          components={{
            em: <em />,
            code1: <code />,
            code2: <code />,
          }}
        />
      </p>

      {settings.custom_domain_available ? (
        <>
          <ol className="muted">
            <li>{t("admin.hostingSettings.customDomain.step1")}</li>
            <li>
              <Trans
                i18nKey="admin.hostingSettings.customDomain.step2"
                t={t}
                values={{ target: settings.cname_target }}
                components={{ code: <code /> }}
              />
            </li>
            <li>{t("admin.hostingSettings.customDomain.step3")}</li>
          </ol>

          <form
            onSubmit={(e) => {
              e.preventDefault();
              const trimmed = value.trim();
              if (trimmed) claim.mutate(trimmed);
            }}
          >
            <label htmlFor={inputId}>
              {t("admin.hostingSettings.customDomain.hostnameLabel")}
            </label>
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
              {claim.isPending
                ? t("admin.hostingSettings.customDomain.adding")
                : t("admin.hostingSettings.customDomain.addDomain")}
            </button>
          </form>
        </>
      ) : (
        <UpgradePrompt
          currentTier={settings.tier}
          message={t("admin.hostingSettings.customDomain.upgradeMessage")}
        />
      )}

      {settings.domains.length > 0 ? (
        <table className="domain-table">
          <caption className="visually-hidden">
            {t("admin.hostingSettings.customDomain.tableCaption")}
          </caption>
          <thead>
            <tr>
              <th scope="col">{t("admin.hostingSettings.customDomain.columns.domain")}</th>
              <th scope="col">{t("admin.hostingSettings.customDomain.columns.status")}</th>
              <th scope="col">
                <span className="visually-hidden">
                  {t("admin.hostingSettings.customDomain.columns.actions")}
                </span>
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
                  {d.status === "active"
                    ? t("admin.hostingSettings.customDomain.statusLive")
                    : t("admin.hostingSettings.customDomain.statusWaiting")}
                </td>
                <td>
                  <button
                    type="button"
                    disabled={release.isPending}
                    onClick={() => release.mutate(d.id)}
                  >
                    {t("admin.hostingSettings.customDomain.remove")}
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
