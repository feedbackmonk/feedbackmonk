import { useQuery } from "@tanstack/react-query";
import { fetchTierStatus } from "../../shared/ApiClient";
import { TIER_LABELS, type TierQuotas } from "../../shared/types.gen";
import { UsageMeter } from "./UsageMeter";
import { UpgradePrompt } from "./UpgradePrompt";

interface CapabilityRow {
  label: string;
  enabled: boolean;
  /**
   * Capability is exposed in the tier matrix but the implementation is
   * deferred. Footnoted so the user knows the flag won't take effect yet.
   */
  notImplemented?: boolean;
}

function capabilities(quotas: TierQuotas): CapabilityRow[] {
  return [
    { label: "Custom branding", enabled: quotas.custom_branding },
    {
      label: "Custom domain",
      enabled: quotas.custom_domain,
      notImplemented: quotas.custom_domain,
    },
    {
      label: "EU data residency",
      enabled: quotas.eu_residency,
      notImplemented: quotas.eu_residency,
    },
    {
      label: "Free-tier footer (“powered by feedbackmonk”)",
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
  const query = useQuery({
    queryKey: ["admin-tier-status"],
    queryFn: fetchTierStatus,
  });

  return (
    <main className="tier-settings-page">
      <header className="page-header">
        <h1>Plan & usage</h1>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          Failed to load tier status.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted">Loading…</p>
      ) : query.data ? (
        <>
          <section
            className="tier-card"
            aria-labelledby="tier-card-heading"
          >
            <h2 id="tier-card-heading">
              Current plan:{" "}
              <span className="tier-badge">{TIER_LABELS[query.data.tier]}</span>
            </h2>
          </section>

          <section className="tier-usage" aria-label="Usage">
            <h3>Usage</h3>
            <UsageMeter
              label="Projects"
              current={query.data.usage.projects}
              limit={query.data.quotas.projects_per_org}
            />
            <UsageMeter
              label="Monthly feedback"
              current={query.data.usage.feedback_monthly}
              limit={query.data.quotas.monthly_feedback_volume}
            />
            <p className="muted tier-usage-period">
              Rolling 30-day window since{" "}
              <time dateTime={query.data.usage.period_start}>
                {new Date(query.data.usage.period_start).toLocaleDateString()}
              </time>
              .
            </p>
          </section>

          <section className="tier-capabilities" aria-label="Capabilities">
            <h3>Capabilities</h3>
            <ul className="capability-list">
              {capabilities(query.data.quotas).map((cap) => (
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
                    {cap.enabled ? "Enabled: " : "Disabled: "}
                  </span>
                  <span>{cap.label}</span>
                  {cap.notImplemented ? (
                    <span className="capability-note">
                      {" "}
                      (configurable; implementation pending)
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
