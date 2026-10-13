// A signed-in viewer's priority list (FR-FBR-50/51, Contract C43).
//
// CONTRACT STUB — the `contracts` piece froze the route and props; the
// `public-ui` piece builds the page against `fetchMyPriorities` /
// `saveMyPriorities`: reorder by keyboard and pointer, save the whole order,
// show "not yet ranked" pool items, report items removed from the pool once,
// and say a list of fewer than two items does not yet influence priority.
export interface MyPrioritiesProps {
  projectId: string;
}

export function MyPriorities(_props: MyPrioritiesProps) {
  return null;
}
