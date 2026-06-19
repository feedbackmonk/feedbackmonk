import { useId } from "react";
import {
  SENTIMENT_LABELS,
  SENTIMENT_ORDER,
  type SentimentTrendResponse,
} from "../shared/types.gen";
import { formatAbsolute } from "../shared/format";

interface SentimentTrendChartProps {
  data: SentimentTrendResponse;
}

// Dependency-free stacked-bar satisfaction chart. NO charting library is
// installed (and none may be added) — this is a hand-rolled inline SVG, plus a
// headline summary line and a visually-hidden data table so the figure is
// never color/visual-only (WCAG 1.4.1 / 1.1.1). Matches the a11y rigor of
// UsageMeter: a `role`/aria-described figure with a text-decodable fallback.
//
// Each bucket renders one vertical bar with negative / neutral / positive
// segments using the --sentiment-* color tokens; the segment ORDER is fixed
// (SENTIMENT_ORDER) so the stack reads consistently. Bar heights are scaled to
// the busiest bucket so a sparse series still reads clearly.

const CHART_HEIGHT = 120; // px — plot area height (excludes axis label band)
const BAR_WIDTH = 18;
const BAR_GAP = 10;
const PAD_X = 4;
const PAD_TOP = 4;

export function SentimentTrendChart({ data }: SentimentTrendChartProps) {
  const captionId = useId();
  const tableId = useId();
  const { buckets, totals, bucket } = data;

  // Empty state — sparse series with no sentiment-bearing periods.
  if (buckets.length === 0 || totals.total === 0) {
    return (
      <figure className="sentiment-trend sentiment-trend-empty">
        <figcaption className="muted">No sentiment data yet</figcaption>
      </figure>
    );
  }

  const pctPositive = Math.round((totals.positive / totals.total) * 100);
  const maxBucketTotal = Math.max(...buckets.map((b) => b.total), 1);

  const innerWidth = buckets.length * BAR_WIDTH + (buckets.length - 1) * BAR_GAP;
  const svgWidth = innerWidth + PAD_X * 2;
  const svgHeight = CHART_HEIGHT + PAD_TOP;

  const summary = `${totals.total.toLocaleString()} classified feedback — ${pctPositive}% positive (${totals.positive.toLocaleString()} positive, ${totals.neutral.toLocaleString()} neutral, ${totals.negative.toLocaleString()} negative), bucketed by ${bucket}.`;

  return (
    <figure className="sentiment-trend" aria-describedby={tableId}>
      <figcaption id={captionId} className="sentiment-trend-summary">
        <span className="sentiment-trend-pct">{pctPositive}% positive</span>{" "}
        <span className="muted">
          across {totals.total.toLocaleString()} classified
        </span>
      </figcaption>

      <svg
        className="sentiment-trend-svg"
        width={svgWidth}
        height={svgHeight}
        viewBox={`0 0 ${svgWidth} ${svgHeight}`}
        role="img"
        aria-label={summary}
        preserveAspectRatio="xMinYMin meet"
      >
        {buckets.map((b, i) => {
          const x = PAD_X + i * (BAR_WIDTH + BAR_GAP);
          const barHeight = (b.total / maxBucketTotal) * CHART_HEIGHT;
          let yCursor = PAD_TOP + CHART_HEIGHT - barHeight;
          return (
            <g key={b.bucket_start}>
              {SENTIMENT_ORDER.map((s) => {
                const count = b[s];
                if (count === 0) return null;
                const segHeight = (count / b.total) * barHeight;
                const y = yCursor;
                yCursor += segHeight;
                return (
                  <rect
                    key={s}
                    className={`sentiment-trend-seg sentiment-fill-${s}`}
                    x={x}
                    y={y}
                    width={BAR_WIDTH}
                    height={segHeight}
                  >
                    <title>
                      {`${formatAbsolute(b.bucket_start)}: ${count} ${SENTIMENT_LABELS[s].toLowerCase()}`}
                    </title>
                  </rect>
                );
              })}
            </g>
          );
        })}
      </svg>

      {/* Visually-hidden data table — the non-visual fallback so the chart is
          fully decodable by screen readers and in monochrome. */}
      <table id={tableId} className="visually-hidden">
        <caption>{summary}</caption>
        <thead>
          <tr>
            <th scope="col">Period start</th>
            <th scope="col">{SENTIMENT_LABELS.negative}</th>
            <th scope="col">{SENTIMENT_LABELS.neutral}</th>
            <th scope="col">{SENTIMENT_LABELS.positive}</th>
            <th scope="col">Total</th>
          </tr>
        </thead>
        <tbody>
          {buckets.map((b) => (
            <tr key={b.bucket_start}>
              <th scope="row">{formatAbsolute(b.bucket_start)}</th>
              <td>{b.negative}</td>
              <td>{b.neutral}</td>
              <td>{b.positive}</td>
              <td>{b.total}</td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr>
            <th scope="row">Totals</th>
            <td>{totals.negative}</td>
            <td>{totals.neutral}</td>
            <td>{totals.positive}</td>
            <td>{totals.total}</td>
          </tr>
        </tfoot>
      </table>

      {/* Visible legend — icon-free here (the table carries the full a11y
          payload); the segment classes pair color with the legend text label,
          so meaning is not color-only. */}
      <ul className="sentiment-trend-legend" aria-hidden="true">
        {SENTIMENT_ORDER.map((s) => (
          <li key={s} className={`sentiment-legend-${s}`}>
            <span className={`sentiment-legend-swatch sentiment-fill-${s}`} />
            {SENTIMENT_LABELS[s]}
          </li>
        ))}
      </ul>
    </figure>
  );
}
