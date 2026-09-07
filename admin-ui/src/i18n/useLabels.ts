// Localized labels for the wire enums (Contract C35, `status` namespace).
//
// The enum VALUES are wire contract (`wontfix`, `in-progress`, `bug`, …) and
// never change; only how we spell them for a human does. This hook is the one
// place that mapping lives for localized surfaces.
//
// WHY THE ENGLISH CONSTANTS IN `types.gen.ts` STAY (this stage): the admin
// console still renders them directly and is not extracted until Stage 2
// (W-D). They double as this hook's fallback, so a key that has not been added
// to `status.json` yet renders the same English word it renders today rather
// than a raw `status.wontfix`. W-D deletes them once every consumer is on this
// hook.
//
// KEY GRAMMAR (announced to the Rust side, which reads the same file):
//   status.<FeedbackStatus>          kind.<FeedbackKind>
//   sentiment.<SentimentValue>       roadmapStatus.<RoadmapItemStatus>

import { useMemo } from "react";
import { useTranslation } from "./index";
import {
  KIND_LABELS,
  ROADMAP_STATUS_LABELS,
  SENTIMENT_LABELS,
  STATUS_LABELS,
  type FeedbackKind,
  type FeedbackStatus,
  type RoadmapItemStatus,
  type SentimentValue,
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
      status: (value) =>
        t(`status.${value}`, { defaultValue: STATUS_LABELS[value] ?? value }),
      kind: (value) =>
        t(`kind.${value}`, { defaultValue: KIND_LABELS[value] ?? value }),
      sentiment: (value) =>
        t(`sentiment.${value}`, {
          defaultValue: SENTIMENT_LABELS[value] ?? value,
        }),
      roadmapStatus: (value) =>
        t(`roadmapStatus.${value}`, {
          defaultValue: ROADMAP_STATUS_LABELS[value] ?? value,
        }),
    }),
    [t],
  );
}
