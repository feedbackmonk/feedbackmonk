// Localized labels for the wire enums (Contract C35, `status` namespace).
//
// The enum VALUES are wire contract (`wontfix`, `in-progress`, `bug`, …) and
// never change; only how we spell them for a human does. This hook is the one
// place that mapping lives for localized surfaces.
//
// Every admin-console consumer of the four shared families now reads this
// hook (Stage 2 / W-D) — the `*_LABELS` English constants that used to double
// as its fallback are gone from `types.gen.ts` (R-1/R-3), so the fallback is
// the wire value itself. `status.json` is the only source now; a key missing
// from it falls back to the wire value only if `en` itself is missing the key
// (a defensive floor, not the normal path — `en/status.json` always has it).
//
// KEY GRAMMAR (announced to the Rust side, which reads the same file):
//   status.<FeedbackStatus>          kind.<FeedbackKind>
//   sentiment.<SentimentValue>       roadmapStatus.<RoadmapItemStatus>

import { useMemo } from "react";
import { useTranslation } from "./index";
import type {
  FeedbackKind,
  FeedbackStatus,
  RoadmapItemStatus,
  SentimentValue,
} from "../shared/types.gen";

export interface Labels {
  status: (value: FeedbackStatus) => string;
  kind: (value: FeedbackKind) => string;
  sentiment: (value: SentimentValue) => string;
  roadmapStatus: (value: RoadmapItemStatus) => string;
}

export function useLabels(): Labels {
  const { t } = useTranslation("status");

  return useMemo<Labels>(
    () => ({
      status: (value) => t(`status.${value}`, { defaultValue: value }),
      kind: (value) => t(`kind.${value}`, { defaultValue: value }),
      sentiment: (value) => t(`sentiment.${value}`, { defaultValue: value }),
      roadmapStatus: (value) =>
        t(`roadmapStatus.${value}`, { defaultValue: value }),
    }),
    [t],
  );
}
