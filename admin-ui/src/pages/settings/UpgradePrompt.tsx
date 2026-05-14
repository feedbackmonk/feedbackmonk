import { TIER_LABELS, type Tier } from "../../shared/types.gen";

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
  if (currentTier === "self_host") return null;

  const defaultMessage =
    currentTier === "free"
      ? `You're on ${TIER_LABELS.free}. Upgrade to Starter for 3 projects per org and 500 monthly feedback.`
      : currentTier === "starter"
        ? `You're on ${TIER_LABELS.starter}. Upgrade to Pro for unlimited projects, 10,000 monthly feedback, custom domain, and EU residency.`
        : `You're on ${TIER_LABELS.pro}. Self-hosting available for unlimited usage.`;

  return (
    <div className="upgrade-prompt" role="region" aria-label="Upgrade options">
      <p className="upgrade-prompt-message">{message ?? defaultMessage}</p>
      <a
        className="upgrade-prompt-button"
        href="mailto:support@feedbackmonk.com?subject=Upgrade%20request"
      >
        Contact support to upgrade
      </a>
    </div>
  );
}
