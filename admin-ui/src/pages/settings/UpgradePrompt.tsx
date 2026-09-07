import { type Tier } from "../../shared/types.gen";
import { useTranslation } from "../../i18n";
import { useAdminLabels } from "../../i18n/useAdminLabels";

interface UpgradePromptProps {
  /** Current tier — drives whether the upgrade button renders. */
  currentTier: Tier;
  /** Optional override for the CTA copy. Defaults to a generic upsell. */
  message?: string;
}

// Reusable upgrade-CTA — Stage 2 plan §UpgradePrompt. Polar billing is
// DEFERRED per DEC-FBR-DEFER-01, so the button copy is the explicit
// "Contact support to upgrade" stub, NOT "Upgrade" — no checkout flow is
// implied. When Polar lands, this component gets a `<Link>` to the Polar
// checkout URL instead of the mailto fallback.
//
// Self-host tier renders no CTA at all (no upsell from the cap-free tier).
export function UpgradePrompt({ currentTier, message }: UpgradePromptProps) {
  const { t } = useTranslation("admin");
  const adminLabels = useAdminLabels();
  if (currentTier === "self_host") return null;

  const defaultMessage =
    currentTier === "free"
      ? t("admin.upgradePrompt.defaultMessage.free", {
          tier: adminLabels.tier("free"),
        })
      : currentTier === "starter"
        ? t("admin.upgradePrompt.defaultMessage.starter", {
            tier: adminLabels.tier("starter"),
          })
        : t("admin.upgradePrompt.defaultMessage.pro", {
            tier: adminLabels.tier("pro"),
          });

  return (
    <div
      className="upgrade-prompt"
      role="region"
      aria-label={t("admin.upgradePrompt.aria")}
    >
      <p className="upgrade-prompt-message">{message ?? defaultMessage}</p>
      <a
        className="upgrade-prompt-button"
        href="mailto:support@feedbackmonk.com?subject=Upgrade%20request"
      >
        {t("admin.upgradePrompt.cta")}
      </a>
    </div>
  );
}
