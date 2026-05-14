import { useId } from "react";

interface UsageMeterProps {
  /** Human-readable resource label, e.g., "Projects" or "Monthly feedback". */
  label: string;
  /** Current count consumed. */
  current: number;
  /** Cap; `null` means unlimited (Pro / Self-host). */
  limit: number | null;
}

// Accessible usage meter — Stage 2 plan §UsageMeter. Renders a progressbar
// with WCAG 1.4.1 conformance: meaning is carried by both color (token) AND
// an explicit text status ("OK" / "Approaching cap" / "Over cap") so the
// state is decodable in monochrome / by screen readers.
//
// Color thresholds per P3 plan §Stage 2 deferred decisions:
//   green  (ok)        <70%
//   amber  (warn)      70–95%
//   red    (danger)    >95%
//
// Unlimited case (limit === null) renders an "unlimited" status with no bar.
export function UsageMeter({ label, current, limit }: UsageMeterProps) {
  const labelId = useId();

  if (limit === null) {
    return (
      <div className="usage-meter usage-meter-unlimited">
        <div className="usage-meter-row">
          <span id={labelId} className="usage-meter-label">
            {label}
          </span>
          <span className="usage-meter-counts">
            {current.toLocaleString()} / unlimited
          </span>
        </div>
      </div>
    );
  }

  const ratio = limit === 0 ? 1 : current / limit;
  const pct = Math.min(100, Math.max(0, Math.round(ratio * 100)));
  const state: "ok" | "warn" | "danger" =
    ratio > 0.95 ? "danger" : ratio >= 0.7 ? "warn" : "ok";
  const stateLabel =
    state === "danger"
      ? "Over cap"
      : state === "warn"
        ? "Approaching cap"
        : "OK";
  const valueText = `${current.toLocaleString()} of ${limit.toLocaleString()} ${label.toLowerCase()} used (${pct}%, ${stateLabel.toLowerCase()})`;

  return (
    <div className={`usage-meter usage-meter-${state}`}>
      <div className="usage-meter-row">
        <span id={labelId} className="usage-meter-label">
          {label}
        </span>
        <span className="usage-meter-counts">
          {current.toLocaleString()} / {limit.toLocaleString()}
          {" — "}
          <span className="usage-meter-state">{stateLabel}</span>
        </span>
      </div>
      <div
        role="progressbar"
        aria-labelledby={labelId}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-valuetext={valueText}
        className="usage-meter-bar"
      >
        <div className="usage-meter-fill" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}
