import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { SentimentTrendChart } from "./SentimentTrendChart";
import type { SentimentTrendResponse } from "../shared/types.gen";

const SAMPLE: SentimentTrendResponse = {
  bucket: "week",
  since: "2026-03-21T00:00:00Z",
  buckets: [
    {
      bucket_start: "2026-03-16T00:00:00Z",
      negative: 2,
      neutral: 5,
      positive: 11,
      total: 18,
    },
    {
      bucket_start: "2026-03-23T00:00:00Z",
      negative: 2,
      neutral: 4,
      positive: 9,
      total: 15,
    },
  ],
  totals: { negative: 4, neutral: 9, positive: 20, total: 33 },
};

const EMPTY: SentimentTrendResponse = {
  bucket: "week",
  since: "2026-03-21T00:00:00Z",
  buckets: [],
  totals: { negative: 0, neutral: 0, positive: 0, total: 0 },
};

describe("SentimentTrendChart", () => {
  it("renders one stacked bar group per bucket", () => {
    const { container } = render(<SentimentTrendChart data={SAMPLE} />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    // One <g> per bucket.
    expect(svg!.querySelectorAll("g")).toHaveLength(SAMPLE.buckets.length);
    // Each bucket here has all three sentiments non-zero ⇒ 3 segments each.
    expect(container.querySelectorAll(".sentiment-trend-seg")).toHaveLength(6);
  });

  it("renders a headline summary with the % positive", () => {
    render(<SentimentTrendChart data={SAMPLE} />);
    // 20 / 33 ≈ 61% positive.
    expect(screen.getByText("61% positive")).toBeInTheDocument();
    // Total appears in both the visible caption ("across N classified") and the
    // visually-hidden table caption — both are valid; assert at least one.
    expect(screen.getAllByText(/33 classified/).length).toBeGreaterThan(0);
  });

  it("provides a non-visual data-table fallback with per-bucket rows + totals", () => {
    render(<SentimentTrendChart data={SAMPLE} />);
    const table = screen.getByRole("table");
    // Header + 2 data rows + totals footer row = 4 rows.
    expect(within(table).getAllByRole("row")).toHaveLength(4);
    // Totals footer carries the aggregate positive count.
    expect(within(table).getByRole("rowheader", { name: "Totals" })).toBeInTheDocument();
  });

  it("omits zero-count segments (sparse-friendly)", () => {
    const oneSided: SentimentTrendResponse = {
      bucket: "week",
      since: "2026-03-21T00:00:00Z",
      buckets: [
        {
          bucket_start: "2026-03-16T00:00:00Z",
          negative: 0,
          neutral: 0,
          positive: 7,
          total: 7,
        },
      ],
      totals: { negative: 0, neutral: 0, positive: 7, total: 7 },
    };
    const { container } = render(<SentimentTrendChart data={oneSided} />);
    // Only the positive segment is drawn.
    const segs = container.querySelectorAll(".sentiment-trend-seg");
    expect(segs).toHaveLength(1);
    expect(segs[0]).toHaveClass("sentiment-fill-positive");
  });

  it("renders an empty state when there are no buckets", () => {
    render(<SentimentTrendChart data={EMPTY} />);
    expect(screen.getByText(/No sentiment data yet/i)).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });
});
