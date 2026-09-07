import { useQuery } from "@tanstack/react-query";
import { fetchTierStatus } from "../../shared/ApiClient";
import { type TierQuotas } from "../../shared/types.gen";
import { UsageMeter } from "./UsageMeter";
import { UpgradePrompt } from "./UpgradePrompt";
import { useTranslation } from "../../i18n";
import { useAdminLabels } from "../../i18n/useAdminLabels";
import { useLocale } from "../../i18n/useLocale";

interface CapabilityRow {
  label: string;
  enabled: boolean;
  /**
   * Capability is exposed in the tier matrix but the implementation is
   * deferred. Footnoted so the user knows the flag won't take effect yet.
   */
  notImplemented?: boolean;
}

function capabilities(
  quotas: TierQuotas,
  t: (key: string) => string,
): CapabilityRow[] {
  return [
    {
      label: t("admin.tierSettings.capabilities.customBranding"),
      enabled: quotas.custom_branding,
    },
    // FR-FBR-33 shipped: the flag now has an implementation behind it
    // (/admin/settings/hosting), so the `notImplemented` footnote is gone. It
    // had been true-and-unimplemented since P3 — the marketing pricing card
    // advertised it while nothing enforced or delivered it.
    {
      label: t("admin.tierSettings.capabilities.customDomain"),
      enabled: quotas.custom_domain,
    },
    {
      label: t("admin.tierSettings.capabilities.euResidency"),
      enabled: quotas.eu_residency,
      notImplemented: quotas.eu_residency,
    },
    {
      label: t("admin.tierSettings.capabilities.freeTierFooter"),
      // Inverted semantics: the footer is a Free-tier *constraint*, so the
      // capability "no free-tier footer" is enabled when footer_text is null.
      enabled: quotas.footer_text === null,
    },
  ];
}

// /admin/settings/tier — Stage 2 plan §TierSettings. Renders:
//   - Current tier card (Free / Starter / Pro / Self-host)
//   - UsageMeter for projects + monthly feedback
//   - Capability matrix per Contract C19 (custom branding, custom domain,
//     EU residency, free-tier footer)
//   - UpgradePrompt CTA (hidden on Self-host)
//
// Data shape consumed verbatim from Contract C17 (`fetchTierStatus`).
export function TierSettings() {
  const { t } = useTranslation("admin");
  const adminLabels = useAdminLabels();
  const { locale } = useLocale();
  const query = useQuery({
    queryKey: ["admin-tier-status"],
    queryFn: fetchTierStatus,
  });

  return (
    <main className="tier-settings-page">
      <header className="page-header">
        <h1>{t("admin.tierSettings.title")}</h1>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.tierSettings.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted">{t("admin.common.loading")}</p>
      ) : query.data ? (
        <>
          <section
            className="tier-card"
            aria-labelledby="tier-card-heading"
          >
            <h2 id="tier-card-heading">
              {t("admin.tierSettings.currentPlanLabel")}{" "}
              <span className="tier-badge">
                {adminLabels.tier(query.data.tier)}
              </span>
            </h2>
          </section>

          <section className="tier-usage" aria-label={t("admin.tierSettings.usageHeading")}>
            <h3>{t("admin.tierSettings.usageHeading")}</h3>
            <UsageMeter
              label={t("admin.tierSettings.projectsLabel")}
              current={query.data.usage.projects}
              limit={query.data.quotas.projects_per_org}
            />
            <UsageMeter
              label={t("admin.tierSettings.monthlyFeedbackLabel")}
              current={query.data.usage.feedback_monthly}
              limit={query.data.quotas.monthly_feedback_volume}
            />
            <p className="muted tier-usage-period">
              {t("admin.tierSettings.usagePeriod", {
                date: new Date(query.data.usage.period_start).toLocaleDateString(
                  locale,
                ),
              })}
            </p>
          </section>

          <section
            className="tier-capabilities"
            aria-label={t("admin.tierSettings.capabilitiesHeading")}
          >
            <h3>{t("admin.tierSettings.capabilitiesHeading")}</h3>
            <ul className="capability-list">
              {capabilities(query.data.quotas, t).map((cap) => (
                <li
                  key={cap.label}
                  className={
                    cap.enabled ? "capability-on" : "capability-off"
                  }
                >
                  <span
                    aria-hidden="true"
                    className="capability-mark"
                  >
                    {cap.enabled ? "✓" : "✗"}
                  </span>
                  <span className="visually-hidden">
                    {cap.enabled
                      ? t("admin.tierSettings.enabledPrefix")
                      : t("admin.tierSettings.disabledPrefix")}
                  </span>
                  <span>{cap.label}</span>
                  {cap.notImplemented ? (
                    <span className="capability-note">
                      {" "}
                      {t("admin.tierSettings.notImplementedNote")}
                    </span>
                  ) : null}
                </li>
              ))}
            </ul>
          </section>

          <UpgradePrompt currentTier={query.data.tier} />
        </>
      ) : null}
    </main>
  );
}
