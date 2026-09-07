import { type SentimentValue } from "../shared/types.gen";
import { useLabels } from "../i18n/useLabels";

const SENTIMENT_ICONS: Record<SentimentValue, string> = {
  negative: "▽",
  neutral: "○",
  positive: "△",
};

// Color alone never carries meaning (WCAG 1.4.1); the icon + label pair is
// load-bearing for non-sighted and color-deficient users alike. Mirrors
// StatusBadge exactly (icon aria-hidden, visible text label).
export function SentimentBadge({ sentiment }: { sentiment: SentimentValue }) {
  const labels = useLabels();
  return (
    <span className={`sentiment-badge sentiment-${sentiment}`}>
      <span aria-hidden="true" className="sentiment-badge-icon">
        {SENTIMENT_ICONS[sentiment]}
      </span>
      <span className="sentiment-badge-label">
        {labels.sentiment(sentiment)}
      </span>
    </span>
  );
}
