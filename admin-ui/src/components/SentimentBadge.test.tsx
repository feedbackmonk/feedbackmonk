import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { SentimentBadge } from "./SentimentBadge";
import {
  SENTIMENT_LABELS,
  SENTIMENT_ORDER,
  type SentimentValue,
} from "../shared/types.gen";

const EXPECTED_ICONS: Record<SentimentValue, string> = {
  negative: "▽",
  neutral: "○",
  positive: "△",
};

describe("SentimentBadge — icon + label invariant (WCAG 1.4.1)", () => {
  it.each(SENTIMENT_ORDER)(
    "renders the visible text label for %s",
    (sentiment) => {
      render(<SentimentBadge sentiment={sentiment} />);
      expect(
        screen.getByText(SENTIMENT_LABELS[sentiment]),
      ).toBeInTheDocument();
    },
  );

  it.each(SENTIMENT_ORDER)(
    "renders a distinct aria-hidden icon glyph for %s",
    (sentiment) => {
      const { container } = render(<SentimentBadge sentiment={sentiment} />);
      const icon = container.querySelector(".sentiment-badge-icon");
      expect(icon).not.toBeNull();
      expect(icon).toHaveTextContent(EXPECTED_ICONS[sentiment]);
      expect(icon).toHaveAttribute("aria-hidden", "true");
    },
  );

  it("applies the sentiment-specific color class (color is paired, not sole)", () => {
    const { container } = render(<SentimentBadge sentiment="positive" />);
    const badge = container.querySelector(".sentiment-badge");
    expect(badge).toHaveClass("sentiment-positive");
  });
});
