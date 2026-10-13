// Bands the priority fit's per-item uncertainty (Contract C44: the standard
// error of the fitted strength) into three words an owner can read at a glance.
// The number is still shown beside the band; the band only says how much to
// trust a position. A value that is not a finite number is treated as the least
// certain, never the most.

export type UncertaintyBand = "low" | "medium" | "high";

export const UNCERTAINTY_MEDIUM_FROM = 0.25;
export const UNCERTAINTY_HIGH_FROM = 0.6;

export function uncertaintyBand(value: number): UncertaintyBand {
  if (!Number.isFinite(value)) return "high";
  if (value < UNCERTAINTY_MEDIUM_FROM) return "low";
  if (value < UNCERTAINTY_HIGH_FROM) return "medium";
  return "high";
}
