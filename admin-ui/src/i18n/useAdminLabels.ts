// Localized labels for the ADMIN-ONLY wire enums (Contract C35, `admin`
// namespace, LD ruling R-1 — collab-20260907-034037 GUIDE.md § 11).
//
// Mirrors `useLabels.ts` exactly, but for the ten enum families that are
// admin-console-only rather than shared with the public SPA + the Rust email
// binary. Putting them in `status.json` (like `useLabels`'s four families)
// would ship admin chrome inside the backend binary in 31 languages, which is
// not what "shared enum vocabulary" means (R-1 rationale).
//
// KEY GRAMMAR: admin.enum.<family>.<wireValue>
//   admin.enum.tier.*            admin.enum.resource.*
//   admin.enum.workOrderState.*  admin.enum.actionType.*
//   admin.enum.clusterPriority.* admin.enum.clusterStatus.*
//   admin.enum.recommendationStatus.*
//   admin.enum.autonomyRung.label.<rung>
//   admin.enum.autonomyRung.description.<rung>
//   admin.enum.workOrderEvent.*  admin.enum.keyClass.*
//   admin.enum.moderationStatus.*  admin.enum.tokenLifecycle.*
//
// `moderationStatus` (Contract C28, `shared/boardModerationApi.ts`) and
// `tokenLifecycle` (a display-only derived state, `settings/RunnerTokenCard.tsx`)
// are not among R-1's original ten — they are the same admin-only-enum shape,
// added here rather than left as raw English so ModerationQueue/
// ModerationActions/RunnerTokenCard don't regress the ratchet. Neither file is
// touched to add them; this hook is the single place the mapping lives.
//
// Every function falls back to the wire value itself when a catalog does not
// (yet) have the key — same discipline as `useLabels`, and the same reason:
// `admin.json` is the only source now that the `types.gen.ts` `*_LABELS`
// constants are deleted (R-1/R-3).

import { useMemo } from "react";
import { useTranslation } from "./index";
import type {
  ActionType,
  AutonomyRung,
  ClusterPriority,
  ClusterStatus,
  KeyClass,
  RecommendationStatus,
  ResourceKind,
  Tier,
  WorkOrderState,
} from "../shared/types.gen";
import type { ModerationStatus } from "../shared/boardModerationApi";

type TokenLifecycle = "active" | "revoked" | "expired";

export interface AdminLabels {
  tier: (value: Tier) => string;
  resource: (value: ResourceKind) => string;
  workOrderState: (value: WorkOrderState) => string;
  actionType: (value: ActionType) => string;
  clusterPriority: (value: ClusterPriority) => string;
  clusterStatus: (value: ClusterStatus) => string;
  recommendationStatus: (value: RecommendationStatus) => string;
  autonomyRungLabel: (value: AutonomyRung) => string;
  autonomyRungDescription: (value: AutonomyRung) => string;
  workOrderEvent: (value: string) => string;
  keyClass: (value: KeyClass) => string;
  moderationStatus: (value: ModerationStatus) => string;
  tokenLifecycle: (value: TokenLifecycle) => string;
}

export function useAdminLabels(): AdminLabels {
  const { t } = useTranslation("admin");

  return useMemo<AdminLabels>(
    () => ({
      tier: (value) =>
        t(`admin.enum.tier.${value}`, { defaultValue: value }),
      resource: (value) =>
        t(`admin.enum.resource.${value}`, { defaultValue: value }),
      workOrderState: (value) =>
        t(`admin.enum.workOrderState.${value}`, { defaultValue: value }),
      actionType: (value) =>
        t(`admin.enum.actionType.${value}`, { defaultValue: value }),
      clusterPriority: (value) =>
        t(`admin.enum.clusterPriority.${value}`, { defaultValue: value }),
      clusterStatus: (value) =>
        t(`admin.enum.clusterStatus.${value}`, { defaultValue: value }),
      recommendationStatus: (value) =>
        t(`admin.enum.recommendationStatus.${value}`, {
          defaultValue: value,
        }),
      autonomyRungLabel: (value) =>
        t(`admin.enum.autonomyRung.label.${value}`, {
          defaultValue: `Rung ${value}`,
        }),
      autonomyRungDescription: (value) =>
        t(`admin.enum.autonomyRung.description.${value}`, {
          defaultValue: "",
        }),
      workOrderEvent: (value) =>
        t(`admin.enum.workOrderEvent.${value}`, { defaultValue: value }),
      keyClass: (value) =>
        t(`admin.enum.keyClass.${value}`, { defaultValue: value }),
      moderationStatus: (value) =>
        t(`admin.enum.moderationStatus.${value}`, { defaultValue: value }),
      tokenLifecycle: (value) =>
        t(`admin.enum.tokenLifecycle.${value}`, { defaultValue: value }),
    }),
    [t],
  );
}
